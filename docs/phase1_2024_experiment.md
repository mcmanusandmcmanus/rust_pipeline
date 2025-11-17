# Phase 1 – 2024-Only Rust vs. Python Experiment

Author: ML Ops Lead  
Last updated: 2025‑11‑16 22:00 CST

## 1. Purpose & Scope

This phase is a **research-grade experiment** to validate whether swapping our Python feature engineering pipeline for Rust delivers measurable wins on **public safety CAD/response data**. To keep the problem bounded and repeatable, we restrict every run to **calendar year 2024** for both APD and LAFD datasets.

Goals:

- Demonstrate Rust prep speed and reliability vs. the legacy Python path on the exact same 2024 rows.
- Quantify end-to-end runtime, cost, and variance across repeated runs.
- Prove that the Rust-prepped feature tables are schema-compatible with the current Python modeling layer.

Out of scope for this phase: multi-year ingestion, full 5–7M row volumes, geo enrichment, and advanced feature engineering. Those re-enter the roadmap only after this phase is stable.

## 2. Pipelines Under Test

| Pipeline | Purpose | Implementation |
| --- | --- | --- |
| **Control** | Baseline timing/quality | `py_prep/` (Python) → `py_model/` |
| **Variable** | Rust prep hypothesis | `rust_prep/` (Rust) → `py_model/` |

Both pipelines ingest the same 2024-filtered slices and emit Parquet + JSON artifacts with identical schemas, so the shared modeling module can load either output without code changes.

## 3. Data Rules (Apply in Both Pipelines)

- **Source files**:  
  `APD_Computer_Aided_Dispatch_Incidents_20251101.csv`  
  `LAFD_Response_Metrics_-_Raw_Data_20251101.csv`
- **Filter logic**:  
  - APD → `Response Year == 2024` (or `year(Response Datetime) == 2024`).  
  - LAFD → `year(Incident Creation Time (GMT)) == 2024`.
- **Row caps**: start with **≤1.5M rows per dataset** (stratified sampling if needed). Raise the cap only after runtime/memory headroom is proven.
- **Schema subset**: use the reduced column list from `docs/data_profile.md` to minimize IO.

## 4. Pipeline Details

### 4.1 Python Prep (`py_prep/`)

1. Load raw CSVs with pandas/polars.
2. Filter to 2024 rows and select the agreed subset of columns.
3. Optional stratified sampling to respect the 1.5M-row cap.
4. Feature engineering parity with Rust: timestamp parsing, duration math, categorical normalization, stratification keys, etc.
5. Emit Parquet feature tables + prep benchmark JSON → `data/processed/...`.
6. Log run metadata to `benchmarks/prep_history_2024.json` (see Section 6).

### 4.2 Rust Prep (`rust_prep/`)

Same semantics as Python prep, implemented with Polars streaming:

```powershell
cargo run --release -- ^
  --apd-file ..\data\raw\APD_Computer_Aided_Dispatch_Incidents_20251101.csv ^
  --lafd-file ..\data\raw\LAFD_Response_Metrics_-_Raw_Data_20251101.csv ^
  --year-filter 2024 ^
  --row-cap 1500000 ^
  --output-dir ..\data\processed
```

Key deliverables per run:

- `apd_2024_sample.parquet`, `lafd_2024_sample.parquet`
- `category_maps_2024.json`
- `prep_rust_bench_2024.json` (timings + row counts)

### 4.3 Shared Modeling (`py_model/`)

Both prep paths terminate in the existing modeling CLI:

```powershell
python -m venv .venv
.\.venv\Scripts\pip install -e .
.\.venv\Scripts\python src\train.py ^
  --apd-path ..\data\processed\apd_2024_sample.parquet ^
  --lafd-path ..\data\processed\lafd_2024_sample.parquet ^
  --category-map ..\data\processed\category_maps_2024.json ^
  --artifacts-dir artifacts\control_or_rust ^
  --seed 42
```

Outputs (per dataset, per pipeline type):

- `*_metrics.json` with train/validation/test splits, recall/F1/accuracy/AUC.
- ROC + confusion-matrix PNGs.
- Feature-importance JSON/PNGs.
- `model_history_2024.json` entry (Section 6).

## 5. Sampling & Capacity Strategy

- Begin with the full 2024 slice. If `rows > 1.5M`, perform stratified sampling by `priority` (APD) or `dispatch status` (LAFD) to maintain class balance.
- All sampling must be reproducible: persist RNG seeds per run and log them with the benchmark records.
- If both pipelines stay under 15 minutes and <24 GB RAM on the reference laptop, raise the cap to 2.5M rows and repeat the measurements.

## 6. Benchmark & Cost Logging

Append a JSON record after every prep/model run:

- `benchmarks/prep_history_2024.json`
- `benchmarks/model_history_2024.json`

Each record must include:

| Field | Example |
| --- | --- |
| `run_id` | `rust-prep-2024-2025-11-16T2200Z` |
| `pipeline_type` | `python_prep` or `rust_prep` |
| `dataset` | `apd` or `lafd` |
| `rows_input` / `rows_output` | integer |
| `duration_seconds` | float |
| `cpu_peak_pct` / `mem_peak_gb` | floats |
| `row_cap_applied` | boolean |
| `cost_estimate_usd` | derived via `config/cost_model.json` |
| `git_rev` | `git rev-parse HEAD` |

Use these logs to populate the dashboard and the executive summary once phase 1 wraps.

## 7. Execution Playbook

1. **Calibrate row caps** – Determine the max 2024 row volume that holds under 32 GB RAM; document the threshold.
2. **Run both pipelines** – Execute control and variable prep for APD/LAFD; capture artifacts + logs.
3. **Repeatability** – Run 3–5 repetitions per combination (dataset × pipeline) to capture variance.
4. **Model training** – Feed each feature set through `py_model` to collect metrics.
5. **Dashboard refresh** – Update the Rust webapp’s JSON paths/env vars to point at the latest artifacts and highlight “2024-only experiment” language.
6. **Review & decide** – Summarize performance, quality, and cost deltas. Approve/deny scaling to multi-year/full-volume experiments.

## 8. Dashboard Messaging

- Hero text and tooltips must explicitly mention the **2024-only** scope.
- Display prep throughput, APD AUC/recall, and LAFD regression metrics separately for control vs. experiment once both artifacts exist.
- `/api/metrics` should expose `year_filter: "2024"` and `pipeline_type` metadata so downstream systems can distinguish runs.

## 9. Roadmap Beyond Phase 1

Once the 2024 experiment proves stable and beneficial:

1. Raise row caps within 2024 if hardware allows.
2. Expand to 2023–2024, then 2022–2024 slices.
3. Revisit full raw CSV volumes (5M+ rows) with upgraded hardware or cloud runners.
4. Layer on richer features (geo joins, injury severity, temporal seasonality).
5. Automate nightly benchmarks + dashboards, using this phase as the regression baseline.

Keep every future phase comparable back to the 2024 baseline via consistent logging and metadata tagging.
