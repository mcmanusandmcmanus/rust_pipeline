# Python Modeling (`py_model`)

This folder contains the Python counterpart to the Rust prep engine: it loads the curated Parquet tables, trains baseline vs XGBoost models, and emits ready-to-embed plots/metrics.

## Quick start

```powershell
cd py_model
.\.venv\Scripts\python src\train.py ^
  --apd-path ..\data\processed\apd_sample.parquet ^
  --lafd-path ..\data\processed\lafd_sample.parquet ^
  --category-map ..\data\processed\category_maps.json ^
  --artifacts-dir artifacts ^
  --seed 42
```

Artifacts written to `py_model/artifacts/`:

- `apd_metrics.json` / `lafd_metrics.json`
- `apd_roc_xgb.png`, `apd_roc_logit.png`, and matching confusion matrices
- Feature-importance plots (`*_feature_importance.png`) for storytelling
- `lafd_actual_vs_pred.png` plus bucketed confusion matrix for regime detection

All scripts rely only on the files produced by `rust_prep`, so you can iterate on modeling without touching the 1 GB source CSVs again.
