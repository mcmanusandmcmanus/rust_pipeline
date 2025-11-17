# Rust Pipeline (Rust + Python + XGBoost)

End-to-end demo that ingests multi-GB public safety CSVs with Rust, prepares modeling features, benchmarks Python/XGBoost models, and surfaces the story through a lightweight Rust web dashboard.

> The raw CSVs are **not** committed. Drop them at the repo root (or update the flags below). The dashboard now ships with baked-in sample metrics so you can demo the visuals even before running the heavy prep/model stages.

## Repo Layout

| Path | Description |
| --- | --- |
| `docs/` | Planning notes, schema profiles, sampling strategy. |
| `rust_prep/` | Rust CLI (Polars) that streams the raw CSVs, engineers features, stratified-samples ~250k rows per dataset, and emits Parquet + benchmark JSON. |
| `py_model/` | Python Typer CLI that trains Logistic/XGBoost (APD) and regression/classifier (LAFD), producing metrics + plots in `artifacts/`. |
| `webapp/` | Axum + Askama dashboard that reads the JSON artifacts (or its own sample bundle) and renders the “Rust engine, Python lab coat” narrative with Plotly charts. |

## Quick Start (full pipeline)

1. **Rust feature prep**
   ```powershell
   cd rust_prep
   cargo run --release -- ^
     --apd-file ..\APD_Computer_Aided_Dispatch_Incidents_20251101.csv ^
     --lafd-file ..\LAFD_Response_Metrics_-_Raw_Data_20251101.csv ^
     --output-dir ..\data\processed
   ```
   Outputs: `data/processed/apd_sample.parquet`, `lafd_sample.parquet`, `category_maps.json`, `prep_rust_bench.json`.

2. **Python modeling**
   ```powershell
   cd py_model
   python -m venv .venv
   .\.venv\Scripts\pip install --upgrade pip
   .\.venv\Scripts\pip install -e .
   .\.venv\Scripts\python src\train.py ^
     --apd-path ..\data\processed\apd_sample.parquet ^
     --lafd-path ..\data\processed\lafd_sample.parquet ^
     --category-map ..\data\processed\category_maps.json ^
     --artifacts-dir artifacts
   ```
   Outputs JSON metrics + ROC/feature-importance plots under `py_model/artifacts/`.

3. **Story dashboard**
   ```powershell
   cd webapp
   cargo run
   ```
   Visit <http://127.0.0.1:8080>. Export `/api/metrics` if you need raw data for other dashboards/agents. Adjust env vars (`WEBAPP_PORT`, `PREP_BENCH_PATH`, etc.) as needed.

If the JSON artifacts aren’t present, the dashboard shows its built-in sample story and labels it clearly so viewers know they’re seeing demo data.

## Phase 1 – 2024-Only Experiment

Planning the Rust-vs-Python bake-off on the 2024 subset? Follow `docs/phase1_2024_experiment.md`. Key points:

- Filter both APD and LAFD to **calendar year 2024** before any heavy lifting; keep the initial cap around **1.5M rows per dataset** until you verify headroom.
- Treat `py_prep/` + `py_model/` as the control path and `rust_prep/` + `py_model/` as the experimental path; both must emit schema-compatible Parquet + JSON so the modeling layer stays unchanged.
- Log every prep/model run to `benchmarks/*_history_2024.json` with runtime, resource, cost, and git metadata; those records feed the dashboard and the cost model.
- Update the dashboard copy + `/api/metrics` payloads to label “2024-only experiment” and surface `pipeline_type`/`year_filter` metadata once both artifact sets exist.

This 2024 baseline is the regression target before expanding to multi-year or full-volume datasets.

## Deploying the dashboard to Render

The repo includes a multi-stage Dockerfile plus `render.yaml`. Render auto-detects and builds the Rust binary, so you can share the hosted URL quickly:

1. Push this repo to GitHub (or your fork) so Render can pull it.
2. In Render, choose **New > Blueprint** and point it at the repository. Render reads `render.yaml` and provisions a single web service.
3. The provided Docker image binds to `$PORT` automatically; no extra configuration is needed. The baked-in sample metrics ensure the landing page and Plotly charts are populated immediately.
4. Once the service is live, share the Render URL with your nephew. When you’re ready to swap in real metrics, upload the JSON artifacts with a deploy hook or persistent disk and set `PREP_BENCH_PATH`, `APD_METRICS_PATH`, and `LAFD_METRICS_PATH` as service env vars.

## Hardware Envelope

- CPU: Intel Core Ultra 7 155U @ 1.70 GHz
- RAM: 32 GB (≈31 GB usable)

This setup comfortably handles the 1 GB CSVs with Rust streaming + 250k-row XGBoost experiments without stressing the laptop.
