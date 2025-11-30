from __future__ import annotations

import json
import random
import statistics
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, Iterable, List, Sequence

import pandas as pd
import typer
from rich.console import Console

BASE_DIR = Path(__file__).resolve().parents[1]
DEFAULT_APD_CSV = BASE_DIR / "APD_Computer_Aided_Dispatch_Incidents_20251101.csv"
DEFAULT_LAFD_CSV = BASE_DIR / "LAFD_Response_Metrics_-_Raw_Data_20251101.csv"
DEFAULT_OUTPUT_DIR = BASE_DIR / "data" / "processed"
DEFAULT_HISTORY = BASE_DIR / "benchmarks" / "prep_history_2024.json"
DEFAULT_COST_MODEL = BASE_DIR / "config" / "cost_model.json"
DEFAULT_RUST_APD = DEFAULT_OUTPUT_DIR / "apd_sample.parquet"
DEFAULT_RUST_LAFD = DEFAULT_OUTPUT_DIR / "lafd_sample.parquet"
DEFAULT_CONTROL_APD = DEFAULT_OUTPUT_DIR / "apd_control_sample.parquet"
DEFAULT_CONTROL_LAFD = DEFAULT_OUTPUT_DIR / "lafd_control_sample.parquet"
DEFAULT_DATA_QUALITY = DEFAULT_OUTPUT_DIR / "data_quality_summary.json"
DEFAULT_VARIANCE = DEFAULT_OUTPUT_DIR / "run_variance_summary.json"

YEAR_FILTER = 2024

APD_FEATURE_COLUMNS = [
    "report_written",
    "priority_level_ord",
    "units_arrived",
    "mental_health_flag",
    "incident_type",
    "call_disposition",
    "response_minutes",
    "call_duration_minutes",
]
APD_CHUNK_COLUMNS = [
    "Report Written Flag",
    "Priority Level",
    "Number of Units Arrived",
    "Mental Health Flag",
    "Incident Type",
    "Call Disposition Description",
    "Response Datetime",
    "First Unit Arrived Datetime",
    "Call Closed Datetime",
    "Response Year",
]
APD_DT_FORMAT = "%Y %b %d %I:%M:%S %p"

LAFD_FEATURE_COLUMNS = [
    "unit_type",
    "dispatch_status",
    "emergency_dispatch_code",
    "first_in_district",
    "dispatch_sequence",
    "dispatch_delay_s",
    "enroute_delay_s",
    "arrival_delay_s",
    "total_response_s",
]
LAFD_CHUNK_COLUMNS = [
    "Unit Type",
    "Dispatch Status",
    "Emergency Dispatch Code",
    "First In District",
    "Dispatch Sequence",
    "Incident Creation Time (GMT)",
    "Time of Dispatch (GMT)",
    "En Route Time (GMT)",
    "On Scene Time (GMT)",
]

console = Console()
app = typer.Typer(help="Python control prep pipeline + reporting helpers.")


@dataclass
class DatasetMetrics:
    name: str
    source_path: str
    year_filter: int
    rows_after_filters: int
    rows_sampled: int
    duration_ms: int
    output_path: str
    feature_columns: List[str]


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def save_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def transform_apd_chunk(chunk: pd.DataFrame) -> pd.DataFrame:
    if chunk.empty:
        return pd.DataFrame(columns=APD_FEATURE_COLUMNS)
    frame = chunk.copy()
    frame["Response Year"] = pd.to_numeric(frame["Response Year"], errors="coerce").astype(
        "Int64"
    )
    response_dt = pd.to_datetime(
        frame["Response Datetime"], format=APD_DT_FORMAT, errors="coerce"
    )
    response_year = frame["Response Year"].fillna(response_dt.dt.year)
    frame = frame[response_year.eq(YEAR_FILTER)]
    if frame.empty:
        return pd.DataFrame(columns=APD_FEATURE_COLUMNS)
    response_dt = response_dt.loc[frame.index]

    # Normalize categorical columns
    frame["incident_type"] = frame["Incident Type"].astype("string")
    frame["mental_health_flag"] = frame["Mental Health Flag"].astype("string")
    frame["call_disposition"] = frame["Call Disposition Description"].astype("string")

    priority_clean = (
        frame["Priority Level"]
        .astype("string")
        .str.replace("Priority", "", regex=False)
        .str.strip()
    )
    frame["priority_level_ord"] = pd.to_numeric(priority_clean, errors="coerce")
    frame["units_arrived"] = pd.to_numeric(
        frame["Number of Units Arrived"], errors="coerce"
    )
    frame["report_written"] = (
        frame["Report Written Flag"]
            .astype("string")
            .str.strip()
            .str.lower()
            .eq("yes")
            .astype("uint8", copy=False)
    )

    arrival_dt = pd.to_datetime(
        frame["First Unit Arrived Datetime"], format=APD_DT_FORMAT, errors="coerce"
    )
    closed_dt = pd.to_datetime(
        frame["Call Closed Datetime"], format=APD_DT_FORMAT, errors="coerce"
    )
    frame["response_minutes"] = (
        (arrival_dt - response_dt).dt.total_seconds() / 60.0
    )
    frame["call_duration_minutes"] = (
        (closed_dt - response_dt).dt.total_seconds() / 60.0
    )

    features = frame[
        [
            "report_written",
            "priority_level_ord",
            "units_arrived",
            "mental_health_flag",
            "incident_type",
            "call_disposition",
            "response_minutes",
            "call_duration_minutes",
        ]
    ].copy()
    mask = (
        features["response_minutes"].notna()
        & features["call_duration_minutes"].notna()
        & features["report_written"].notna()
        & (features["response_minutes"] > 0)
        & (features["call_duration_minutes"] > 0)
    )
    filtered = features.loc[mask].copy()
    filtered["priority_level_ord"] = filtered["priority_level_ord"].astype("Int16", copy=False)
    filtered["units_arrived"] = filtered["units_arrived"].astype("Int16", copy=False)
    return filtered


def transform_lafd_chunk(chunk: pd.DataFrame) -> pd.DataFrame:
    if chunk.empty:
        return pd.DataFrame(columns=LAFD_FEATURE_COLUMNS)
    frame = chunk.copy()
    frame = frame[frame["On Scene Time (GMT)"].notna()]
    if frame.empty:
        return pd.DataFrame(columns=LAFD_FEATURE_COLUMNS)

    frame["unit_type"] = frame["Unit Type"].astype("string")
    frame["dispatch_status"] = frame["Dispatch Status"].astype("string")
    frame["emergency_dispatch_code"] = frame["Emergency Dispatch Code"].astype("string")
    frame["dispatch_sequence"] = pd.to_numeric(
        frame["Dispatch Sequence"], errors="coerce"
    ).astype("Int32")
    frame["first_in_district"] = pd.to_numeric(
        frame["First In District"], errors="coerce"
    ).astype("Int32")

    synth_prefix = f"{YEAR_FILTER}-01-01 "

    def to_dt(series: pd.Series) -> pd.Series:
        return pd.to_datetime(
            synth_prefix + series.astype("string"),
            errors="coerce",
        )

    creation_dt = to_dt(frame["Incident Creation Time (GMT)"])
    dispatch_dt = to_dt(frame["Time of Dispatch (GMT)"])
    enroute_dt = to_dt(frame["En Route Time (GMT)"])
    on_scene_dt = to_dt(frame["On Scene Time (GMT)"])

    frame["dispatch_delay_s"] = (
        (dispatch_dt - creation_dt).dt.total_seconds()
    )
    frame["enroute_delay_s"] = (
        (enroute_dt - dispatch_dt).dt.total_seconds()
    )
    frame["arrival_delay_s"] = (
        (on_scene_dt - enroute_dt).dt.total_seconds()
    )
    frame["total_response_s"] = (
        (on_scene_dt - creation_dt).dt.total_seconds()
    )

    features = frame[
        [
            "unit_type",
            "dispatch_status",
            "emergency_dispatch_code",
            "first_in_district",
            "dispatch_sequence",
            "dispatch_delay_s",
            "enroute_delay_s",
            "arrival_delay_s",
            "total_response_s",
        ]
    ].copy()
    mask = (
        features["dispatch_delay_s"].notna()
        & features["total_response_s"].notna()
        & (features["dispatch_delay_s"] >= 0)
        & (features["total_response_s"] > 0)
    )
    filtered = features.loc[mask].copy()
    return filtered


def concat_frames(frames: Iterable[pd.DataFrame], columns: Sequence[str]) -> pd.DataFrame:
    collected = [frame for frame in frames if not frame.empty]
    if not collected:
        return pd.DataFrame(columns=columns)
    return pd.concat(collected, ignore_index=True)


def stratified_sample(
    df: pd.DataFrame, columns: Sequence[str], desired: int, seed: int
) -> pd.DataFrame:
    if df.empty:
        return df
    if desired <= 0 or desired >= len(df):
        return df.sample(frac=1.0, random_state=seed).reset_index(drop=True)

    df = df.reset_index(drop=True)
    grouped = list(df.groupby(list(columns), dropna=False, sort=False))
    total_rows = len(df)
    rng = random.Random(seed)
    selected: list[int] = []

    for idx, (_, group) in enumerate(grouped):
        part_rows = len(group)
        if part_rows == 0:
            continue
        remaining_groups = len(grouped) - idx
        take = round((part_rows / total_rows) * desired)
        take = max(1, min(part_rows, take))
        if idx == len(grouped) - 1 or len(selected) + take > desired or remaining_groups == 1:
            take = min(desired - len(selected), part_rows)
        if take <= 0:
            continue
        sample = group.sample(
            n=take,
            random_state=rng.randint(0, 1_000_000_000),
        )
        selected.extend(sample.index.tolist())
        if len(selected) >= desired:
            break

    remaining = desired - len(selected)
    if remaining > 0:
        selected_set = set(selected)
        pool = [idx for idx in range(len(df)) if idx not in selected_set]
        if pool:
            extra = rng.sample(pool, k=min(len(pool), remaining))
            selected.extend(extra)

    sampled = df.iloc[selected]
    return sampled.sample(frac=1.0, random_state=seed).head(desired).reset_index(drop=True)


def build_category_map(df: pd.DataFrame, columns: Sequence[str]) -> Dict[str, List[str]]:
    mapping: Dict[str, List[str]] = {}
    for column in columns:
        if column not in df.columns:
            continue
        counts = (
            df[column]
            .dropna()
            .astype(str)
            .value_counts()
        )
        mapping[column] = counts.index.tolist()
    return mapping


def prep_apd(
    apd_path: Path,
    sample_size: int,
    seed: int,
    chunk_size: int,
) -> tuple[pd.DataFrame, DatasetMetrics, Dict[str, List[str]]]:
    console.log(f"[cyan]Loading APD CSV from {apd_path}[/cyan]")
    start = time.perf_counter()
    frames: list[pd.DataFrame] = []
    for chunk in pd.read_csv(apd_path, usecols=APD_CHUNK_COLUMNS, chunksize=chunk_size):
        frames.append(transform_apd_chunk(chunk))
    features = concat_frames(frames, APD_FEATURE_COLUMNS)
    filtered_rows = len(features)
    console.log(f"[cyan]APD rows after filters: {filtered_rows:,}[/cyan]")
    sampled = stratified_sample(
        features, ["report_written", "priority_level_ord"], sample_size, seed
    )
    duration_ms = int((time.perf_counter() - start) * 1000)
    metrics = DatasetMetrics(
        name="apd_dispatch",
        source_path=str(apd_path),
        year_filter=YEAR_FILTER,
        rows_after_filters=filtered_rows,
        rows_sampled=len(sampled),
        duration_ms=duration_ms,
        output_path="",
        feature_columns=list(APD_FEATURE_COLUMNS),
    )
    categories = build_category_map(
        sampled, ["incident_type", "mental_health_flag", "call_disposition"]
    )
    return sampled, metrics, categories


def prep_lafd(
    lafd_path: Path,
    sample_size: int,
    seed: int,
    chunk_size: int,
) -> tuple[pd.DataFrame, DatasetMetrics, Dict[str, List[str]]]:
    console.log(f"[cyan]Loading LAFD CSV from {lafd_path}[/cyan]")
    start = time.perf_counter()
    frames: list[pd.DataFrame] = []
    for chunk in pd.read_csv(lafd_path, usecols=LAFD_CHUNK_COLUMNS, chunksize=chunk_size):
        frames.append(transform_lafd_chunk(chunk))
    features = concat_frames(frames, LAFD_FEATURE_COLUMNS)
    filtered_rows = len(features)
    console.log(f"[cyan]LAFD rows after filters: {filtered_rows:,}[/cyan]")
    sampled = stratified_sample(features, ["unit_type"], sample_size, seed ^ 0xABCDEF)
    duration_ms = int((time.perf_counter() - start) * 1000)
    metrics = DatasetMetrics(
        name="lafd_response",
        source_path=str(lafd_path),
        year_filter=YEAR_FILTER,
        rows_after_filters=filtered_rows,
        rows_sampled=len(sampled),
        duration_ms=duration_ms,
        output_path="",
        feature_columns=list(LAFD_FEATURE_COLUMNS),
    )
    categories = build_category_map(
        sampled, ["unit_type", "dispatch_status", "emergency_dispatch_code"]
    )
    return sampled, metrics, categories


def save_parquet(frame: pd.DataFrame, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    frame.to_parquet(path, index=False)


def git_rev() -> str:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout.strip()
    except Exception:
        return "unknown"


def load_cost_model(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def bench_entries(
    bench: dict, pipeline_type: str, cost_model: dict, git_sha: str
) -> List[dict]:
    compute_cost = cost_model.get("compute_cost_per_hour", 0.0)
    entries: List[dict] = []
    for key in ("apd", "lafd"):
        dataset = bench.get(key, {})
        duration_ms = dataset.get("duration_ms")
        if duration_ms is None:
            continue
        duration_seconds = duration_ms / 1000.0
        rows_after = dataset.get("rows_after_filters", 0)
        rows_sampled = dataset.get("rows_sampled", 0)
        year_filter = dataset.get("year_filter")
        cost_estimate = (duration_seconds / 3600.0) * compute_cost
        entries.append(
            {
                "run_id": f"{pipeline_type}-{key}-{bench.get('generated_at_utc', 'na')}",
                "pipeline_type": pipeline_type,
                "dataset": key,
                "year_filter": year_filter,
                "rows_input": rows_after,
                "rows_input_2024": rows_after if year_filter == YEAR_FILTER else None,
                "rows_output": rows_sampled,
                "rows_output_2024": rows_sampled if year_filter == YEAR_FILTER else None,
                "duration_seconds": duration_seconds,
                "cpu_peak_pct": None,
                "mem_peak_gb": None,
                "row_cap_applied": rows_after > rows_sampled,
                "cost_estimate_usd": cost_estimate,
                "git_rev": git_sha,
            }
        )
    return entries


def append_history(entries: List[dict], history_path: Path) -> None:
    history: List[dict] = []
    if history_path.exists():
        history = json.loads(history_path.read_text(encoding="utf-8"))
    history.extend(entries)
    save_json(history_path, history)


@app.command("control-prep")
def control_prep(
    apd_file: Path = typer.Option(
        DEFAULT_APD_CSV, help="Path to the APD dispatch CSV file."
    ),
    lafd_file: Path = typer.Option(
        DEFAULT_LAFD_CSV, help="Path to the LAFD response CSV file."
    ),
    output_dir: Path = typer.Option(
        DEFAULT_OUTPUT_DIR, help="Directory for Parquet + benchmark outputs."
    ),
    apd_sample: int = typer.Option(120_000, help="APD rows to sample."),
    lafd_sample: int = typer.Option(120_000, help="LAFD rows to sample."),
    seed: int = typer.Option(42, help="Seed for sampling."),
    chunk_size: int = typer.Option(250_000, help="Chunk size for CSV streaming."),
    tag: str = typer.Option(
        "control",
        help="Suffix used for Parquet + category map names (apd_<tag>_sample.parquet).",
    ),
    history_path: Path = typer.Option(
        DEFAULT_HISTORY,
        help="Path to append run history entries (JSON array).",
    ),
    cost_model_path: Path = typer.Option(
        DEFAULT_COST_MODEL,
        help="Cost model JSON for estimating run cost.",
    ),
) -> None:
    """
    Run the pandas-based control prep to mirror the Rust Polars pipeline.
    """

    output_dir.mkdir(parents=True, exist_ok=True)
    overall_start = time.perf_counter()

    apd_frame, apd_metrics, apd_categories = prep_apd(
        apd_file, apd_sample, seed, chunk_size
    )
    apd_output = output_dir / f"apd_{tag}_sample.parquet"
    save_parquet(apd_frame, apd_output)
    apd_metrics.output_path = str(apd_output)
    console.log(f"[green]APD control sample written to {apd_output}[/green]")

    lafd_frame, lafd_metrics, lafd_categories = prep_lafd(
        lafd_file, lafd_sample, seed, chunk_size
    )
    lafd_output = output_dir / f"lafd_{tag}_sample.parquet"
    save_parquet(lafd_frame, lafd_output)
    lafd_metrics.output_path = str(lafd_output)
    console.log(f"[green]LAFD control sample written to {lafd_output}[/green]")

    category_path = output_dir / f"category_maps_{tag}.json"
    save_json(category_path, {"apd": apd_categories, "lafd": lafd_categories})
    console.log(f"[green]Category map saved to {category_path}[/green]")

    bench = {
        "generated_at_utc": utc_now(),
        "total_duration_ms": int((time.perf_counter() - overall_start) * 1000),
        "apd": asdict(apd_metrics),
        "lafd": asdict(lafd_metrics),
        "command": [
            Path(sys.executable).name,
            "-m",
            "py_prep.cli",
            *sys.argv[1:],
        ],
    }
    bench_path = output_dir / "prep_python_bench.json"
    save_json(bench_path, bench)
    console.log(f"[green]Benchmark written to {bench_path}[/green]")

    cost_model = load_cost_model(cost_model_path)
    git_sha = git_rev()
    entries = bench_entries(bench, "python_prep", cost_model, git_sha)
    append_history(entries, history_path)
    console.log(f"[green]Appended {len(entries)} run history entries.[/green]")


def read_parquet(path: Path) -> pd.DataFrame:
    return pd.read_parquet(path)


def compare_dataset(
    variant_path: Path,
    control_path: Path,
    columns: Sequence[str],
) -> List[dict]:
    variant = read_parquet(variant_path)
    control = read_parquet(control_path)
    summary: List[dict] = []
    for column in columns:
        summary.append(
            {
                "column": column,
                "python_nulls": int(control[column].isna().sum()) if column in control else None,
                "rust_nulls": int(variant[column].isna().sum()) if column in variant else None,
                "python_distinct": int(control[column].nunique(dropna=True)) if column in control else None,
                "rust_distinct": int(variant[column].nunique(dropna=True)) if column in variant else None,
            }
        )
    return summary


@app.command("compare-quality")
def compare_quality(
    rust_apd: Path = typer.Option(DEFAULT_RUST_APD, help="Rust APD Parquet."),
    control_apd: Path = typer.Option(
        DEFAULT_CONTROL_APD, help="Python control APD Parquet."
    ),
    rust_lafd: Path = typer.Option(DEFAULT_RUST_LAFD, help="Rust LAFD Parquet."),
    control_lafd: Path = typer.Option(
        DEFAULT_CONTROL_LAFD, help="Python control LAFD Parquet."
    ),
    output_path: Path = typer.Option(
        DEFAULT_DATA_QUALITY, help="Output JSON for data quality diffs."
    ),
) -> None:
    """
    Compare null + distinct counts between Rust and Python feature tables.
    """

    payload = {
        "apd": compare_dataset(rust_apd, control_apd, APD_FEATURE_COLUMNS),
        "lafd": compare_dataset(rust_lafd, control_lafd, LAFD_FEATURE_COLUMNS),
    }
    save_json(output_path, payload)
    console.log(f"[green]Data quality summary saved to {output_path}[/green]")


def pipeline_stats(entries: List[dict], pipeline_type: str) -> dict | None:
    durations = [
        entry["duration_seconds"]
        for entry in entries
        if entry.get("pipeline_type") == pipeline_type
    ]
    if not durations:
        return None
    mean_seconds = statistics.mean(durations)
    std_seconds = statistics.pstdev(durations) if len(durations) > 1 else 0.0
    return {"mean_seconds": mean_seconds, "std_seconds": std_seconds}


def build_scaling(entries: List[dict]) -> List[dict]:
    points: dict[int, dict] = {}
    for entry in entries:
        rows = entry.get("rows_input")
        if rows is None:
            continue
        bucket = points.setdefault(rows, {"rows": rows})
        key = (
            "python_seconds"
            if entry.get("pipeline_type") == "python_prep"
            else "rust_seconds"
        )
        bucket.setdefault(key, []).append(entry.get("duration_seconds", 0.0))

    scaling: List[dict] = []
    for rows, data in sorted(points.items(), key=lambda item: item[0]):
        record = {"rows": rows}
        python_durations = data.get("python_seconds")
        rust_durations = data.get("rust_seconds")
        if python_durations:
            record["python_seconds"] = statistics.mean(python_durations)
        else:
            record["python_seconds"] = None
        if rust_durations:
            record["rust_seconds"] = statistics.mean(rust_durations)
        else:
            record["rust_seconds"] = None
        scaling.append(record)
    return scaling


@app.command("summarize-variance")
def summarize_variance(
    history_path: Path = typer.Option(
        DEFAULT_HISTORY, help="Prep history JSON used to compute variance."
    ),
    output_path: Path = typer.Option(
        DEFAULT_VARIANCE, help="Destination for run variance summary."
    ),
) -> None:
    """
    Aggregate run history and emit run variance JSON for the dashboard.
    """

    if not history_path.exists():
        raise typer.BadParameter(f"No history found at {history_path}")
    entries = json.loads(history_path.read_text(encoding="utf-8"))
    payload = {
        "runs": len(entries),
        "rows": sum(entry.get("rows_output", 0) for entry in entries),
        "python": pipeline_stats(entries, "python_prep"),
        "rust": pipeline_stats(entries, "rust_prep"),
        "scaling": build_scaling(entries),
    }
    save_json(output_path, payload)
    console.log(f"[green]Run variance summary saved to {output_path}[/green]")


@app.command("ingest-run")
def ingest_run(
    bench_path: Path = typer.Argument(
        ..., help="Path to a prep benchmark JSON file."
    ),
    pipeline_type: str = typer.Option(
        ...,
        "--pipeline",
        "-p",
        help="Pipeline label to store in history (e.g., python_prep or rust_prep).",
    ),
    history_path: Path = typer.Option(
        DEFAULT_HISTORY,
        help="History JSON that accumulates runs.",
    ),
    cost_model_path: Path = typer.Option(
        DEFAULT_COST_MODEL, help="Cost model JSON for estimating compute spend."
    ),
) -> None:
    """
    Convert a prep benchmark JSON into history entries (one per dataset).
    """

    bench = json.loads(bench_path.read_text(encoding="utf-8"))
    cost_model = load_cost_model(cost_model_path)
    git_sha = git_rev()
    entries = bench_entries(bench, pipeline_type, cost_model, git_sha)
    append_history(entries, history_path)
    console.log(
        f"[green]Loaded {len(entries)} entries from {bench_path} into {history_path}[/green]"
    )


if __name__ == "__main__":
    app()
