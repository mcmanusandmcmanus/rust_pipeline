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
use tracing::{Level, info, warn};

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
    apd_metrics_path: PathBuf,
    lafd_metrics_path: PathBuf,
}

impl AppConfig {
    fn from_env() -> Result<Self> {
        let port = env::var("PORT")
            .or_else(|_| env::var("WEBAPP_PORT"))
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
        let apd_path = env::var("APD_METRICS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../py_model/artifacts/apd_metrics.json"));
        let lafd_path = env::var("LAFD_METRICS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("../py_model/artifacts/lafd_metrics.json"));
        Ok(Self {
            bind_addr,
            prep_path,
            apd_metrics_path: apd_path,
            lafd_metrics_path: lafd_path,
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
        let prep_blob =
            load_or_sample(&self.config.prep_path, "prep benchmarks", sample_data::prep).await;
        let apd_blob = load_or_sample(
            &self.config.apd_metrics_path,
            "APD metrics",
            sample_data::apd,
        )
        .await;
        let lafd_blob = load_or_sample(
            &self.config.lafd_metrics_path,
            "LAFD metrics",
            sample_data::lafd,
        )
        .await;

        let prep_summary = serde_json::from_value::<PrepFile>(prep_blob.value.clone())
            .ok()
            .map(PrepSummary::from);
        let apd_summary = serde_json::from_value::<ApdMetricsFile>(apd_blob.value.clone())
            .ok()
            .map(ApdSummary::from);
        let lafd_summary = serde_json::from_value::<LafdMetricsFile>(lafd_blob.value.clone())
            .ok()
            .map(LafdSummary::from);

        let raw_metrics = json!({
            "prep": prep_blob.value,
            "apd": apd_blob.value,
            "lafd": lafd_blob.value
        });

        let metrics_b64 = STANDARD.encode(serde_json::to_vec(&raw_metrics).unwrap_or_default());

        let sample_mode = prep_blob.used_sample || apd_blob.used_sample || lafd_blob.used_sample;

        let template = DashboardTemplateData {
            hardware: HardwareSpec::default(),
            prep: prep_summary,
            apd: apd_summary,
            lafd: lafd_summary,
            metrics_b64,
            sample_mode,
        };

        Ok(DashboardPayload {
            template,
            raw_metrics,
        })
    }
}

struct LoadedValue {
    value: Value,
    used_sample: bool,
}

async fn load_or_sample<F>(path: &Path, label: &str, sample: F) -> LoadedValue
where
    F: FnOnce() -> Value,
{
    match read_json(path).await {
        Ok(value) => LoadedValue {
            value,
            used_sample: false,
        },
        Err(err) => {
            warn!(
                target: "dashboard",
                error = %err,
                path = %path.display(),
                "{} missing; using baked sample metrics",
                label
            );
            LoadedValue {
                value: sample(),
                used_sample: true,
            }
        }
    }
}

async fn read_json(path: &Path) -> Result<Value> {
    let contents = fs::read_to_string(path).await?;
    let value = serde_json::from_str(&contents)?;
    Ok(value)
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
    hardware: HardwareSpec,
    prep: Option<PrepSummary>,
    apd: Option<ApdSummary>,
    lafd: Option<LafdSummary>,
    metrics_b64: String,
    sample_mode: bool,
}

#[derive(Serialize)]
struct HardwareSpec {
    cpu: &'static str,
    ram: &'static str,
    tagline: &'static str,
    narrative: &'static str,
}

impl Default for HardwareSpec {
    fn default() -> Self {
        Self {
            cpu: "Intel Core Ultra 7 155U @ 1.70 GHz",
            ram: "32 GB RAM (31.4 GB usable)",
            tagline: "Rust is my data engine, Python is my lab coat.",
            narrative: "Rust chews through the multi-GB CSVs, Python runs the lab-grade comparisons, and the dashboard stitches the story together for execs and ops.",
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

#[derive(Serialize)]
struct PrepSummary {
    total_duration_ms: u128,
    apd_rows: usize,
    lafd_rows: usize,
    apd_duration_ms: u128,
    lafd_duration_ms: u128,
    apd_output: String,
    lafd_output: String,
}

impl From<PrepFile> for PrepSummary {
    fn from(value: PrepFile) -> Self {
        Self {
            total_duration_ms: value.total_duration_ms,
            apd_rows: value.apd.rows_sampled,
            lafd_rows: value.lafd.rows_sampled,
            apd_duration_ms: value.apd.duration_ms,
            lafd_duration_ms: value.lafd.duration_ms,
            apd_output: value.apd.output_path,
            lafd_output: value.lafd.output_path,
        }
    }
}

impl PrepSummary {
    fn apd_seconds_label(&self) -> String {
        format!("{:.1}", self.apd_duration_ms as f64 / 1000.0)
    }

    fn lafd_seconds_label(&self) -> String {
        format!("{:.1}", self.lafd_duration_ms as f64 / 1000.0)
    }

    fn apd_output_hint(&self) -> String {
        dataset_output_hint(&self.apd_output)
    }

    fn lafd_output_hint(&self) -> String {
        dataset_output_hint(&self.lafd_output)
    }
}

fn dataset_output_hint(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.to_string())
}

#[derive(Deserialize)]
struct ModelScores {
    auc: Option<f64>,
    accuracy: Option<f64>,
    f1: Option<f64>,
}

#[derive(Deserialize)]
struct ApdMetricsFile {
    logistic_regression: ModelScores,
    xgboost: ModelScores,
}

#[derive(Serialize)]
struct ApdSummary {
    logistic_auc: f64,
    logistic_accuracy: f64,
    logistic_f1: f64,
    xgb_auc: f64,
    xgb_accuracy: f64,
    xgb_f1: f64,
}

impl From<ApdMetricsFile> for ApdSummary {
    fn from(value: ApdMetricsFile) -> Self {
        Self {
            logistic_auc: value.logistic_regression.auc.unwrap_or_default(),
            logistic_accuracy: value.logistic_regression.accuracy.unwrap_or_default(),
            logistic_f1: value.logistic_regression.f1.unwrap_or_default(),
            xgb_auc: value.xgboost.auc.unwrap_or_default(),
            xgb_accuracy: value.xgboost.accuracy.unwrap_or_default(),
            xgb_f1: value.xgboost.f1.unwrap_or_default(),
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

    fn logistic_f1_pct(&self) -> String {
        format!("{:.1}", self.logistic_f1 * 100.0)
    }

    fn xgb_f1_pct(&self) -> String {
        format!("{:.1}", self.xgb_f1 * 100.0)
    }
}

#[derive(Deserialize)]
struct LafdRegression {
    mae: f64,
    rmse: f64,
    r2: f64,
}

#[derive(Deserialize)]
struct LafdBucket {
    accuracy: Option<f64>,
    #[serde(rename = "f1_macro")]
    f1_macro: Option<f64>,
}

#[derive(Deserialize)]
struct LafdMetricsFile {
    regression: LafdRegression,
    #[serde(default)]
    bucket_classifier: Option<LafdBucket>,
}

#[derive(Serialize)]
struct LafdSummary {
    mae: f64,
    rmse: f64,
    r2: f64,
    bucket_accuracy: Option<f64>,
    bucket_f1: Option<f64>,
}

struct BucketSummaryText {
    accuracy: String,
    f1: String,
}

impl From<LafdMetricsFile> for LafdSummary {
    fn from(value: LafdMetricsFile) -> Self {
        Self {
            mae: value.regression.mae,
            rmse: value.regression.rmse,
            r2: value.regression.r2,
            bucket_accuracy: value.bucket_classifier.as_ref().and_then(|b| b.accuracy),
            bucket_f1: value.bucket_classifier.as_ref().and_then(|b| b.f1_macro),
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

mod sample_data {
    use serde_json::{Value, json};

    pub fn prep() -> Value {
        json!({
            "total_duration_ms": 61842,
            "apd": {
                "rows_sampled": 250_000,
                "duration_ms": 27_400,
                "output_path": "data/processed/apd_sample.parquet"
            },
            "lafd": {
                "rows_sampled": 240_000,
                "duration_ms": 33_950,
                "output_path": "data/processed/lafd_sample.parquet"
            }
        })
    }

    pub fn apd() -> Value {
        json!({
            "logistic_regression": {
                "auc": 0.842,
                "accuracy": 0.781,
                "f1": 0.744
            },
            "xgboost": {
                "auc": 0.918,
                "accuracy": 0.846,
                "f1": 0.812
            }
        })
    }

    pub fn lafd() -> Value {
        json!({
            "regression": {
                "mae": 38.7,
                "rmse": 61.3,
                "r2": 0.741
            },
            "bucket_classifier": {
                "accuracy": 0.716,
                "f1_macro": 0.684
            }
        })
    }
}
