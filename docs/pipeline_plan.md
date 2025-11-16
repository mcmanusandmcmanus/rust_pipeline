# Rust vs Python Pipeline — Execution Plan

## Hardware Envelope
- CPU: Intel Core Ultra 7 155U (1.70 GHz, 10C/12T efficiency/perform.)
- RAM: 32 GB (≈29 GB effectively accessible after OS/services).
- Feasible concurrent workload: one Rust prep binary + one Python/XGBoost job when each keeps <10 GB resident; avoid duplicate 1 GB CSV copies in memory.
- Storage reminder: leverage streaming reads and incremental writes to keep SSD churn manageable.

## Deliverable Threads
1. **Rust Sampling & Feature Builder**
   - Stream both CSVs, keep only ML-relevant columns, engineer a few numeric aggregations, and capture timings.
   - Emit column schema + manifest that Python can read without re-inferring types.
2. **Python Modeling Lab**
   - Load Rust-curated tables, perform encoding/scaling, and benchmark baseline vs XGBoost with AUC/F1 + SHAP-style importances.
   - Persist metrics/plots for later visualization (SVG/JSON) rather than re-running heavy training.
3. **Rust Storytelling Web App**
   - Serve cached prep + modeling outputs, render charts (egui/plotters or frontend JS bundle), and present narrative copy comparing Rust/Python.
   - Provide a gated “re-run locally” endpoint that shells into the prep/model binaries but warns about runtime cost.

## Guardrails & Best Practices
- Target training sample size per dataset: **200k–300k rows** after filtering to high-signal columns; aligns with XGBoost sweet spot on 32 GB RAM.
- Favor 20–30 engineered features to balance generalization vs overfitting.
- Use Arrow/Parquet between stages to avoid repeated CSV parsing.
- Capture reproducibility metadata (command, sample seed, Git SHA) alongside every artifact for trust in benchmarks.

## File/Directory Layout Draft
```
docs/
  pipeline_plan.md
rust_prep/
  Cargo.toml
  src/main.rs
  README.md
py_model/
  pyproject.toml
  src/train.py
  notebooks/eda.ipynb
  artifacts/
webapp/
  Cargo.toml (Axum + Askama)
  src/main.rs
data/
  processed/ (small samples, parquet, metrics JSON)
```

## Next Steps
1. Profile CSV schemas + cardinalities (chunked read) to confirm column selections.
2. Implement Rust prep CLI with Polars + serde_json for manifests.
3. Stand up Python virtual env, write modeling script, cache plots/metrics.
4. Build Axum web UI that reads cached artifacts and renders markdown + charts + action controls.
