#!/usr/bin/env python3
"""Capture source-bound M6 Time Profiler evidence for confirmed baseline scopes."""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from types import ModuleType
from typing import Any, Sequence


PROFILE_SCHEMA_VERSION = 1
DEFAULT_CORPUS = Path("target/performance-corpus/m6-performance-baseline-v1")
DEFAULT_BUILD_DIR = Path("target/m6-profile-build")
DEFAULT_TRACE_ROOT = Path("target/performance-profiles")
DEFAULT_CAPTURES = 3
MIN_SCOPED_SAMPLES = 1_000
SCOPE_COVERAGE_MIN = 0.85
SCOPE_COVERAGE_MAX = 1.15
RUST_HASH_SUFFIX = re.compile(r"::h[0-9a-f]{16}$")
LLVM_SUFFIX = re.compile(r"\.llvm\.[0-9A-Fa-f]+$")


class ProfileError(RuntimeError):
    """A profiling capability, capture, or evidence validation failure."""


def load_baseline_support() -> ModuleType:
    module_name = "_macinmeter_m6_baseline_support"
    existing = sys.modules.get(module_name)
    if existing is not None:
        return existing
    path = Path(__file__).resolve().with_name("run-performance-baseline.py")
    specification = importlib.util.spec_from_file_location(module_name, path)
    if specification is None or specification.loader is None:
        raise ProfileError(f"cannot load baseline support from {path}")
    module = importlib.util.module_from_spec(specification)
    sys.modules[module_name] = module
    specification.loader.exec_module(module)
    return module


baseline = load_baseline_support()


@dataclasses.dataclass(frozen=True)
class ProfileCase:
    case_id: str
    scope: str
    description: str
    arguments: tuple[str, ...]
    anchor: str

    @property
    def mode(self) -> str:
        return self.arguments[0]


@dataclasses.dataclass(frozen=True)
class Frame:
    function: str
    binary: str
    source: str | None
    line: int | None


def profile_cases(corpus: Path) -> tuple[ProfileCase, ...]:
    flac = str((corpus / "stereo-s16-60s.flac").resolve())
    return (
        ProfileCase(
            "analysis/stereo",
            "analysis",
            "Direct finite f64 AnalyzerSession, 2 channels, 24,000 audio seconds",
            ("analysis", "2", "48000", str(24_000 * 48_000), "4096"),
            "m6_baseline_worker::timed_analysis_workload",
        ),
        ProfileCase(
            "analysis/64ch",
            "analysis",
            "Maximum product geometry, 64 channels, 900 audio seconds",
            ("analysis", "64", "48000", str(900 * 48_000), "4096"),
            "m6_baseline_worker::timed_analysis_workload",
        ),
        ProfileCase(
            "decode/flac-s16",
            "decode",
            "Content probe plus complete FLAC decoding, repeated 100 times",
            ("decode", flac, "100"),
            "m6_baseline_worker::timed_decode_workload",
        ),
    )


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def filesystem_component(value: str) -> str:
    component = re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-")
    if not component:
        raise ProfileError(f"cannot form a filesystem component from {value!r}")
    return component


def normalized_function(name: str | None) -> str:
    if not name:
        return "<unknown>"
    return LLVM_SUFFIX.sub("", RUST_HASH_SUFFIX.sub("", name))


def normalized_path(path: str | None, root: Path) -> str | None:
    if path is None:
        return None
    candidate = Path(path)
    try:
        relative = candidate.resolve().relative_to(root.resolve())
        return f"$REPO/{relative.as_posix()}"
    except (OSError, ValueError):
        pass
    home = Path.home()
    try:
        relative = candidate.resolve().relative_to(home.resolve())
        if relative.parts and relative.parts[0] == ".cargo":
            return f"$CARGO_HOME/{Path(*relative.parts[1:]).as_posix()}"
        if relative.parts and relative.parts[0] == ".rustup":
            return f"$RUSTUP_HOME/{Path(*relative.parts[1:]).as_posix()}"
        return f"$HOME/{relative.as_posix()}"
    except (OSError, ValueError):
        return path


def frame_key(frame: Frame) -> tuple[str, str]:
    return frame.function, frame.binary


def category_for_frame(frame: Frame) -> str:
    function = frame.function
    if function.startswith("macinmeter_analysis::"):
        return "macinmeter_analysis"
    if function.startswith("macinmeter_codecs::"):
        return "macinmeter_codecs"
    if function.startswith(
        ("symphonia_bundle_flac::", "symphonia_codec_flac::")
    ):
        return "symphonia_flac"
    if function.startswith("symphonia_core::"):
        return "symphonia_core"
    if function.startswith("symphonia_format_"):
        return "symphonia_format"
    if function.startswith("m6_baseline_worker::"):
        return "worker"
    if (
        function.startswith("core::")
        or function.startswith("std::")
        or function.startswith("alloc::")
        or function.startswith("_$LT$core..")
        or function.startswith("_R")
    ):
        return "rust_runtime"
    if frame.binary.startswith(("libsystem_", "dyld", "libc++", "libobjc")):
        return "system"
    return "other"


def percentage(weight: int, total: int) -> float:
    return 0.0 if total == 0 else weight * 100.0 / total


class TimeProfileParser:
    def __init__(self, repository_root: Path, anchor: str) -> None:
        self.repository_root = repository_root.resolve()
        self.anchor = normalized_function(anchor)
        self._frames: dict[str, Frame] = {}
        self._backtraces: dict[str, tuple[Frame, ...]] = {}
        self._weights: dict[str, int] = {}
        self._binaries: dict[str, tuple[str, str | None]] = {}
        self._paths: dict[str, str] = {}

    def parse(self, xml_path: Path) -> dict[str, object]:
        try:
            document = ET.parse(xml_path)
        except (OSError, ET.ParseError) as error:
            raise ProfileError(f"cannot parse Time Profiler export {xml_path}: {error}") from error

        all_rows = 0
        rows_with_stack = 0
        all_weight_ns = 0
        scoped_samples = 0
        scoped_weight_ns = 0
        leaf_counts: Counter[tuple[str, str]] = Counter()
        leaf_weights: Counter[tuple[str, str]] = Counter()
        project_counts: Counter[tuple[str, str]] = Counter()
        project_weights: Counter[tuple[str, str]] = Counter()
        inclusive_counts: Counter[tuple[str, str]] = Counter()
        inclusive_weights: Counter[tuple[str, str]] = Counter()
        category_counts: Counter[str] = Counter()
        category_weights: Counter[str] = Counter()
        source_counts: Counter[tuple[str, int, str]] = Counter()
        source_weights: Counter[tuple[str, int, str]] = Counter()
        folded_counts: Counter[tuple[str, ...]] = Counter()
        folded_weights: Counter[tuple[str, ...]] = Counter()
        metadata: dict[tuple[str, str], Frame] = {}

        for row in document.getroot().iter("row"):
            all_rows += 1
            weight = self._resolve_weight(row.find("weight"))
            all_weight_ns += weight
            frames = self._resolve_backtrace(row.find("tagged-backtrace"))
            if not frames:
                continue
            rows_with_stack += 1
            try:
                anchor_index = next(
                    index
                    for index, frame in enumerate(frames)
                    if self.anchor in frame.function
                )
            except StopIteration:
                continue

            scope_frames = frames[: anchor_index + 1]
            scoped_samples += 1
            scoped_weight_ns += weight

            leaf = scope_frames[0]
            leaf_identity = frame_key(leaf)
            metadata.setdefault(leaf_identity, leaf)
            leaf_counts[leaf_identity] += 1
            leaf_weights[leaf_identity] += weight
            category = category_for_frame(leaf)
            category_counts[category] += 1
            category_weights[category] += weight

            project_leaf = next(
                (
                    frame
                    for frame in scope_frames
                    if frame.source is not None
                    and frame.source.startswith("$REPO/")
                ),
                None,
            )
            if project_leaf is not None:
                project_identity = frame_key(project_leaf)
                metadata.setdefault(project_identity, project_leaf)
                project_counts[project_identity] += 1
                project_weights[project_identity] += weight
                if project_leaf.line is not None and project_leaf.source is not None:
                    source_identity = (
                        project_leaf.source,
                        project_leaf.line,
                        project_leaf.function,
                    )
                    source_counts[source_identity] += 1
                    source_weights[source_identity] += weight

            seen: set[tuple[str, str]] = set()
            for frame in scope_frames:
                identity = frame_key(frame)
                metadata.setdefault(identity, frame)
                if identity in seen:
                    continue
                seen.add(identity)
                inclusive_counts[identity] += 1
                inclusive_weights[identity] += weight

            folded = tuple(frame.function for frame in reversed(scope_frames))
            folded_counts[folded] += 1
            folded_weights[folded] += weight

        if scoped_samples == 0:
            raise ProfileError(
                f"Time Profiler export contains no stack with scope anchor {self.anchor!r}"
            )

        return {
            "rows": all_rows,
            "rowsWithStack": rows_with_stack,
            "allWeightNs": all_weight_ns,
            "scopedSamples": scoped_samples,
            "scopedWeightNs": scoped_weight_ns,
            "leafFunctions": function_entries(
                leaf_counts, leaf_weights, metadata, scoped_weight_ns
            ),
            "projectLeafFunctions": function_entries(
                project_counts, project_weights, metadata, scoped_weight_ns
            ),
            "inclusiveFunctions": function_entries(
                inclusive_counts, inclusive_weights, metadata, scoped_weight_ns
            ),
            "leafCategories": named_entries(
                category_counts, category_weights, scoped_weight_ns, "category"
            ),
            "projectSourceLines": source_entries(
                source_counts, source_weights, scoped_weight_ns
            ),
            "foldedStacks": folded_entries(
                folded_counts, folded_weights, scoped_weight_ns
            ),
        }

    def _resolve_weight(self, element: ET.Element | None) -> int:
        if element is None:
            return 0
        reference = element.get("ref")
        if reference is not None:
            try:
                return self._weights[reference]
            except KeyError as error:
                raise ProfileError(f"unknown Time Profiler weight ref {reference}") from error
        try:
            weight = int(element.text or "0")
        except ValueError as error:
            raise ProfileError(f"invalid Time Profiler weight {element.text!r}") from error
        identifier = element.get("id")
        if identifier is not None:
            self._weights[identifier] = weight
        return weight

    def _resolve_backtrace(
        self, element: ET.Element | None
    ) -> tuple[Frame, ...] | None:
        if element is None:
            return None
        reference = element.get("ref")
        if reference is not None:
            try:
                return self._backtraces[reference]
            except KeyError as error:
                raise ProfileError(
                    f"unknown Time Profiler backtrace ref {reference}"
                ) from error
        frames = tuple(self._resolve_frame(frame) for frame in element.findall("frame"))
        identifier = element.get("id")
        if identifier is not None:
            self._backtraces[identifier] = frames
        return frames

    def _resolve_frame(self, element: ET.Element) -> Frame:
        reference = element.get("ref")
        if reference is not None:
            try:
                return self._frames[reference]
            except KeyError as error:
                raise ProfileError(f"unknown Time Profiler frame ref {reference}") from error

        binary_name, _ = self._resolve_binary(element.find("binary"))
        source_element = element.find("source")
        source = None
        line = None
        if source_element is not None:
            source = self._resolve_path(source_element.find("path"))
            raw_line = source_element.get("line")
            if raw_line is not None:
                try:
                    parsed_line = int(raw_line)
                except ValueError as error:
                    raise ProfileError(f"invalid source line {raw_line!r}") from error
                line = parsed_line if parsed_line > 0 else None
        frame = Frame(
            function=normalized_function(element.get("name")),
            binary=binary_name,
            source=normalized_path(source, self.repository_root),
            line=line,
        )
        identifier = element.get("id")
        if identifier is not None:
            self._frames[identifier] = frame
        return frame

    def _resolve_binary(
        self, element: ET.Element | None
    ) -> tuple[str, str | None]:
        if element is None:
            return "<unknown>", None
        reference = element.get("ref")
        if reference is not None:
            try:
                return self._binaries[reference]
            except KeyError as error:
                raise ProfileError(f"unknown Time Profiler binary ref {reference}") from error
        binary = (element.get("name", "<unknown>"), element.get("path"))
        identifier = element.get("id")
        if identifier is not None:
            self._binaries[identifier] = binary
        return binary

    def _resolve_path(self, element: ET.Element | None) -> str | None:
        if element is None:
            return None
        reference = element.get("ref")
        if reference is not None:
            try:
                return self._paths[reference]
            except KeyError as error:
                raise ProfileError(f"unknown Time Profiler path ref {reference}") from error
        path = element.text or ""
        identifier = element.get("id")
        if identifier is not None:
            self._paths[identifier] = path
        return path


def function_entries(
    counts: Counter[tuple[str, str]],
    weights: Counter[tuple[str, str]],
    metadata: dict[tuple[str, str], Frame],
    total_weight: int,
) -> list[dict[str, object]]:
    entries = []
    for identity, weight in sorted(
        weights.items(), key=lambda item: (-item[1], item[0])
    ):
        frame = metadata[identity]
        entries.append(
            {
                "function": frame.function,
                "binary": frame.binary,
                "source": frame.source,
                "line": frame.line,
                "samples": counts[identity],
                "weightNs": weight,
                "percentOfScopedWeight": percentage(weight, total_weight),
            }
        )
    return entries


def named_entries(
    counts: Counter[str],
    weights: Counter[str],
    total_weight: int,
    field: str,
) -> list[dict[str, object]]:
    return [
        {
            field: name,
            "samples": counts[name],
            "weightNs": weight,
            "percentOfScopedWeight": percentage(weight, total_weight),
        }
        for name, weight in sorted(
            weights.items(), key=lambda item: (-item[1], item[0])
        )
    ]


def source_entries(
    counts: Counter[tuple[str, int, str]],
    weights: Counter[tuple[str, int, str]],
    total_weight: int,
) -> list[dict[str, object]]:
    return [
        {
            "source": source,
            "line": line,
            "function": function,
            "samples": counts[(source, line, function)],
            "weightNs": weight,
            "percentOfScopedWeight": percentage(weight, total_weight),
        }
        for (source, line, function), weight in sorted(
            weights.items(), key=lambda item: (-item[1], item[0])
        )
    ]


def folded_entries(
    counts: Counter[tuple[str, ...]],
    weights: Counter[tuple[str, ...]],
    total_weight: int,
) -> list[dict[str, object]]:
    return [
        {
            "stack": list(stack),
            "samples": counts[stack],
            "weightNs": weight,
            "percentOfScopedWeight": percentage(weight, total_weight),
        }
        for stack, weight in sorted(
            weights.items(), key=lambda item: (-item[1], item[0])
        )
    ]


def command_output(
    command: Sequence[str],
    *,
    environment: dict[str, str] | None = None,
    required: bool = False,
) -> str | None:
    try:
        completed = subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
            timeout=20,
            env=environment,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        if required:
            raise ProfileError(f"command failed: {command!r}: {error}") from error
        return None
    return completed.stdout.strip() or completed.stderr.strip()


def developer_environment(developer_dir: Path) -> dict[str, str]:
    environment = os.environ.copy()
    environment["DEVELOPER_DIR"] = str(developer_dir)
    return environment


def discover_developer_dir(explicit: Path | None) -> tuple[Path, dict[str, str]]:
    candidates: list[Path] = []
    if explicit is not None:
        candidates.append(explicit.expanduser())
    configured = os.environ.get("DEVELOPER_DIR")
    if configured:
        candidates.append(Path(configured).expanduser())
    selected = command_output(("xcode-select", "-p"))
    if selected:
        candidates.append(Path(selected))
    candidates.extend(
        (
            Path("/Applications/Xcode.app/Contents/Developer"),
            Path("/Applications/Xcode-beta.app/Contents/Developer"),
        )
    )

    failures = []
    seen: set[Path] = set()
    for candidate in candidates:
        try:
            resolved = candidate.resolve()
        except OSError:
            resolved = candidate
        if resolved in seen:
            continue
        seen.add(resolved)
        environment = developer_environment(resolved)
        version = command_output(
            ("/usr/bin/xcrun", "xctrace", "version"), environment=environment
        )
        if version is None:
            failures.append(str(resolved))
            continue
        xcode = command_output(
            ("/usr/bin/xcodebuild", "-version"),
            environment=environment,
            required=True,
        )
        executable = command_output(
            ("/usr/bin/xcrun", "--find", "xctrace"),
            environment=environment,
            required=True,
        )
        return resolved, {
            "xctraceVersion": version,
            "xcodeVersion": xcode or "",
            "xctraceExecutable": executable or "",
        }
    raise ProfileError(
        "no installed Xcode developer directory provides xctrace; checked "
        + ", ".join(failures)
    )


def build_profile_worker(root: Path, target_dir: Path) -> tuple[Path, dict[str, str]]:
    environment = os.environ.copy()
    build_environment = {
        "CARGO_TARGET_DIR": str(target_dir.resolve()),
        "CARGO_PROFILE_RELEASE_DEBUG": "1",
        "CARGO_PROFILE_RELEASE_STRIP": "false",
        "CARGO_INCREMENTAL": "0",
    }
    environment.update(build_environment)
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
        env=environment,
        check=True,
    )
    worker = target_dir.resolve() / "release/examples/m6_baseline_worker"
    if not worker.is_file() or not os.access(worker, os.X_OK):
        raise ProfileError(f"profile worker was not produced at {worker}")
    return worker, build_environment


def parse_trace_toc(path: Path) -> dict[str, object]:
    try:
        root = ET.parse(path).getroot()
    except (OSError, ET.ParseError) as error:
        raise ProfileError(f"cannot parse xctrace table of contents: {error}") from error
    run = root.find("run")
    if run is None:
        raise ProfileError("xctrace table of contents has no run")
    target = run.find("./info/target/process")
    summary = run.find("./info/summary")
    if target is None or summary is None:
        raise ProfileError("xctrace table of contents omits target or summary")
    if target.get("return-exit-status") != "0":
        raise ProfileError(
            "profiled worker did not exit successfully: "
            f"{target.get('return-exit-status')!r}"
        )
    time_profile = next(
        (
            table
            for table in run.findall("./data/table")
            if table.get("schema") == "time-profile"
        ),
        None,
    )
    time_sample = next(
        (
            table
            for table in run.findall("./data/table")
            if table.get("schema") == "time-sample"
        ),
        None,
    )
    if time_profile is None or time_sample is None:
        raise ProfileError("trace does not contain Time Profiler schemas")
    try:
        sample_interval_ns = int(
            time_sample.get("sample-rate-micro-seconds", "")
        ) * 1_000
    except ValueError as error:
        raise ProfileError("trace does not declare a valid sample interval") from error
    if sample_interval_ns <= 0:
        raise ProfileError("trace sample interval must be positive")
    return {
        "target": {
            "name": target.get("name"),
            "pid": int(target.get("pid", "0")),
            "arguments": target.get("arguments", ""),
            "returnExitStatus": int(target.get("return-exit-status", "0")),
            "terminationReason": target.get("termination-reason"),
        },
        "recording": {
            "startDate": text_at(summary, "start-date"),
            "endDate": text_at(summary, "end-date"),
            "durationSeconds": float(text_at(summary, "duration")),
            "endReason": text_at(summary, "end-reason"),
            "instrumentsVersion": text_at(summary, "instruments-version"),
            "template": text_at(summary, "template-name"),
            "recordingMode": text_at(summary, "recording-mode"),
            "sampleIntervalNs": sample_interval_ns,
            "highFrequencySampling": time_profile.get("high-frequency-sampling"),
            "contextSwitchSampling": time_profile.get("context-switch-sampling"),
            "recordWaitingThreads": time_profile.get("record-waiting-threads"),
            "kernelCallstacks": time_profile.get("needs-kernel-callstack"),
        },
    }


def text_at(parent: ET.Element, path: str) -> str:
    element = parent.find(path)
    if element is None or element.text is None:
        raise ProfileError(f"xctrace table of contents omits {path}")
    return element.text


def sha256_tree(path: Path) -> tuple[str, int, int]:
    digest = hashlib.sha256()
    total_size = 0
    files = 0
    for child in sorted(item for item in path.rglob("*") if item.is_file()):
        relative = child.relative_to(path).as_posix().encode("utf-8")
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        with child.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
                total_size += len(chunk)
        files += 1
    if files == 0:
        raise ProfileError(f"trace bundle contains no files: {path}")
    return digest.hexdigest(), total_size, files


def capture_profile(
    *,
    root: Path,
    case: ProfileCase,
    capture_index: int,
    worker: Path,
    developer_dir: Path,
    capture_dir: Path,
    corpus_manifest: dict[str, object],
) -> dict[str, object]:
    capture_dir.mkdir(parents=True, exist_ok=False)
    trace = capture_dir / "profile.trace"
    worker_stdout = capture_dir / "worker.stdout"
    toc = capture_dir / "toc.xml"
    table = capture_dir / "time-profile.xml"
    environment = developer_environment(developer_dir)

    record = subprocess.run(
        (
            "/usr/bin/xcrun",
            "xctrace",
            "record",
            "--no-prompt",
            "--template",
            "Time Profiler",
            "--output",
            str(trace),
            "--target-stdout",
            str(worker_stdout),
            "--launch",
            "--",
            str(worker),
            *case.arguments,
        ),
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
    )
    if record.returncode != 0:
        raise ProfileError(
            f"{case.case_id} capture {capture_index} failed: "
            f"{record.stderr.strip() or record.stdout.strip()}"
        )
    if not trace.is_dir():
        raise ProfileError(f"xctrace did not produce trace bundle {trace}")
    try:
        raw_worker_output = worker_stdout.read_bytes()
    except OSError as error:
        raise ProfileError(f"cannot read profiled worker output: {error}") from error
    worker_output = baseline.parse_worker_output(raw_worker_output, case.mode)
    validate_worker_output(case, worker_output, corpus_manifest)

    export_toc = subprocess.run(
        (
            "/usr/bin/xcrun",
            "xctrace",
            "export",
            "--input",
            str(trace),
            "--toc",
            "--output",
            str(toc),
        ),
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
    )
    if export_toc.returncode != 0:
        raise ProfileError(
            f"cannot export {case.case_id} trace TOC: {export_toc.stderr.strip()}"
        )
    toc_identity = parse_trace_toc(toc)
    validate_trace_configuration(case, toc_identity)
    toc_identity["target"]["arguments"] = str(
        toc_identity["target"]["arguments"]
    ).replace(str(root.resolve()), "$REPO")

    export_table = subprocess.run(
        (
            "/usr/bin/xcrun",
            "xctrace",
            "export",
            "--input",
            str(trace),
            "--xpath",
            '/trace-toc/run[@number="1"]/data/table[@schema="time-profile"]',
            "--output",
            str(table),
        ),
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
    )
    if export_table.returncode != 0:
        raise ProfileError(
            f"cannot export {case.case_id} Time Profiler table: "
            f"{export_table.stderr.strip()}"
        )
    profile = TimeProfileParser(root, case.anchor).parse(table)
    validate_profile_coverage(case, profile, worker_output)
    trace_sha, trace_size, trace_files = sha256_tree(trace)
    return {
        "captureIndex": capture_index,
        "workerOutput": worker_output,
        "trace": {
            "committed": False,
            "bundleSha256": trace_sha,
            "sizeBytes": trace_size,
            "files": trace_files,
            "tocSha256": baseline.sha256_file(toc),
            "timeProfileXmlSha256": baseline.sha256_file(table),
        },
        "toc": toc_identity,
        "profile": profile,
    }


def validate_trace_configuration(
    case: ProfileCase, toc: dict[str, object]
) -> None:
    recording = toc["recording"]
    expected = {
        "template": "Time Profiler",
        "sampleIntervalNs": 1_000_000,
        "highFrequencySampling": "0",
        "contextSwitchSampling": "0",
        "recordWaitingThreads": "0",
        "kernelCallstacks": "0",
    }
    drift = {
        key: {"actual": recording.get(key), "expected": value}
        for key, value in expected.items()
        if recording.get(key) != value
    }
    if drift:
        raise ProfileError(
            f"{case.case_id} Time Profiler configuration drifted: {drift}"
        )


def validate_worker_output(
    case: ProfileCase,
    output: dict[str, Any],
    corpus_manifest: dict[str, object],
) -> None:
    benchmark_case = baseline.BenchmarkCase(
        case.case_id, case.scope, case.description, case.arguments
    )
    baseline.validate_corpus_work(
        [
            {
                "caseId": case.case_id,
                "work": output["work"],
                "details": output["details"],
            }
        ],
        [benchmark_case],
        Path(case.arguments[1]).parent
        if case.mode == "decode"
        else Path("."),
        corpus_manifest,
    )


def validate_profile_coverage(
    case: ProfileCase,
    profile: dict[str, object],
    worker_output: dict[str, Any],
) -> None:
    scoped_samples = int(profile["scopedSamples"])
    scoped_weight = int(profile["scopedWeightNs"])
    elapsed = int(worker_output["workerElapsedNs"])
    if scoped_samples < MIN_SCOPED_SAMPLES:
        raise ProfileError(
            f"{case.case_id} produced only {scoped_samples} scoped samples; "
            f"minimum is {MIN_SCOPED_SAMPLES}"
        )
    ratio = scoped_weight / elapsed
    if not SCOPE_COVERAGE_MIN <= ratio <= SCOPE_COVERAGE_MAX:
        raise ProfileError(
            f"{case.case_id} scoped sample weight / worker time is {ratio:.3f}; "
            f"expected {SCOPE_COVERAGE_MIN:.2f}..{SCOPE_COVERAGE_MAX:.2f}"
        )
    profile["scopedWeightToWorkerElapsedRatio"] = ratio


def merge_profiles(captures: Sequence[dict[str, object]]) -> dict[str, object]:
    if not captures:
        raise ProfileError("cannot merge no profile captures")
    profile_values = [capture["profile"] for capture in captures]
    total_weight = sum(int(profile["scopedWeightNs"]) for profile in profile_values)
    merged: dict[str, object] = {
        "captures": len(captures),
        "rows": sum(int(profile["rows"]) for profile in profile_values),
        "rowsWithStack": sum(
            int(profile["rowsWithStack"]) for profile in profile_values
        ),
        "allWeightNs": sum(
            int(profile["allWeightNs"]) for profile in profile_values
        ),
        "scopedSamples": sum(
            int(profile["scopedSamples"]) for profile in profile_values
        ),
        "scopedWeightNs": total_weight,
    }
    for field, identity_fields in (
        ("leafFunctions", ("function", "binary")),
        ("projectLeafFunctions", ("function", "binary")),
        ("inclusiveFunctions", ("function", "binary")),
        ("leafCategories", ("category",)),
        ("projectSourceLines", ("source", "line", "function")),
        ("foldedStacks", ("stack",)),
    ):
        merged[field] = merge_entries(
            profile_values, field, identity_fields, total_weight
        )
    elapsed_values = [
        float(capture["workerOutput"]["workerElapsedNs"]) for capture in captures
    ]
    coverage_values = [
        float(capture["profile"]["scopedWeightToWorkerElapsedRatio"])
        for capture in captures
    ]
    merged["workerElapsedNs"] = baseline.distribution(elapsed_values)
    merged["scopedWeightToWorkerElapsedRatio"] = baseline.distribution(
        coverage_values
    )
    return merged


def merge_entries(
    profiles: Sequence[dict[str, object]],
    field: str,
    identity_fields: Sequence[str],
    total_weight: int,
) -> list[dict[str, object]]:
    counts: Counter[tuple[object, ...]] = Counter()
    weights: Counter[tuple[object, ...]] = Counter()
    prototypes: dict[tuple[object, ...], dict[str, object]] = {}
    for profile in profiles:
        entries = profile[field]
        if not isinstance(entries, list):
            raise ProfileError(f"profile field {field} is not a list")
        for entry in entries:
            if not isinstance(entry, dict):
                raise ProfileError(f"profile field {field} contains a non-object")
            identity = tuple(
                tuple(entry[name]) if isinstance(entry[name], list) else entry[name]
                for name in identity_fields
            )
            prototypes.setdefault(identity, entry)
            counts[identity] += int(entry["samples"])
            weights[identity] += int(entry["weightNs"])
    result = []
    for identity, weight in sorted(
        weights.items(), key=lambda item: (-item[1], repr(item[0]))
    ):
        entry = {
            key: value
            for key, value in prototypes[identity].items()
            if key not in ("samples", "weightNs", "percentOfScopedWeight")
        }
        entry.update(
            {
                "samples": counts[identity],
                "weightNs": weight,
                "percentOfScopedWeight": percentage(weight, total_weight),
            }
        )
        result.append(entry)
    return result


def validate_capture_consistency(
    case: ProfileCase, captures: Sequence[dict[str, object]]
) -> None:
    fingerprints = {
        capture["workerOutput"]["resultFingerprintSha256"] for capture in captures
    }
    work = {
        baseline.canonical_json_bytes(capture["workerOutput"]["work"])
        for capture in captures
    }
    details = {
        baseline.canonical_json_bytes(capture["workerOutput"]["details"])
        for capture in captures
    }
    if len(fingerprints) != 1 or len(work) != 1 or len(details) != 1:
        raise ProfileError(f"{case.case_id} worker result changed across captures")


def selected_cases(
    available: Sequence[ProfileCase], requested: Sequence[str]
) -> tuple[ProfileCase, ...]:
    if not requested:
        return tuple(available)
    by_id = {case.case_id: case for case in available}
    unknown = sorted(set(requested) - by_id.keys())
    if unknown:
        raise ProfileError(f"unknown profile case id(s): {unknown}")
    return tuple(by_id[case_id] for case_id in requested)


def case_manifest(
    cases: Sequence[ProfileCase], corpus: Path
) -> list[dict[str, object]]:
    corpus_path = str(corpus.resolve())
    return [
        {
            "caseId": case.case_id,
            "scope": case.scope,
            "description": case.description,
            "arguments": [
                argument.replace(corpus_path, "$CORPUS")
                for argument in case.arguments
            ],
            "scopeAnchor": case.anchor,
        }
        for case in cases
    ]


def relative_or_normalized(path: Path, root: Path) -> str:
    try:
        return f"$REPO/{path.resolve().relative_to(root.resolve()).as_posix()}"
    except ValueError:
        return str(path.resolve())


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus-dir", type=Path, default=DEFAULT_CORPUS)
    parser.add_argument("--build-dir", type=Path, default=DEFAULT_BUILD_DIR)
    parser.add_argument("--trace-root", type=Path, default=DEFAULT_TRACE_ROOT)
    parser.add_argument("--developer-dir", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--captures", type=int, default=DEFAULT_CAPTURES)
    parser.add_argument(
        "--case",
        action="append",
        default=[],
        help="capture only this exact case id; may be repeated",
    )
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--list-cases", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(__file__).resolve().parent.parent
    corpus = args.corpus_dir.resolve()
    available = profile_cases(corpus)
    if args.list_cases:
        for case in available:
            print(f"{case.case_id}\t{case.scope}\t{case.description}")
        return 0
    try:
        if sys.platform != "darwin":
            raise ProfileError("M6 xctrace sampling profiles currently require macOS")
        if args.captures <= 0:
            raise ProfileError("--captures must be greater than zero")
        source = baseline.git_identity(root, args.allow_dirty)
        corpus_manifest, corpus_manifest_sha = baseline.verify_corpus(root, corpus)
        cases = selected_cases(available, args.case)
        developer_dir, profiler_identity = discover_developer_dir(args.developer_dir)
        worker, build_environment = build_profile_worker(root, args.build_dir)
        manifests = case_manifest(cases, corpus)
        suite_sha = baseline.sha256_bytes(
            baseline.canonical_json_bytes(manifests)
        )
        run_token = (
            f"{source['commit'][:12]}-"
            f"{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
        )
        trace_directory = args.trace_root.resolve() / run_token
        if trace_directory.exists():
            raise ProfileError(f"trace run directory already exists: {trace_directory}")
        trace_directory.mkdir(parents=True)

        started_at = utc_now()
        load_average_start = list(os.getloadavg())
        case_results = []
        for case in cases:
            captures = []
            case_dir = trace_directory / filesystem_component(case.case_id)
            case_dir.mkdir()
            for capture_index in range(1, args.captures + 1):
                print(
                    f"[profile {case.case_id} {capture_index}/{args.captures}]",
                    file=sys.stderr,
                )
                captures.append(
                    capture_profile(
                        root=root,
                        case=case,
                        capture_index=capture_index,
                        worker=worker,
                        developer_dir=developer_dir,
                        capture_dir=case_dir / f"capture-{capture_index}",
                        corpus_manifest=corpus_manifest,
                    )
                )
            validate_capture_consistency(case, captures)
            case_results.append(
                {
                    "caseId": case.case_id,
                    "scope": case.scope,
                    "captures": captures,
                    "aggregate": merge_profiles(captures),
                }
            )

        source_after_capture = baseline.git_identity(root, True)
        if source_after_capture != source:
            raise ProfileError(
                "repository source identity changed while profiles were being captured"
            )
        environment = baseline.environment_identity(root)
        environment["runner"] = {
            "path": "scripts/run-performance-profile.py",
            "sha256": baseline.sha256_file(Path(__file__).resolve()),
        }
        environment["build"] = {
            **environment["build"],
            "debugInfo": 1,
            "strip": False,
            "incremental": False,
            "environmentOverrides": {
                **build_environment,
                "CARGO_TARGET_DIR": relative_or_normalized(
                    Path(build_environment["CARGO_TARGET_DIR"]), root
                ),
            },
        }
        result: dict[str, object] = {
            "schemaVersion": PROFILE_SCHEMA_VERSION,
            "kind": "m6_sampling_profile",
            "run": {
                "startedAtUtc": started_at,
                "completedAtUtc": utc_now(),
                "loadAverageStart": load_average_start,
                "loadAverageEnd": list(os.getloadavg()),
            },
            "source": source,
            "suite": {
                "id": "m6-sampling-profile-v1",
                "sha256": suite_sha,
                "capturesPerCase": args.captures,
                "cases": manifests,
                "scopeRule": (
                    "include a sample only when its symbolicated stack contains "
                    "the noinline worker function enclosing the worker-timed region"
                ),
                "minimumScopedSamplesPerCapture": MIN_SCOPED_SAMPLES,
                "scopeWeightToWorkerElapsedRange": [
                    SCOPE_COVERAGE_MIN,
                    SCOPE_COVERAGE_MAX,
                ],
            },
            "corpus": {
                "id": corpus_manifest.get("corpusId"),
                "manifestSha256": corpus_manifest_sha,
                "generator": corpus_manifest.get("generator"),
                "mediaFiles": len(corpus_manifest.get("media", [])),
                "committed": False,
            },
            "worker": {
                "path": worker.name,
                "sha256": baseline.sha256_file(worker),
                "sizeBytes": worker.stat().st_size,
                "optimizedReleaseWithDebugSymbols": True,
            },
            "profiler": {
                "kind": "xcode_time_profiler",
                "developerDir": str(developer_dir),
                **profiler_identity,
                "traceBundlesCommitted": False,
                "traceRoot": relative_or_normalized(trace_directory, root),
            },
            "environment": environment,
            "cases": case_results,
        }
        baseline.ensure_finite_json(result)
        target = baseline.rustc_target(environment)
        output = (
            args.output.resolve()
            if args.output is not None
            else trace_directory
            / f"{source['commit'][:12]}-{target}-m6-sampling-profile.json"
        )
        baseline.write_result(output, result, False)
        print(f"performance profile written to {output}")
        print(f"result SHA-256: {baseline.sha256_file(output)}")
        return 0
    except (
        ProfileError,
        baseline.BaselineError,
        OSError,
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
    ) as error:
        print(f"performance profile error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
