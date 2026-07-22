#!/usr/bin/env python3
"""Compare a direct-PCM MacinMeter suite with the fixed x64 evidence.

Only public final fields are compared. The tool does not require MacinMeter and
the reference DLL to expose the same intermediate state or data structures.
Raw public binary32 fields and rendered report tokens use exact equality.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import struct
import sys
from pathlib import Path
from typing import Any


def _load_sibling(module_name: str, filename: str) -> Any:
    path = Path(__file__).with_name(filename)
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


CORE = _load_sibling(
    "_m4_candidate_core_comparator",
    "compare_foo_dr_meter_core_suite_to_report.py",
)
REPORT = _load_sibling(
    "_m4_candidate_report_comparator",
    "compare_macinmeter_report_metrics_to_foo_dr_meter.py",
)


SCHEMA_VERSION = 1
CANDIDATE_SUITE_KIND = "macinmeter_candidate_v1_direct_pcm_suite"
COMPARISON_KIND = "macinmeter_candidate_v1_x64_numeric_comparison"
WORKER_RESULT_KIND = "macinmeter_candidate_v1_conformance_result"
SOURCE_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
F32_BITS_RE = re.compile(r"^[0-9a-f]{8}$")


class CandidateComparisonError(ValueError):
    """An input does not satisfy the bounded M4 comparison contract."""


def _require_object(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise CandidateComparisonError(f"{context} must be an object")
    return value


def _require_array(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise CandidateComparisonError(f"{context} must be an array")
    return value


def _require_exact_keys(
    value: dict[str, Any],
    expected: set[str],
    context: str,
) -> None:
    if set(value) != expected:
        raise CandidateComparisonError(f"{context} has unexpected fields")


def _require_sha256(value: Any, context: str) -> str:
    try:
        return CORE.require_sha256(value, context)
    except CORE.ComparisonError as error:
        raise CandidateComparisonError(str(error)) from error


def _require_f32_bits(
    value: Any,
    context: str,
    *,
    optional: bool = False,
) -> str | None:
    if optional and value is None:
        return None
    if not isinstance(value, str) or F32_BITS_RE.fullmatch(value) is None:
        raise CandidateComparisonError(
            f"{context} must be lowercase binary32 bits"
        )
    return value


def _f32_bits(value: Any, context: str) -> str:
    number = REPORT.finite_number(value, context)
    try:
        raw = struct.pack("<f", number)
    except (OverflowError, struct.error) as error:
        raise CandidateComparisonError(
            f"{context} is not representable as finite binary32"
        ) from error
    return f"{struct.unpack('<I', raw)[0]:08x}"


def _load_json(path: Path, context: str) -> tuple[bytes, dict[str, Any]]:
    try:
        raw, value = CORE.load_json_bytes(path)
    except CORE.ComparisonError as error:
        raise CandidateComparisonError(f"{context}: {error}") from error
    return raw, value


def _validate_candidate_suite(
    suite: dict[str, Any],
    reference_suite: dict[str, Any],
) -> list[dict[str, Any]]:
    _require_exact_keys(
        suite,
        {
            "schemaVersion",
            "kind",
            "suiteId",
            "corpus",
            "implementation",
            "execution",
            "items",
            "summary",
            "claims",
            "limitations",
        },
        "candidateSuite",
    )
    if (
        suite.get("schemaVersion") != SCHEMA_VERSION
        or suite.get("kind") != CANDIDATE_SUITE_KIND
    ):
        raise CandidateComparisonError(
            "candidateSuite has the wrong schema or kind"
        )
    _require_sha256(suite.get("suiteId"), "candidateSuite.suiteId")
    try:
        CORE.assert_path_free(suite, "candidateSuite")
    except CORE.ComparisonError as error:
        raise CandidateComparisonError(str(error)) from error

    corpus = _require_object(suite.get("corpus"), "candidateSuite.corpus")
    reference_corpus = _require_object(
        reference_suite.get("corpus"), "referenceCoreSuite.corpus"
    )
    if corpus != reference_corpus:
        raise CandidateComparisonError(
            "candidate and reference suites use different fixed corpora"
        )

    implementation = _require_object(
        suite.get("implementation"), "candidateSuite.implementation"
    )
    _require_exact_keys(
        implementation,
        {
            "sourceCommit",
            "workerSha256",
            "workerByteLength",
        },
        "candidateSuite.implementation",
    )
    source_commit = implementation.get("sourceCommit")
    if (
        not isinstance(source_commit, str)
        or SOURCE_COMMIT_RE.fullmatch(source_commit) is None
    ):
        raise CandidateComparisonError(
            "candidateSuite source commit is not a lowercase Git object ID"
        )
    worker_sha256 = _require_sha256(
        implementation.get("workerSha256"),
        "candidateSuite.implementation.workerSha256",
    )
    worker_byte_length = CORE.require_int(
        implementation.get("workerByteLength"),
        "candidateSuite.implementation.workerByteLength",
        minimum=1,
    )
    execution = _require_object(
        suite.get("execution"), "candidateSuite.execution"
    )
    _require_exact_keys(
        execution,
        {
            "timeoutSeconds",
            "blockFrames",
            "processModel",
            "inputBoundary",
            "decoderUsed",
        },
        "candidateSuite.execution",
    )
    block_frames = CORE.require_int(
        execution.get("blockFrames"),
        "candidateSuite.execution.blockFrames",
        minimum=1,
    )
    timeout = execution.get("timeoutSeconds")
    if (
        not isinstance(timeout, (int, float))
        or isinstance(timeout, bool)
        or float(timeout) <= 0.0
    ):
        raise CandidateComparisonError(
            "candidateSuite execution timeout is invalid"
        )
    if (
        execution.get("processModel") != "one_worker_process_per_input"
        or execution.get("inputBoundary") != "finite_interleaved_f64le"
        or execution.get("decoderUsed") is not False
    ):
        raise CandidateComparisonError(
            "candidateSuite is not decoder-independent and process-isolated"
        )

    expected_identity = {
        "schemaVersion": SCHEMA_VERSION,
        "manifestSha256": corpus["manifestSha256"],
        "corpusId": corpus["id"],
        "safeCaseIds": corpus["safeCaseIds"],
        "workerSha256": worker_sha256,
        "workerByteLength": worker_byte_length,
        "sourceCommit": source_commit,
        "blockFrames": block_frames,
        "timeoutSeconds": float(timeout),
    }
    expected_suite_id = CORE.sha256_bytes(
        CORE.canonical_json_bytes(expected_identity)
    )
    if suite.get("suiteId") != expected_suite_id:
        raise CandidateComparisonError(
            "candidateSuite ID does not bind its declared execution"
        )

    summary = _require_object(suite.get("summary"), "candidateSuite.summary")
    expected_count = CORE.EXPECTED_TRACK_COUNT
    if summary != {
        "status": "success",
        "total": expected_count,
        "succeeded": expected_count,
        "failed": 0,
    }:
        raise CandidateComparisonError(
            "candidateSuite is not one complete successful run"
        )
    claims = _require_object(suite.get("claims"), "candidateSuite.claims")
    if claims.get("referenceParity") != "not_assessed":
        raise CandidateComparisonError(
            "candidateSuite makes an unsupported reference-parity claim"
        )
    limitations = _require_array(
        suite.get("limitations"), "candidateSuite.limitations"
    )
    if not limitations or not all(isinstance(item, str) for item in limitations):
        raise CandidateComparisonError(
            "candidateSuite limitations must be non-empty strings"
        )

    candidate_items = _require_array(
        suite.get("items"), "candidateSuite.items"
    )
    reference_items = _require_array(
        reference_suite.get("items"), "referenceCoreSuite.items"
    )
    if len(candidate_items) != expected_count:
        raise CandidateComparisonError(
            "candidateSuite does not contain exactly 39 items"
        )

    validated: list[dict[str, Any]] = []
    for position, (candidate_raw, reference_raw, expected_id) in enumerate(
        zip(
            candidate_items,
            reference_items,
            CORE.EXPECTED_CASE_IDS,
            strict=True,
        ),
        1,
    ):
        context = f"candidateSuite.items[{position - 1}]"
        item = _require_object(candidate_raw, context)
        _require_exact_keys(
            item,
            {
                "manifestOrder",
                "inputId",
                "requestId",
                "input",
                "result",
            },
            context,
        )
        if (
            item.get("manifestOrder") != position
            or item.get("inputId") != expected_id
        ):
            raise CandidateComparisonError(
                f"{context} is out of fixed manifest order"
            )
        input_value = _require_object(item.get("input"), f"{context}.input")
        reference_input = _require_object(
            _require_object(reference_raw, "reference item").get("input"),
            "reference item input",
        )
        if input_value != reference_input:
            raise CandidateComparisonError(
                f"{context}.input differs from the reference-side PCM identity"
            )
        request_semantic = {
            "schemaVersion": SCHEMA_VERSION,
            "inputId": expected_id,
            "pcmSha256": input_value["pcmSha256"],
            "pcmByteLength": input_value["pcmByteLength"],
            "sampleRateHz": input_value["sampleRateHz"],
            "channels": input_value["channels"],
            "frames": input_value["frames"],
            "workerSha256": worker_sha256,
            "workerByteLength": worker_byte_length,
            "sourceCommit": source_commit,
            "blockFrames": block_frames,
        }
        if item.get("requestId") != CORE.sha256_bytes(
            CORE.canonical_json_bytes(request_semantic)
        ):
            raise CandidateComparisonError(
                f"{context}.requestId does not bind its input and worker"
            )

        result_wrapper = _require_object(
            item.get("result"), f"{context}.result"
        )
        _require_exact_keys(
            result_wrapper, {"kind", "data"}, f"{context}.result"
        )
        if result_wrapper.get("kind") != "success":
            raise CandidateComparisonError(f"{context} is not successful")
        result = _require_object(
            result_wrapper.get("data"), f"{context}.result.data"
        )
        _require_exact_keys(
            result,
            {
                "schemaVersion",
                "kind",
                "inputId",
                "input",
                "algorithm",
                "coreBits",
                "analysis",
                "claims",
            },
            f"{context}.result.data",
        )
        if (
            result.get("schemaVersion") != SCHEMA_VERSION
            or result.get("kind") != WORKER_RESULT_KIND
            or result.get("inputId") != expected_id
        ):
            raise CandidateComparisonError(
                f"{context} worker result identity differs"
            )
        worker_input = _require_object(
            result.get("input"), f"{context}.workerInput"
        )
        if worker_input != {
            "sampleRateHz": input_value["sampleRateHz"],
            "channels": input_value["channels"],
            "frames": input_value["frames"],
            "blockFrames": block_frames,
            "sampleEncoding": "f64le-interleaved",
        }:
            raise CandidateComparisonError(
                f"{context} worker input geometry differs"
            )

        algorithm = _require_object(
            result.get("algorithm"), f"{context}.algorithm"
        )
        if (
            "profile" in algorithm
            or "profileVersion" in algorithm
            or "compatibility" in algorithm
        ):
            raise CandidateComparisonError(
                f"{context} worker exposes a report status or profile"
            )
        _require_object(
            algorithm.get("parameters"), f"{context}.algorithm.parameters"
        )
        claims = _require_object(result.get("claims"), f"{context}.claims")
        if claims.get("referenceParity") != "not_assessed":
            raise CandidateComparisonError(
                f"{context} worker claims exceed Candidate scope"
            )

        analysis = _require_object(
            result.get("analysis"), f"{context}.analysis"
        )
        if (
            analysis.get("algorithm") != algorithm
            or analysis.get("framesSeen") != input_value["frames"]
        ):
            raise CandidateComparisonError(
                f"{context} analysis identity or frame count differs"
            )
        stream = _require_object(
            analysis.get("stream"), f"{context}.analysis.stream"
        )
        if (
            stream.get("sampleRate") != input_value["sampleRateHz"]
            or stream.get("channels") != input_value["channels"]
        ):
            raise CandidateComparisonError(
                f"{context} analysis stream geometry differs"
            )
        channels = _require_array(
            analysis.get("channels"), f"{context}.analysis.channels"
        )
        if len(channels) != input_value["channels"]:
            raise CandidateComparisonError(
                f"{context} analysis channel geometry differs"
            )
        for channel_index, channel_raw in enumerate(channels):
            channel = _require_object(
                channel_raw,
                f"{context}.analysis.channels[{channel_index}]",
            )
            if channel.get("channelIndex") != channel_index:
                raise CandidateComparisonError(
                    f"{context} analysis channel indices are not contiguous"
                )

        core_bits = _require_object(
            result.get("coreBits"), f"{context}.coreBits"
        )
        direct_track_bits = _require_f32_bits(
            core_bits.get("trackDrBits"),
            f"{context}.coreBits.trackDrBits",
            optional=True,
        )
        aggregates = _require_object(
            analysis.get("aggregates"), f"{context}.analysis.aggregates"
        )
        track = _require_object(
            aggregates.get("track"), f"{context}.analysis.aggregates.track"
        )
        track_dr = track.get("drDb")
        projected_track_bits = (
            None
            if track_dr is None
            else _f32_bits(track_dr, f"{context}.analysis.track.drDb")
        )
        if direct_track_bits != projected_track_bits:
            raise CandidateComparisonError(
                f"{context} track bit projection differs from public analysis"
            )

        channel_bits = _require_array(
            core_bits.get("channelResults"),
            f"{context}.coreBits.channelResults",
        )
        if len(channel_bits) != len(channels):
            raise CandidateComparisonError(
                f"{context} core-bit channel geometry differs"
            )
        validated_channel_bits: list[dict[str, str | None]] = []
        for channel_index, (bits_raw, channel_raw) in enumerate(
            zip(channel_bits, channels, strict=True)
        ):
            bits = _require_object(
                bits_raw, f"{context}.coreBits.channelResults[{channel_index}]"
            )
            channel = _require_object(
                channel_raw,
                f"{context}.analysis.channels[{channel_index}]",
            )
            if bits.get("index") != channel_index:
                raise CandidateComparisonError(
                    f"{context} core-bit channel indices are not contiguous"
                )
            report = _require_object(
                channel.get("report"),
                f"{context}.analysis.channels[{channel_index}].report",
            )
            outcome = _require_object(
                channel.get("outcome"),
                f"{context}.analysis.channels[{channel_index}].outcome",
            )
            outcome_status = outcome.get("status")
            if outcome_status == "measured":
                measurement = _require_object(
                    outcome.get("measurement"),
                    f"{context}.analysis.channels[{channel_index}].measurement",
                )
                projected_dr = _f32_bits(
                    measurement.get("drDb"),
                    f"{context}.analysis.channels[{channel_index}].drDb",
                )
            elif outcome_status == "silent":
                projected_dr = "00000000"
            elif outcome_status == "insufficient_data":
                projected_dr = None
            else:
                raise CandidateComparisonError(
                    f"{context} channel outcome is unsupported"
                )
            direct = {
                "drBits": _require_f32_bits(
                    bits.get("drBits"),
                    f"{context}.channel[{channel_index}].drBits",
                    optional=True,
                ),
                "rmsBits": _require_f32_bits(
                    bits.get("rmsBits"),
                    f"{context}.channel[{channel_index}].rmsBits",
                ),
                "peakBits": _require_f32_bits(
                    bits.get("peakBits"),
                    f"{context}.channel[{channel_index}].peakBits",
                ),
            }
            projected = {
                "drBits": projected_dr,
                "rmsBits": _f32_bits(
                    report.get("overallRmsLinear"),
                    f"{context}.channel[{channel_index}].overallRmsLinear",
                ),
                "peakBits": _f32_bits(
                    report.get("primaryPeakLinear"),
                    f"{context}.channel[{channel_index}].primaryPeakLinear",
                ),
            }
            if direct != projected:
                raise CandidateComparisonError(
                    f"{context} channel bit projection differs from public analysis"
                )
            validated_channel_bits.append(direct)

        validated.append(
            {
                "fixtureId": expected_id,
                "manifestOrder": position,
                "sampleRateHz": input_value["sampleRateHz"],
                "channels": input_value["channels"],
                "frames": input_value["frames"],
                "trackDrBits": direct_track_bits,
                "channelBits": validated_channel_bits,
                "analysis": analysis,
            }
        )

    return validated


def _add_difference(
    differences: list[dict[str, Any]],
    *,
    fixture_id: str,
    field: str,
    reference: Any,
    candidate: Any,
    channel_index: int | None = None,
) -> None:
    difference = {
        "fixtureId": fixture_id,
        "field": field,
        "reference": reference,
        "candidate": candidate,
    }
    if channel_index is not None:
        difference["channelIndex"] = channel_index
    differences.append(difference)


def compare(
    candidate_suite_path: Path,
    reference_core_suite_path: Path,
    normalized_report_path: Path,
) -> dict[str, Any]:
    candidate_raw, candidate_suite = _load_json(
        candidate_suite_path, "candidate suite"
    )
    reference_raw, reference_suite = _load_json(
        reference_core_suite_path, "reference core suite"
    )
    report_raw, normalized_report = _load_json(
        normalized_report_path, "normalized report"
    )
    try:
        reference_items = CORE.validate_suite(reference_suite)
        report_cases = CORE.validate_report(normalized_report)
    except CORE.ComparisonError as error:
        raise CandidateComparisonError(str(error)) from error
    candidate_items = _validate_candidate_suite(
        candidate_suite, reference_suite
    )

    counters = {
        "trackDrBitsMatched": 0,
        "channelDrBitsMatched": 0,
        "channelRmsBitsMatched": 0,
        "channelPeakBitsMatched": 0,
        "trackDrTokenMatched": 0,
        "channelDrTokenMatched": 0,
        "overallPeakTokenMatched": 0,
        "overallRmsTokenMatched": 0,
        "channelRmsTokenMatched": 0,
        "durationTokenMatched": 0,
    }
    differences: list[dict[str, Any]] = []
    for candidate, reference, report in zip(
        candidate_items, reference_items, report_cases, strict=True
    ):
        if not (
            candidate["fixtureId"]
            == reference["fixtureId"]
            == report["fixtureId"]
        ):
            raise CandidateComparisonError(
                "candidate, core, and report fixture order differs"
            )
        fixture_id = candidate["fixtureId"]
        reference_item = reference_suite["items"][
            candidate["manifestOrder"] - 1
        ]
        report_record = normalized_report["cases"][
            candidate["manifestOrder"] - 1
        ]
        reference_bits = reference_item["result"]["data"]

        if candidate["trackDrBits"] == reference_bits["trackDrBits"]:
            counters["trackDrBitsMatched"] += 1
        else:
            _add_difference(
                differences,
                fixture_id=fixture_id,
                field="trackDrBits",
                reference=reference_bits["trackDrBits"],
                candidate=candidate["trackDrBits"],
            )

        for channel_index, (direct, core_bits) in enumerate(
            zip(
                candidate["channelBits"],
                reference_bits["channelResults"],
                strict=True,
            )
        ):
            for field, counter in (
                ("drBits", "channelDrBitsMatched"),
                ("rmsBits", "channelRmsBitsMatched"),
                ("peakBits", "channelPeakBitsMatched"),
            ):
                if direct[field] == core_bits[field]:
                    counters[counter] += 1
                else:
                    _add_difference(
                        differences,
                        fixture_id=fixture_id,
                        field=field,
                        reference=core_bits[field],
                        candidate=direct[field],
                        channel_index=channel_index,
                    )

        analysis = candidate["analysis"]
        aggregate = analysis["aggregates"]["track"]
        candidate_track_token = aggregate["roundedDr"]
        if candidate_track_token == report["trackDr"]:
            counters["trackDrTokenMatched"] += 1
        else:
            _add_difference(
                differences,
                fixture_id=fixture_id,
                field="trackDrToken",
                reference=report["trackDr"],
                candidate=candidate_track_token,
            )

        candidate_channels = analysis["channels"]
        for channel_index, (channel, reference_dr, reference_rms) in enumerate(
            zip(
                candidate_channels,
                report["channelDrDbTokens"],
                report["channelRmsDbfsTokens"],
                strict=True,
            )
        ):
            candidate_dr = REPORT.channel_dr_token(
                channel,
                0.0,
                f"{fixture_id}.channel[{channel_index}]",
            )
            if candidate_dr == reference_dr:
                counters["channelDrTokenMatched"] += 1
            else:
                _add_difference(
                    differences,
                    fixture_id=fixture_id,
                    field="channelDrDbToken",
                    reference=reference_dr,
                    candidate=candidate_dr,
                    channel_index=channel_index,
                )
            candidate_rms = REPORT.report_db_token(
                channel["report"]["overallRmsDbfs"],
                f"{fixture_id}.channel[{channel_index}].overallRmsDbfs",
            )
            if candidate_rms == reference_rms:
                counters["channelRmsTokenMatched"] += 1
            else:
                _add_difference(
                    differences,
                    fixture_id=fixture_id,
                    field="channelRmsDbfsToken",
                    reference=reference_rms,
                    candidate=candidate_rms,
                    channel_index=channel_index,
                )

        track_report = analysis["report"]
        candidate_peak = REPORT.report_db_token(
            track_report["primaryPeakDbfs"],
            f"{fixture_id}.primaryPeakDbfs",
        )
        if candidate_peak == report["peakDbfsToken"]:
            counters["overallPeakTokenMatched"] += 1
        else:
            _add_difference(
                differences,
                fixture_id=fixture_id,
                field="overallPeakDbfsToken",
                reference=report["peakDbfsToken"],
                candidate=candidate_peak,
            )
        candidate_rms = REPORT.report_db_token(
            track_report["overallRmsDbfs"],
            f"{fixture_id}.overallRmsDbfs",
        )
        if candidate_rms == report_record["rmsDbfsToken"]:
            counters["overallRmsTokenMatched"] += 1
        else:
            _add_difference(
                differences,
                fixture_id=fixture_id,
                field="overallRmsDbfsToken",
                reference=report_record["rmsDbfsToken"],
                candidate=candidate_rms,
            )
        candidate_duration = REPORT.duration_token(
            track_report["duration"], f"{fixture_id}.duration"
        )
        reference_duration = report_record["durationToken"]
        if candidate_duration == reference_duration:
            counters["durationTokenMatched"] += 1
        else:
            _add_difference(
                differences,
                fixture_id=fixture_id,
                field="durationToken",
                reference=reference_duration,
                candidate=candidate_duration,
            )

    track_total = CORE.EXPECTED_TRACK_COUNT
    channel_total = CORE.EXPECTED_CHANNEL_COUNT
    summary = {
        "status": "match" if not differences else "systematic_difference",
        **counters,
        "trackDrBitsTotal": track_total,
        "channelDrBitsTotal": channel_total,
        "channelRmsBitsTotal": channel_total,
        "channelPeakBitsTotal": channel_total,
        "trackDrTokenTotal": track_total,
        "channelDrTokenTotal": channel_total,
        "overallPeakTokenTotal": track_total,
        "overallRmsTokenTotal": track_total,
        "channelRmsTokenTotal": channel_total,
        "durationTokenTotal": track_total,
        "differenceCount": len(differences),
        "fixtureSetExact": True,
        "manifestOrderExact": True,
    }
    implementation = candidate_suite["implementation"]
    return {
        "schemaVersion": SCHEMA_VERSION,
        "kind": COMPARISON_KIND,
        "target": {
            "sha256": reference_suite["target"]["sha256"],
            "runtimeProfile": reference_suite["target"]["runtimeProfile"],
        },
        "corpus": candidate_suite["corpus"],
        "evidence": {
            "referenceCoreSuiteSha256": CORE.sha256_bytes(reference_raw),
            "normalizedReportSha256": CORE.sha256_bytes(report_raw),
        },
        "implementation": {
            "candidateSuiteSha256": CORE.sha256_bytes(candidate_raw),
            **implementation,
        },
        "policy": {
            "rawPublicBinary32": "exact lowercase IEEE-754 bits",
            "reportTokens": "exact fixed renderer tokens",
            "numericToleranceDb": 0.0,
            "intermediateStateCompared": False,
            "decoderUsed": False,
        },
        "summary": summary,
        "differences": differences,
        "claims": {
            "scope": "bounded foo_dr_meter 1.0.8 x64 numeric fields",
        },
        "limitations": [
            "This comparison does not establish decoder, host lifecycle, metadata, grouping, optional weighting, or text parity.",
            "Exact results apply to the fixed target, fixed corpus, declared public fields, and recorded implementation identity.",
        ],
    }


def _write_record(value: dict[str, Any], output: Path | None) -> None:
    raw = CORE.canonical_json_bytes(value)
    if output is None:
        sys.stdout.buffer.write(raw)
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(raw)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-suite", required=True, type=Path)
    parser.add_argument("--reference-core-suite", required=True, type=Path)
    parser.add_argument("--normalized-report", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        record = compare(
            args.candidate_suite,
            args.reference_core_suite,
            args.normalized_report,
        )
        _write_record(record, args.output)
        return 0 if record["summary"]["differenceCount"] == 0 else 1
    except (CandidateComparisonError, OSError):
        print("Candidate comparison error: contract_violation", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
