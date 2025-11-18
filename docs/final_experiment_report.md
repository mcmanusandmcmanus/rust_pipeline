# Final Experiment Run - Rust vs Python Prep

## Runtime + Pipeline Checks
- **Rust prep:** `cargo run --release -- --apd-file ..\\APD_Computer_Aided_Dispatch_Incidents_20251101.csv --lafd-file ..\\LAFD_Response_Metrics_-_Raw_Data_20251101.csv --output-dir ..\\data\\processed --apd-sample 120000 --lafd-sample 120000 --seed 42`
  - APD rows filtered: 1,896,832 -> sampled 120k in 3.98 s.
  - LAFD rows filtered: 6,090,465 -> sampled 120k in 6.82 s.
  - Benchmark JSON: `data/processed/prep_rust_bench.json`.
- **Python control prep:** `python3 -m py_prep.cli control-prep --apd-sample 120000 --lafd-sample 120000 --seed 42`
  - APD rows filtered: 1,896,832 -> sampled 120k in 34.75 s.
  - LAFD rows filtered: 6,090,465 -> sampled 120k in 26.12 s.
  - Artifacts: `apd_control_sample.parquet`, `lafd_control_sample.parquet`, `category_maps_control.json`, `prep_python_bench.json`.
- **Python modeling:** `.venv\\Scripts\\python src\\train.py --apd-path ..\\data\\processed\\apd_sample.parquet --lafd-path ..\\data\\processed\\lafd_sample.parquet --category-map ..\\data\\processed\\category_maps.json --artifacts-dir artifacts --seed 42`.
  - Produces refreshed plots + `apd_metrics.json` / `lafd_metrics.json`.
- **Compilation checks:** `cargo check` for both `rust_prep` and `webapp` pass after dependency bumps (polars 0.52.x with timezone + dtype-u8 support).

## Model Metrics Snapshot (py_model/artifacts)
| Dataset | Model | Accuracy | AUC/R^2 | Notes |
|---|---|---|---|---|
| APD | Logistic Regression | 0.804 | AUC 0.863 | Balanced precision: 0.89 (no-report) vs 0.60 (report). |
| APD | XGBoost | 0.811 | AUC 0.864 | Gains recall on the minority ("report written") class. |
| LAFD | Gradient Boosted Regression | - | R^2 0.623, MAE 26 s | Response-time regression across 120k incidents. |
| LAFD | Bucket Classifier | 0.988 | F1_macro 0.988 | Buckets: fast/typical/slow (class counts logged in JSON). |

Artifacts include ROC curves, confusion matrices, feature-importance JSON/PNGs, and scatter plots for regression residuals.

## Data EDA highlights (docs/eda_snapshot.txt)
- **APD sample:** 120k calls, 25.7% "report written". Response times avg 32 min, long tail to 1,943 min. 90% of units arrive in <=30 min. Distribution of incident types: 96.7% dispatched vs 3.3% officer-initiated. Mental-health flag ~11.6% of cases.
- **LAFD sample:** 120k runs, avg dispatch delay 125 s, total response 521 s with heavy-tailed delays (max 18 hours). Unit mix dominated by engines and ALS/BLS ambulances. Dispatch status heavily skewed toward QTR (quarters) launches.

## Documentation + Math sanity checks
- `docs/eda_snapshot.txt` stores `.info()` output, descriptive statistics, and top-k categorical counts for each dataset.
- Cost model (`config/cost_model.json`) now feeds the web dashboard; the control prep automatically appends python timings + cost estimates at `benchmarks/prep_history_2024.json` via `py_prep`.
- Stratified sampling keeps `unit_type` when enabling `include_key=true` in Polars 0.52; regression/bucket splits now encode categorical labels so the XGBoost multi-class objective sees [0,1,2] instead of strings.
- Console logging uses ASCII (no "?") to run cleanly under Windows codepages.
- `python3 -m py_prep.cli compare-quality` and `summarize-variance` rebuild `data/processed/data_quality_summary.json` and `run_variance_summary.json` from the latest Parquet + history instead of static placeholders.

## Outstanding gaps / recommended next steps
1. Wire the Rust prep binary into the shared run-history appender so we no longer have to invoke `py_prep.cli ingest-run` manually after each release build.
2. Add automated parity asserts (row counts, summary stats) between `apd_sample.parquet` and `apd_control_sample.parquet` in CI to flag regressions.
3. Optionally pin Polars features via a workspace `config.toml` to speed up future compiles.
4. Expand webapp `/api/metrics` verification with an integration test (e.g., `cargo test` hitting handlers with fixture JSON) now that both prep artifacts exist.
