"""
Python modeling side for the Rust ↔ XGBoost benchmark.

Loads the Parquet artifacts emitted by `rust_prep`, runs baseline + XGBoost models,
and saves metrics/plots inside `py_model/artifacts/`.
"""

from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Dict, List

import matplotlib

# headless rendering
matplotlib.use("Agg")

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import polars as pl
import seaborn as sns
import typer
import xgboost as xgb
from rich.console import Console
from sklearn.compose import ColumnTransformer
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import (
    ConfusionMatrixDisplay,
    RocCurveDisplay,
    accuracy_score,
    classification_report,
    f1_score,
    mean_absolute_error,
    mean_squared_error,
    r2_score,
    roc_auc_score,
)
from sklearn.model_selection import train_test_split
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import OneHotEncoder, StandardScaler

app = typer.Typer(help="Train models on the Rust-prepped feature tables.")
console = Console()

APD_NUMERIC = [
    "response_minutes",
    "call_duration_minutes",
    "units_arrived",
    "priority_level_ord",
]
APD_CATEGORICAL = [
    "mental_health_flag",
    "incident_type",
    "call_disposition",
]

LAFD_NUMERIC = [
    "dispatch_delay_s",
    "enroute_delay_s",
    "arrival_delay_s",
    "dispatch_sequence",
    "first_in_district",
]
LAFD_CATEGORICAL = [
    "unit_type",
    "dispatch_status",
    "emergency_dispatch_code",
]


def build_preprocessor(num_cols: List[str], cat_cols: List[str]) -> ColumnTransformer:
    return ColumnTransformer(
        transformers=[
            ("num", StandardScaler(), num_cols),
            ("cat", OneHotEncoder(handle_unknown="ignore", sparse_output=False), cat_cols),
        ],
        remainder="drop",
    )


def load_category_caps(path: Path) -> Dict[str, Dict[str, List[str]]]:
    if not path.exists():
        console.log(f"[yellow]Category map {path} not found; proceeding without caps.[/yellow]")
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def apply_category_cap(frame: pd.DataFrame, column: str, allowed: List[str], top_k: int = 15) -> None:
    if column not in frame.columns:
        return
    keep = [value for value in allowed[:top_k] if isinstance(value, str)]
    series = frame[column].fillna("Missing").astype(str)
    if keep:
        frame[column] = np.where(series.isin(keep), series, "Other")
    else:
        frame[column] = series


def save_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def plot_feature_importance(
    names: List[str],
    importances: np.ndarray,
    top_k: int,
    path: Path,
    title: str,
) -> List[dict]:
    order = np.argsort(importances)[::-1][:top_k]
    top_names = [names[idx] for idx in order]
    top_scores = importances[order]
    plt.figure(figsize=(8, max(3, top_k * 0.35)))
    sns.barplot(x=top_scores, y=top_names, palette="viridis")
    plt.xlabel("Gain")
    plt.title(title)
    plt.tight_layout()
    path.parent.mkdir(parents=True, exist_ok=True)
    plt.savefig(path, dpi=150)
    plt.close()
    return [{"feature": feat, "gain": float(score)} for feat, score in zip(top_names, top_scores)]


def train_apd(
    apd_path: Path,
    artifacts: Path,
    category_caps: Dict[str, List[str]],
    seed: int,
) -> dict:
    console.log(f"[cyan]Loading APD features from {apd_path}[/cyan]")
    apd_df = pl.read_parquet(apd_path).to_pandas()
    for column in APD_CATEGORICAL:
        cap_values = category_caps.get(column, [])
        apply_category_cap(apd_df, column, cap_values, top_k=20)
    apd_df["report_written"] = apd_df["report_written"].astype(int)

    X = apd_df[APD_NUMERIC + APD_CATEGORICAL]
    y = apd_df["report_written"]
    X_train, X_test, y_train, y_test = train_test_split(
        X,
        y,
        test_size=0.2,
        stratify=y,
        random_state=seed,
    )

    logit_model = Pipeline(
        steps=[
            ("prep", build_preprocessor(APD_NUMERIC, APD_CATEGORICAL)),
            (
                "model",
                LogisticRegression(
                    max_iter=250,
                    class_weight="balanced",
                    solver="lbfgs",
                ),
            ),
        ]
    )

    xgb_model = Pipeline(
        steps=[
            ("prep", build_preprocessor(APD_NUMERIC, APD_CATEGORICAL)),
            (
                "model",
                xgb.XGBClassifier(
                    n_estimators=400,
                    max_depth=6,
                    subsample=0.85,
                    colsample_bytree=0.9,
                    learning_rate=0.08,
                    reg_lambda=1.0,
                    tree_method="hist",
                    random_state=seed,
                    n_jobs=8,
                    eval_metric="auc",
                ),
            ),
        ]
    )

    logit_model.fit(X_train, y_train)
    xgb_model.fit(X_train, y_train)

    logit_metrics = evaluate_classifier(logit_model, X_test, y_test, artifacts / "apd_roc_logit.png", "Logistic", seed)
    xgb_metrics = evaluate_classifier(xgb_model, X_test, y_test, artifacts / "apd_roc_xgb.png", "XGBoost", seed)

    # Feature importance from the XGBoost core estimator
    feature_names = xgb_model.named_steps["prep"].get_feature_names_out()
    importances = xgb_model.named_steps["model"].feature_importances_
    fi_records = plot_feature_importance(
        names=feature_names.tolist(),
        importances=importances,
        top_k=20,
        path=artifacts / "apd_feature_importance.png",
        title="APD - XGB Feature Gain",
    )
    save_json(artifacts / "apd_feature_importance.json", fi_records)

    metrics_payload = {
        "logistic_regression": logit_metrics,
        "xgboost": xgb_metrics,
        "class_balance": {
            "train_pos_pct": float(y_train.mean()),
            "test_pos_pct": float(y_test.mean()),
        },
        "records_used": len(apd_df),
        "features": APD_NUMERIC + APD_CATEGORICAL,
    }
    save_json(artifacts / "apd_metrics.json", metrics_payload)
    console.log("[green]APD models trained[/green]")
    return metrics_payload


def evaluate_classifier(
    model: Pipeline,
    X_test: pd.DataFrame,
    y_test: pd.Series,
    roc_path: Path,
    label: str,
    seed: int,
) -> dict:
    probs = model.predict_proba(X_test)[:, 1]
    preds = (probs >= 0.5).astype(int)
    auc = roc_auc_score(y_test, probs)
    acc = accuracy_score(y_test, preds)
    f1 = f1_score(y_test, preds)
    report = classification_report(y_test, preds, output_dict=True)

    RocCurveDisplay.from_predictions(y_test, probs, name=label)
    plt.title(f"{label} ROC Curve")
    plt.tight_layout()
    roc_path.parent.mkdir(parents=True, exist_ok=True)
    plt.savefig(roc_path, dpi=150)
    plt.close()

    cm = ConfusionMatrixDisplay.from_predictions(y_test, preds, display_labels=["No Report", "Report"])
    plt.title(f"{label} Confusion Matrix")
    plt.tight_layout()
    cm_path = roc_path.with_name(roc_path.stem.replace("roc", "cm") + "_cm.png")
    plt.savefig(cm_path, dpi=150)
    plt.close()

    return {
        "auc": float(auc),
        "accuracy": float(acc),
        "f1": float(f1),
        "classification_report": report,
        "roc_curve": roc_path.as_posix(),
        "confusion_matrix": cm_path.as_posix(),
        "seed": seed,
    }


def train_lafd(
    lafd_path: Path,
    artifacts: Path,
    category_caps: Dict[str, List[str]],
    seed: int,
) -> dict:
    console.log(f"[cyan]Loading LAFD features from {lafd_path}[/cyan]")
    lafd_df = pl.read_parquet(lafd_path).to_pandas()
    for column in LAFD_CATEGORICAL:
        cap_values = category_caps.get(column, [])
        apply_category_cap(lafd_df, column, cap_values, top_k=10)

    target = lafd_df["total_response_s"]
    features = lafd_df[LAFD_NUMERIC + LAFD_CATEGORICAL]
    X_train, X_test, y_train, y_test = train_test_split(
        features,
        target,
        test_size=0.2,
        random_state=seed,
    )

    reg_pipeline = Pipeline(
        steps=[
            ("prep", build_preprocessor(LAFD_NUMERIC, LAFD_CATEGORICAL)),
            (
                "model",
                xgb.XGBRegressor(
                    n_estimators=350,
                    max_depth=7,
                    subsample=0.9,
                    colsample_bytree=0.85,
                    learning_rate=0.05,
                    reg_lambda=1.0,
                    reg_alpha=0.2,
                    tree_method="hist",
                    random_state=seed,
                    n_jobs=8,
                ),
            ),
        ]
    )

    reg_pipeline.fit(X_train, y_train)
    preds = reg_pipeline.predict(X_test)
    mae = mean_absolute_error(y_test, preds)
    rmse = math.sqrt(mean_squared_error(y_test, preds))
    r2 = r2_score(y_test, preds)

    plt.figure(figsize=(6, 6))
    sns.scatterplot(x=y_test, y=preds, alpha=0.3)
    plt.xlabel("Actual total_response_s")
    plt.ylabel("Predicted total_response_s")
    plt.title("LAFD Response Regression")
    lims = [
        min(y_test.min(), preds.min()),
        max(y_test.max(), preds.max()),
    ]
    plt.plot(lims, lims, "--", color="black")
    plt.tight_layout()
    scatter_path = artifacts / "lafd_actual_vs_pred.png"
    plt.savefig(scatter_path, dpi=150)
    plt.close()

    feature_names = reg_pipeline.named_steps["prep"].get_feature_names_out()
    fi_records = plot_feature_importance(
        names=feature_names.tolist(),
        importances=reg_pipeline.named_steps["model"].feature_importances_,
        top_k=20,
        path=artifacts / "lafd_feature_importance.png",
        title="LAFD - XGBReg Feature Gain",
    )
    save_json(artifacts / "lafd_feature_importance.json", fi_records)

    # Optional bucketed classification for storytelling
    bucket_labels = ["fast", "typical", "slow"]
    lafd_df["response_bucket"] = pd.qcut(
        lafd_df["total_response_s"], q=3, labels=bucket_labels, duplicates="drop"
    )
    cls_metrics = train_lafd_bucket_classifier(lafd_df, artifacts, seed)

    payload = {
        "regression": {
            "mae": float(mae),
            "rmse": float(rmse),
            "r2": float(r2),
            "scatter_plot": scatter_path.as_posix(),
        },
        "bucket_classifier": cls_metrics,
        "records_used": len(lafd_df),
        "features": LAFD_NUMERIC + LAFD_CATEGORICAL,
    }
    save_json(artifacts / "lafd_metrics.json", payload)
    console.log("[green]LAFD regression + classifier complete[/green]")
    return payload


def train_lafd_bucket_classifier(df: pd.DataFrame, artifacts: Path, seed: int) -> dict:
    dropna_df = df.dropna(subset=["response_bucket"])
    if dropna_df.empty:
        return {}
    X = dropna_df[LAFD_NUMERIC + LAFD_CATEGORICAL]
    bucket_series = dropna_df["response_bucket"].astype("category")
    bucket_labels = list(bucket_series.cat.categories)
    y = bucket_series.cat.codes

    X_train, X_test, y_train, y_test = train_test_split(
        X,
        y,
        test_size=0.2,
        random_state=seed,
        stratify=y,
    )
    clf = Pipeline(
        steps=[
            ("prep", build_preprocessor(LAFD_NUMERIC, LAFD_CATEGORICAL)),
            (
                "model",
                xgb.XGBClassifier(
                    n_estimators=300,
                    max_depth=5,
                    learning_rate=0.08,
                    subsample=0.85,
                    colsample_bytree=0.8,
                    objective="multi:softprob",
                    num_class=len(bucket_labels),
                    tree_method="hist",
                    n_jobs=8,
                    random_state=seed,
                ),
            ),
        ]
    )
    clf.fit(X_train, y_train)
    preds = clf.predict(X_test)
    acc = accuracy_score(y_test, preds)
    f1 = f1_score(y_test, preds, average="macro")
    cm = ConfusionMatrixDisplay.from_predictions(
        y_test, preds, cmap="Blues", display_labels=bucket_labels
    )
    plt.title("LAFD Bucket Confusion Matrix")
    plt.tight_layout()
    cm_path = artifacts / "lafd_bucket_confusion.png"
    plt.savefig(cm_path, dpi=150)
    plt.close()
    return {
        "accuracy": float(acc),
        "f1_macro": float(f1),
        "confusion_matrix": cm_path.as_posix(),
        "labels": bucket_labels,
    }


@app.command()
def train(
    apd_path: Path = typer.Option(
        Path("../data/processed/apd_sample.parquet"),
        help="Path to the APD feature Parquet file.",
    ),
    lafd_path: Path = typer.Option(
        Path("../data/processed/lafd_sample.parquet"),
        help="Path to the LAFD feature Parquet file.",
    ),
    category_map: Path = typer.Option(
        Path("../data/processed/category_maps.json"),
        help="Category map emitted by rust_prep (optional).",
    ),
    artifacts_dir: Path = typer.Option(
        Path("artifacts"),
        help="Directory to store metrics + plots.",
    ),
    seed: int = typer.Option(42, help="Random seed for splits."),
) -> None:
    artifacts_dir.mkdir(parents=True, exist_ok=True)
    caps = load_category_caps(category_map)
    apd_metrics = train_apd(apd_path, artifacts_dir, caps.get("apd", {}), seed)
    lafd_metrics = train_lafd(lafd_path, artifacts_dir, caps.get("lafd", {}), seed)
    summary = {
        "apd": apd_metrics,
        "lafd": lafd_metrics,
    }
    save_json(artifacts_dir / "model_summary.json", summary)
    console.log(f"[bold green]Artifacts ready in {artifacts_dir.resolve()}[/bold green]")


if __name__ == "__main__":
    app()
