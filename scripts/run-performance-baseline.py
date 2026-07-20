#!/usr/bin/env python3
"""Run the reproducible M6 scalar baseline or an interleaved A/B comparison."""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import math
import os
import platform
import random
import re
import statistics
import subprocess
import sys
import tempfile
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Sequence


RUN_SCHEMA_VERSION = 1
WORKER_SCHEMA_VERSION = 1
DEFAULT_CORPUS = Path("target/performance-corpus/m6-performance-baseline-v1")
DEFAULT_RESULTS = Path("target/performance-results")
DEFAULT_SEED = 0x4D36_0001
DEFAULT_SAMPLES = 7
DEFAULT_WARMUPS = 1
DEFAULT_SAMPLE_INTERVAL_MS = 10
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
VARIANT_PATTERN = re.compile(r"^[a-z0-9][a-z0-9_-]{0,31}$")
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40,64}$")


class BaselineError(RuntimeError):
    """A benchmark protocol, environment, or result validation failure."""


@dataclasses.dataclass(frozen=True)
class BenchmarkCase:
    case_id: str
    scope: str
    description: str
    arguments: tuple[str, ...]

    @property
    def mode(self) -> str:
        return self.arguments[0]


def suite_cases(corpus: Path) -> tuple[BenchmarkCase, ...]:
    def media(name: str) -> str:
        return str((corpus / name).resolve())

    return (
        BenchmarkCase(
            "analysis/stereo-600s",
            "analysis",
            "Direct finite f64 AnalyzerSession, 2 channels, 600 audio seconds",
            ("analysis", "2", "48000", str(600 * 48_000), "4096"),
        ),
        BenchmarkCase(
            "analysis/8ch-180s",
            "analysis",
            "Direct finite f64 AnalyzerSession, 8 channels, 180 audio seconds",
            ("analysis", "8", "48000", str(180 * 48_000), "4096"),
        ),
        BenchmarkCase(
            "analysis/64ch-30s",
            "analysis",
            "Maximum product geometry, 64 channels, 30 audio seconds",
            ("analysis", "64", "48000", str(30 * 48_000), "4096"),
        ),
        BenchmarkCase(
            "decode/wave-s16",
            "decode",
            "Content probe plus complete WAV PCM integer decoding",
            ("decode", media("stereo-s16-60s.wav"), "12"),
        ),
        BenchmarkCase(
            "decode/aiff-s16",
            "decode",
            "Content probe plus complete AIFF PCM integer decoding",
            ("decode", media("stereo-s16-60s.aiff"), "12"),
        ),
        BenchmarkCase(
            "decode/flac-s16",
            "decode",
            "Content probe plus complete FLAC decoding",
            ("decode", media("stereo-s16-60s.flac"), "4"),
        ),
        BenchmarkCase(
            "decode/wave-f64",
            "decode",
            "Content probe plus complete WAV IEEE float64 decoding",
            ("decode", media("stereo-f64-60s.wav"), "10"),
        ),
        BenchmarkCase(
            "application/wave-s16",
            "application",
            "Application reservation, WAV decoding, analysis, and report construction",
            ("application", media("stereo-s16-60s.wav"), "8"),
        ),
        BenchmarkCase(
            "application/aiff-s16",
            "application",
            "Application reservation, AIFF decoding, analysis, and report construction",
            ("application", media("stereo-s16-60s.aiff"), "8"),
        ),
        BenchmarkCase(
            "application/flac-s16",
            "application",
            "Application reservation, FLAC decoding, analysis, and report construction",
            ("application", media("stereo-s16-60s.flac"), "3"),
        ),
        BenchmarkCase(
            "application/wave-f64",
            "application",
            "Application reservation, WAV float64 decoding, analysis, and report construction",
            ("application", media("stereo-f64-60s.wav"), "8"),
        ),
        BenchmarkCase(
            "application/wave-s24-6ch",
            "application",
            "Application path at a representative six-channel geometry",
            ("application", media("surround-s24-6ch-30s.wav"), "6"),
        ),
        BenchmarkCase(
            "batch/8-wave-tracks",
            "batch",
            "Current serial application batch over eight independent WAV tracks",
            ("batch", media("batch"), "4"),
        ),
        BenchmarkCase(
            "discovery/1024-supported",
            "discovery",
            "Recursive warm/OS-managed discovery over 1024 supported and 256 ignored files",
            ("discovery", media("discovery"), "128"),
        ),
        BenchmarkCase(
            "render/wire-json",
            "rendering",
            "Pretty JSON wire-v3 rendering with analysis outside the timed region",
            ("render-json", media("stereo-s16-60s.wav"), "50000"),
        ),
    )


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json_bytes(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def command_output(command: Sequence[str], *, required: bool = False) -> str | None:
    try:
        completed = subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        if required:
            raise BaselineError(f"command failed: {command!r}: {error}") from error
        return None
    return completed.stdout.strip()


def git_identity(root: Path, allow_dirty: bool) -> dict[str, str]:
    commit = command_output(
        ("git", "-C", str(root), "rev-parse", "HEAD"), required=True
    )
    assert commit is not None
    status = command_output(
        (
            "git",
            "-C",
            str(root),
            "status",
            "--porcelain",
            "--untracked-files=all",
        ),
        required=True,
    )
    state = "dirty" if status else "clean"
    if state == "dirty" and not allow_dirty:
        raise BaselineError(
            "the performance baseline requires a clean worktree; "
            "use --allow-dirty only for non-authoritative harness development"
        )
    return {"commit": commit, "state": state}


def build_default_worker(root: Path) -> Path:
    subprocess.run(
        (
            "cargo",
            "build",
            "--locked",
            "--release",
            "-p",
            "macinmeter",
            "--example",
            "m6_baseline_worker",
        ),
        cwd=root,
        check=True,
    )
    worker = root / "target/release/examples/m6_baseline_worker"
    if not worker.is_file() or not os.access(worker, os.X_OK):
        raise BaselineError(f"release worker was not produced at {worker}")
    return worker


def parse_variant(specification: str) -> tuple[str, Path]:
    name, separator, raw_path = specification.partition("=")
    if not separator or not VARIANT_PATTERN.fullmatch(name):
        raise BaselineError(
            f"invalid variant {specification!r}; expected lower-case NAME=EXECUTABLE"
        )
    path = Path(raw_path).expanduser().resolve()
    if not path.is_file() or not os.access(path, os.X_OK):
        raise BaselineError(f"variant {name!r} is not an executable file: {path}")
    return name, path


def parse_variant_source(specification: str) -> tuple[str, str]:
    name, separator, commit = specification.partition("=")
    if (
        not separator
        or not VARIANT_PATTERN.fullmatch(name)
        or not COMMIT_PATTERN.fullmatch(commit)
    ):
        raise BaselineError(
            f"invalid variant source {specification!r}; "
            "expected lower-case NAME=40_TO_64_HEX_COMMIT"
        )
    return name, commit


def ensure_finite_json(value: object, path: str = "$") -> None:
    if isinstance(value, float) and not math.isfinite(value):
        raise BaselineError(f"non-finite numeric value at {path}")
    if isinstance(value, dict):
        for key, item in value.items():
            ensure_finite_json(item, f"{path}.{key}")
    elif isinstance(value, list):
        for index, item in enumerate(value):
            ensure_finite_json(item, f"{path}[{index}]")


def parse_worker_output(raw: bytes, expected_mode: str) -> dict[str, Any]:
    try:
        text = raw.decode("utf-8")
        value = json.loads(text)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise BaselineError(f"worker stdout is not one valid UTF-8 JSON value: {error}") from error
    if not isinstance(value, dict):
        raise BaselineError("worker output is not a JSON object")
    ensure_finite_json(value)
    if value.get("schemaVersion") != WORKER_SCHEMA_VERSION:
        raise BaselineError("worker schema version drifted")
    if value.get("mode") != expected_mode.replace("-", "_"):
        raise BaselineError(
            f"worker mode {value.get('mode')!r} does not match {expected_mode!r}"
        )
    elapsed = value.get("workerElapsedNs")
    if not isinstance(elapsed, int) or isinstance(elapsed, bool) or elapsed <= 0:
        raise BaselineError("worker elapsed time is not a positive integer")
    fingerprint = value.get("resultFingerprintSha256")
    if not isinstance(fingerprint, str) or not SHA256_PATTERN.fullmatch(fingerprint):
        raise BaselineError("worker result fingerprint is not SHA-256")
    work = value.get("work")
    if not isinstance(work, dict):
        raise BaselineError("worker work-unit object is missing")
    for key in ("iterations", "audioFrames", "interleavedSamples", "logicalItems"):
        if not isinstance(work.get(key), int) or isinstance(work.get(key), bool):
            raise BaselineError(f"worker work unit {key!r} is not an integer")
    audio_seconds = work.get("audioSeconds")
    if not isinstance(audio_seconds, (int, float)) or audio_seconds < 0:
        raise BaselineError("worker audioSeconds is invalid")
    return value


DARWIN_TIME_FIRST_LINE = re.compile(
    r"^\s*([0-9.]+) real\s+([0-9.]+) user\s+([0-9.]+) sys\s*$"
)
DARWIN_TIME_VALUE = re.compile(r"^\s*(\d+)\s+(.+?)\s*$")


def parse_darwin_time(text: str) -> dict[str, int | float]:
    lines = text.splitlines()
    if not lines:
        raise BaselineError("Darwin time output is empty")
    first = DARWIN_TIME_FIRST_LINE.match(lines[0])
    if first is None:
        raise BaselineError(f"cannot parse Darwin time summary: {lines[0]!r}")
    result: dict[str, int | float] = {
        "realSeconds": float(first.group(1)),
        "userSeconds": float(first.group(2)),
        "systemSeconds": float(first.group(3)),
    }
    names = {
        "maximum resident set size": "maxResidentSetBytes",
        "peak memory footprint": "peakMemoryFootprintBytes",
        "instructions retired": "instructionsRetired",
        "cycles elapsed": "cyclesElapsed",
        "page faults": "pageFaults",
        "swaps": "swaps",
        "block input operations": "blockInputOperations",
        "block output operations": "blockOutputOperations",
        "voluntary context switches": "voluntaryContextSwitches",
        "involuntary context switches": "involuntaryContextSwitches",
    }
    for line in lines[1:]:
        match = DARWIN_TIME_VALUE.match(line)
        if match is not None and match.group(2) in names:
            result[names[match.group(2)]] = int(match.group(1))
    if "maxResidentSetBytes" not in result:
        raise BaselineError("Darwin time output omitted maximum resident set size")
    return result


def parse_gnu_time(text: str) -> dict[str, int | float]:
    values: dict[str, str] = {}
    for line in text.splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            values[key.strip()] = value.strip()
    required = (
        "Elapsed (wall clock) time (h:mm:ss or m:ss)",
        "User time (seconds)",
        "System time (seconds)",
        "Maximum resident set size (kbytes)",
    )
    if any(key not in values for key in required):
        raise BaselineError("GNU time output omitted required metrics")
    elapsed_parts = values[required[0]].split(":")
    if len(elapsed_parts) == 2:
        real_seconds = float(elapsed_parts[0]) * 60 + float(elapsed_parts[1])
    elif len(elapsed_parts) == 3:
        real_seconds = (
            float(elapsed_parts[0]) * 3600
            + float(elapsed_parts[1]) * 60
            + float(elapsed_parts[2])
        )
    else:
        raise BaselineError("GNU time wall-clock value has an unknown shape")
    return {
        "realSeconds": real_seconds,
        "userSeconds": float(values[required[1]]),
        "systemSeconds": float(values[required[2]]),
        "maxResidentSetBytes": int(values[required[3]]) * 1024,
        "pageFaults": int(values.get("Major (requiring I/O) page faults", "0")),
        "swaps": int(values.get("Swaps", "0")),
        "voluntaryContextSwitches": int(values.get("Voluntary context switches", "0")),
        "involuntaryContextSwitches": int(
            values.get("Involuntary context switches", "0")
        ),
    }


def parse_ps_rows(text: str) -> dict[int, tuple[int, int]]:
    rows: dict[int, tuple[int, int]] = {}
    for line in text.splitlines():
        fields = line.split()
        if len(fields) != 3:
            continue
        try:
            pid, parent, rss_kib = (int(field) for field in fields)
        except ValueError:
            continue
        rows[pid] = (parent, rss_kib * 1024)
    return rows


def descendant_rss(rows: dict[int, tuple[int, int]], root_pid: int) -> tuple[int, int]:
    descendants: set[int] = set()
    changed = True
    while changed:
        changed = False
        for pid, (parent, _) in rows.items():
            if pid != root_pid and parent in descendants | {root_pid} and pid not in descendants:
                descendants.add(pid)
                changed = True
    return sum(rows[pid][1] for pid in descendants), len(descendants)


def sample_process_tree(root_pid: int) -> tuple[int, int]:
    completed = subprocess.run(
        ("ps", "-axo", "pid=,ppid=,rss="),
        check=True,
        capture_output=True,
        text=True,
    )
    return descendant_rss(parse_ps_rows(completed.stdout), root_pid)


def time_command_prefix(metrics_path: Path) -> tuple[list[str], str]:
    if sys.platform == "darwin":
        return ["/usr/bin/time", "-l", "-o", str(metrics_path)], "darwin_time_l"
    if sys.platform.startswith("linux"):
        return ["/usr/bin/time", "-v", "-o", str(metrics_path)], "gnu_time_v"
    raise BaselineError(
        "process-tree/RSS baseline currently supports macOS and Linux only"
    )


def run_sample(
    worker: Path,
    case: BenchmarkCase,
    sample_interval_seconds: float,
) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="macinmeter-m6-time-") as temporary:
        metrics_path = Path(temporary) / "time.txt"
        prefix, timer_kind = time_command_prefix(metrics_path)
        command = [*prefix, str(worker), *case.arguments]
        started_ns = time.monotonic_ns()
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        peak_tree_rss = 0
        peak_tree_processes = 0
        tree_samples = 0
        while process.poll() is None:
            try:
                rss, processes = sample_process_tree(process.pid)
            except (OSError, subprocess.CalledProcessError):
                rss, processes = 0, 0
            peak_tree_rss = max(peak_tree_rss, rss)
            peak_tree_processes = max(peak_tree_processes, processes)
            tree_samples += 1
            time.sleep(sample_interval_seconds)
        stdout, stderr = process.communicate()
        runner_wall_ns = time.monotonic_ns() - started_ns
        if process.returncode != 0:
            raise BaselineError(
                f"{case.case_id} worker exited {process.returncode}: "
                f"{stderr.decode('utf-8', errors='replace').strip()}"
            )
        if stderr:
            raise BaselineError(
                f"{case.case_id} worker wrote unexpected stderr: "
                f"{stderr.decode('utf-8', errors='replace').strip()}"
            )
        output = parse_worker_output(stdout, case.mode)
        if peak_tree_rss <= 0 or peak_tree_processes <= 0 or tree_samples <= 0:
            raise BaselineError(
                f"{case.case_id} completed without a usable process-tree RSS sample"
            )
        try:
            native_text = metrics_path.read_text(encoding="utf-8")
        except OSError as error:
            raise BaselineError(f"cannot read native time metrics: {error}") from error
        native = (
            parse_darwin_time(native_text)
            if timer_kind == "darwin_time_l"
            else parse_gnu_time(native_text)
        )
        return {
            "workerElapsedNs": output["workerElapsedNs"],
            "runnerWallNs": runner_wall_ns,
            "nativeMetrics": native,
            "processTree": {
                "peakRssBytes": peak_tree_rss,
                "peakProcesses": peak_tree_processes,
                "samples": tree_samples,
                "samplingIntervalMs": sample_interval_seconds * 1000,
                "measurementWrapperExcluded": True,
            },
            "work": output["work"],
            "resultFingerprintSha256": output["resultFingerprintSha256"],
            "resultBytes": output["resultBytes"],
            "details": output["details"],
        }


def randomized_schedule(
    cases: Sequence[BenchmarkCase],
    variants: Sequence[str],
    repetitions: int,
    seed: int,
) -> list[tuple[BenchmarkCase, str, int]]:
    schedule = [
        (case, variant, repetition)
        for repetition in range(repetitions)
        for case in cases
        for variant in variants
    ]
    random.Random(seed).shuffle(schedule)
    return schedule


def percentile(values: Sequence[float], quantile: float) -> float:
    if not values:
        raise BaselineError("cannot compute a percentile of no values")
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * quantile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    fraction = position - lower
    return ordered[lower] * (1.0 - fraction) + ordered[upper] * fraction


def distribution(values: Sequence[float]) -> dict[str, float]:
    if not values:
        raise BaselineError("cannot summarize no samples")
    median = statistics.median(values)
    deviations = [abs(value - median) for value in values]
    return {
        "min": min(values),
        "p10": percentile(values, 0.10),
        "median": median,
        "p90": percentile(values, 0.90),
        "max": max(values),
        "medianAbsoluteDeviation": statistics.median(deviations),
    }


def summarize_samples(samples: Sequence[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for sample in samples:
        grouped[(sample["caseId"], sample["variant"])].append(sample)

    summaries: list[dict[str, Any]] = []
    for (case_id, variant), group in sorted(grouped.items()):
        worker_ns = [float(sample["workerElapsedNs"]) for sample in group]
        tree_rss = [
            float(sample["processTree"]["peakRssBytes"]) for sample in group
        ]
        native_rss = [
            float(sample["nativeMetrics"]["maxResidentSetBytes"]) for sample in group
        ]
        user_seconds = [
            float(sample["nativeMetrics"]["userSeconds"]) for sample in group
        ]
        system_seconds = [
            float(sample["nativeMetrics"]["systemSeconds"]) for sample in group
        ]
        work = group[0]["work"]
        audio_seconds = float(work["audioSeconds"])
        interleaved_samples = float(work["interleavedSamples"])
        logical_items = float(work["logicalItems"])
        throughput: dict[str, object] = {}
        if audio_seconds > 0:
            throughput["audioSecondsPerSecond"] = distribution(
                [
                    audio_seconds / (float(sample["workerElapsedNs"]) / 1_000_000_000)
                    for sample in group
                ]
            )
        if interleaved_samples > 0:
            throughput["millionSamplesPerSecond"] = distribution(
                [
                    interleaved_samples
                    / (float(sample["workerElapsedNs"]) / 1_000_000_000)
                    / 1_000_000
                    for sample in group
                ]
            )
        if logical_items > 0:
            throughput["logicalItemsPerSecond"] = distribution(
                [
                    logical_items
                    / (float(sample["workerElapsedNs"]) / 1_000_000_000)
                    for sample in group
                ]
            )

        fingerprints = {sample["resultFingerprintSha256"] for sample in group}
        work_shapes = {canonical_json_bytes(sample["work"]) for sample in group}
        details = {canonical_json_bytes(sample["details"]) for sample in group}
        if len(fingerprints) != 1 or len(work_shapes) != 1 or len(details) != 1:
            raise BaselineError(f"{case_id}/{variant} output changed across samples")
        summaries.append(
            {
                "caseId": case_id,
                "variant": variant,
                "samples": len(group),
                "workerElapsedNs": distribution(worker_ns),
                "processTreePeakRssBytes": distribution(tree_rss),
                "nativeMaxResidentSetBytes": distribution(native_rss),
                "nativeUserSeconds": distribution(user_seconds),
                "nativeSystemSeconds": distribution(system_seconds),
                "throughput": throughput,
                "work": work,
                "resultFingerprintSha256": next(iter(fingerprints)),
                "outliersRemoved": 0,
            }
        )
    return summaries


def validate_cross_variant_fingerprints(
    samples: Sequence[dict[str, Any]],
) -> None:
    by_case: dict[str, set[str]] = defaultdict(set)
    for sample in samples:
        by_case[sample["caseId"]].add(sample["resultFingerprintSha256"])
    mismatches = {
        case_id: sorted(fingerprints)
        for case_id, fingerprints in by_case.items()
        if len(fingerprints) != 1
    }
    if mismatches:
        raise BaselineError(f"cross-variant result fingerprints differ: {mismatches}")


def validate_corpus_work(
    samples: Sequence[dict[str, Any]],
    cases: Sequence[BenchmarkCase],
    corpus: Path,
    manifest: dict[str, object],
) -> None:
    raw_media = manifest.get("media")
    if not isinstance(raw_media, list):
        raise BaselineError("performance corpus manifest has no media list")
    media: dict[str, dict[str, object]] = {}
    for entry in raw_media:
        if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
            raise BaselineError("performance corpus media entry is invalid")
        media[entry["path"]] = entry
    by_case = {case.case_id: case for case in cases}
    analysis_fingerprints: dict[str, set[str]] = defaultdict(set)

    for sample in samples:
        case = by_case[sample["caseId"]]
        work = sample["work"]
        details = sample["details"]
        if case.mode == "analysis":
            channels = int(case.arguments[1])
            sample_rate = int(case.arguments[2])
            frames = int(case.arguments[3])
            assert_work_units(
                case.case_id,
                work,
                frames=frames,
                samples=frames * channels,
                seconds=frames / sample_rate,
                logical_items=1,
            )
            continue

        if case.mode in ("decode", "application", "render-json"):
            path = Path(case.arguments[1])
            try:
                relative = path.resolve().relative_to(corpus.resolve()).as_posix()
            except ValueError as error:
                raise BaselineError(
                    f"{case.case_id} input is outside the declared corpus"
                ) from error
            entry = media.get(relative)
            if entry is None:
                raise BaselineError(
                    f"{case.case_id} input is absent from the corpus manifest: {relative}"
                )
            iterations = int(case.arguments[2])
            if case.mode == "render-json":
                assert_work_units(
                    case.case_id,
                    work,
                    frames=0,
                    samples=0,
                    seconds=0.0,
                    logical_items=iterations,
                )
                continue
            frames = required_manifest_int(entry, "frames")
            channels = required_manifest_int(entry, "channels")
            sample_rate = required_manifest_int(entry, "sampleRate")
            assert_work_units(
                case.case_id,
                work,
                frames=frames * iterations,
                samples=frames * channels * iterations,
                seconds=(frames / sample_rate) * iterations,
                logical_items=iterations,
            )
            if case.mode == "decode":
                expected_pcm = entry.get("normalizedInterleavedF64LeSha256")
                if details.get("pcmF64LeSha256") != expected_pcm:
                    raise BaselineError(
                        f"{case.case_id} decoded PCM fingerprint does not match the corpus oracle"
                    )
            else:
                if details.get("decodedFramesPerIteration") != frames:
                    raise BaselineError(
                        f"{case.case_id} application frame count drifted"
                    )
                analysis_fingerprint = details.get(
                    "analysisResultFingerprintSha256"
                )
                if (
                    not isinstance(analysis_fingerprint, str)
                    or not SHA256_PATTERN.fullmatch(analysis_fingerprint)
                ):
                    raise BaselineError(
                        f"{case.case_id} application analysis fingerprint is invalid"
                    )
                pcm_oracle = entry.get("normalizedInterleavedF64LeSha256")
                if not isinstance(pcm_oracle, str):
                    raise BaselineError(
                        f"{case.case_id} corpus PCM oracle is invalid"
                    )
                analysis_fingerprints[pcm_oracle].add(analysis_fingerprint)
            continue

        if case.mode == "batch":
            iterations = int(case.arguments[2])
            batch_entries = [
                entry
                for relative, entry in media.items()
                if relative.startswith("batch/")
            ]
            frames = sum(required_manifest_int(entry, "frames") for entry in batch_entries)
            interleaved_samples = sum(
                required_manifest_int(entry, "frames")
                * required_manifest_int(entry, "channels")
                for entry in batch_entries
            )
            seconds = sum(
                required_manifest_int(entry, "frames")
                / required_manifest_int(entry, "sampleRate")
                for entry in batch_entries
            )
            assert_work_units(
                case.case_id,
                work,
                frames=frames * iterations,
                samples=interleaved_samples * iterations,
                seconds=seconds * iterations,
                logical_items=len(batch_entries) * iterations,
            )
            if details.get("filesPerIteration") != len(batch_entries):
                raise BaselineError(f"{case.case_id} batch file count drifted")
            continue

        if case.mode == "discovery":
            discovery = manifest.get("discovery")
            if not isinstance(discovery, dict):
                raise BaselineError("performance discovery manifest is invalid")
            supported = discovery.get("supportedFiles")
            if not isinstance(supported, int):
                raise BaselineError("performance discovery file count is invalid")
            iterations = int(case.arguments[2])
            assert_work_units(
                case.case_id,
                work,
                frames=0,
                samples=0,
                seconds=0.0,
                logical_items=supported * iterations,
            )
            if details.get("filesPerIteration") != supported:
                raise BaselineError(f"{case.case_id} discovery file count drifted")
            continue

        raise BaselineError(f"{case.case_id} uses an unknown workload mode")

    mismatched_pcm_groups = {
        pcm: sorted(fingerprints)
        for pcm, fingerprints in analysis_fingerprints.items()
        if len(fingerprints) != 1
    }
    if mismatched_pcm_groups:
        raise BaselineError(
            "application analysis differs across containers carrying identical PCM: "
            f"{mismatched_pcm_groups}"
        )


def required_manifest_int(entry: dict[str, object], key: str) -> int:
    value = entry.get(key)
    if not isinstance(value, int) or isinstance(value, bool):
        raise BaselineError(f"performance corpus field {key!r} is not an integer")
    return value


def assert_work_units(
    case_id: str,
    work: dict[str, object],
    *,
    frames: int,
    samples: int,
    seconds: float,
    logical_items: int,
) -> None:
    expected = {
        "audioFrames": frames,
        "interleavedSamples": samples,
        "logicalItems": logical_items,
    }
    for key, value in expected.items():
        if work.get(key) != value:
            raise BaselineError(
                f"{case_id} work unit {key} is {work.get(key)!r}, expected {value}"
            )
    actual_seconds = work.get("audioSeconds")
    if not isinstance(actual_seconds, (int, float)) or not math.isclose(
        float(actual_seconds), seconds, rel_tol=1e-12, abs_tol=1e-12
    ):
        raise BaselineError(
            f"{case_id} audioSeconds is {actual_seconds!r}, expected {seconds}"
        )


def sysctl_value(name: str) -> str | None:
    return command_output(("sysctl", "-n", name))


def environment_identity(root: Path) -> dict[str, object]:
    rustc = command_output(("rustc", "-Vv"), required=True)
    cargo = command_output(("cargo", "-V"), required=True)
    assert rustc is not None and cargo is not None
    rustc_fields = {}
    for line in rustc.splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            rustc_fields[key.strip()] = value.strip()

    identity: dict[str, object] = {
        "os": {
            "system": platform.system(),
            "release": platform.release(),
            "version": platform.version(),
            "machine": platform.machine(),
        },
        "toolchain": {
            "rustc": rustc_fields,
            "cargo": cargo,
            "python": platform.python_version(),
        },
        "build": {
            "profile": "release",
            "optLevel": 3,
            "lto": "thin",
            "codegenUnits": 1,
            "strip": True,
            "overflowChecks": True,
            "rustflags": os.environ.get("RUSTFLAGS", ""),
            "cargoBuildTarget": os.environ.get("CARGO_BUILD_TARGET", ""),
        },
    }
    if sys.platform == "darwin":
        sw_vers = command_output(("sw_vers",))
        if sw_vers:
            identity["os"]["swVers"] = sw_vers
        identity["hardware"] = {
            "model": sysctl_value("hw.model"),
            "cpuBrand": sysctl_value("machdep.cpu.brand_string"),
            "physicalCpu": integer_or_none(sysctl_value("hw.physicalcpu")),
            "logicalCpu": integer_or_none(sysctl_value("hw.logicalcpu")),
            "memoryBytes": integer_or_none(sysctl_value("hw.memsize")),
        }
        battery = command_output(("pmset", "-g", "batt"))
        power_source = None
        if battery:
            first = battery.splitlines()[0]
            match = re.search(r"'([^']+)'", first)
            power_source = match.group(1) if match else first
        identity["power"] = {
            "source": power_source,
            "policy": command_output(("pmset", "-g", "custom")),
        }
    else:
        identity["hardware"] = {
            "processor": platform.processor(),
            "logicalCpu": os.cpu_count(),
            "memoryBytes": linux_memory_bytes(),
        }
    identity["repositoryRootName"] = root.name
    identity["runner"] = {
        "path": "scripts/run-performance-baseline.py",
        "sha256": sha256_file(Path(__file__).resolve()),
    }
    return identity


def integer_or_none(value: str | None) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def linux_memory_bytes() -> int | None:
    try:
        for line in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1]) * 1024
    except (OSError, ValueError, IndexError):
        return None
    return None


def verify_corpus(root: Path, corpus: Path) -> tuple[dict[str, object], str]:
    subprocess.run(
        (
            sys.executable,
            str(root / "scripts/generate-performance-corpus.py"),
            "--output-dir",
            str(corpus),
            "--check",
        ),
        cwd=root,
        check=True,
    )
    manifest_path = corpus / "manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise BaselineError(f"cannot read performance corpus manifest: {error}") from error
    if not isinstance(manifest, dict):
        raise BaselineError("performance corpus manifest is not an object")
    return manifest, sha256_file(manifest_path)


def case_manifest(cases: Sequence[BenchmarkCase], corpus: Path) -> list[dict[str, object]]:
    root = str(corpus.resolve())
    return [
        {
            "caseId": case.case_id,
            "scope": case.scope,
            "description": case.description,
            "arguments": [
                argument.replace(root, "$CORPUS") for argument in case.arguments
            ],
        }
        for case in cases
    ]


def write_result(path: Path, result: dict[str, object], replace: bool) -> None:
    if path.exists() and not replace:
        raise BaselineError(f"result already exists; pass --replace to overwrite {path}")
    if path.is_symlink():
        raise BaselineError(f"refusing to write through symlinked result path: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(
        json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def selected_cases(
    available: Sequence[BenchmarkCase], requested: Sequence[str]
) -> tuple[BenchmarkCase, ...]:
    if not requested:
        return tuple(available)
    by_id = {case.case_id: case for case in available}
    unknown = sorted(set(requested) - by_id.keys())
    if unknown:
        raise BaselineError(f"unknown case id(s): {unknown}")
    return tuple(by_id[case_id] for case_id in requested)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus-dir", type=Path, default=DEFAULT_CORPUS)
    parser.add_argument(
        "--output",
        type=Path,
        help="result JSON (default: target/performance-results/<source>-<target>.json)",
    )
    parser.add_argument("--samples", type=int, default=DEFAULT_SAMPLES)
    parser.add_argument("--warmups", type=int, default=DEFAULT_WARMUPS)
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument(
        "--sampling-interval-ms", type=int, default=DEFAULT_SAMPLE_INTERVAL_MS
    )
    parser.add_argument(
        "--case",
        action="append",
        default=[],
        help="run only this exact case id; may be repeated",
    )
    parser.add_argument(
        "--variant",
        action="append",
        default=[],
        metavar="NAME=EXECUTABLE",
        help="compare one or more prebuilt protocol-compatible workers",
    )
    parser.add_argument(
        "--variant-source",
        action="append",
        default=[],
        metavar="NAME=COMMIT",
        help="required source commit for every explicitly supplied variant",
    )
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--replace", action="store_true")
    parser.add_argument("--list-cases", action="store_true")
    return parser.parse_args()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def main() -> int:
    args = parse_args()
    root = Path(__file__).resolve().parent.parent
    corpus = args.corpus_dir.resolve()
    available = suite_cases(corpus)
    if args.list_cases:
        for case in available:
            print(f"{case.case_id}\t{case.scope}\t{case.description}")
        return 0
    try:
        if args.samples <= 0:
            raise BaselineError("--samples must be greater than zero")
        if args.warmups < 0:
            raise BaselineError("--warmups cannot be negative")
        if args.sampling_interval_ms <= 0:
            raise BaselineError("--sampling-interval-ms must be greater than zero")
        source = git_identity(root, args.allow_dirty)
        corpus_manifest, corpus_manifest_sha = verify_corpus(root, corpus)
        cases = selected_cases(available, args.case)

        variants: dict[str, Path] = {}
        variant_sources: dict[str, str] = {}
        if args.variant:
            for specification in args.variant:
                name, path = parse_variant(specification)
                if name in variants:
                    raise BaselineError(f"duplicate variant name: {name}")
                variants[name] = path
            for specification in args.variant_source:
                name, commit = parse_variant_source(specification)
                if name in variant_sources:
                    raise BaselineError(f"duplicate variant source name: {name}")
                variant_sources[name] = commit
            if variant_sources.keys() != variants.keys():
                raise BaselineError(
                    "every explicit --variant requires exactly one matching "
                    "--variant-source NAME=COMMIT"
                )
        else:
            if args.variant_source:
                raise BaselineError("--variant-source requires an explicit --variant")
            variants["scalar"] = build_default_worker(root)
            variant_sources["scalar"] = source["commit"]

        variant_identity = {
            name: {
                "path": path.name,
                "sha256": sha256_file(path),
                "sizeBytes": path.stat().st_size,
                "sourceCommit": variant_sources[name],
            }
            for name, path in variants.items()
        }
        manifest_cases = case_manifest(cases, corpus)
        suite_sha = sha256_bytes(canonical_json_bytes(manifest_cases))
        interval_seconds = args.sampling_interval_ms / 1000
        environment = environment_identity(root)
        run_started_at = utc_now()
        load_average_start = list(os.getloadavg())

        warmup_results: list[dict[str, object]] = []
        warmup_schedule = randomized_schedule(
            cases, tuple(variants), args.warmups, args.seed ^ 0x5752_4D55
        )
        for index, (case, variant, repetition) in enumerate(warmup_schedule, start=1):
            print(
                f"[warmup {index}/{len(warmup_schedule)}] {case.case_id} / {variant}",
                file=sys.stderr,
            )
            sample = run_sample(variants[variant], case, interval_seconds)
            warmup_results.append(
                {
                    "caseId": case.case_id,
                    "variant": variant,
                    "repetition": repetition,
                    **sample,
                }
            )

        samples: list[dict[str, object]] = []
        schedule = randomized_schedule(cases, tuple(variants), args.samples, args.seed)
        for index, (case, variant, repetition) in enumerate(schedule, start=1):
            print(
                f"[sample {index}/{len(schedule)}] {case.case_id} / {variant}",
                file=sys.stderr,
            )
            sample = run_sample(variants[variant], case, interval_seconds)
            samples.append(
                {
                    "scheduleIndex": index - 1,
                    "caseId": case.case_id,
                    "scope": case.scope,
                    "variant": variant,
                    "repetition": repetition,
                    **sample,
                }
            )

        validate_cross_variant_fingerprints(samples)
        validate_corpus_work(samples, cases, corpus, corpus_manifest)
        summaries = summarize_samples(samples)
        result: dict[str, object] = {
            "schemaVersion": RUN_SCHEMA_VERSION,
            "kind": "m6_performance_baseline",
            "run": {
                "startedAtUtc": run_started_at,
                "completedAtUtc": utc_now(),
                "loadAverageStart": load_average_start,
                "loadAverageEnd": list(os.getloadavg()),
            },
            "source": source,
            "suite": {
                "id": "m6-scalar-baseline-v1",
                "sha256": suite_sha,
                "seed": args.seed,
                "warmupsPerCaseVariant": args.warmups,
                "samplesPerCaseVariant": args.samples,
                "schedule": "seeded_fully_interleaved",
                "outlierPolicy": "retain_all",
                "cases": manifest_cases,
            },
            "corpus": {
                "id": corpus_manifest.get("corpusId"),
                "manifestSha256": corpus_manifest_sha,
                "generator": corpus_manifest.get("generator"),
                "mediaFiles": len(corpus_manifest.get("media", [])),
                "committed": False,
            },
            "variants": variant_identity,
            "environment": environment,
            "measurement": {
                "workerTimer": "std::time::Instant around the named workload only",
                "nativeTimer": (
                    "Darwin /usr/bin/time -l"
                    if sys.platform == "darwin"
                    else "GNU /usr/bin/time -v"
                ),
                "processTreeRss": (
                    "sum of descendant RSS sampled with ps; /usr/bin/time wrapper excluded"
                ),
                "samplingIntervalMs": args.sampling_interval_ms,
                "decodeVerification": (
                    "full decoded f64 SHA-256 is outside the timed decode interval"
                ),
                "coldCacheClaim": False,
                "outliersRemoved": 0,
            },
            "warmups": warmup_results,
            "samples": samples,
            "summary": summaries,
        }
        ensure_finite_json(result)
        target = rustc_target(environment)
        if args.output is None:
            output = (
                root
                / DEFAULT_RESULTS
                / f"{source['commit'][:12]}-{target}-m6-baseline.json"
            )
        else:
            output = args.output.resolve()
        write_result(output, result, args.replace)
        print(f"performance baseline written to {output}")
        print(f"result SHA-256: {sha256_file(output)}")
        return 0
    except (
        BaselineError,
        OSError,
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
    ) as error:
        print(f"performance baseline error: {error}", file=sys.stderr)
        return 1


def rustc_target(environment: dict[str, object]) -> str:
    toolchain = environment.get("toolchain")
    if isinstance(toolchain, dict):
        rustc = toolchain.get("rustc")
        if isinstance(rustc, dict) and isinstance(rustc.get("host"), str):
            return rustc["host"]
    return platform.machine().replace(" ", "_")


if __name__ == "__main__":
    raise SystemExit(main())
