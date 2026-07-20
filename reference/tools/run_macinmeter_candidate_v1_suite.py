#!/usr/bin/env python3
"""Run the safe complete-v2 corpus through the MacinMeter Candidate V1 worker.

The runner is intentionally decoder-independent. It validates each manifest
WAV and converts its declared sample encoding to finite interleaved f64le with
the same reference-side adapter used by the isolated x64 core suite. It then
starts one pinned MacinMeter worker process per input and records only path-free
identities and final public results.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


REFERENCE_SUITE_PATH = Path(__file__).with_name(
    "run_foo_dr_meter_108_core_suite.py"
)
REFERENCE_SUITE_SPEC = importlib.util.spec_from_file_location(
    "_macinmeter_candidate_reference_suite", REFERENCE_SUITE_PATH
)
if REFERENCE_SUITE_SPEC is None or REFERENCE_SUITE_SPEC.loader is None:
    raise RuntimeError(f"cannot import {REFERENCE_SUITE_PATH}")
REFERENCE_SUITE = importlib.util.module_from_spec(REFERENCE_SUITE_SPEC)
sys.modules[REFERENCE_SUITE_SPEC.name] = REFERENCE_SUITE
REFERENCE_SUITE_SPEC.loader.exec_module(REFERENCE_SUITE)
PARENT = REFERENCE_SUITE.PARENT


SCHEMA_VERSION = 1
RECORD_KIND = "macinmeter_candidate_v1_direct_pcm_suite"
WORKER_RESULT_KIND = "macinmeter_candidate_v1_conformance_result"
EXPECTED_PROFILE = "foo_dr_meter_1_0_8_candidate_v1"
EXPECTED_COMPATIBILITY = "unverified"
SOURCE_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
F32_BITS_RE = re.compile(r"^[0-9a-f]{8}$")
MAX_WORKER_BYTES = 256 * 1024 * 1024
MAX_WORKER_OUTPUT_BYTES = 4 * 1024 * 1024
DEFAULT_BLOCK_FRAMES = 4096
MAX_BLOCK_FRAMES = 1_048_576


class CandidateSuiteError(ValueError):
    """The direct Candidate suite contract was not satisfied."""


def _require_dict(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise CandidateSuiteError(f"{context} must be an object")
    return value


def _require_list(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise CandidateSuiteError(f"{context} must be an array")
    return value


def _require_string(value: Any, context: str) -> str:
    if not isinstance(value, str):
        raise CandidateSuiteError(f"{context} must be a string")
    return value


def _require_f32_bits(value: Any, context: str, *, optional: bool = False) -> None:
    if optional and value is None:
        return
    if not isinstance(value, str) or F32_BITS_RE.fullmatch(value) is None:
        raise CandidateSuiteError(f"{context} must be lowercase binary32 bits")


def _worker_identity(worker_path: Path, expected_sha256: str) -> Any:
    raw = PARENT.require_regular_file_bytes(
        worker_path,
        "MacinMeter Candidate worker",
        maximum_bytes=MAX_WORKER_BYTES,
    )
    return PARENT.assert_expected_identity(
        raw,
        PARENT.require_sha256(expected_sha256, "Candidate worker SHA-256"),
        None,
        "MacinMeter Candidate worker",
    )


def _request_identity(
    *,
    prepared: Any,
    worker_sha256: str,
    worker_byte_length: int,
    source_commit: str,
    block_frames: int,
) -> str:
    semantic = {
        "schemaVersion": SCHEMA_VERSION,
        "inputId": prepared.input_id,
        "pcmSha256": prepared.pcm_identity.sha256,
        "pcmByteLength": prepared.pcm_identity.byte_length,
        "sampleRateHz": prepared.sample_rate,
        "channels": prepared.channels,
        "frames": prepared.frames,
        "workerSha256": worker_sha256,
        "workerByteLength": worker_byte_length,
        "sourceCommit": source_commit,
        "blockFrames": block_frames,
    }
    return PARENT.sha256_bytes(PARENT.canonical_json_bytes(semantic))


def _validate_worker_result(
    value: Any,
    *,
    prepared: Any,
    block_frames: int,
) -> dict[str, Any]:
    result = _require_dict(value, "worker result")
    if result.get("schemaVersion") != SCHEMA_VERSION:
        raise CandidateSuiteError("worker result schema version is unsupported")
    if result.get("kind") != WORKER_RESULT_KIND:
        raise CandidateSuiteError("worker result kind is unsupported")
    if result.get("inputId") != prepared.input_id:
        raise CandidateSuiteError("worker result input ID differs")

    input_value = _require_dict(result.get("input"), "worker result input")
    expected_input = {
        "sampleRateHz": prepared.sample_rate,
        "channels": prepared.channels,
        "frames": prepared.frames,
        "blockFrames": block_frames,
        "sampleEncoding": "f64le-interleaved",
    }
    if input_value != expected_input:
        raise CandidateSuiteError("worker result input geometry differs")

    algorithm = _require_dict(result.get("algorithm"), "worker algorithm")
    if algorithm.get("profile") != EXPECTED_PROFILE:
        raise CandidateSuiteError("worker used an unexpected analysis profile")
    if algorithm.get("profileVersion") != 1:
        raise CandidateSuiteError("worker used an unexpected profile version")
    if algorithm.get("compatibility") != EXPECTED_COMPATIBILITY:
        raise CandidateSuiteError("worker changed the compatibility status")

    analysis = _require_dict(result.get("analysis"), "worker analysis")
    if analysis.get("framesSeen") != prepared.frames:
        raise CandidateSuiteError("worker analysis frame count differs")
    stream = _require_dict(analysis.get("stream"), "worker analysis stream")
    if stream.get("sampleRate") != prepared.sample_rate:
        raise CandidateSuiteError("worker analysis sample rate differs")
    if stream.get("channels") != prepared.channels:
        raise CandidateSuiteError("worker analysis channel count differs")
    analysis_channels = _require_list(
        analysis.get("channels"), "worker analysis channels"
    )
    if len(analysis_channels) != prepared.channels:
        raise CandidateSuiteError("worker analysis channel geometry differs")

    core_bits = _require_dict(result.get("coreBits"), "worker core bits")
    _require_f32_bits(
        core_bits.get("trackDrBits"),
        "worker track DR bits",
        optional=True,
    )
    channel_bits = _require_list(
        core_bits.get("channelResults"), "worker channel bit results"
    )
    if len(channel_bits) != prepared.channels:
        raise CandidateSuiteError("worker core-bit channel geometry differs")
    for index, channel_value in enumerate(channel_bits):
        channel = _require_dict(channel_value, f"worker channel bits {index}")
        if channel.get("index") != index:
            raise CandidateSuiteError("worker channel bit indices are not contiguous")
        if channel.get("outcome") not in {
            "measured",
            "silent",
            "insufficient_data",
        }:
            raise CandidateSuiteError("worker channel outcome is unsupported")
        _require_f32_bits(
            channel.get("drBits"),
            f"worker channel {index} DR bits",
            optional=True,
        )
        _require_f32_bits(
            channel.get("rmsBits"), f"worker channel {index} RMS bits"
        )
        _require_f32_bits(
            channel.get("peakBits"), f"worker channel {index} peak bits"
        )

    claims = _require_dict(result.get("claims"), "worker claims")
    if claims.get("compatibility") != EXPECTED_COMPATIBILITY:
        raise CandidateSuiteError("worker claims changed compatibility status")
    if claims.get("referenceParity") != "not_assessed":
        raise CandidateSuiteError("worker result makes an unsupported parity claim")

    PARENT.assert_path_free(result, "MacinMeter Candidate worker result")
    PARENT.canonical_json_bytes(result)
    return result


def _run_worker(
    *,
    worker_path: Path,
    prepared: Any,
    block_frames: int,
    timeout_seconds: float,
) -> dict[str, Any]:
    command = [
        str(worker_path.resolve(strict=True)),
        prepared.input_id,
        str(prepared.sample_rate),
        str(prepared.channels),
        str(prepared.frames),
        str(block_frames),
    ]
    try:
        completed = subprocess.run(
            command,
            input=prepared.pcm,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        raise CandidateSuiteError("Candidate worker timed out") from error
    if completed.returncode != 0:
        raise CandidateSuiteError("Candidate worker reported failure")
    if completed.stderr:
        raise CandidateSuiteError("Candidate worker wrote stderr on success")
    if not completed.stdout or len(completed.stdout) > MAX_WORKER_OUTPUT_BYTES:
        raise CandidateSuiteError("Candidate worker stdout size is invalid")
    try:
        value = json.loads(completed.stdout)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise CandidateSuiteError("Candidate worker stdout is not JSON") from error
    return _validate_worker_result(
        value,
        prepared=prepared,
        block_frames=block_frames,
    )


def _input_record(prepared: Any) -> dict[str, Any]:
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


def run_suite(
    *,
    manifest_path: Path,
    corpus_root: Path,
    worker_path: Path,
    worker_sha256: str,
    source_commit: str,
    timeout_seconds: float = 30.0,
    block_frames: int = DEFAULT_BLOCK_FRAMES,
) -> dict[str, Any]:
    if SOURCE_COMMIT_RE.fullmatch(source_commit) is None:
        raise CandidateSuiteError("source commit must be a lowercase Git object ID")
    if (
        isinstance(timeout_seconds, bool)
        or not isinstance(timeout_seconds, (int, float))
        or not math.isfinite(timeout_seconds)
        or not 0 < timeout_seconds <= 300
    ):
        raise CandidateSuiteError("suite timeout is outside (0, 300]")
    block_frames = PARENT.require_int(
        block_frames,
        "Candidate suite block frames",
        minimum=1,
        maximum=MAX_BLOCK_FRAMES,
    )
    manifest, manifest_raw, safe_cases = REFERENCE_SUITE._load_safe_cases(
        manifest_path
    )
    manifest_sha256 = PARENT.sha256_bytes(manifest_raw)
    worker = _worker_identity(worker_path, worker_sha256)

    items: list[dict[str, Any]] = []
    for case in safe_cases:
        prepared = None
        request_id = None
        try:
            prepared = REFERENCE_SUITE._prepare_manifest_snapshot_case(
                case,
                manifest_sha256=manifest_sha256,
                corpus_root=corpus_root,
            )
            request_id = _request_identity(
                prepared=prepared,
                worker_sha256=worker.sha256,
                worker_byte_length=worker.byte_length,
                source_commit=source_commit,
                block_frames=block_frames,
            )
            result: dict[str, Any] = {
                "kind": "success",
                "data": _run_worker(
                    worker_path=worker_path,
                    prepared=prepared,
                    block_frames=block_frames,
                    timeout_seconds=float(timeout_seconds),
                ),
            }
            input_record = _input_record(prepared)
        except (CandidateSuiteError, PARENT.CoreHarnessError, OSError):
            result = {
                "kind": "error",
                "stage": "input" if prepared is None else "worker",
                "code": "contract_violation",
            }
            input_record = {
                "inputId": case.get("id"),
                "manifestSha256": manifest_sha256,
            }
        items.append(
            {
                "manifestOrder": case["order"],
                "inputId": case["id"],
                "requestId": request_id,
                "input": input_record,
                "result": result,
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
        "workerSha256": worker.sha256,
        "workerByteLength": worker.byte_length,
        "sourceCommit": source_commit,
        "blockFrames": block_frames,
        "timeoutSeconds": float(timeout_seconds),
    }
    record = {
        "schemaVersion": SCHEMA_VERSION,
        "kind": RECORD_KIND,
        "suiteId": PARENT.sha256_bytes(
            PARENT.canonical_json_bytes(suite_identity)
        ),
        "corpus": {
            "id": manifest["corpusId"],
            "manifestSha256": manifest_sha256,
            "safeCaseCount": len(safe_cases),
            "safeCaseIds": [case["id"] for case in safe_cases],
        },
        "implementation": {
            "sourceCommit": source_commit,
            "workerSha256": worker.sha256,
            "workerByteLength": worker.byte_length,
            "profile": EXPECTED_PROFILE,
            "profileVersion": 1,
            "compatibility": EXPECTED_COMPATIBILITY,
        },
        "execution": {
            "timeoutSeconds": float(timeout_seconds),
            "blockFrames": block_frames,
            "processModel": "one_worker_process_per_input",
            "inputBoundary": "finite_interleaved_f64le",
            "decoderUsed": False,
        },
        "items": items,
        "summary": {
            "status": status,
            "total": len(items),
            "succeeded": succeeded,
            "failed": failed,
        },
        "claims": {
            "scope": "decoder-independent MacinMeter Candidate V1 suite",
            "compatibility": EXPECTED_COMPATIBILITY,
            "referenceParity": "not_assessed",
        },
        "limitations": [
            "No product decoder, discovery, application scheduling, CLI, Tauri, album grouping, or text renderer was exercised.",
            "Manifest WAV conversion is a reference-side fixture operation, not a product codec claim.",
        ],
    }
    PARENT.assert_path_free(record, "MacinMeter Candidate suite record")
    PARENT.canonical_json_bytes(record)
    return record


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--corpus-root", required=True, type=Path)
    parser.add_argument("--worker", required=True, type=Path)
    parser.add_argument("--worker-sha256", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    parser.add_argument("--block-frames", type=int, default=DEFAULT_BLOCK_FRAMES)
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
            source_commit=args.source_commit,
            timeout_seconds=args.timeout_seconds,
            block_frames=args.block_frames,
        )
        PARENT._write_record(record, args.output)
        return 0 if record["summary"]["failed"] == 0 else 1
    except (CandidateSuiteError, PARENT.CoreHarnessError, OSError):
        print("Candidate suite error: contract_violation", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
