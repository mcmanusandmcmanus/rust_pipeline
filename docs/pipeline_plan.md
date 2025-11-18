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
2. **Python Control Prep (`py_prep/`)**
   - Mirror the Rust transforms in pandas, emit control Parquet/category maps, and log benchmarks via `python3 -m py_prep.cli control-prep`.
   - Append benchmark entries to `benchmarks/prep_history_2024.json` (`ingest-run` for Rust, automatic for Python) so we can compute run variance.
3. **Python Modeling Lab**
   - Load Rust-curated tables, perform encoding/scaling, and benchmark baseline vs XGBoost with AUC/F1 + SHAP-style importances.
   - Persist metrics/plots for later visualization (SVG/JSON) rather than re-running heavy training.
4. **Rust Storytelling Web App**
   - Serve cached prep + modeling outputs, render charts (egui/plotters or frontend JS bundle), and present narrative copy comparing Rust/Python.
   - Provide a gated “re-run locally” endpoint that shells into the prep/model binaries but warns about runtime cost.

## Guardrails & Best Practices
- Target training sample size per dataset: **~120k rows by default** (fast demo loop) with the option to crank back up to 300k when we need extra statistical power.
- Favor sub-10 core features for the narrative demo; keep the richer transforms behind feature flags if we need them later.
- Use Arrow/Parquet between stages to avoid repeated CSV parsing.
- Capture reproducibility metadata (command, sample seed, Git SHA) alongside every artifact for trust in benchmarks.
- Rebuild `data/processed/data_quality_summary.json` and `run_variance_summary.json` via `python3 -m py_prep.cli compare-quality` and `summarize-variance` each time fresh Parquet/history data lands.

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
3. Ship the pandas control prep + reporting commands (`py_prep/cli.py`) and wire them into the history/quality automation.
4. Stand up Python virtual env, write modeling script, cache plots/metrics.
5. Build Axum web UI that reads cached artifacts and renders markdown + charts + action controls.
