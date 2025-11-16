# `rust_prep`

Rust CLI that turns the multi‑GB raw CSVs into lean Parquet feature tables so Python + XGBoost can iterate quickly.

## Features

- Streams the original CSVs with Polars (no giant pandas DataFrame in RAM).
- Filters to the most recent APD calls, engineers time deltas, and emits a 250k row labeled slice for documentation-risk modeling.
- Builds response-delay features for 250k LAFD runs (regression/classification ready).
- Deterministic sampling (seeded) with lightweight benchmarking + category dictionaries for consistent encoding.

## Usage

```powershell
cargo run --release -- \
  --apd-file ..\APD_Computer_Aided_Dispatch_Incidents_20251101.csv \
  --lafd-file ..\LAFD_Response_Metrics_-_Raw_Data_20251101.csv \
  --output-dir ..\data\processed \
  --apd-sample 250000 \
  --lafd-sample 250000 \
  --seed 42
```

Outputs inside `data/processed/`:

- `apd_sample.parquet`
- `lafd_sample.parquet`
- `category_maps.json`
- `prep_rust_bench.json`

Tweak `--apd-sample` / `--lafd-sample` if you need smaller quick iterations (e.g., 50_000 rows) for laptop demos.
