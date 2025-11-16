"""
Quick schema/profile scan for the large CSVs so we can design samples & features.
"""

from __future__ import annotations

import json
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable

import pandas as pd
from pandas.api.types import CategoricalDtype

BASE_DIR = Path(__file__).resolve().parents[2]
DATA_DIR = BASE_DIR
DOCS_DIR = BASE_DIR / "docs"
OUTPUT_JSON = DOCS_DIR / "data_profile.json"
OUTPUT_MD = DOCS_DIR / "data_profile.md"

SAMPLE_ROWS = 200_000  # enough to infer distributions without blowing RAM


@dataclass
class ColumnSummary:
    name: str
    dtype: str
    inferred_role: str
    missing_pct: float
    unique_approx: int
    example_values: list[str]
    top_categories: dict[str, int] | None


@dataclass
class DatasetProfile:
    file_name: str
    total_rows_est: int
    sample_rows: int
    columns: list[ColumnSummary]
    recommended_features: list[str]
    candidate_target: str | None


def count_rows(path: Path) -> int:
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        # subtract header line
        return max(sum(1 for _ in fh) - 1, 0)


def infer_role(series: pd.Series) -> str:
    if pd.api.types.is_numeric_dtype(series):
        if series.name.lower().endswith("id"):
            return "identifier"
        if series.name.lower().endswith(("count", "number")):
            return "metric"
        return "numeric"
    # convert string to heuristics
    unique = series.nunique(dropna=True)
    if unique < 40:
        return "categorical"
    if "time" in series.name.lower() or "date" in series.name.lower():
        return "timestamp_string"
    return "text"


def top_categories(series: pd.Series, limit: int = 8) -> dict[str, int] | None:
    dtype = series.dtype
    if not pd.api.types.is_object_dtype(series) and not isinstance(dtype, CategoricalDtype):
        return None
    counts = (
        series.dropna()
        .astype(str)
        .apply(lambda value: value.strip())
        .value_counts()
        .head(limit)
    )
    if counts.empty:
        return None
    return counts.to_dict()


def summarize_columns(df: pd.DataFrame) -> list[ColumnSummary]:
    summaries: list[ColumnSummary] = []
    for column in df.columns:
        series = df[column]
        summaries.append(
            ColumnSummary(
                name=column,
                dtype=str(series.dtype),
                inferred_role=infer_role(series),
                missing_pct=float(series.isna().mean() * 100),
                unique_approx=int(series.nunique(dropna=True)),
                example_values=[str(v) for v in series.dropna().head(3).tolist()],
                top_categories=top_categories(series),
            )
        )
    return summaries


def profile_dataset(file_name: str, recommended_features: list[str], target: str | None) -> DatasetProfile:
    path = DATA_DIR / file_name
    total_rows = count_rows(path)
    df = pd.read_csv(path, nrows=SAMPLE_ROWS)
    columns = summarize_columns(df)
    return DatasetProfile(
        file_name=file_name,
        total_rows_est=total_rows,
        sample_rows=len(df),
        columns=columns,
        recommended_features=recommended_features,
        candidate_target=target,
    )


def write_markdown(profiles: Iterable[DatasetProfile]) -> None:
    md_lines = ["# Data Profiles", ""]
    for profile in profiles:
        md_lines.append(f"## {profile.file_name}")
        md_lines.append(
            f"- Estimated rows: **{profile.total_rows_est:,}** | Sampled: {profile.sample_rows:,}"
        )
        if profile.candidate_target:
            md_lines.append(f"- Candidate target: `{profile.candidate_target}`")
        md_lines.append(
            f"- Suggested feature columns ({len(profile.recommended_features)}): "
            + ", ".join(f"`{col}`" for col in profile.recommended_features)
        )
        md_lines.append("")
        md_lines.append("| Column | Role | Missing % | Unique | dtype | examples |")
        md_lines.append("| --- | --- | --- | --- | --- | --- |")
        for column in profile.columns:
            examples = ", ".join(column.example_values)
            md_lines.append(
                f"| {column.name} | {column.inferred_role} | "
                f"{column.missing_pct:.1f}% | {column.unique_approx:,} | "
                f"{column.dtype} | {examples} |"
            )
        md_lines.append("")
        categorical_columns = [
            column for column in profile.columns if column.top_categories
        ]
        if categorical_columns:
            md_lines.append("### Top Categories (sample)")
            for column in categorical_columns:
                top = ", ".join(f"{k} ({v})" for k, v in column.top_categories.items())
                md_lines.append(f"- **{column.name}:** {top}")
            md_lines.append("")
    DOCS_DIR.mkdir(parents=True, exist_ok=True)
    DOCS_DIR.joinpath("data_profile.md").write_text("\n".join(md_lines), encoding="utf-8")


def main() -> None:
    profiles = [
        profile_dataset(
            "APD_Computer_Aided_Dispatch_Incidents_20251101.csv",
            recommended_features=[
                "Priority Level",
                "Response Time",
                "Number of Units Arrived",
                "Unit Time on Scene",
                "Mental Health Flag",
                "Council District",
                "Response Hour",
                "Response Day of Week",
                "Initial Problem Category",
                "Final Problem Category",
                "Call Disposition Description",
            ],
            target="Report Written Flag",
        ),
        profile_dataset(
            "LAFD_Response_Metrics_-_Raw_Data_20251101.csv",
            recommended_features=[
                "Emergency Dispatch Code",
                "Dispatch Sequence",
                "Dispatch Status",
                "Unit Type",
                "PPE Level",
                "First In District",
                "Time of Dispatch (GMT)",
                "En Route Time (GMT)",
                "On Scene Time (GMT)",
            ],
            target=None,
        ),
    ]
    OUTPUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_JSON.write_text(
        json.dumps([asdict(profile) for profile in profiles], indent=2), encoding="utf-8"
    )
    write_markdown(profiles)
    print(f"Wrote {OUTPUT_JSON} and {OUTPUT_MD}")


if __name__ == "__main__":
    main()
