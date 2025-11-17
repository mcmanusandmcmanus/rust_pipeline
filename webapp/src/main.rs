use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use askama::Template;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{fs, net::TcpListener};
use tower_http::trace::TraceLayer;
use tracing::{Level, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    let config = AppConfig::from_env()?;
    let state = Arc::new(AppState { config });
    let router = Router::new()
        .route("/", get(index))
        .route("/api/metrics", get(metrics))
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http());

    info!("Dashboard listening on http://{}", state.config.bind_addr);
    let listener = TcpListener::bind(state.config.bind_addr).await?;
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}

struct AppState {
    config: AppConfig,
}

#[derive(Clone)]
struct AppConfig {
    bind_addr: SocketAddr,
    prep_path: PathBuf,
    control_prep_path: PathBuf,
    apd_metrics_path: PathBuf,
    lafd_metrics_path: PathBuf,
    data_quality_path: PathBuf,
    robustness_path: PathBuf,
    cost_model_path: PathBuf,
}

impl AppConfig {
    fn from_env() -> Result<Self> {
        let port = env::var("WEBAPP_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(8080);
        let host = env::var("WEBAPP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let bind_addr = format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], port)));
        let prep_path = env::var("PREP_BENCH_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../data/processed/prep_rust_bench.json"));
        let control_prep_path = env::var("CONTROL_PREP_BENCH_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../data/processed/prep_python_bench.json"));
        let apd_path = env::var("APD_METRICS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../py_model/artifacts/apd_metrics.json"));
        let lafd_path = env::var("LAFD_METRICS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../py_model/artifacts/lafd_metrics.json"));
        let data_quality_path = env::var("DATA_QUALITY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../data/processed/data_quality_summary.json"));
        let robustness_path = env::var("ROBUSTNESS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../data/processed/run_variance_summary.json"));
        let cost_model_path = env::var("COST_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../config/cost_model.json"));
        Ok(Self {
            bind_addr,
            prep_path,
            control_prep_path,
            apd_metrics_path: apd_path,
            lafd_metrics_path: lafd_path,
            data_quality_path,
            robustness_path,
            cost_model_path,
        })
    }
}

async fn index(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse, AppError> {
    let payload = state.load_context().await?;
    let template = DashboardTemplate {
        ctx: &payload.template,
    };
    let rendered = template.render()?;
    Ok(Html(rendered))
}

async fn metrics(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse, AppError> {
    let payload = state.load_context().await?;
    Ok((StatusCode::OK, Json(payload.raw_metrics)))
}

impl AppState {
    async fn load_context(&self) -> Result<DashboardPayload> {
        let prep_value = read_json(&self.config.prep_path).await;
        let control_value = read_json(&self.config.control_prep_path).await;
        let apd_value = read_json(&self.config.apd_metrics_path).await;
        let lafd_value = read_json(&self.config.lafd_metrics_path).await;
        let data_quality_value = read_json(&self.config.data_quality_path).await;
        let robustness_value = read_json(&self.config.robustness_path).await;
        let cost_model_value = read_json(&self.config.cost_model_path).await;

        let prep_summary = prep_value
            .as_ref()
            .and_then(|v| serde_json::from_value::<PrepFile>(v.clone()).ok())
            .map(PrepSummary::from);
        let control_summary = control_value
            .as_ref()
            .and_then(|v| serde_json::from_value::<PrepFile>(v.clone()).ok())
            .map(PrepSummary::from);
        let apd_summary = apd_value
            .as_ref()
            .and_then(|v| serde_json::from_value::<ApdMetricsFile>(v.clone()).ok())
            .map(ApdSummary::from);
        let lafd_summary = lafd_value
            .as_ref()
            .and_then(|v| serde_json::from_value::<LafdMetricsFile>(v.clone()).ok())
            .map(LafdSummary::from);

        let data_quality_file = data_quality_value
            .as_ref()
            .and_then(|v| serde_json::from_value::<DataQualityFile>(v.clone()).ok());
        let robustness_file = robustness_value
            .as_ref()
            .and_then(|v| serde_json::from_value::<RobustnessFile>(v.clone()).ok());
        let cost_model = cost_model_value
            .as_ref()
            .and_then(|v| serde_json::from_value::<CostModelFile>(v.clone()).ok());

        let prep_comparison = PrepComparison {
            control: control_summary.clone(),
            variant: prep_summary.clone(),
        };
        let data_quality_context = DataQualityContext::from_file(data_quality_file);
        let robustness_context = robustness_file.map(RobustnessContext::from);
        let cost_impact = CostImpact::from_inputs(&prep_comparison, cost_model.as_ref());

        let hardware = HardwareSpec::default();
        let overview = OverviewContext::new(&hardware);
        let key_questions = KeyQuestions::new(
            &prep_comparison,
            cost_impact.as_ref(),
            &data_quality_context,
        );
        let performance_tables = prep_comparison.build_tables();
        let methods = MethodsContext::default();

        let template = DashboardTemplateData {
            overview,
            key_questions,
            pipelines: PipelineSection::default(),
            performance_tables,
            cost_impact,
            apd: apd_summary,
            lafd: lafd_summary,
            data_quality: data_quality_context,
            robustness: robustness_context,
            methods,
            metrics_b64: STANDARD.encode(serde_json::to_vec(&json!({
                "prep_variant": prep_value,
                "prep_control": control_value,
                "apd": apd_value,
                "lafd": lafd_value,
                "data_quality": data_quality_value,
                "robustness": robustness_value,
                "cost_model": cost_model_value
            }))?),
        };

        Ok(DashboardPayload {
            template,
            raw_metrics: json!({
                "prep_variant": prep_value,
                "prep_control": control_value,
                "apd": apd_value,
                "lafd": lafd_value,
                "data_quality": data_quality_value,
                "robustness": robustness_value,
                "cost_model": cost_model_value
            }),
        })
    }
}

async fn read_json(path: &Path) -> Option<Value> {
    let contents = fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&contents).ok()
}

struct DashboardPayload {
    template: DashboardTemplateData,
    raw_metrics: Value,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate<'a> {
    ctx: &'a DashboardTemplateData,
}

#[derive(Serialize)]
struct DashboardTemplateData {
    overview: OverviewContext,
    key_questions: KeyQuestions,
    pipelines: PipelineSection,
    performance_tables: Vec<PerformanceTable>,
    cost_impact: Option<CostImpact>,
    apd: Option<ApdSummary>,
    lafd: Option<LafdSummary>,
    data_quality: DataQualityContext,
    robustness: Option<RobustnessContext>,
    methods: MethodsContext,
    metrics_b64: String,
}

#[derive(Serialize)]
struct OverviewContext {
    eyebrow: &'static str,
    title: &'static str,
    subtitle: &'static str,
    chips: Vec<String>,
}

impl OverviewContext {
    fn new(hardware: &HardwareSpec) -> Self {
        Self {
            eyebrow: "Live Experiment Panel",
            title: "Rust & Python Data Prep Experiment",
            subtitle: "We run the Python control and the Rust-accelerated variant on the same CAD + fire datasets to see where Rust creates leverage.",
            chips: vec![
                "Data: APD CAD + LAFD response extracts".to_string(),
                "Pipelines: Python control vs Rust-accelerated prep".to_string(),
                format!("Machine: {} · {}", hardware.cpu, hardware.ram),
                "Run Source: latest artifacts on disk".to_string(),
            ],
        }
    }
}

#[derive(Serialize)]
struct KeyQuestions {
    cards: Vec<KeyQuestionCard>,
}

impl KeyQuestions {
    fn new(
        comparison: &PrepComparison,
        cost_impact: Option<&CostImpact>,
        data_quality: &DataQualityContext,
    ) -> Self {
        let testing_card = KeyQuestionCard {
            title: "What is being tested?",
            body: comparison.card_copy(),
        };
        let value_body = cost_impact
            .map(|impact| {
                format!(
                    "Rust saves ~{} per run which translates to {} annually if you execute {:.1} runs per day.",
                    impact.minutes_saved, impact.cost_saved_per_run, impact.runs_per_day
                )
            })
            .unwrap_or_else(|| {
                "Once both pipelines finish prep we estimate time and cost deltas automatically."
                    .to_string()
            });
        let value_card = KeyQuestionCard {
            title: "Is there value in using Rust?",
            body: value_body,
        };
        let interpret_card = KeyQuestionCard {
            title: "How should we interpret this run?",
            body: data_quality.status.clone(),
        };
        Self {
            cards: vec![testing_card, value_card, interpret_card],
        }
    }
}

#[derive(Serialize)]
struct KeyQuestionCard {
    title: &'static str,
    body: String,
}

#[derive(Serialize)]
struct PipelineSection {
    control: PipelineCard,
    variant: PipelineCard,
}

impl Default for PipelineSection {
    fn default() -> Self {
        Self {
            control: PipelineCard {
                title: "Control · Python Prep Stack",
                description: "Reference implementation that ingests CSVs, engineers features, and saves Parquet fully in Python.",
                steps: vec![
                    "Load & filter CAD/fire CSV extracts",
                    "Engineer calendar + response-time features in pandas/polars",
                    "Persist Parquet outputs",
                    "Train baseline models in Python",
                ],
            },
            variant: PipelineCard {
                title: "Variant · Rust-Accelerated Prep",
                description: "Rust handles the heavy I/O + feature engineering, then hands Parquet files back to the Python modeling stack.",
                steps: vec![
                    "Rust (Polars) streams CSV input",
                    "Apply identical filters & feature schema",
                    "Emit Parquet + metadata for downstream jobs",
                    "Reuse the same Python model training code",
                ],
            },
        }
    }
}

#[derive(Serialize)]
struct PipelineCard {
    title: &'static str,
    description: &'static str,
    steps: Vec<&'static str>,
}

#[derive(Serialize)]
struct PerformanceTable {
    title: &'static str,
    rows_label: String,
    stats: Vec<PipelinePerfRow>,
}

#[derive(Serialize)]
struct PipelinePerfRow {
    label: &'static str,
    wall_time: Option<String>,
    throughput: Option<String>,
    delta: String,
}

#[derive(Serialize)]
struct CostImpact {
    minutes_saved: String,
    cost_saved_per_run: String,
    annual_cost_delta: String,
    runs_per_day: f64,
    status: String,
}

impl CostImpact {
    fn from_inputs(
        comparison: &PrepComparison,
        cost_model: Option<&CostModelFile>,
    ) -> Option<Self> {
        let cost_model = cost_model?;
        let control = comparison.control.as_ref()?;
        let variant = comparison.variant.as_ref()?;
        let delta_minutes =
            (control.total_duration_ms as f64 - variant.total_duration_ms as f64) / 60_000.0;
        let delta_hours = delta_minutes / 60.0;
        let compute_delta = delta_hours * cost_model.compute_cost_per_hour;
        let analyst_delta = delta_hours * cost_model.analyst_cost_per_hour;
        let cost_saved_per_run = compute_delta + analyst_delta;
        let annual_delta = cost_saved_per_run * cost_model.runs_per_day * 365.0;
        Some(Self {
            minutes_saved: format!("{delta_minutes:.1} min"),
            cost_saved_per_run: format_currency(cost_saved_per_run),
            annual_cost_delta: format_currency(annual_delta),
            runs_per_day: cost_model.runs_per_day,
            status: if delta_minutes >= 0.0 {
                "Rust trims time off the prep stage; negative cost means Python is currently faster."
                    .to_string()
            } else {
                "Rust runs slower than control in the current artifacts.".to_string()
            },
        })
    }
}

#[derive(Serialize)]
struct DataQualityContext {
    apd: Vec<DataQualityRow>,
    lafd: Vec<DataQualityRow>,
    status: String,
    show_apd: bool,
    show_lafd: bool,
}

impl DataQualityContext {
    fn from_file(file: Option<DataQualityFile>) -> Self {
        let mut status = "Run the data-quality check to compare outputs.".to_string();
        if let Some(file) = file {
            let apd_rows = file
                .apd
                .into_iter()
                .map(DataQualityRow::from)
                .collect::<Vec<_>>();
            let lafd_rows = file
                .lafd
                .into_iter()
                .map(DataQualityRow::from)
                .collect::<Vec<_>>();
            let show_apd = !apd_rows.is_empty();
            let show_lafd = !lafd_rows.is_empty();
            let highlighted = apd_rows
                .iter()
                .chain(lafd_rows.iter())
                .filter(|row| row.highlighted)
                .count();
            status = if highlighted == 0 {
                format!(
                    "{} columns reviewed · no material drift detected.",
                    apd_rows.len() + lafd_rows.len()
                )
            } else {
                format!(
                    "{} columns reviewed · {} show drift worth investigating.",
                    apd_rows.len() + lafd_rows.len(),
                    highlighted
                )
            };
            return Self {
                apd: apd_rows,
                lafd: lafd_rows,
                status,
                show_apd,
                show_lafd,
            };
        }
        Self {
            apd: Vec::new(),
            lafd: Vec::new(),
            status,
            show_apd: false,
            show_lafd: false,
        }
    }
}

#[derive(Serialize)]
struct DataQualityRow {
    column: String,
    python_nulls: Option<u64>,
    rust_nulls: Option<u64>,
    python_distinct: Option<u64>,
    rust_distinct: Option<u64>,
    highlighted: bool,
}

impl From<DataQualityColumn> for DataQualityRow {
    fn from(value: DataQualityColumn) -> Self {
        let highlighted = value
            .python_nulls
            .zip(value.rust_nulls)
            .map(|(p, r)| p.abs_diff(r) > 250)
            .unwrap_or(false)
            || value
                .python_distinct
                .zip(value.rust_distinct)
                .map(|(p, r)| p.abs_diff(r) > 2)
                .unwrap_or(false);
        Self {
            column: value.column,
            python_nulls: value.python_nulls,
            rust_nulls: value.rust_nulls,
            python_distinct: value.python_distinct,
            rust_distinct: value.rust_distinct,
            highlighted,
        }
    }
}

#[derive(Serialize)]
struct RobustnessContext {
    summary: String,
    runs: Option<u32>,
    rows: Option<usize>,
    python: Option<RuntimeStats>,
    rust: Option<RuntimeStats>,
    scaling: Vec<ScalingPointDisplay>,
    has_scaling: bool,
}

impl From<RobustnessFile> for RobustnessContext {
    fn from(file: RobustnessFile) -> Self {
        let summary = match (file.python.as_ref(), file.rust.as_ref()) {
            (Some(py), Some(rs)) => format!(
                "Across {} runs (~{} rows) Rust averaged {:.1}s vs Python {:.1}s.",
                file.runs.unwrap_or(0),
                file.rows.unwrap_or(0),
                rs.mean_seconds,
                py.mean_seconds
            ),
            _ => "Populate both pipelines with repeated runs to analyze variance.".to_string(),
        };
        let has_scaling = !file.scaling.is_empty();
        let scaling = file
            .scaling
            .into_iter()
            .map(ScalingPointDisplay::from)
            .collect();
        Self {
            summary,
            runs: file.runs,
            rows: file.rows,
            python: file.python.map(RuntimeStats::from),
            rust: file.rust.map(RuntimeStats::from),
            scaling,
            has_scaling,
        }
    }
}

#[derive(Serialize)]
struct RuntimeStats {
    label: String,
    variance: String,
}

impl From<PipelineVariance> for RuntimeStats {
    fn from(value: PipelineVariance) -> Self {
        Self {
            label: format!("{:.1}", value.mean_seconds),
            variance: format!("{:.1}", value.std_seconds),
        }
    }
}

#[derive(Serialize)]
struct ScalingPointDisplay {
    rows_label: String,
    python_label: Option<String>,
    rust_label: Option<String>,
}

impl From<ScalingPoint> for ScalingPointDisplay {
    fn from(value: ScalingPoint) -> Self {
        Self {
            rows_label: format!("{} rows", value.rows),
            python_label: value.python_seconds.map(|s| format!("{:.1}s (Py)", s)),
            rust_label: value.rust_seconds.map(|s| format!("{:.1}s (Rust)", s)),
        }
    }
}

#[derive(Serialize)]
struct MethodsContext {
    bullets: Vec<&'static str>,
}

impl Default for MethodsContext {
    fn default() -> Self {
        Self {
            bullets: vec![
                "Data sources: CAD incidents + fire response extracts filtered from the latest exports.",
                "Both pipelines enforce the same schema, filters, and feature definitions.",
                "Rust handles I/O + feature engineering before writing Parquet; Python reuses the same modeling code.",
                "Cost estimates blend compute rates with analyst wait-time assumptions from config/cost_model.json.",
                "Charts + tables read the JSON artifacts under data/processed/ and py_model/artifacts/.",
            ],
        }
    }
}

#[derive(Serialize)]
struct HardwareSpec {
    cpu: &'static str,
    ram: &'static str,
}

impl Default for HardwareSpec {
    fn default() -> Self {
        Self {
            cpu: "Intel Core Ultra 7 155U",
            ram: "32 GB RAM",
        }
    }
}

#[derive(Deserialize)]
struct PrepFile {
    total_duration_ms: u128,
    apd: DatasetMetrics,
    lafd: DatasetMetrics,
}

#[derive(Deserialize)]
struct DatasetMetrics {
    rows_sampled: usize,
    duration_ms: u128,
    output_path: String,
}

#[derive(Serialize, Clone)]
struct PrepSummary {
    total_duration_ms: u128,
    apd_rows: usize,
    lafd_rows: usize,
    apd_duration_ms: u128,
    lafd_duration_ms: u128,
}

impl From<PrepFile> for PrepSummary {
    fn from(value: PrepFile) -> Self {
        Self {
            total_duration_ms: value.total_duration_ms,
            apd_rows: value.apd.rows_sampled,
            lafd_rows: value.lafd.rows_sampled,
            apd_duration_ms: value.apd.duration_ms,
            lafd_duration_ms: value.lafd.duration_ms,
        }
    }
}

impl PrepSummary {
    fn dataset_rows(&self, kind: DatasetKind) -> usize {
        match kind {
            DatasetKind::Apd => self.apd_rows,
            DatasetKind::Lafd => self.lafd_rows,
        }
    }

    fn dataset_duration_ms(&self, kind: DatasetKind) -> u128 {
        match kind {
            DatasetKind::Apd => self.apd_duration_ms,
            DatasetKind::Lafd => self.lafd_duration_ms,
        }
    }

    fn dataset_duration_seconds(&self, kind: DatasetKind) -> f64 {
        self.dataset_duration_ms(kind) as f64 / 1000.0
    }

    fn throughput(&self, kind: DatasetKind) -> f64 {
        let seconds = self.dataset_duration_seconds(kind).max(1.0);
        self.dataset_rows(kind) as f64 / seconds
    }
}

#[derive(Clone)]
struct PrepComparison {
    control: Option<PrepSummary>,
    variant: Option<PrepSummary>,
}

impl PrepComparison {
    fn card_copy(&self) -> String {
        match (&self.control, &self.variant) {
            (Some(control), Some(variant)) => format!(
                "Both pipelines prep roughly {} APD rows and {} fire rows. Rust mirrors the control logic so we can compare throughput, data-quality, and robustness.",
                variant.apd_rows.max(control.apd_rows),
                variant.lafd_rows.max(control.lafd_rows)
            ),
            _ => "We compare the Python control pipeline to a Rust-accelerated variant once both exports are available.".to_string(),
        }
    }

    fn build_tables(&self) -> Vec<PerformanceTable> {
        let mut tables = Vec::new();
        for dataset in [DatasetKind::Apd, DatasetKind::Lafd] {
            let rows = self
                .variant
                .as_ref()
                .map(|s| s.dataset_rows(dataset))
                .or_else(|| self.control.as_ref().map(|s| s.dataset_rows(dataset)));
            let mut stats = Vec::new();
            let control_seconds = self
                .control
                .as_ref()
                .map(|summary| summary.dataset_duration_seconds(dataset));
            stats.push(PipelinePerfRow::from_summary(
                "Python Control",
                self.control.as_ref(),
                dataset,
                None,
            ));
            stats.push(PipelinePerfRow::from_summary(
                "Rust Variant",
                self.variant.as_ref(),
                dataset,
                control_seconds,
            ));

            if stats
                .iter()
                .all(|row| row.wall_time.is_none() && row.throughput.is_none())
            {
                continue;
            }

            tables.push(PerformanceTable {
                title: dataset.title(),
                rows_label: rows
                    .map(|r| format!("{} rows", r))
                    .unwrap_or_else(|| "Rows pending".to_string()),
                stats,
            });
        }
        tables
    }
}

#[derive(Clone, Copy)]
enum DatasetKind {
    Apd,
    Lafd,
}

impl DatasetKind {
    fn title(&self) -> &'static str {
        match self {
            DatasetKind::Apd => "APD Prep Performance",
            DatasetKind::Lafd => "LAFD Prep Performance",
        }
    }
}

impl PipelinePerfRow {
    fn from_summary(
        label: &'static str,
        summary: Option<&PrepSummary>,
        dataset: DatasetKind,
        baseline_seconds: Option<f64>,
    ) -> Self {
        if let Some(summary) = summary {
            let wall_time = Some(format!("{:.1}s", summary.dataset_duration_seconds(dataset)));
            let throughput = Some(format!("{:.0} rows/s", summary.throughput(dataset)));
            let delta = if label == "Python Control" {
                "Baseline".to_string()
            } else if let Some(baseline) = baseline_seconds {
                let current = summary.dataset_duration_seconds(dataset);
                if baseline > 0.0 {
                    let ratio = baseline / current.max(0.001);
                    if ratio > 1.0 {
                        format!("{:.1}x faster", ratio)
                    } else {
                        format!("{:.1}x slower", 1.0 / ratio.max(0.001))
                    }
                } else {
                    "Baseline missing".to_string()
                }
            } else {
                "Awaiting baseline".to_string()
            };
            return Self {
                label,
                wall_time,
                throughput,
                delta,
            };
        }
        Self {
            label,
            wall_time: None,
            throughput: None,
            delta: "Awaiting run".to_string(),
        }
    }
}

#[derive(Deserialize)]
struct DataQualityFile {
    apd: Vec<DataQualityColumn>,
    lafd: Vec<DataQualityColumn>,
}

#[derive(Deserialize)]
struct DataQualityColumn {
    column: String,
    python_nulls: Option<u64>,
    rust_nulls: Option<u64>,
    python_distinct: Option<u64>,
    rust_distinct: Option<u64>,
}

#[derive(Deserialize)]
struct RobustnessFile {
    runs: Option<u32>,
    rows: Option<usize>,
    python: Option<PipelineVariance>,
    rust: Option<PipelineVariance>,
    #[serde(default)]
    scaling: Vec<ScalingPoint>,
}

#[derive(Deserialize)]
struct PipelineVariance {
    mean_seconds: f64,
    std_seconds: f64,
}

#[derive(Deserialize)]
struct ScalingPoint {
    rows: usize,
    python_seconds: Option<f64>,
    rust_seconds: Option<f64>,
}

#[derive(Deserialize)]
struct CostModelFile {
    compute_cost_per_hour: f64,
    analyst_cost_per_hour: f64,
    runs_per_day: f64,
}

#[derive(Deserialize)]
struct PhaseScores<T> {
    training: Option<T>,
    validation: Option<T>,
    testing: Option<T>,
}

#[derive(Deserialize)]
struct ClassificationScores {
    auc: Option<f64>,
    accuracy: Option<f64>,
    f1: Option<f64>,
}

#[derive(Deserialize)]
struct RegressionScores {
    mae: Option<f64>,
    rmse: Option<f64>,
    r2: Option<f64>,
}

#[derive(Deserialize)]
struct ApdMetricsFile {
    logistic_regression: PhaseScores<ClassificationScores>,
    xgboost: PhaseScores<ClassificationScores>,
}

#[derive(Serialize)]
struct ClassificationValidationTable {
    model_name: &'static str,
    rows: Vec<ClassificationPhaseRow>,
    status: String,
}

#[derive(Serialize)]
struct ClassificationPhaseRow {
    phase: &'static str,
    auc: Option<String>,
    accuracy: Option<String>,
    f1: Option<String>,
}

#[derive(Serialize)]
struct ApdSummary {
    logistic_auc: f64,
    logistic_accuracy: f64,
    xgb_auc: f64,
    xgb_accuracy: f64,
    tables: Vec<ClassificationValidationTable>,
}

impl From<ApdMetricsFile> for ApdSummary {
    fn from(value: ApdMetricsFile) -> Self {
        let logistic_table =
            build_classification_table("Logistic Regression", &value.logistic_regression);
        let xgb_table = build_classification_table("XGBoost Classifier", &value.xgboost);
        Self {
            logistic_auc: pick_classification_metric(&value.logistic_regression, |scores| {
                scores.auc
            })
            .unwrap_or_default(),
            logistic_accuracy: pick_classification_metric(&value.logistic_regression, |scores| {
                scores.accuracy
            })
            .unwrap_or_default(),
            xgb_auc: pick_classification_metric(&value.xgboost, |scores| scores.auc)
                .unwrap_or_default(),
            xgb_accuracy: pick_classification_metric(&value.xgboost, |scores| scores.accuracy)
                .unwrap_or_default(),
            tables: vec![logistic_table, xgb_table],
        }
    }
}

impl ApdSummary {
    fn logistic_auc_label(&self) -> String {
        format!("{:.3}", self.logistic_auc)
    }

    fn xgb_auc_label(&self) -> String {
        format!("{:.3}", self.xgb_auc)
    }

    fn logistic_accuracy_pct(&self) -> String {
        format!("{:.1}", self.logistic_accuracy * 100.0)
    }

    fn xgb_accuracy_pct(&self) -> String {
        format!("{:.1}", self.xgb_accuracy * 100.0)
    }
}

#[derive(Deserialize)]
struct LafdMetricsFile {
    regression: PhaseScores<RegressionScores>,
    #[serde(default)]
    bucket_classifier: Option<PhaseScores<ClassificationScores>>,
}

#[derive(Serialize)]
struct RegressionValidationTable {
    model_name: &'static str,
    rows: Vec<RegressionPhaseRow>,
    status: String,
}

#[derive(Serialize)]
struct RegressionPhaseRow {
    phase: &'static str,
    mae: Option<String>,
    rmse: Option<String>,
    r2: Option<String>,
}

#[derive(Serialize)]
struct LafdSummary {
    mae: f64,
    rmse: f64,
    r2: f64,
    bucket_accuracy: Option<f64>,
    bucket_f1: Option<f64>,
    regression_table: RegressionValidationTable,
    bucket_table: Option<ClassificationValidationTable>,
}

struct BucketSummaryText {
    accuracy: String,
    f1: String,
}

impl From<LafdMetricsFile> for LafdSummary {
    fn from(value: LafdMetricsFile) -> Self {
        let regression_table = build_regression_table("LAFD Regression", &value.regression);
        let bucket_table = value
            .bucket_classifier
            .as_ref()
            .map(|table| build_classification_table("LAFD Bucket Classifier", table));
        Self {
            mae: pick_regression_metric(&value.regression, |scores| scores.mae).unwrap_or_default(),
            rmse: pick_regression_metric(&value.regression, |scores| scores.rmse)
                .unwrap_or_default(),
            r2: pick_regression_metric(&value.regression, |scores| scores.r2).unwrap_or_default(),
            bucket_accuracy: value
                .bucket_classifier
                .as_ref()
                .and_then(|scores| pick_classification_metric(scores, |s| s.accuracy)),
            bucket_f1: value
                .bucket_classifier
                .as_ref()
                .and_then(|scores| pick_classification_metric(scores, |s| s.f1)),
            regression_table,
            bucket_table,
        }
    }
}

impl LafdSummary {
    fn mae_label(&self) -> String {
        format!("{:.1}", self.mae)
    }

    fn rmse_label(&self) -> String {
        format!("{:.1}", self.rmse)
    }

    fn r2_label(&self) -> String {
        format!("{:.3}", self.r2)
    }

    fn bucket_summary(&self) -> Option<BucketSummaryText> {
        Some(BucketSummaryText {
            accuracy: format!("{:.1}", self.bucket_accuracy? * 100.0),
            f1: format!("{:.1}", self.bucket_f1? * 100.0),
        })
    }
}

fn pick_classification_metric(
    phases: &PhaseScores<ClassificationScores>,
    extractor: fn(&ClassificationScores) -> Option<f64>,
) -> Option<f64> {
    phases
        .testing
        .as_ref()
        .and_then(extractor)
        .or_else(|| phases.validation.as_ref().and_then(extractor))
        .or_else(|| phases.training.as_ref().and_then(extractor))
}

fn pick_regression_metric(
    phases: &PhaseScores<RegressionScores>,
    extractor: fn(&RegressionScores) -> Option<f64>,
) -> Option<f64> {
    phases
        .testing
        .as_ref()
        .and_then(extractor)
        .or_else(|| phases.validation.as_ref().and_then(extractor))
        .or_else(|| phases.training.as_ref().and_then(extractor))
}

fn build_classification_table(
    name: &'static str,
    phases: &PhaseScores<ClassificationScores>,
) -> ClassificationValidationTable {
    let mut rows = Vec::new();
    if let Some(value) = phases.training.as_ref() {
        rows.push(ClassificationPhaseRow::from("Training", value));
    }
    if let Some(value) = phases.validation.as_ref() {
        rows.push(ClassificationPhaseRow::from("Validation", value));
    }
    if let Some(value) = phases.testing.as_ref() {
        rows.push(ClassificationPhaseRow::from("Testing", value));
    }
    let status = classify_overfit_status(phases);
    ClassificationValidationTable {
        model_name: name,
        rows,
        status,
    }
}

impl ClassificationPhaseRow {
    fn from(phase: &'static str, scores: &ClassificationScores) -> Self {
        Self {
            phase,
            auc: scores.auc.map(|v| format!("{:.3}", v)),
            accuracy: scores.accuracy.map(|v| format!("{:.1}%", v * 100.0)),
            f1: scores.f1.map(|v| format!("{:.1}%", v * 100.0)),
        }
    }
}

fn classify_overfit_status(phases: &PhaseScores<ClassificationScores>) -> String {
    let training = phases.training.as_ref().and_then(|scores| scores.accuracy);
    let testing = phases.testing.as_ref().and_then(|scores| scores.accuracy);
    match (training, testing) {
        (Some(train), Some(test)) => {
            let diff = (train - test).abs() * 100.0;
            if diff <= 2.0 {
                "Splits aligned: training vs testing accuracy within 2%.".to_string()
            } else if train > test {
                format!(
                    "Watch for overfit: train {:.1}% vs test {:.1}%.",
                    train * 100.0,
                    test * 100.0
                )
            } else {
                format!(
                    "Testing is stronger ({:.1}% vs {:.1}%). Confirm sampling.",
                    test * 100.0,
                    train * 100.0
                )
            }
        }
        _ => "Collect all three splits to evaluate fit.".to_string(),
    }
}

fn build_regression_table(
    name: &'static str,
    phases: &PhaseScores<RegressionScores>,
) -> RegressionValidationTable {
    let mut rows = Vec::new();
    if let Some(value) = phases.training.as_ref() {
        rows.push(RegressionPhaseRow::from("Training", value));
    }
    if let Some(value) = phases.validation.as_ref() {
        rows.push(RegressionPhaseRow::from("Validation", value));
    }
    if let Some(value) = phases.testing.as_ref() {
        rows.push(RegressionPhaseRow::from("Testing", value));
    }
    let status = regression_overfit_status(phases);
    RegressionValidationTable {
        model_name: name,
        rows,
        status,
    }
}

impl RegressionPhaseRow {
    fn from(phase: &'static str, scores: &RegressionScores) -> Self {
        Self {
            phase,
            mae: scores.mae.map(|v| format!("{:.1}s", v)),
            rmse: scores.rmse.map(|v| format!("{:.1}s", v)),
            r2: scores.r2.map(|v| format!("{:.3}", v)),
        }
    }
}

fn regression_overfit_status(phases: &PhaseScores<RegressionScores>) -> String {
    let training = phases.training.as_ref().and_then(|scores| scores.mae);
    let testing = phases.testing.as_ref().and_then(|scores| scores.mae);
    match (training, testing) {
        (Some(train), Some(test)) => {
            let diff = (train - test).abs();
            if diff <= 5.0 {
                "MAE stays within five seconds across splits.".to_string()
            } else if train < test {
                format!(
                    "Testing MAE grew by {:.1}s vs training; verify generalization.",
                    diff
                )
            } else {
                format!(
                    "Testing MAE improved by {:.1}s; confirm consistent sampling.",
                    diff
                )
            }
        }
        _ => "Collect training + testing regression stats.".to_string(),
    }
}

struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("{:?}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
    }
}

fn format_currency(value: f64) -> String {
    if value >= 0.0 {
        format!("${:.2}", value)
    } else {
        format!("-${:.2}", value.abs())
    }
}
