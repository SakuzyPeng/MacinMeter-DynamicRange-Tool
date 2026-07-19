#!/usr/bin/env python3
"""Run every safe manifest case through the isolated x64 core worker.

This is a serial orchestration layer over run_foo_dr_meter_108_core.py.  Every
safe case gets a fresh worker process and the complete single-input identity,
timeout, PCM and response validation.  It never runs foobar2000 and cannot
claim host, decoder, component, renderer, or compatibility parity.
"""

from __future__ import annotations

import argparse
import importlib.util
import math
import sys
from pathlib import Path
from typing import Any


PARENT_PATH = Path(__file__).with_name("run_foo_dr_meter_108_core.py")
PARENT_SPEC = importlib.util.spec_from_file_location(
    "_foo_dr_meter_108_core_parent", PARENT_PATH
)
if PARENT_SPEC is None or PARENT_SPEC.loader is None:
    raise RuntimeError(f"cannot import {PARENT_PATH}")
PARENT = importlib.util.module_from_spec(PARENT_SPEC)
sys.modules[PARENT_SPEC.name] = PARENT
PARENT_SPEC.loader.exec_module(PARENT)


SCHEMA_VERSION = 1
RECORD_KIND = "foo_dr_meter_108_core_suite_record"
RUNTIME_PROFILE = "fixed_foobar_2_25_10"


def _load_safe_cases(
    manifest_path: Path,
) -> tuple[dict[str, Any], bytes, list[dict[str, Any]]]:
    raw = PARENT.require_regular_file_bytes(
        manifest_path, "suite manifest", maximum_bytes=16 * 1024 * 1024
    )
    manifest = PARENT.load_json_object_bytes(raw, "suite manifest")
    if manifest.get("schemaVersion") != 2:
        raise PARENT.CoreHarnessError("suite manifest schema is unsupported")
    PARENT.require_identifier(manifest.get("corpusId"), "suite corpus ID")
    cases = manifest.get("cases")
    if not isinstance(cases, list) or not cases:
        raise PARENT.CoreHarnessError("suite manifest cases must be non-empty")

    identifiers: set[str] = set()
    orders: list[int] = []
    safe: list[dict[str, Any]] = []
    for index, value in enumerate(cases):
        if not isinstance(value, dict):
            raise PARENT.CoreHarnessError("suite manifest case must be an object")
        case_id = PARENT.require_identifier(
            value.get("id"), f"suite manifest case {index} ID"
        )
        if case_id in identifiers:
            raise PARENT.CoreHarnessError("suite manifest repeats a case ID")
        identifiers.add(case_id)
        order = PARENT.require_int(
            value.get("order"), f"suite manifest case {case_id} order", minimum=1
        )
        orders.append(order)
        execution_class = value.get("executionClass")
        if not isinstance(execution_class, str) or not execution_class:
            raise PARENT.CoreHarnessError(
                "suite manifest case executionClass is invalid"
            )
        if execution_class == "safe":
            PARENT.require_sha256(
                value.get("fileSha256"),
                f"suite manifest case {case_id} file SHA-256",
            )
            PARENT.require_int(
                value.get("byteLength"),
                f"suite manifest case {case_id} byte length",
                minimum=1,
            )
            PARENT.require_identifier(
                value.get("encoding"),
                f"suite manifest case {case_id} encoding",
            )
            PARENT.require_int(
                value.get("sampleRateHz"),
                f"suite manifest case {case_id} sample rate",
                minimum=1,
            )
            PARENT.require_int(
                value.get("channels"),
                f"suite manifest case {case_id} channels",
                minimum=1,
                maximum=PARENT.MAX_CHANNELS,
            )
            PARENT.require_int(
                value.get("frames"),
                f"suite manifest case {case_id} frames",
                minimum=0,
            )
            safe.append(value)

    expected_orders = list(range(1, len(cases) + 1))
    if orders != expected_orders:
        raise PARENT.CoreHarnessError(
            "suite manifest cases are not in canonical contiguous order"
        )
    budgets = manifest.get("budgets")
    if not isinstance(budgets, dict):
        raise PARENT.CoreHarnessError("suite manifest budgets must be an object")
    expected_safe = PARENT.require_int(
        budgets.get("expectedSafeMasterEntries"),
        "suite manifest expected safe count",
        minimum=1,
    )
    if len(safe) != expected_safe:
        raise PARENT.CoreHarnessError(
            "suite manifest safe count differs from its budget"
        )
    return manifest, raw, safe


def _runtime_artifacts(
    sources: dict[str, tuple[Path, str]],
) -> tuple[PARENT.RuntimeArtifact, ...]:
    if set(sources) != set(PARENT.RUNTIME_ARTIFACT_NAMES):
        raise PARENT.CoreHarnessError("suite runtime allowlist differs")
    artifacts: list[PARENT.RuntimeArtifact] = []
    resolved_paths: set[Path] = set()
    for name in PARENT.RUNTIME_ARTIFACT_NAMES:
        path, expected_sha256 = sources[name]
        if path.name.casefold() != name:
            raise PARENT.CoreHarnessError("suite runtime basename differs")
        try:
            resolved = path.resolve(strict=True)
        except OSError as error:
            raise PARENT.CoreHarnessError("suite runtime artifact is absent") from error
        if resolved in resolved_paths:
            raise PARENT.CoreHarnessError("suite runtime sources are not unique")
        resolved_paths.add(resolved)
        raw = PARENT.require_regular_file_bytes(
            path, f"suite runtime {name}", maximum_bytes=64 * 1024 * 1024
        )
        identity = PARENT.assert_expected_identity(
            raw, expected_sha256, None, f"suite runtime {name}"
        )
        artifacts.append(PARENT.RuntimeArtifact(name, path, identity))
    return tuple(artifacts)


def _common_identities(
    *,
    worker_path: Path,
    worker_sha256: str,
    target_path: Path,
    runtime_artifact_sources: dict[str, tuple[Path, str]],
) -> tuple[
    PARENT.FileIdentity,
    PARENT.FileIdentity,
    tuple[PARENT.RuntimeArtifact, ...],
]:
    worker_raw = PARENT.require_regular_file_bytes(
        worker_path, "suite worker", maximum_bytes=256 * 1024 * 1024
    )
    worker = PARENT.assert_expected_identity(
        worker_raw, worker_sha256, None, "suite worker"
    )
    target_raw = PARENT.require_regular_file_bytes(
        target_path, "suite target", maximum_bytes=16 * 1024 * 1024
    )
    target = PARENT.assert_expected_identity(
        target_raw,
        PARENT.EXPECTED_TARGET_SHA256,
        PARENT.EXPECTED_TARGET_BYTE_LENGTH,
        "suite target",
    )
    return worker, target, _runtime_artifacts(runtime_artifact_sources)


def _prepared_input_record(prepared: PARENT.PreparedPcm) -> dict[str, Any]:
    return {
        "inputId": prepared.input_id,
        "sourceKind": prepared.source_kind,
        "sourceEncoding": prepared.source_encoding,
        "conversion": prepared.conversion,
        "sourceSha256": prepared.source_identity.sha256,
        "sourceByteLength": prepared.source_identity.byte_length,
        "manifestSha256": prepared.manifest_sha256,
        "pcmSha256": prepared.pcm_identity.sha256,
        "pcmByteLength": prepared.pcm_identity.byte_length,
        "sampleRateHz": prepared.sample_rate,
        "channels": prepared.channels,
        "frames": prepared.frames,
    }


def _prepare_manifest_snapshot_case(
    case: dict[str, Any],
    *,
    manifest_sha256: str,
    corpus_root: Path,
) -> PARENT.PreparedPcm:
    """Prepare one fixture from the manifest object loaded for this suite.

    The single-input helper intentionally reads its manifest path itself.  A
    suite must not do that for every item: replacing the manifest between
    worker processes could otherwise combine cases from multiple manifests
    under one suite identity.
    """

    fixture_id = PARENT.require_identifier(case.get("id"), "suite fixture ID")
    relative = PARENT.require_portable_relative_path(
        case.get("path"), "suite fixture manifest case path"
    )
    try:
        root = corpus_root.resolve(strict=True)
        fixture_path = (root / relative).resolve(strict=True)
        fixture_path.relative_to(root)
    except (OSError, ValueError) as error:
        raise PARENT.CoreHarnessError(
            "suite fixture path escapes or is absent from corpus"
        ) from error
    raw = PARENT.require_regular_file_bytes(
        fixture_path, "suite fixture file", maximum_bytes=512 * 1024 * 1024
    )
    source_identity = PARENT.assert_expected_identity(
        raw,
        PARENT.require_sha256(
            case.get("fileSha256"), "suite fixture manifest file SHA-256"
        ),
        PARENT.require_int(
            case.get("byteLength"), "suite fixture manifest byte length"
        ),
        "suite fixture file",
    )
    info, data = PARENT._parse_wave(raw)
    expected_fields = {
        "encoding": info["encoding"],
        "sampleRateHz": info["sampleRate"],
        "channels": info["channels"],
        "frames": info["frames"],
    }
    for key, actual in expected_fields.items():
        if case.get(key) != actual:
            raise PARENT.CoreHarnessError(
                f"suite fixture manifest {key} differs from WAVE"
            )
    if PARENT.require_sha256(
        case.get("dataSha256"), "suite fixture manifest data SHA-256"
    ) != PARENT.sha256_bytes(data):
        raise PARENT.CoreHarnessError(
            "suite fixture WAVE data SHA-256 differs from manifest"
        )
    pcm = PARENT._wave_data_to_f64le(info, data)
    PARENT._validate_f64le(
        pcm,
        channels=int(info["channels"]),
        frames=int(info["frames"]),
        context="converted suite fixture PCM",
    )
    return PARENT.PreparedPcm(
        input_id=fixture_id,
        source_kind="manifest_wav_fixture",
        source_encoding=str(info["encoding"]),
        conversion="strict_wav_sample_to_binary64",
        source_identity=source_identity,
        pcm=pcm,
        sample_rate=int(info["sampleRate"]),
        channels=int(info["channels"]),
        frames=int(info["frames"]),
        manifest_sha256=manifest_sha256,
    )


def _unprepared_input_record(
    case: dict[str, Any], manifest_sha256: str
) -> dict[str, Any]:
    return {
        "inputId": case["id"],
        "sourceKind": "manifest_wav_fixture",
        "sourceEncoding": case.get("encoding"),
        "conversion": "strict_wav_sample_to_binary64",
        "sourceSha256": case["fileSha256"],
        "sourceByteLength": case["byteLength"],
        "manifestSha256": manifest_sha256,
        "pcmSha256": None,
        "pcmByteLength": None,
        "sampleRateHz": case.get("sampleRateHz"),
        "channels": case.get("channels"),
        "frames": case.get("frames"),
    }


def _item_claims() -> dict[str, str]:
    return {
        "scope": "isolated foo_dr_meter 1.0.8 x64 analyzer core",
        "compatibility": "none",
        "foobarParity": "not_assessed",
    }


def run_suite(
    *,
    manifest_path: Path,
    corpus_root: Path,
    worker_path: Path,
    worker_sha256: str,
    target_path: Path,
    runtime_artifact_sources: dict[str, tuple[Path, str]],
    timeout_seconds: float = 30.0,
    block_frames: int = PARENT.DEFAULT_BLOCK_FRAMES,
) -> dict[str, Any]:
    if (
        isinstance(timeout_seconds, bool)
        or not isinstance(timeout_seconds, (int, float))
        or not math.isfinite(timeout_seconds)
        or timeout_seconds <= 0
        or timeout_seconds > 300
    ):
        raise PARENT.CoreHarnessError("suite timeout is outside (0, 300]")
    block_frames = PARENT.require_int(
        block_frames,
        "suite block frames",
        minimum=1,
        maximum=PARENT.MAX_BLOCK_FRAMES,
    )
    manifest, manifest_raw, safe_cases = _load_safe_cases(manifest_path)
    manifest_sha256 = PARENT.sha256_bytes(manifest_raw)
    worker_identity, target_identity, runtime_artifacts = _common_identities(
        worker_path=worker_path,
        worker_sha256=worker_sha256,
        target_path=target_path,
        runtime_artifact_sources=runtime_artifact_sources,
    )
    runtime_config = {
        artifact.name: (
            artifact.source_path,
            artifact.identity.sha256,
        )
        for artifact in runtime_artifacts
    }

    items: list[dict[str, Any]] = []
    for case in safe_cases:
        prepared: PARENT.PreparedPcm | None = None
        request_id: str | None = None
        input_record = _unprepared_input_record(case, manifest_sha256)
        try:
            prepared = _prepare_manifest_snapshot_case(
                case,
                manifest_sha256=manifest_sha256,
                corpus_root=corpus_root,
            )
            input_record = _prepared_input_record(prepared)
            request_id = PARENT._request_identity(
                prepared,
                worker_identity,
                target_identity,
                runtime_artifacts,
                RUNTIME_PROFILE,
                block_frames,
            )
            record = PARENT.run_core_worker(
                prepared,
                worker_path=worker_path,
                worker_sha256=worker_identity.sha256,
                target_path=target_path,
                runtime_artifact_sources=runtime_config,
                runtime_profile=RUNTIME_PROFILE,
                timeout_seconds=float(timeout_seconds),
                block_frames=block_frames,
            )
            if record["requestId"] != request_id:
                raise PARENT.CoreHarnessError("suite item request identity changed")
            item_result: dict[str, Any] = {
                "kind": "success",
                "data": record["result"],
            }
        except PARENT.WorkerReportedError as error:
            item_result = {
                "kind": "error",
                "stage": "worker",
                "code": "worker_reported_error",
                "workerCode": error.code,
            }
        except PARENT.CoreHarnessError:
            item_result = {
                "kind": "error",
                "stage": "input" if prepared is None else "worker",
                "code": "contract_violation",
                "workerCode": None,
            }
        except OSError:
            item_result = {
                "kind": "error",
                "stage": "input" if prepared is None else "worker",
                "code": "io_failure",
                "workerCode": None,
            }
        items.append(
            {
                "manifestOrder": case["order"],
                "inputId": case["id"],
                "requestId": request_id,
                "input": input_record,
                "result": item_result,
                "claims": _item_claims(),
            }
        )

    succeeded = sum(item["result"]["kind"] == "success" for item in items)
    failed = len(items) - succeeded
    status = "success" if failed == 0 else ("failed" if succeeded == 0 else "partial")
    suite_identity = {
        "schemaVersion": SCHEMA_VERSION,
        "manifestSha256": manifest_sha256,
        "corpusId": manifest["corpusId"],
        "safeCaseIds": [case["id"] for case in safe_cases],
        "worker": {
            "sha256": worker_identity.sha256,
            "byteLength": worker_identity.byte_length,
        },
        "target": {
            "sha256": target_identity.sha256,
            "byteLength": target_identity.byte_length,
            "runtimeProfile": RUNTIME_PROFILE,
            "runtimeArtifacts": [
                {
                    "name": artifact.name,
                    "sha256": artifact.identity.sha256,
                    "byteLength": artifact.identity.byte_length,
                }
                for artifact in runtime_artifacts
            ],
        },
        "timeoutSeconds": float(timeout_seconds),
        "blockFrames": block_frames,
    }
    record = {
        "schemaVersion": SCHEMA_VERSION,
        "kind": RECORD_KIND,
        "suiteId": PARENT.sha256_bytes(PARENT.canonical_json_bytes(suite_identity)),
        "corpus": {
            "id": manifest["corpusId"],
            "manifestSha256": manifest_sha256,
            "safeCaseCount": len(safe_cases),
            "safeCaseIds": [case["id"] for case in safe_cases],
        },
        "execution": {
            "workerSha256": worker_identity.sha256,
            "workerByteLength": worker_identity.byte_length,
            "timeoutSeconds": float(timeout_seconds),
            "blockFrames": block_frames,
            "processModel": "one_worker_process_per_input",
        },
        "target": suite_identity["target"],
        "items": items,
        "summary": {
            "status": status,
            "total": len(items),
            "succeeded": succeeded,
            "failed": failed,
        },
        "claims": {
            "scope": "serial isolated analyzer-core suite",
            "compatibility": "none",
            "foobarParity": "not_assessed",
        },
        "limitations": [
            "No foobar decoder, component lifecycle, metadata, album grouping, or renderer was exercised.",
            "Manifest WAV conversion is a harness operation, not a foobar decoder observation.",
        ],
    }
    PARENT.assert_path_free(record, "core suite record")
    PARENT.canonical_json_bytes(record)
    return record


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--corpus-root", required=True, type=Path)
    parser.add_argument("--worker", required=True, type=Path)
    parser.add_argument("--worker-sha256", required=True)
    parser.add_argument("--target-dll", required=True, type=Path)
    parser.add_argument("--shared-dll", required=True, type=Path)
    parser.add_argument("--shared-sha256", required=True)
    parser.add_argument("--msvcp140-dll", required=True, type=Path)
    parser.add_argument("--msvcp140-sha256", required=True)
    parser.add_argument("--vcruntime140-dll", required=True, type=Path)
    parser.add_argument("--vcruntime140-sha256", required=True)
    parser.add_argument("--vcruntime140-1-dll", required=True, type=Path)
    parser.add_argument("--vcruntime140-1-sha256", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    parser.add_argument("--block-frames", type=int, default=PARENT.DEFAULT_BLOCK_FRAMES)
    parser.add_argument("--output", type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        record = run_suite(
            manifest_path=args.manifest,
            corpus_root=args.corpus_root,
            worker_path=args.worker,
            worker_sha256=args.worker_sha256,
            target_path=args.target_dll,
            runtime_artifact_sources={
                "shared.dll": (args.shared_dll, args.shared_sha256),
                "msvcp140.dll": (args.msvcp140_dll, args.msvcp140_sha256),
                "vcruntime140.dll": (
                    args.vcruntime140_dll,
                    args.vcruntime140_sha256,
                ),
                "vcruntime140_1.dll": (
                    args.vcruntime140_1_dll,
                    args.vcruntime140_1_sha256,
                ),
            },
            timeout_seconds=args.timeout_seconds,
            block_frames=args.block_frames,
        )
        PARENT._write_record(record, args.output)
        return 0 if record["summary"]["failed"] == 0 else 1
    except (PARENT.CoreHarnessError, OSError):
        print("core suite error: contract_violation", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
