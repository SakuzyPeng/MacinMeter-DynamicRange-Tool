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


# 2: the run block records host occupancy in the terms the host actually has
# rather than a `loadAverage` field Windows cannot fill.
RUN_SCHEMA_VERSION = 2
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


# Worker counts the ADR-0014 packet-worker sweep compares in one run.
PACKET_WORKER_COUNTS = (1, 2, 4, 8)
MAX_DECODE_WORKERS = 8
# Minimum legal and fixed product maximum reorder permits, swept at the
# widest worker count. The plan's own derivation for 8 workers is 32.
ALAC_QUEUE_SWEEP_WORKERS = 8
ALAC_QUEUE_CAPACITIES = (8, 64)
# ADR-0014 P1 lane widths. One lane is the product request and the serial
# reference; the wider widths spend the same plan, so each one narrows the
# per-lane decoder that pays for it.
FILE_LANE_COUNTS = (1, 2, 4, 8)
# Mirrors of the crate-private application plan derivation the worker uses.
DECODE_QUEUE_DEPTH_PER_WORKER = 4
DECODE_IN_FLIGHT_PCM_BYTES_PER_WORKER = 4 * 1024 * 1024
MAX_DECODE_QUEUE_CAPACITY = 64
MAX_IN_FLIGHT_PCM_BYTES = 64 * 1024 * 1024


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
        # ADR-0014 packet workers are a decode allocation rather than a separate
        # binary, so worker count is a case argument. The runner interleaves
        # these with every other case in one run, and each independently has to
        # reproduce the corpus PCM oracle, which makes the sweep a differential
        # rather than four unrelated timings.
        # Both tracks carry the same geometry at opposite ends of the
        # compression range, so the sweep can state whether its result depends
        # on how hard the codec has to work rather than assuming it does not.
        *(
            BenchmarkCase(
                f"decode/{track}-w{workers}",
                "decode",
                f"Content probe plus complete {label} ALAC decoding on "
                f"{workers} decode worker(s)",
                ("decode", media(filename), "1", str(workers)),
            )
            for track, label, filename in (
                (
                    "alac-s16-240s",
                    "near-incompressible",
                    "stereo-s16-alac-240s.m4a",
                ),
                (
                    "alac-tonal-240s",
                    "tonal",
                    "stereo-s16-alac-tonal-240s.m4a",
                ),
                (
                    "alac-varied-240s",
                    "worst-case load-imbalanced",
                    "stereo-s16-alac-varied-240s.m4a",
                ),
            )
            for workers in PACKET_WORKER_COUNTS
        ),
        # FLAC's packet workers carry a cost ALAC's do not: the product hashes
        # the stream signature at the single in-order commit point, and hashing
        # cannot be parallelized. Depth decides how many bytes that is and
        # compressibility decides how much decode work it is competing with, so
        # the three tracks separate the two rather than reporting one number.
        *(
            BenchmarkCase(
                f"decode/{track}-w{workers}",
                "decode",
                f"Content probe plus complete {label} FLAC decoding on "
                f"{workers} decode worker(s)",
                ("decode", media(filename), "1", str(workers)),
            )
            for track, label, filename in (
                (
                    "flac-s24-240s",
                    "24-bit near-incompressible",
                    "stereo-s24-flac-240s.flac",
                ),
                (
                    "flac-s24-tonal-240s",
                    "24-bit tonal",
                    "stereo-s24-flac-tonal-240s.flac",
                ),
                (
                    "flac-s16-240s",
                    "16-bit near-incompressible",
                    "stereo-s16-flac-240s.flac",
                ),
            )
            for workers in PACKET_WORKER_COUNTS
        ),
        # The plan never varies queue capacity, so sweep it separately at the
        # widest worker count. The minimum legal capacity equals the worker
        # count, which drives each inbox down to a zero-capacity rendezvous and
        # is also the tightest long-stream memory case the permits allow.
        *(
            BenchmarkCase(
                f"decode/alac-tonal-240s-w{ALAC_QUEUE_SWEEP_WORKERS}-q{capacity}",
                "decode",
                "Content probe plus complete tonal ALAC decoding on "
                f"{ALAC_QUEUE_SWEEP_WORKERS} decode workers and a "
                f"{capacity}-packet reorder permit",
                (
                    "decode",
                    media("stereo-s16-alac-tonal-240s.m4a"),
                    "1",
                    str(ALAC_QUEUE_SWEEP_WORKERS),
                    str(capacity),
                ),
            )
            for capacity in ALAC_QUEUE_CAPACITIES
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
        # The short application case keeps the broad baseline affordable, but
        # it is too small to decide whether moving FLAC's ordered signature
        # hash off the commit/analysis thread earns back one decoder permit.
        # These long cases use the same inputs as the explicit worker sweep and
        # therefore isolate depth and compressibility while timing the complete
        # decode-analysis-report path. Baseline and candidate binaries receive
        # the same host-derived application allocation in an interleaved run.
        *(
            BenchmarkCase(
                f"application/{track}",
                "application",
                f"Application reservation, complete {label} FLAC decoding, "
                "analysis, and report construction",
                ("application", media(filename), "1"),
            )
            for track, label, filename in (
                (
                    "flac-s24-240s",
                    "24-bit near-incompressible",
                    "stereo-s24-flac-240s.flac",
                ),
                (
                    "flac-s24-tonal-240s",
                    "24-bit tonal",
                    "stereo-s24-flac-tonal-240s.flac",
                ),
                (
                    "flac-s16-240s",
                    "16-bit near-incompressible",
                    "stereo-s16-flac-240s.flac",
                ),
            )
        ),
        # The explicit ALAC worker sweep passes its own worker count and so
        # never observes the allocation the product actually grants. Only the
        # application path takes the host-derived plan, which is what changed
        # when the ALAC route graduated. These cases use the same three tracks
        # as that sweep so route selection, not the input, is the difference.
        *(
            BenchmarkCase(
                f"application/{track}",
                "application",
                f"Application reservation, complete {label} ALAC decoding, "
                "analysis, and report construction",
                ("application", media(filename), "1"),
            )
            for track, label, filename in (
                (
                    "alac-s16-240s",
                    "near-incompressible",
                    "stereo-s16-alac-240s.m4a",
                ),
                (
                    "alac-tonal-240s",
                    "tonal",
                    "stereo-s16-alac-tonal-240s.m4a",
                ),
                (
                    "alac-varied-240s",
                    "worst-case load-imbalanced",
                    "stereo-s16-alac-varied-240s.m4a",
                ),
            )
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
        # ADR-0014 §7 requires WAV, FLAC, ALAC and mixed durations before file
        # lanes may be enabled by default. The eight-WAV case above cannot
        # supply that: identical items give every lane identical work, so it
        # cannot show tail latency, unfair assignment, or how a lane holding a
        # packet-parallel route shares the plan with one that decodes serially.
        # Twelve items over the eight-worker ceiling at a 12:1 duration spread
        # make all three observable, and this serial number is the reference a
        # later lane implementation has to beat in the same interleaved run.
        BenchmarkCase(
            "batch/12-mixed-tracks",
            "batch",
            "Current serial application batch over twelve mixed-route, "
            "mixed-duration tracks",
            ("batch", media("batch-mixed"), "1"),
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


def attribution_cases(corpus: Path) -> tuple[BenchmarkCase, ...]:
    """Explicit phase cases that are selectable but not in the default suite."""

    def media(name: str) -> str:
        return str((corpus / name).resolve())

    return tuple(
        BenchmarkCase(
            f"decode-phases/{track}-w{workers}",
            "attribution",
            f"Attribute complete {label} decode between open and drain on "
            f"{workers} total worker permit(s)",
            ("decode-phases", media(filename), "1", str(workers)),
        )
        for track, label, filename in (
            (
                "alac-s16-240s",
                "near-incompressible ALAC",
                "stereo-s16-alac-240s.m4a",
            ),
            (
                "alac-tonal-240s",
                "tonal ALAC",
                "stereo-s16-alac-tonal-240s.m4a",
            ),
            (
                "alac-varied-240s",
                "load-imbalanced ALAC",
                "stereo-s16-alac-varied-240s.m4a",
            ),
            (
                "flac-s16-240s",
                "16-bit near-incompressible FLAC",
                "stereo-s16-flac-240s.flac",
            ),
            (
                "flac-s24-240s",
                "24-bit near-incompressible FLAC",
                "stereo-s24-flac-240s.flac",
            ),
            (
                "flac-s24-tonal-240s",
                "24-bit tonal FLAC",
                "stereo-s24-flac-tonal-240s.flac",
            ),
        )
        for workers in PACKET_WORKER_COUNTS
    )


def file_lane_cases(corpus: Path) -> tuple[BenchmarkCase, ...]:
    """ADR-0014 P1 lane widths, selectable but absent from the default suite.

    Lanes and per-lane decode workers come out of one plan, so a width is not a
    free dimension: four lanes means two decode workers each. The mixed batch is
    the only input that can price that trade, since it holds packet-parallel and
    serial routes side by side. The product asks for one lane until this sweep
    and a formal A/B say otherwise, so these stay out of the default suite.
    """
    directory = str((corpus / "batch-mixed").resolve())
    return tuple(
        BenchmarkCase(
            f"batch-lanes/12-mixed-l{lanes}",
            "batch",
            f"Mixed-route batch over {lanes} file lane(s) from one shared plan",
            ("batch", directory, "1", str(lanes)),
        )
        for lanes in FILE_LANE_COUNTS
    )


def pipeline_attribution_cases(corpus: Path) -> tuple[BenchmarkCase, ...]:
    """Explicit source-owned pipeline probes, absent from the default suite."""

    def media(name: str) -> str:
        return str((corpus / name).resolve())

    return tuple(
        BenchmarkCase(
            f"decode-pipeline/{track}-w{workers}",
            "attribution",
            f"Attribute {label} decode across caller, demux, decoder slots, "
            f"conversion and ordered hashing on {workers} total permit(s)",
            ("decode-pipeline", media(filename), "1", str(workers)),
        )
        for track, label, filename in (
            (
                "alac-s16-240s",
                "near-incompressible ALAC",
                "stereo-s16-alac-240s.m4a",
            ),
            (
                "alac-tonal-240s",
                "tonal ALAC",
                "stereo-s16-alac-tonal-240s.m4a",
            ),
            (
                "alac-varied-240s",
                "load-imbalanced ALAC",
                "stereo-s16-alac-varied-240s.m4a",
            ),
            (
                "flac-s16-240s",
                "16-bit near-incompressible FLAC",
                "stereo-s16-flac-240s.flac",
            ),
            (
                "flac-s24-240s",
                "24-bit near-incompressible FLAC",
                "stereo-s24-flac-240s.flac",
            ),
            (
                "flac-s24-tonal-240s",
                "24-bit tonal FLAC",
                "stereo-s24-flac-tonal-240s.flac",
            ),
        )
        for workers in PACKET_WORKER_COUNTS
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


EXECUTABLE_SUFFIX = ".exe" if sys.platform == "win32" else ""


def build_default_worker(root: Path, performance_probes: bool = False) -> Path:
    command = [
            "cargo",
            "build",
            "--locked",
            "--release",
            "-p",
            "macinmeter",
            "--example",
            "m6_baseline_worker",
    ]
    if performance_probes:
        command.extend(("--features", "performance-probes"))
    subprocess.run(
        command,
        cwd=root,
        check=True,
    )
    # Cargo names the artifact after the host, so ask the host rather than
    # assuming the POSIX spelling.
    worker = root / "target/release/examples" / f"m6_baseline_worker{EXECUTABLE_SUFFIX}"
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
    measurements = value.get("measurements", {})
    if not isinstance(measurements, dict):
        raise BaselineError("worker measurements are not a JSON object")
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
    if sys.platform == "win32":
        return windows_descendant_rss(root_pid)
    completed = subprocess.run(
        ("ps", "-axo", "pid=,ppid=,rss="),
        check=True,
        capture_output=True,
        text=True,
    )
    return descendant_rss(parse_ps_rows(completed.stdout), root_pid)


# Windows has no `/usr/bin/time` and its `ps` reports no RSS, so the same two
# measurements are taken from the Win32 process APIs instead. The values are
# read through a handle this runner opens itself and holds for the child's whole
# lifetime, which also pins the process id against reuse.
WINDOWS_PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
WINDOWS_PROCESS_VM_READ = 0x0010
WINDOWS_TH32CS_SNAPPROCESS = 0x0002
WINDOWS_INVALID_HANDLE = -1
# FILETIME counts 100-nanosecond intervals.
WINDOWS_FILETIME_TICKS_PER_SECOND = 10_000_000
# Long enough for the idle counter to move, short enough not to delay a run.
WINDOWS_OCCUPANCY_SAMPLE_SECONDS = 1.0


if sys.platform == "win32":
    import ctypes
    from ctypes import wintypes

    class _FileTime(ctypes.Structure):
        _fields_ = [
            ("dwLowDateTime", wintypes.DWORD),
            ("dwHighDateTime", wintypes.DWORD),
        ]

    class _ProcessMemoryCounters(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    class _ProcessEntry32(ctypes.Structure):
        _fields_ = [
            ("dwSize", wintypes.DWORD),
            ("cntUsage", wintypes.DWORD),
            ("th32ProcessID", wintypes.DWORD),
            ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
            ("th32ModuleID", wintypes.DWORD),
            ("cntThreads", wintypes.DWORD),
            ("th32ParentProcessID", wintypes.DWORD),
            ("pcPriClassBase", ctypes.c_long),
            ("dwFlags", wintypes.DWORD),
            ("szExeFile", ctypes.c_char * 260),
        ]

    _KERNEL32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _PSAPI = ctypes.WinDLL("psapi", use_last_error=True)


def _windows_filetime_seconds(value: "_FileTime") -> float:
    ticks = (value.dwHighDateTime << 32) | value.dwLowDateTime
    return ticks / WINDOWS_FILETIME_TICKS_PER_SECOND


def windows_open_process(pid: int) -> int:
    handle = _KERNEL32.OpenProcess(
        WINDOWS_PROCESS_QUERY_LIMITED_INFORMATION | WINDOWS_PROCESS_VM_READ,
        False,
        pid,
    )
    if not handle:
        raise BaselineError(
            f"cannot open process {pid} for measurement: "
            f"Win32 error {ctypes.get_last_error()}"
        )
    return handle


def windows_memory_counters(handle: int) -> "_ProcessMemoryCounters":
    counters = _ProcessMemoryCounters()
    counters.cb = ctypes.sizeof(_ProcessMemoryCounters)
    if not _PSAPI.GetProcessMemoryInfo(
        wintypes.HANDLE(handle), ctypes.byref(counters), counters.cb
    ):
        raise BaselineError(
            f"GetProcessMemoryInfo failed: Win32 error {ctypes.get_last_error()}"
        )
    return counters


def windows_process_metrics(handle: int) -> dict[str, int | float]:
    """Read one exited child's CPU time and peak working set.

    `maxResidentSetBytes` carries `PeakWorkingSetSize`, which is Windows' own
    peak-resident measure. It is named after the POSIX field so one summary
    shape serves every platform, and the record states which API produced it.
    """
    creation, exit_time, kernel, user = (
        _FileTime(),
        _FileTime(),
        _FileTime(),
        _FileTime(),
    )
    if not _KERNEL32.GetProcessTimes(
        wintypes.HANDLE(handle),
        ctypes.byref(creation),
        ctypes.byref(exit_time),
        ctypes.byref(kernel),
        ctypes.byref(user),
    ):
        raise BaselineError(
            f"GetProcessTimes failed: Win32 error {ctypes.get_last_error()}"
        )
    counters = windows_memory_counters(handle)
    created = (creation.dwHighDateTime << 32) | creation.dwLowDateTime
    exited = (exit_time.dwHighDateTime << 32) | exit_time.dwLowDateTime
    if exited <= created:
        raise BaselineError("GetProcessTimes reported no elapsed interval")
    return {
        "realSeconds": (exited - created) / WINDOWS_FILETIME_TICKS_PER_SECOND,
        "userSeconds": _windows_filetime_seconds(user),
        "systemSeconds": _windows_filetime_seconds(kernel),
        "maxResidentSetBytes": int(counters.PeakWorkingSetSize),
        "pageFaults": int(counters.PageFaultCount),
    }


def windows_descendant_rss(root_pid: int) -> tuple[int, int]:
    snapshot = _KERNEL32.CreateToolhelp32Snapshot(WINDOWS_TH32CS_SNAPPROCESS, 0)
    if snapshot == WINDOWS_INVALID_HANDLE:
        raise BaselineError(
            f"CreateToolhelp32Snapshot failed: Win32 error {ctypes.get_last_error()}"
        )
    try:
        entry = _ProcessEntry32()
        entry.dwSize = ctypes.sizeof(_ProcessEntry32)
        parents: dict[int, int] = {}
        if not _KERNEL32.Process32First(
            wintypes.HANDLE(snapshot), ctypes.byref(entry)
        ):
            raise BaselineError("Process32First returned no entries")
        while True:
            parents[int(entry.th32ProcessID)] = int(entry.th32ParentProcessID)
            if not _KERNEL32.Process32Next(
                wintypes.HANDLE(snapshot), ctypes.byref(entry)
            ):
                break
    finally:
        _KERNEL32.CloseHandle(wintypes.HANDLE(snapshot))

    descendants = {root_pid}
    changed = True
    while changed:
        changed = False
        for pid, parent in parents.items():
            if parent in descendants and pid not in descendants:
                descendants.add(pid)
                changed = True

    total = 0
    counted = 0
    for pid in descendants:
        try:
            handle = windows_open_process(pid)
        except BaselineError:
            # The process exited between the snapshot and this read.
            continue
        try:
            total += int(windows_memory_counters(handle).WorkingSetSize)
            counted += 1
        except BaselineError:
            continue
        finally:
            _KERNEL32.CloseHandle(wintypes.HANDLE(handle))
    return total, counted


def filetime_ticks(value: "_FileTime") -> int:
    return (value.dwHighDateTime << 32) | value.dwLowDateTime


def host_occupancy() -> dict[str, object]:
    """Describe how busy this host is, in whatever terms it actually has.

    Contamination by other work is the failure mode that most easily turns a
    sweep into a wrong conclusion, so a record has to carry a before/after
    occupancy signal. Load average is a POSIX concept with no Windows
    equivalent, so Windows records the fraction of wall time the CPUs spent out
    of idle instead. The two are not comparable and are therefore reported
    under different names rather than one field that means different things.
    """
    if sys.platform != "win32":
        return {"kind": "posix_load_average", "loadAverage": list(os.getloadavg())}

    def system_times() -> tuple[int, int]:
        idle, kernel, user = _FileTime(), _FileTime(), _FileTime()
        if not _KERNEL32.GetSystemTimes(
            ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)
        ):
            raise BaselineError(
                f"GetSystemTimes failed: Win32 error {ctypes.get_last_error()}"
            )
        # Kernel time includes idle time, so total is kernel + user.
        return filetime_ticks(idle), filetime_ticks(kernel) + filetime_ticks(user)

    first_idle, first_total = system_times()
    time.sleep(WINDOWS_OCCUPANCY_SAMPLE_SECONDS)
    second_idle, second_total = system_times()
    total = second_total - first_total
    if total <= 0:
        raise BaselineError("GetSystemTimes reported no elapsed CPU interval")
    busy = 1.0 - (second_idle - first_idle) / total
    return {
        "kind": "windows_cpu_busy_fraction",
        "cpuBusyFraction": max(0.0, min(1.0, busy)),
        "sampleSeconds": WINDOWS_OCCUPANCY_SAMPLE_SECONDS,
    }


def native_timer_description() -> str:
    """Name the exact tool that produced `nativeMetrics` on this host.

    Peak-resident bytes are not the same measurement on every platform, so a
    record states which one it holds rather than leaving readers to assume.
    """
    if sys.platform == "darwin":
        return "Darwin /usr/bin/time -l"
    if sys.platform == "win32":
        return (
            "Win32 GetProcessTimes and GetProcessMemoryInfo on a handle held for "
            "the child's lifetime; maxResidentSetBytes is PeakWorkingSetSize"
        )
    return "GNU /usr/bin/time -v"


def process_tree_rss_description() -> str:
    if sys.platform == "win32":
        return (
            "sum of descendant WorkingSetSize from a Toolhelp32 snapshot; "
            "no measurement wrapper process exists on this platform"
        )
    return "sum of descendant RSS sampled with ps; /usr/bin/time wrapper excluded"


def time_command_prefix(metrics_path: Path) -> tuple[list[str], str]:
    if sys.platform == "darwin":
        return ["/usr/bin/time", "-l", "-o", str(metrics_path)], "darwin_time_l"
    if sys.platform.startswith("linux"):
        return ["/usr/bin/time", "-v", "-o", str(metrics_path)], "gnu_time_v"
    if sys.platform == "win32":
        # No wrapper process: the same metrics come from the Win32 process APIs.
        return [], "windows_process_api"
    raise BaselineError(
        "process-tree/RSS baseline currently supports macOS, Linux and Windows only"
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
        # Held for the child's whole lifetime so its counters stay readable
        # after it exits and its process id cannot be reused meanwhile.
        measurement_handle = (
            windows_open_process(process.pid)
            if timer_kind == "windows_process_api"
            else None
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
        if measurement_handle is not None:
            try:
                native = windows_process_metrics(measurement_handle)
            finally:
                _KERNEL32.CloseHandle(wintypes.HANDLE(measurement_handle))
        else:
            try:
                native_text = metrics_path.read_text(encoding="utf-8")
            except OSError as error:
                raise BaselineError(
                    f"cannot read native time metrics: {error}"
                ) from error
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
            "measurements": output.get("measurements", {}),
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
        summary = {
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
        measurement_keys = {
            key for sample in group for key in sample.get("measurements", {})
        }
        if measurement_keys:
            if any(
                set(sample.get("measurements", {})) != measurement_keys
                for sample in group
            ):
                raise BaselineError(
                    f"{case_id}/{variant} measurement fields changed across samples"
                )
            summary["measurements"] = {
                key: distribution(
                    [float(sample["measurements"][key]) for sample in group]
                )
                for key in sorted(measurement_keys)
            }
        summaries.append(summary)
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

        if case.mode in (
            "decode",
            "decode-phases",
            "decode-pipeline",
            "application",
            "render-json",
        ):
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
            if case.mode in ("decode", "decode-phases", "decode-pipeline"):
                expected_pcm = entry.get("normalizedInterleavedF64LeSha256")
                if details.get("pcmF64LeSha256") != expected_pcm:
                    raise BaselineError(
                        f"{case.case_id} decoded PCM fingerprint does not match the corpus oracle"
                    )
                assert_decode_allocation(case, details)
                if case.mode == "decode-phases":
                    assert_decode_phase_attribution(case, sample)
                elif case.mode == "decode-pipeline":
                    assert_decode_pipeline_attribution(case, sample)
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
            # Take the directory from the case rather than a fixed prefix, so a
            # second batch directory is validated against its own manifest
            # entries instead of silently inheriting the first one's totals.
            directory = Path(case.arguments[1]).name
            batch_entries = [
                entry
                for relative, entry in media.items()
                if relative.startswith(f"{directory}/")
            ]
            if not batch_entries:
                raise BaselineError(
                    f"{case.case_id} matched no manifest media under {directory!r}"
                )
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


def assert_decode_allocation(case: BenchmarkCase, details: dict[str, object]) -> None:
    """Check the decode case ran on the allocation the plan would have granted.

    The application concurrency plan is crate-private, so the worker mirrors its
    derivation. Recomputing it here means a drift between the two shows up as a
    failed run rather than as a silently mistuned comparison.
    """
    requested = int(case.arguments[3]) if len(case.arguments) > 3 else 1
    requested_queue = int(case.arguments[4]) if len(case.arguments) > 4 else None
    if details.get("decodeWorkers") != requested:
        raise BaselineError(
            f"{case.case_id} ran on {details.get('decodeWorkers')!r} decode workers, "
            f"expected {requested}"
        )

    if requested == 1 and requested_queue is None:
        expected_queue, expected_bytes = 1, 0
    else:
        expected_queue = min(requested * DECODE_QUEUE_DEPTH_PER_WORKER, MAX_DECODE_QUEUE_CAPACITY)
        expected_bytes = min(
            requested * DECODE_IN_FLIGHT_PCM_BYTES_PER_WORKER, MAX_IN_FLIGHT_PCM_BYTES
        )
        if requested_queue is not None:
            # Only the queue bound is swept; the in-flight PCM permit stays on
            # the plan's derivation so the two dimensions stay separable.
            expected_queue = requested_queue
    if details.get("decodeQueueCapacity") != expected_queue:
        raise BaselineError(
            f"{case.case_id} decode queue capacity {details.get('decodeQueueCapacity')!r} "
            f"does not match the plan's derivation {expected_queue}"
        )
    if details.get("decodeMaxInFlightPcmBytes") != expected_bytes:
        raise BaselineError(
            f"{case.case_id} decode in-flight PCM permit "
            f"{details.get('decodeMaxInFlightPcmBytes')!r} does not match the plan's "
            f"derivation {expected_bytes}"
        )


def assert_decode_phase_attribution(
    case: BenchmarkCase, sample: dict[str, object]
) -> None:
    """Reject incomplete phase accounting and silent route fallback."""

    details = sample["details"]
    measurements = sample.get("measurements")
    if not isinstance(measurements, dict):
        raise BaselineError(f"{case.case_id} has no phase measurement object")
    requested = int(case.arguments[3])
    is_flac = Path(case.arguments[1]).suffix.lower() == ".flac"
    expected_engine = (
        "Serial"
        if requested == 1
        else "FlacPacketWorkers"
        if is_flac
        else "AlacPacketWorkers"
    )
    expected_hashers = int(is_flac and requested == MAX_DECODE_WORKERS)
    expected_decoders = requested - expected_hashers
    expected = {
        "selectedEngine": expected_engine,
        "selectedTotalWorkers": requested,
        "selectedDecoderWorkers": expected_decoders,
        "selectedHasherWorkers": expected_hashers,
    }
    for key, value in expected.items():
        if details.get(key) != value:
            raise BaselineError(
                f"{case.case_id} attribution field {key} is {details.get(key)!r}, "
                f"expected {value!r}"
            )

    phases = []
    for key in ("openElapsedNs", "drainElapsedNs", "unattributedElapsedNs"):
        value = measurements.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise BaselineError(
                f"{case.case_id} attribution field {key} is not a nonnegative integer"
            )
        phases.append(value)
    if sum(phases) != sample.get("workerElapsedNs"):
        raise BaselineError(
            f"{case.case_id} phase nanoseconds do not sum to workerElapsedNs"
        )


def assert_decode_pipeline_attribution(
    case: BenchmarkCase, sample: dict[str, object]
) -> None:
    """Reject incomplete thread accounting, fallback, or bound drift."""

    assert_decode_phase_attribution(case, sample)
    details = sample["details"]
    measurements = sample["measurements"]
    decoder_workers = details.get("selectedDecoderWorkers")
    if details.get("probeDecoderWorkers") != decoder_workers:
        raise BaselineError(
            f"{case.case_id} probe saw {details.get('probeDecoderWorkers')!r} "
            f"decoder workers, expected {decoder_workers!r}"
        )
    if not isinstance(decoder_workers, int) or isinstance(decoder_workers, bool):
        raise BaselineError(f"{case.case_id} decoder worker count is invalid")
    if any(
        not isinstance(value, int) or isinstance(value, bool) or value < 0
        for value in measurements.values()
    ):
        raise BaselineError(f"{case.case_id} has an invalid pipeline measurement")

    open_keys = (
        "fileIdentifyNs",
        "containerInspectionNs",
        "backendProbeNs",
        "routeSetupNs",
        "openUnattributedNs",
    )
    if sum(measurements.get(key, -1) for key in open_keys) != measurements.get(
        "openElapsedNs"
    ):
        raise BaselineError(f"{case.case_id} open phase accounting is incomplete")
    caller_keys = (
        "callerResultWaitNs",
        "callerCommitNs",
        "callerFinishNs",
        "callerOtherNs",
    )
    required_global = {
        "openElapsedNs",
        "drainElapsedNs",
        "unattributedElapsedNs",
        "demuxPacketReadNs",
        "demuxDispatchWaitNs",
        "reorderStalls",
        "peakReorderPackets",
        "peakReorderBytes",
        "hasherPackets",
        "hasherReceiveWaitNs",
        "hasherActiveNs",
        "hasherSendWaitNs",
        "hasherLifetimeNs",
        *open_keys,
        *caller_keys,
    }
    missing_global = sorted(required_global - measurements.keys())
    if missing_global:
        raise BaselineError(
            f"{case.case_id} is missing pipeline measurements: {missing_global}"
        )
    if sum(measurements.get(key, -1) for key in caller_keys) != measurements.get(
        "drainElapsedNs"
    ):
        raise BaselineError(f"{case.case_id} caller phase accounting is incomplete")

    expected_packets = details.get("blocksPerIteration")
    packet_counts = []
    for slot in range(decoder_workers):
        prefix = f"worker{slot}"
        required = (
            f"{prefix}Packets",
            f"{prefix}BackendDecodeNs",
            f"{prefix}IntegrityConversionNs",
            f"{prefix}PcmConversionNs",
            f"{prefix}InboxWaitNs",
            f"{prefix}ResultSendWaitNs",
            f"{prefix}LifetimeNs",
        )
        if any(key not in measurements for key in required):
            raise BaselineError(
                f"{case.case_id} has incomplete measurements for decoder slot {slot}"
            )
        packet_counts.append(measurements[f"{prefix}Packets"])
    if sum(packet_counts) != expected_packets or any(count <= 0 for count in packet_counts):
        raise BaselineError(
            f"{case.case_id} attributed {sum(packet_counts)} packets across workers, "
            f"expected {expected_packets!r}"
        )
    if any(key.startswith(f"worker{decoder_workers}") for key in measurements):
        raise BaselineError(f"{case.case_id} reported an unallocated decoder slot")

    queue_capacity = details.get("decodeQueueCapacity")
    max_bytes = details.get("decodeMaxInFlightPcmBytes")
    if measurements.get("peakReorderPackets", 0) > queue_capacity:
        raise BaselineError(f"{case.case_id} exceeded its reorder packet permit")
    if measurements.get("peakReorderBytes", 0) > max_bytes:
        raise BaselineError(f"{case.case_id} exceeded its reorder byte permit")

    expected_hasher_packets = (
        expected_packets if details.get("selectedHasherWorkers") == 1 else 0
    )
    if measurements.get("hasherPackets") != expected_hasher_packets:
        raise BaselineError(
            f"{case.case_id} attributed {measurements.get('hasherPackets')!r} hash "
            f"packets, expected {expected_hasher_packets!r}"
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
    default_cases = suite_cases(corpus)
    available = (
        default_cases
        + attribution_cases(corpus)
        + pipeline_attribution_cases(corpus)
        + file_lane_cases(corpus)
    )
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
        cases = selected_cases(available, args.case) if args.case else default_cases

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
            variants["scalar"] = build_default_worker(
                root,
                performance_probes=any(
                    case.mode == "decode-pipeline" or case.case_id.startswith("batch-lanes/")
                    for case in cases
                ),
            )
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
        occupancy_start = host_occupancy()

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
                "occupancyStart": occupancy_start,
                "occupancyEnd": host_occupancy(),
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
                "nativeTimer": native_timer_description(),
                "processTreeRss": process_tree_rss_description(),
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
