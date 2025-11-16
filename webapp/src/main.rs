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
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
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
    apd_metrics_path: PathBuf,
    lafd_metrics_path: PathBuf,
}

impl AppConfig {
    fn from_env() -> Result<Self> {
        let port = env::var("WEBAPP_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(8080);
        let bind_addr = SocketAddr::from(([127, 0, 0, 1], port));
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
        let prep_value = read_json(&self.config.prep_path).await;
        let apd_value = read_json(&self.config.apd_metrics_path).await;
        let lafd_value = read_json(&self.config.lafd_metrics_path).await;

        let prep_summary = prep_value
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

        let raw_metrics = json!({
            "prep": prep_value,
            "apd": apd_value,
            "lafd": lafd_value
        });

        let metrics_b64 =
            STANDARD_NO_PAD.encode(serde_json::to_vec(&raw_metrics).unwrap_or_default());

        let template = DashboardTemplateData {
            hardware: HardwareSpec::default(),
            prep: prep_summary,
            apd: apd_summary,
            lafd: lafd_summary,
            metrics_b64,
        };

        Ok(DashboardPayload {
            template,
            raw_metrics,
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
    hardware: HardwareSpec,
    prep: Option<PrepSummary>,
    apd: Option<ApdSummary>,
    lafd: Option<LafdSummary>,
    metrics_b64: String,
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
    fn apd_seconds_label(&self) -> String {
        format!("{:.1}", self.apd_duration_ms as f64 / 1000.0)
    }

    fn lafd_seconds_label(&self) -> String {
        format!("{:.1}", self.lafd_duration_ms as f64 / 1000.0)
    }
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
    xgb_auc: f64,
    xgb_accuracy: f64,
}

impl From<ApdMetricsFile> for ApdSummary {
    fn from(value: ApdMetricsFile) -> Self {
        Self {
            logistic_auc: value.logistic_regression.auc.unwrap_or_default(),
            logistic_accuracy: value.logistic_regression.accuracy.unwrap_or_default(),
            xgb_auc: value.xgboost.auc.unwrap_or_default(),
            xgb_accuracy: value.xgboost.accuracy.unwrap_or_default(),
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
