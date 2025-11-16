# Rust Pipeline (Rust + Python + XGBoost)

End-to-end demo that ingests multi-GB public safety CSVs with Rust, prepares modeling features, benchmarks Python/XGBoost models, and surfaces the story through a lightweight Rust web dashboard.

## Repo Layout

| Path | Description |
| --- | --- |
| `docs/` | Planning notes, schema profiles, sampling strategy. |
| `rust_prep/` | Rust CLI (Polars) that streams the raw CSVs, engineers features, stratified-samples ~250k rows per dataset, and emits Parquet + benchmark JSON. |
| `py_model/` | Python Typer CLI that trains Logistic/XGBoost (APD) and regression/classifier (LAFD), producing metrics + plots in `artifacts/`. |
| `webapp/` | Axum + Askama dashboard that reads the JSON artifacts and renders the “Rust engine, Python lab coat” narrative with Plotly charts. |

## Quick Start

> ⚠️ The raw CSVs are **not** committed. Drop them at the repo root (or update the flags below).

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

Hardware envelope (documented in `docs/`):

- CPU: Intel Core Ultra 7 155U @ 1.70 GHz
- RAM: 32 GB (≈29 GB usable)

This setup comfortably handles the 1 GB CSVs with Rust streaming + 250k-row XGBoost experiments without stressing the laptop.
