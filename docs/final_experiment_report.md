# Final Experiment Run – Rust vs Python Prep

## Runtime + Pipeline Checks
- **Rust prep:** cargo run --release -- --apd-file ..\\APD_Computer_Aided_Dispatch_Incidents_20251101.csv --lafd-file ..\\LAFD_Response_Metrics_-_Raw_Data_20251101.csv --output-dir ..\\data\\processed --apd-sample 120000 --lafd-sample 120000 --seed 42
  - APD rows filtered: 1,896,832 ? sampled 120k in 3.98s.
  - LAFD rows filtered: 6,090,465 ? sampled 120k in 6.82s.
  - Benchmark JSON: data/processed/prep_rust_bench.json.
- **Python modeling:** .venv\\Scripts\\python src\\train.py --apd-path ..\\data\\processed\\apd_sample.parquet --lafd-path ..\\data\\processed\\lafd_sample.parquet --category-map ..\\data\\processed\\category_maps.json --artifacts-dir artifacts --seed 42.
  - Produces refreshed plots + pd_metrics.json / lafd_metrics.json.
- **Compilation checks:** cargo check for both ust_prep and webapp pass after dependency bumps (polars 0.52.x with 	imezones + dtype-u8).

> Control (Python-only prep) numbers are still placeholders because a pandas-based reference pipeline has not been implemented yet. The web story calls this out as “Awaiting baseline”.

## Model Metrics Snapshot (py_model/artifacts)
| Dataset | Model | Accuracy | AUC/R² | Notes |
|---|---|---|---|---|
| APD | Logistic Regression | 0.804 | AUC 0.863 | Balanced precision: 0.89 (no-report) vs 0.60 (report). |
| APD | XGBoost | 0.811 | AUC 0.864 | Gains recall on the minority (“report written”) class. |
| LAFD | Gradient Boosted Regression | — | R² 0.623, MAE 26 s | Response-time regression across 120k incidents. |
| LAFD | Bucket Classifier | 0.988 | F1_macro 0.988 | Buckets: fast/typical/slow (class counts logged in JSON). |

Artifacts include ROC curves, confusion matrices, feature-importance JSON/PNGs, and scatter plots for regression residuals.

## Data EDA highlights (docs/eda_snapshot.txt)
- **APD sample:** 120k calls, 25.7% “report written”. Response times avg 32 min, long tail to 1,943 min. 90% of units arrive in <=30 min. Distribution of incident types: 96.7% dispatched vs 3.3% officer-initiated. Mental-health flag ~11.6% of cases.
- **LAFD sample:** 120k runs, avg dispatch delay 125 s, total response 521 s with heavy-tailed delays (max 18 hours). Unit mix dominated by engines and ALS/BLS ambulances. Dispatch status heavily skewed toward QTR (quarters) launches.

## Documentation + Math sanity checks
- New docs/eda_snapshot.txt stores .info() output, descriptive statistics, and top-k categorical counts for each dataset.
- Cost model (config/cost_model.json) now feeds the web dashboard; with the latest Rust benchmark the UI shows “Awaiting baseline” until we log Python control timings.
- Stratified sampling keeps unit_type when enabling include_key=true in Polars 0.52; regression/bucket splits now encode categorical labels so the XGBoost multi-class objective sees [0,1,2] instead of strings.
- Console logging uses ASCII (no ?) to run cleanly under Windows codepages.

## Outstanding gaps / recommended next steps
1. Implement a pandas-based control prep script that mirrors ust_prep so prep_python_bench.json reflects apples-to-apples timings.
2. Automate data-quality diffs (current JSON is static) and run-variance stats for repeated batches.
3. Optionally pin Polars features via a workspace config.toml to speed up future compiles.
4. Expand webapp /api/metrics verification with an integration test (e.g., cargo test hitting handlers with fixture JSON) once control artifacts exist.
