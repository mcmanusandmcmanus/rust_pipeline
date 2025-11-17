use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
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
    #[arg(long, default_value_t = 120_000)]
    apd_sample: usize,
    /// Target LAFD sample size (rows).
    #[arg(long, default_value_t = 120_000)]
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

fn apd_schema() -> Schema {
    Schema::from_iter([
        Field::new("Incident Number".into(), DataType::Int64),
        Field::new("Incident Type".into(), DataType::String),
        Field::new("Council District".into(), DataType::Int64),
        Field::new("Mental Health Flag".into(), DataType::String),
        Field::new("Priority Level".into(), DataType::String),
        Field::new("Response Datetime".into(), DataType::String),
        Field::new("Response Year".into(), DataType::Int64),
        Field::new("Response Month".into(), DataType::String),
        Field::new("Response Day of Week".into(), DataType::String),
        Field::new("Response Hour".into(), DataType::Int64),
        Field::new("First Unit Arrived Datetime".into(), DataType::String),
        Field::new("Call Closed Datetime".into(), DataType::String),
        Field::new("Sector".into(), DataType::String),
        Field::new("Initial Problem Description".into(), DataType::String),
        Field::new("Initial Problem Category".into(), DataType::String),
        Field::new("Final Problem Description".into(), DataType::String),
        Field::new("Final Problem Category".into(), DataType::String),
        Field::new("Number of Units Arrived".into(), DataType::Int64),
        Field::new("Unit Time on Scene".into(), DataType::String),
        Field::new("Call Disposition Description".into(), DataType::String),
        Field::new("Report Written Flag".into(), DataType::String),
        Field::new("Response Time".into(), DataType::String),
        Field::new("Officer Injured/Killed Count".into(), DataType::Int64),
        Field::new("Subject Injured/Killed Count".into(), DataType::Int64),
        Field::new("Other Injured/Killed Count".into(), DataType::Int64),
        Field::new("Geo ID".into(), DataType::Int64),
        Field::new("Census Block Group".into(), DataType::Int64),
    ])
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
        "priority_level_ord",
        "units_arrived",
        "mental_health_flag",
        "incident_type",
        "call_disposition",
        "response_minutes",
        "call_duration_minutes",
    ];

    let start = Instant::now();
    let apd_path = cli.apd_file.to_string_lossy().into_owned();
    let lazy = LazyCsvReader::new(PlPath::new(&apd_path))
        .with_has_header(true)
        .with_ignore_errors(false)
        .with_infer_schema_length(Some(2048))
        .with_schema(Some(Arc::new(apd_schema())))
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
            col("Call Disposition Description").alias("call_disposition"),
            col("Number of Units Arrived")
                .cast(DataType::Int32)
                .alias("units_arrived"),
        ])
        .with_columns([
            col("Priority Level")
                .cast(DataType::String)
                .str()
                .replace_all(lit("Priority "), lit(""), true)
                .cast(DataType::Int32)
                .alias("priority_level_ord"),
            col("Report Written Flag")
                .cast(DataType::String)
                .str()
                .to_lowercase()
                .eq(lit("yes"))
                .cast(DataType::UInt8)
                .alias("report_written"),
            col("Response Datetime")
                .cast(DataType::String)
                .str()
                .to_datetime(None, None, dt_options.clone(), lit("raise"))
                .alias("response_dt"),
            col("First Unit Arrived Datetime")
                .cast(DataType::String)
                .str()
                .to_datetime(None, None, dt_options.clone(), lit("raise"))
                .alias("arrival_dt"),
            col("Call Closed Datetime")
                .cast(DataType::String)
                .str()
                .to_datetime(None, None, dt_options, lit("raise"))
                .alias("closed_dt"),
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

    let apd_frame = processed
        .collect()
        .with_context(|| "collecting APD feature frame".to_string())?;
    let filtered_rows = apd_frame.height();
    let apd_sample = stratified_sample(
        &apd_frame,
        &["report_written", "priority_level_ord"],
        cli.apd_sample,
        cli.seed,
    )
    .with_context(|| "sampling APD frame".to_string())?;
    let output_path = cli.output_dir.join("apd_sample.parquet");
    write_parquet(&apd_sample, &output_path)?;

    let categories = build_category_map(
        &apd_sample,
        &["incident_type", "mental_health_flag", "call_disposition"],
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
    let feature_cols = vec![
        "unit_type",
        "dispatch_status",
        "emergency_dispatch_code",
        "first_in_district",
        "dispatch_sequence",
        "dispatch_delay_s",
        "enroute_delay_s",
        "arrival_delay_s",
        "total_response_s",
    ];

    let start = Instant::now();
    let lafd_path = cli.lafd_file.to_string_lossy().into_owned();
    let lazy = LazyCsvReader::new(PlPath::new(&lafd_path))
        .with_has_header(true)
        .with_ignore_errors(false)
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
        .filter(
            col("dispatch_delay_s")
                .is_not_null()
                .and(col("dispatch_delay_s").gt_eq(lit(0)))
                .and(col("total_response_s").gt(lit(0))),
        )
        .select(vec![
            col("Unit Type").alias("unit_type"),
            col("Dispatch Status").alias("dispatch_status"),
            col("Emergency Dispatch Code").alias("emergency_dispatch_code"),
            col("first_in_district"),
            col("dispatch_sequence"),
            col("dispatch_delay_s"),
            col("enroute_delay_s"),
            col("arrival_delay_s"),
            col("total_response_s"),
        ]);

    let lafd_frame = processed
        .collect()
        .with_context(|| "collecting LAFD feature frame".to_string())?;
    let filtered_rows = lafd_frame.height();
    let lafd_sample = stratified_sample(
        &lafd_frame,
        &["unit_type"],
        cli.lafd_sample,
        cli.seed.wrapping_mul(11),
    )
    .with_context(|| "sampling LAFD frame".to_string())?;
    let output_path = cli.output_dir.join("lafd_sample.parquet");
    write_parquet(&lafd_sample, &output_path)?;

    let categories = build_category_map(
        &lafd_sample,
        &["unit_type", "dispatch_status", "emergency_dispatch_code"],
    )?;

    Ok(PrepResult {
        metrics: DatasetMetrics {
            name: "lafd_response".into(),
            source_path: cli.lafd_file.display().to_string(),
            rows_after_filters: filtered_rows,
            rows_sampled: lafd_sample.height(),
            duration_ms: start.elapsed().as_millis(),
            output_path: output_path.display().to_string(),
            feature_columns: feature_cols.iter().map(|s| s.to_string()).collect(),
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

    let partitions = df.partition_by_stable(columns.iter().copied(), true)?;
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
            let value = match series.get(idx) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if value.is_null() {
                continue;
            }
            let text = value.to_string();
            if text.is_empty() {
                continue;
            }
            *counts.entry(text).or_insert(0) += 1;
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
