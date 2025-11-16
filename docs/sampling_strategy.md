# Sampling & Feature Strategy

Grounded in the quick profiles (`docs/data_profile.md`) and laptop envelope (Intel Ultra 7 + 32 GB RAM), the sampling plan balances statistical coverage with practical runtime for XGBoost.

## Why ~250k Rows Per Domain?
- **Model best practice:** XGBoost usually saturates marginal gains beyond ~300k rows unless the feature space is sparse/high-cardinality. 200k–300k rows keeps tree depth manageable and lets us run multiple experiments quickly.
- **Memory budget:** Each dataset has 25–30 columns. 250k rows of float32/int32 features consume <300 MB, leaving plenty of headroom for gradient histograms during training.
- **Benchmarking clarity:** With deterministic stratified sampling we can publish reproducible metrics without re-reading multi-GB CSVs every run.

## Sampling Procedure

| Dataset | Raw rows | Filter | Sample size | Notes |
| --- | --- | --- | --- | --- |
| `APD_Computer_Aided_Dispatch_Incidents_20251101.csv` | 5.2M | Keep incidents from 2019–2024 to reflect modern ops. Drop rows with missing response/arrival timestamps. | 250k stratified by `Report Written Flag` and `Priority Level`. | Balanced representation of high-risk calls and ensures minority "Yes" target values remain >=15%. |
| `LAFD_Response_Metrics_-_Raw_Data_20251101.csv` | 7.2M | Keep rows where `On Scene Time` exists to let us compute full duration deltas. | 250k stratified by `Unit Type`. | Use this set as an auxiliary regression/regime-classification dataset (no direct target supplied yet). |

Sampling will be performed inside the Rust prep CLI so we only touch each 1 GB file once per run.

## Feature Set Highlights

### APD Dispatch Incidents (classification target: `report_written`)
Numeric / engineered:
- `response_minutes`: difference between `Response Datetime` and `First Unit Arrived Datetime`.
- `call_duration_minutes`: `Call Closed Datetime` − `Response Datetime`.
- `units_arrived`: from `Number of Units Arrived` (capped at 10).
- `on_scene_seconds`: parsed from `Unit Time on Scene`.
- `priority_level`: ordinal encoded 0–3.

Categorical (targeted encoding / one-hot):
- `mental_health_flag`, `incident_type`, `initial_problem_category`, `final_problem_category`, `call_disposition_description`, `sector`.
- Temporal buckets: `response_hour` (0–23), `response_day_of_week` (one-hot), `response_month`.

Target:
- `report_written_flag` → binary (`Yes`=1). This measures documentation likelihood and proxies severity / follow-up complexity.

### LAFD Response Metrics (regression/classification experiment)
Engineered numeric durations:
- `dispatch_delay_s`: `Time of Dispatch` − `Incident Creation Time`.
- `enroute_delay_s`: `En Route Time` − `Time of Dispatch`.
- `arrival_delay_s`: `On Scene Time` − `En Route Time`.
- `total_response_s`: creation → on-scene.

Categorical context:
- `dispatch_status`, `unit_type`, `ppe_level`, `emergency_dispatch_code`, `first_in_district` (bucketed), `dispatch_sequence`.

Targets:
- Primary: `total_response_s` regression (predict response times based on unit and dispatch metadata).
- Secondary classification: bucket `total_response_s` into quantiles (fast / median / slow) for easier storytelling inside the dashboard.

## Encoding Choices
- Ordinal encode priority (Priority 0..3) and `Dispatch Sequence`.
- Frequency / target encode high-cardinality text columns (initial/final problem description) to avoid exploding dimensionality; fallback to top-k one-hot + "other".
- Use `polars` in Rust to compute string → category mappings and persist them as artifacts (`data/processed/category_maps.json`) so Python training uses identical encodings.

## File Outputs From Rust Prep
- `data/processed/apd_sample.parquet`: 250k rows, engineered features, target.
- `data/processed/lafd_sample.parquet`: 250k rows, engineered durations + categorical columns.
- `data/processed/category_maps.json`: label dictionaries for consistent encoding.
- `data/processed/prep_rust_bench.json`: runtime + throughput stats to feed the web UI.

These smaller, typed artifacts will be the single source of truth for modeling and the later Rust dashboard.
