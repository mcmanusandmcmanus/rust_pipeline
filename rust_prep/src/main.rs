use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use polars::prelude::*;
use rand::{
    SeedableRng,
    rngs::StdRng,
    seq::{SliceRandom, index},
};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Rust streaming prep for APD + LAFD benchmarks"
)]
struct Cli {
    /// Path to APD dispatch CSV (~1.7GB).
    #[arg(
        long,
        default_value = "APD_Computer_Aided_Dispatch_Incidents_20251101.csv"
    )]
    apd_file: PathBuf,
    /// Path to LAFD response metrics CSV (~1GB).
    #[arg(long, default_value = "LAFD_Response_Metrics_-_Raw_Data_20251101.csv")]
    lafd_file: PathBuf,
    /// Directory for all processed artifacts.
    #[arg(long, default_value = "data/processed")]
    output_dir: PathBuf,
    /// Target APD sample size (rows).
    #[arg(long, default_value_t = 250_000)]
    apd_sample: usize,
    /// Target LAFD sample size (rows).
    #[arg(long, default_value_t = 250_000)]
    lafd_sample: usize,
    /// Random seed shared across operations.
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

#[derive(Debug, Serialize, Clone)]
struct DatasetMetrics {
    name: String,
    source_path: String,
    rows_after_filters: usize,
    rows_sampled: usize,
    duration_ms: u128,
    output_path: String,
    feature_columns: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PrepBenchmark {
    generated_at_utc: String,
    total_duration_ms: u128,
    apd: DatasetMetrics,
    lafd: DatasetMetrics,
    command: Vec<String>,
}

struct PrepResult {
    metrics: DatasetMetrics,
    categories: HashMap<String, Vec<String>>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    fs::create_dir_all(&cli.output_dir)?;
    let pb = setup_progress();
    pb.println("Starting Rust prep pipeline…");
    let overall_start = Instant::now();

    let apd = prep_apd(&cli, &pb).context("failed to prep APD incidents")?;
    pb.println(format!(
        "✅ APD sample ready at {} ({} rows)",
        apd.metrics.output_path, apd.metrics.rows_sampled
    ));

    let lafd = prep_lafd(&cli, &pb).context("failed to prep LAFD metrics")?;
    pb.println(format!(
        "✅ LAFD sample ready at {} ({} rows)",
        lafd.metrics.output_path, lafd.metrics.rows_sampled
    ));

    write_category_maps(&cli.output_dir, &apd.categories, &lafd.categories)?;
    pb.println("📦 Wrote category_maps.json");

    let benchmark = PrepBenchmark {
        generated_at_utc: OffsetDateTime::now_utc().format(&Rfc3339)?,
        total_duration_ms: overall_start.elapsed().as_millis(),
        apd: apd.metrics,
        lafd: lafd.metrics,
        command: std::env::args().collect(),
    };
    let bench_path = cli.output_dir.join("prep_rust_bench.json");
    serde_json::to_writer_pretty(File::create(&bench_path)?, &benchmark)?;
    pb.println(format!(
        "📈 Benchmark log saved to {}",
        bench_path.display()
    ));
    pb.finish_with_message("Rust prep finished.");
    Ok(())
}

fn prep_apd(cli: &Cli, pb: &ProgressBar) -> Result<PrepResult> {
    pb.println("→ Parsing APD dispatch CSV");
    let dt_options = StrptimeOptions {
        format: Some("%Y %b %d %I:%M:%S %p".into()),
        strict: false,
        exact: true,
        cache: true,
    };

    let sample_cols = vec![
        "report_written",
        "priority_label",
        "priority_level_ord",
        "units_arrived",
        "mental_health_flag",
        "incident_type",
        "response_hour",
        "response_day_of_week",
        "response_month",
        "initial_problem_category",
        "final_problem_category",
        "call_disposition",
        "sector",
        "council_district",
        "response_minutes",
        "call_duration_minutes",
        "on_scene_seconds",
        "response_time_seconds",
    ];

    let start = Instant::now();
    let lazy = LazyCsvReader::new(&cli.apd_file)
        .with_has_header(true)
        .with_ignore_errors(true)
        .with_infer_schema_length(Some(2048))
        .finish()
        .with_context(|| format!("unable to read {}", cli.apd_file.display()))?;

    let processed = lazy
        .filter(
            col("Response Year")
                .cast(DataType::Int32)
                .gt_eq(lit(2019))
                .and(col("Response Year").cast(DataType::Int32).lt_eq(lit(2024))),
        )
        .with_columns([
            col("Incident Type").alias("incident_type"),
            col("Mental Health Flag").alias("mental_health_flag"),
            col("Response Day of Week").alias("response_day_of_week"),
            col("Response Month").alias("response_month"),
            col("Sector").alias("sector"),
            col("Initial Problem Category").alias("initial_problem_category"),
            col("Final Problem Category").alias("final_problem_category"),
            col("Call Disposition Description").alias("call_disposition"),
            col("Priority Level").alias("priority_label"),
            col("Council District")
                .cast(DataType::Int32)
                .alias("council_district"),
            col("Response Hour")
                .cast(DataType::Int32)
                .alias("response_hour"),
            col("Number of Units Arrived")
                .cast(DataType::Int32)
                .alias("units_arrived"),
        ])
        .with_columns([
            col("Priority Level")
                .str()
                .replace_all(lit("Priority "), lit(""), true)
                .cast(DataType::Int32)
                .alias("priority_level_ord"),
            col("Report Written Flag")
                .str()
                .to_lowercase()
                .eq(lit("yes"))
                .cast(DataType::UInt8)
                .alias("report_written"),
            col("Response Datetime")
                .str()
                .to_datetime(None, None, dt_options.clone(), lit("raise"))
                .alias("response_dt"),
            col("First Unit Arrived Datetime")
                .str()
                .to_datetime(None, None, dt_options.clone(), lit("raise"))
                .alias("arrival_dt"),
            col("Call Closed Datetime")
                .str()
                .to_datetime(None, None, dt_options, lit("raise"))
                .alias("closed_dt"),
            col("Unit Time on Scene")
                .str()
                .replace_all(lit(","), lit(""), true)
                .cast(DataType::Int32)
                .alias("on_scene_seconds"),
            col("Response Time")
                .str()
                .replace_all(lit(","), lit(""), true)
                .cast(DataType::Int32)
                .alias("response_time_seconds"),
        ])
        .with_columns([
            col("response_dt")
                .dt()
                .timestamp(TimeUnit::Milliseconds)
                .alias("response_ms"),
            col("arrival_dt")
                .dt()
                .timestamp(TimeUnit::Milliseconds)
                .alias("arrival_ms"),
            col("closed_dt")
                .dt()
                .timestamp(TimeUnit::Milliseconds)
                .alias("closed_ms"),
        ])
        .with_columns([
            ((col("arrival_ms") - col("response_ms")).cast(DataType::Float64) / lit(60_000.0))
                .alias("response_minutes"),
            ((col("closed_ms") - col("response_ms")).cast(DataType::Float64) / lit(60_000.0))
                .alias("call_duration_minutes"),
        ])
        .filter(
            col("response_minutes")
                .is_not_null()
                .and(col("response_minutes").gt(lit(0)))
                .and(col("call_duration_minutes").gt(lit(0)))
                .and(col("report_written").is_not_null()),
        )
        .select(
            sample_cols
                .iter()
                .map(|&name| col(name))
                .collect::<Vec<_>>(),
        );

    let apd_frame = processed.collect()?;
    let filtered_rows = apd_frame.height();
    let apd_sample = stratified_sample(
        &apd_frame,
        &["report_written", "priority_level_ord"],
        cli.apd_sample,
        cli.seed,
    )?;
    let output_path = cli.output_dir.join("apd_sample.parquet");
    write_parquet(&apd_sample, &output_path)?;

    let categories = build_category_map(
        &apd_sample,
        &[
            "incident_type",
            "mental_health_flag",
            "response_day_of_week",
            "response_month",
            "initial_problem_category",
            "final_problem_category",
            "call_disposition",
            "sector",
        ],
    )?;

    Ok(PrepResult {
        metrics: DatasetMetrics {
            name: "apd_dispatch".into(),
            source_path: cli.apd_file.display().to_string(),
            rows_after_filters: filtered_rows,
            rows_sampled: apd_sample.height(),
            duration_ms: start.elapsed().as_millis(),
            output_path: output_path.display().to_string(),
            feature_columns: sample_cols.iter().map(|s| s.to_string()).collect(),
        },
        categories,
    })
}

fn prep_lafd(cli: &Cli, pb: &ProgressBar) -> Result<PrepResult> {
    pb.println("→ Parsing LAFD response CSV");
    let dt_options = StrptimeOptions {
        format: Some("%Y-%m-%d %H:%M:%S%.f".into()),
        strict: false,
        exact: true,
        cache: true,
    };
    let sample_cols = vec![
        "unit_type",
        "dispatch_status",
        "emergency_dispatch_code",
        "ppe_level",
        "first_in_district",
        "dispatch_sequence",
        "dispatch_delay_s",
        "enroute_delay_s",
        "arrival_delay_s",
        "total_response_s",
        "creation_hour",
        "dispatch_hour",
    ];

    let start = Instant::now();
    let lazy = LazyCsvReader::new(&cli.lafd_file)
        .with_has_header(true)
        .with_ignore_errors(true)
        .with_infer_schema_length(Some(512))
        .finish()
        .with_context(|| format!("unable to read {}", cli.lafd_file.display()))?;

    let synth_date = lit("2024-01-01 ");
    let processed = lazy
        .filter(col("On Scene Time (GMT)").is_not_null())
        .with_columns([
            col("Unit Type").alias("unit_type"),
            col("Dispatch Status").alias("dispatch_status"),
            col("Emergency Dispatch Code").alias("emergency_dispatch_code"),
            col("PPE Level").alias("ppe_level"),
            col("Dispatch Sequence")
                .cast(DataType::Int32)
                .alias("dispatch_sequence"),
            col("First In District")
                .cast(DataType::Int32)
                .alias("first_in_district"),
        ])
        .with_columns([
            concat_str(
                [synth_date.clone(), col("Incident Creation Time (GMT)")],
                "",
                true,
            )
            .alias("creation_full"),
            concat_str(
                [synth_date.clone(), col("Time of Dispatch (GMT)")],
                "",
                true,
            )
            .alias("dispatch_full"),
            concat_str([synth_date.clone(), col("En Route Time (GMT)")], "", true)
                .alias("enroute_full"),
            concat_str([synth_date, col("On Scene Time (GMT)")], "", true).alias("on_scene_full"),
        ])
        .with_columns([
            col("creation_full")
                .str()
                .to_datetime(None, None, dt_options.clone(), lit("raise"))
                .alias("creation_dt"),
            col("dispatch_full")
                .str()
                .to_datetime(None, None, dt_options.clone(), lit("raise"))
                .alias("dispatch_dt"),
            col("enroute_full")
                .str()
                .to_datetime(None, None, dt_options.clone(), lit("raise"))
                .alias("enroute_dt"),
            col("on_scene_full")
                .str()
                .to_datetime(None, None, dt_options, lit("raise"))
                .alias("on_scene_dt"),
        ])
        .with_columns([
            col("creation_dt")
                .dt()
                .timestamp(TimeUnit::Milliseconds)
                .alias("creation_ms"),
            col("dispatch_dt")
                .dt()
                .timestamp(TimeUnit::Milliseconds)
                .alias("dispatch_ms"),
            col("enroute_dt")
                .dt()
                .timestamp(TimeUnit::Milliseconds)
                .alias("enroute_ms"),
            col("on_scene_dt")
                .dt()
                .timestamp(TimeUnit::Milliseconds)
                .alias("on_scene_ms"),
        ])
        .with_columns([
            ((col("dispatch_ms") - col("creation_ms")).cast(DataType::Float64) / lit(1000.0))
                .alias("dispatch_delay_s"),
            ((col("enroute_ms") - col("dispatch_ms")).cast(DataType::Float64) / lit(1000.0))
                .alias("enroute_delay_s"),
            ((col("on_scene_ms") - col("enroute_ms")).cast(DataType::Float64) / lit(1000.0))
                .alias("arrival_delay_s"),
        ])
        .with_columns([
            ((col("on_scene_ms") - col("creation_ms")).cast(DataType::Float64) / lit(1000.0))
                .alias("total_response_s"),
        ])
        .with_columns([
            col("creation_dt").dt().hour().alias("creation_hour"),
            col("dispatch_dt").dt().hour().alias("dispatch_hour"),
        ])
        .filter(
            col("dispatch_delay_s")
                .is_not_null()
                .and(col("dispatch_delay_s").gt_eq(lit(0)))
                .and(col("total_response_s").gt(lit(0))),
        )
        .select(
            sample_cols
                .iter()
                .map(|&name| col(name))
                .collect::<Vec<_>>(),
        );

    let lafd_frame = processed.collect()?;
    let filtered_rows = lafd_frame.height();
    let lafd_sample = stratified_sample(
        &lafd_frame,
        &["unit_type"],
        cli.lafd_sample,
        cli.seed.wrapping_mul(11),
    )?;
    let output_path = cli.output_dir.join("lafd_sample.parquet");
    write_parquet(&lafd_sample, &output_path)?;

    let categories = build_category_map(
        &lafd_sample,
        &[
            "unit_type",
            "dispatch_status",
            "emergency_dispatch_code",
            "ppe_level",
        ],
    )?;

    Ok(PrepResult {
        metrics: DatasetMetrics {
            name: "lafd_response".into(),
            source_path: cli.lafd_file.display().to_string(),
            rows_after_filters: filtered_rows,
            rows_sampled: lafd_sample.height(),
            duration_ms: start.elapsed().as_millis(),
            output_path: output_path.display().to_string(),
            feature_columns: sample_cols.iter().map(|s| s.to_string()).collect(),
        },
        categories,
    })
}

fn stratified_sample(
    df: &DataFrame,
    columns: &[&str],
    desired: usize,
    seed: u64,
) -> PolarsResult<DataFrame> {
    if desired == 0 || desired >= df.height() {
        return Ok(df.clone());
    }

    let partitions = df.partition_by_stable(columns.iter().copied(), false)?;
    let total_rows = df.height();
    let total_groups = partitions.len();
    let mut collected = Vec::with_capacity(total_groups);
    let mut assigned = 0usize;

    for (idx, part) in partitions.into_iter().enumerate() {
        let part_rows = part.height();
        if part_rows == 0 {
            continue;
        }

        let remaining_groups = total_groups.saturating_sub(idx);
        let mut take = ((part_rows as f64 / total_rows as f64) * desired as f64).round() as usize;
        take = take.clamp(1, part_rows);
        if idx == total_groups.saturating_sub(1)
            || assigned + take > desired
            || remaining_groups == 1
        {
            take = desired.saturating_sub(assigned).min(part_rows);
        }
        if take == 0 {
            continue;
        }
        let strat_seed = seed ^ ((idx as u64 + 1) * 1_000_003);
        let sampled = sample_dataframe(&part, take, strat_seed)?;
        collected.push(sampled);
        assigned += take;
        if assigned >= desired {
            break;
        }
    }

    let mut iter = collected.into_iter();
    let mut combined = if let Some(df) = iter.next() {
        df
    } else {
        return Ok(DataFrame::default());
    };
    for frame in iter {
        combined.vstack_mut(&frame)?;
    }
    combined = shuffle_df(&combined, seed ^ 0x9E37_79B1)?;
    Ok(combined.head(Some(desired)))
}

fn sample_dataframe(df: &DataFrame, take: usize, seed: u64) -> PolarsResult<DataFrame> {
    if take >= df.height() {
        return Ok(df.clone());
    }
    let indices = random_indices(df.height(), take, seed);
    let idx_ca = IdxCa::from_vec(
        PlSmallStr::from_static("idx"),
        indices.into_iter().map(|idx| idx as IdxSize).collect(),
    );
    df.take(&idx_ca)
}

fn shuffle_df(df: &DataFrame, seed: u64) -> PolarsResult<DataFrame> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut indices: Vec<IdxSize> = (0..df.height() as IdxSize).collect();
    indices.as_mut_slice().shuffle(&mut rng);
    let idx_ca = IdxCa::from_vec(PlSmallStr::from_static("shuffle"), indices);
    df.take(&idx_ca)
}

fn random_indices(len: usize, take: usize, seed: u64) -> Vec<usize> {
    if take >= len {
        return (0..len).collect();
    }
    let mut rng = StdRng::seed_from_u64(seed);
    index::sample(&mut rng, len, take).into_vec()
}

fn build_category_map(df: &DataFrame, columns: &[&str]) -> Result<HashMap<String, Vec<String>>> {
    let mut map = HashMap::new();
    for &column in columns {
        let series = df
            .column(column)
            .with_context(|| format!("missing column {column} for category map"))?;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for idx in 0..series.len() {
            let value = series
                .str_value(idx)
                .with_context(|| format!("failed to read string value for {column}"))?;
            if value.as_ref() == "null" {
                continue;
            }
            *counts.entry(value.into_owned()).or_insert(0) += 1;
        }
        let mut entries: Vec<(String, usize)> = counts.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        map.insert(
            column.to_string(),
            entries.into_iter().map(|(k, _)| k).collect(),
        );
    }
    Ok(map)
}

fn write_parquet(df: &DataFrame, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut owned = df.clone();
    ParquetWriter::new(file)
        .finish(&mut owned)
        .context("unable to write parquet")?;
    Ok(())
}

fn write_category_maps(
    output_dir: &Path,
    apd: &HashMap<String, Vec<String>>,
    lafd: &HashMap<String, Vec<String>>,
) -> Result<()> {
    #[derive(Serialize)]
    struct Wrapper<'a> {
        apd: &'a HashMap<String, Vec<String>>,
        lafd: &'a HashMap<String, Vec<String>>,
    }
    let path = output_dir.join("category_maps.json");
    serde_json::to_writer_pretty(File::create(&path)?, &Wrapper { apd, lafd })?;
    Ok(())
}

fn setup_progress() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_chars("/|\\- "),
    );
    pb
}
