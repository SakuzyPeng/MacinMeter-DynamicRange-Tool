#!/usr/bin/env python3
"""Compare schema-v3 MacinMeter report metrics with a normalized DR report.

This comparator is intentionally separate from the schema-v2 DR-only tool so
previous conformance records remain reproducible. It compares only public
fields that now have the same semantics in both reports.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from pathlib import Path
from typing import Any


WIRE_SCHEMA_VERSION = 3
EXPECTED_PROFILE = "foo_dr_meter_1_0_8_candidate_v1"
EXPECTED_COMPATIBILITY = "unverified"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
DB_TOKEN_RE = re.compile(r"^(?:-inf|[-+]?\d+\.\d{2})$")


class ComparisonError(ValueError):
    """An input does not satisfy the fixed comparison contract."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_json_bytes(path: Path) -> tuple[bytes, dict[str, Any]]:
    try:
        raw = path.read_bytes()
        value = json.loads(raw)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ComparisonError(f"cannot read JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise ComparisonError(f"{path} must contain one JSON object")
    return raw, value


def require_dict(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ComparisonError(f"{context} must be an object")
    return value


def require_list(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise ComparisonError(f"{context} must be an array")
    return value


def require_string(value: Any, context: str) -> str:
    if not isinstance(value, str):
        raise ComparisonError(f"{context} must be a string")
    return value


def require_integer(value: Any, context: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise ComparisonError(f"{context} must be an integer")
    return value


def require_sha256(value: Any, context: str) -> str:
    result = require_string(value, context)
    if SHA256_RE.fullmatch(result) is None:
        raise ComparisonError(f"{context} must be a lowercase SHA-256 digest")
    return result


def finite_number(value: Any, context: str) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise ComparisonError(f"{context} must be a number")
    result = float(value)
    if not math.isfinite(result):
        raise ComparisonError(f"{context} must be finite")
    return result


def lround(value: float) -> int:
    """C/C++ lround for finite values: halfway cases move away from zero."""

    if value >= 0.0:
        return math.floor(value + 0.5)
    return math.ceil(value - 0.5)


def report_db_token(value: Any, context: str) -> str:
    """Render one finite/null schema value like the fixed report renderer."""

    if value is None:
        return "-inf"
    number = finite_number(value, context)
    if -0.01 < number < 0.01:
        number = lround(100.0 * number) / 100.0
    return f"{number:.2f}"


def require_reference_token(value: Any, context: str) -> str:
    token = require_string(value, context)
    if DB_TOKEN_RE.fullmatch(token) is None:
        raise ComparisonError(f"{context} is not a canonical two-decimal/-inf token")
    return token


def channel_dr_token(
    channel: dict[str, Any], silent_dr_db: float, context: str
) -> str:
    outcome = require_dict(channel.get("outcome"), f"{context}.outcome")
    status = outcome.get("status")
    if status == "measured":
        measurement = require_dict(outcome.get("measurement"), f"{context}.measurement")
        value = finite_number(measurement.get("drDb"), f"{context}.measurement.drDb")
    elif status == "silent":
        value = silent_dr_db
    else:
        raise ComparisonError(
            f"{context} has non-comparable channel outcome status {status!r}"
        )
    return report_db_token(value, f"{context}.drDb")


def validate_reference(reference: dict[str, Any]) -> list[dict[str, Any]]:
    if reference.get("schemaVersion") != 1:
        raise ComparisonError("reference input must use normalization schema version 1")
    if reference.get("kind") != "foo_dr_meter_report_normalization":
        raise ComparisonError("reference input has the wrong kind")

    source = require_dict(reference.get("source"), "reference.source")
    require_sha256(source.get("rawReportSha256"), "reference.source.rawReportSha256")
    require_sha256(source.get("manifestSha256"), "reference.source.manifestSha256")
    require_string(source.get("corpusId"), "reference.source.corpusId")
    require_string(source.get("playlist"), "reference.source.playlist")
    require_dict(reference.get("header"), "reference.header")

    validation = require_dict(reference.get("validation"), "reference.validation")
    if validation.get("manifestStemsExactlyOnce") is not True:
        raise ComparisonError("reference normalization does not have an exact fixture set")
    if validation.get("manifestOrderExact") is not True:
        raise ComparisonError("reference normalization does not preserve manifest order")

    cases = require_list(reference.get("cases"), "reference.cases")
    seen_fixture_ids: set[str] = set()
    seen_stems: set[str] = set()
    total_channels = 0
    validated: list[dict[str, Any]] = []
    for index, case_value in enumerate(cases):
        context = f"reference.cases[{index}]"
        case = require_dict(case_value, context)
        fixture_id = require_string(case.get("fixtureId"), f"{context}.fixtureId")
        stem = require_string(case.get("stem"), f"{context}.stem")
        if fixture_id in seen_fixture_ids:
            raise ComparisonError(f"reference repeats fixture id {fixture_id!r}")
        if stem in seen_stems:
            raise ComparisonError(f"reference repeats stem {stem!r}")
        seen_fixture_ids.add(fixture_id)
        seen_stems.add(stem)

        channels = require_integer(case.get("channels"), f"{context}.channels")
        if channels < 1:
            raise ComparisonError(f"{context}.channels must be positive")
        channel_dr_tokens = require_list(
            case.get("channelDrDbTokens"), f"{context}.channelDrDbTokens"
        )
        channel_rms_tokens = require_list(
            case.get("channelRmsDbfsTokens"), f"{context}.channelRmsDbfsTokens"
        )
        if len(channel_dr_tokens) != channels or len(channel_rms_tokens) != channels:
            raise ComparisonError(f"{context} channel token counts do not match channels")

        validated.append(
            {
                "fixtureId": fixture_id,
                "stem": stem,
                "trackDr": require_integer(case.get("trackDr"), f"{context}.trackDr"),
                "peakDbfsToken": require_reference_token(
                    case.get("peakDbfsToken"), f"{context}.peakDbfsToken"
                ),
                "rmsDbfsToken": require_reference_token(
                    case.get("rmsDbfsToken"), f"{context}.rmsDbfsToken"
                ),
                "channelDrDbTokens": [
                    require_reference_token(token, f"{context}.channelDrDbTokens[{token_index}]")
                    for token_index, token in enumerate(channel_dr_tokens)
                ],
                "channelRmsDbfsTokens": [
                    require_reference_token(
                        token, f"{context}.channelRmsDbfsTokens[{token_index}]"
                    )
                    for token_index, token in enumerate(channel_rms_tokens)
                ],
            }
        )
        total_channels += channels

    observed_tracks = require_integer(
        validation.get("observedTrackCount"), "reference.validation.observedTrackCount"
    )
    observed_channels = require_integer(
        validation.get("observedChannelValueCount"),
        "reference.validation.observedChannelValueCount",
    )
    if observed_tracks != len(validated) or observed_channels != total_channels:
        raise ComparisonError("reference validation counts disagree with normalized cases")
    return validated


def validate_batch_summary(data: dict[str, Any], item_count: int) -> None:
    if data.get("status") != "succeeded":
        raise ComparisonError("implementation batch must have succeeded completely")
    summary = require_dict(data.get("summary"), "implementation.data.summary")
    total = require_integer(summary.get("total"), "implementation.data.summary.total")
    succeeded = require_integer(
        summary.get("succeeded"), "implementation.data.summary.succeeded"
    )
    failed = require_integer(summary.get("failed"), "implementation.data.summary.failed")
    if (total, succeeded, failed) != (item_count, item_count, 0):
        raise ComparisonError("implementation batch summary disagrees with its successful items")


def add_difference(
    differences: list[dict[str, Any]],
    case: dict[str, Any],
    field: str,
    reference_value: Any,
    implementation_value: Any,
    channel_index: int | None = None,
) -> None:
    difference = {
        "fixtureId": case["fixtureId"],
        "stem": case["stem"],
        "field": field,
        "reference": reference_value,
        "implementation": implementation_value,
    }
    if channel_index is not None:
        difference["channelIndex"] = channel_index
    differences.append(difference)


def compare(
    reference_path: Path, implementation_path: Path, binary_path: Path
) -> dict[str, Any]:
    reference_raw, reference = load_json_bytes(reference_path)
    implementation_raw, implementation = load_json_bytes(implementation_path)
    try:
        binary_raw = binary_path.read_bytes()
    except OSError as error:
        raise ComparisonError(f"cannot read implementation binary: {error}") from error

    reference_cases = validate_reference(reference)
    schema_version = implementation.get("schemaVersion")
    if schema_version != WIRE_SCHEMA_VERSION:
        raise ComparisonError(
            f"implementation WireEnvelope schemaVersion must be {WIRE_SCHEMA_VERSION}; "
            f"got {schema_version!r}"
        )
    if implementation.get("kind") != "batch":
        raise ComparisonError("implementation input must be one batch WireEnvelope")
    tool_version = require_string(
        implementation.get("toolVersion"), "implementation.toolVersion"
    )

    data = require_dict(implementation.get("data"), "implementation.data")
    implementation_items = require_list(data.get("items"), "implementation.data.items")
    if len(implementation_items) != len(reference_cases):
        raise ComparisonError(
            f"implementation has {len(implementation_items)} items; "
            f"reference has {len(reference_cases)}"
        )
    validate_batch_summary(data, len(implementation_items))

    differences: list[dict[str, Any]] = []
    matched_track_dr = 0
    matched_channel_dr = 0
    matched_peak = 0
    matched_rms = 0
    matched_channel_rms = 0
    total_channel_values = 0
    seen_stems: set[str] = set()
    profile_names: set[str] = set()
    compatibility_names: set[str] = set()

    for index, (case, item_value) in enumerate(
        zip(reference_cases, implementation_items, strict=True)
    ):
        item_context = f"implementation.data.items[{index}]"
        item = require_dict(item_value, item_context)
        display_path = require_string(item.get("displayPath"), f"{item_context}.displayPath")
        stem = Path(display_path.replace("\\", "/")).stem
        if stem in seen_stems:
            raise ComparisonError(f"implementation repeats stem {stem!r}")
        seen_stems.add(stem)
        if stem != case["stem"]:
            raise ComparisonError(
                f"implementation item {index} is stem {stem!r}; "
                f"expected {case['stem']!r}"
            )

        outcome = require_dict(item.get("outcome"), f"{item_context}.outcome")
        if outcome.get("status") != "success":
            raise ComparisonError(f"implementation item {stem!r} did not succeed")
        report = require_dict(outcome.get("report"), f"{item_context}.outcome.report")
        source = require_dict(report.get("source"), f"implementation item {stem}.source")
        source_path = require_string(
            source.get("displayPath"), f"implementation item {stem}.source.displayPath"
        )
        if Path(source_path.replace("\\", "/")).stem != stem:
            raise ComparisonError(
                f"implementation item {stem!r} and report source identify different fixtures"
            )

        analysis = require_dict(report.get("analysis"), f"implementation item {stem}.analysis")
        algorithm = require_dict(
            analysis.get("algorithm"), f"implementation item {stem}.algorithm"
        )
        profile = require_string(
            algorithm.get("profile"), f"implementation item {stem}.algorithm.profile"
        )
        compatibility = require_string(
            algorithm.get("compatibility"),
            f"implementation item {stem}.algorithm.compatibility",
        )
        profile_names.add(profile)
        compatibility_names.add(compatibility)
        if profile != EXPECTED_PROFILE:
            raise ComparisonError(
                f"implementation item {stem!r} uses unexpected profile {profile!r}"
            )
        if compatibility != EXPECTED_COMPATIBILITY:
            raise ComparisonError(
                f"implementation item {stem!r} uses unexpected compatibility {compatibility!r}"
            )

        aggregates = require_dict(
            analysis.get("aggregates"), f"implementation item {stem}.aggregates"
        )
        track = require_dict(aggregates.get("track"), f"implementation item {stem}.track")
        implementation_track_dr = track.get("roundedDr")
        if implementation_track_dr is not None:
            implementation_track_dr = require_integer(
                implementation_track_dr, f"implementation item {stem}.track.roundedDr"
            )
        if implementation_track_dr == case["trackDr"]:
            matched_track_dr += 1
        else:
            add_difference(
                differences,
                case,
                "trackDr",
                case["trackDr"],
                implementation_track_dr,
            )

        parameters = require_dict(
            algorithm.get("parameters"), f"implementation item {stem}.parameters"
        )
        silent_dr_db = finite_number(
            parameters.get("silentChannelDrDb"),
            f"implementation item {stem}.parameters.silentChannelDrDb",
        )
        implementation_channels = require_list(
            analysis.get("channels"), f"implementation item {stem}.channels"
        )
        reference_channel_dr = case["channelDrDbTokens"]
        reference_channel_rms = case["channelRmsDbfsTokens"]
        if len(implementation_channels) != len(reference_channel_dr):
            raise ComparisonError(
                f"implementation item {stem!r} has {len(implementation_channels)} channels; "
                f"reference has {len(reference_channel_dr)}"
            )

        for channel_index, channel_value in enumerate(implementation_channels):
            channel_context = (
                f"implementation item {stem}.channels[{channel_index}]"
            )
            channel = require_dict(channel_value, channel_context)
            if channel.get("channelIndex") != channel_index:
                raise ComparisonError(
                    f"implementation item {stem!r} has non-canonical channel order"
                )

            implementation_channel_dr = channel_dr_token(
                channel, silent_dr_db, channel_context
            )
            total_channel_values += 1
            if implementation_channel_dr == reference_channel_dr[channel_index]:
                matched_channel_dr += 1
            else:
                add_difference(
                    differences,
                    case,
                    "channelDrDbToken",
                    reference_channel_dr[channel_index],
                    implementation_channel_dr,
                    channel_index,
                )

            channel_report = require_dict(
                channel.get("report"), f"{channel_context}.report"
            )
            implementation_channel_rms = report_db_token(
                channel_report.get("overallRmsDbfs"),
                f"{channel_context}.report.overallRmsDbfs",
            )
            if implementation_channel_rms == reference_channel_rms[channel_index]:
                matched_channel_rms += 1
            else:
                add_difference(
                    differences,
                    case,
                    "channelRmsDbfsToken",
                    reference_channel_rms[channel_index],
                    implementation_channel_rms,
                    channel_index,
                )

        track_report = require_dict(
            analysis.get("report"), f"implementation item {stem}.reportMetrics"
        )
        implementation_peak = report_db_token(
            track_report.get("primaryPeakDbfs"),
            f"implementation item {stem}.reportMetrics.primaryPeakDbfs",
        )
        if implementation_peak == case["peakDbfsToken"]:
            matched_peak += 1
        else:
            add_difference(
                differences,
                case,
                "peakDbfsToken",
                case["peakDbfsToken"],
                implementation_peak,
            )

        implementation_rms = report_db_token(
            track_report.get("overallRmsDbfs"),
            f"implementation item {stem}.reportMetrics.overallRmsDbfs",
        )
        if implementation_rms == case["rmsDbfsToken"]:
            matched_rms += 1
        else:
            add_difference(
                differences,
                case,
                "rmsDbfsToken",
                case["rmsDbfsToken"],
                implementation_rms,
            )

    if len(profile_names) != 1 or len(compatibility_names) != 1:
        raise ComparisonError("implementation batch mixes algorithm identities")

    source = require_dict(reference["source"], "reference.source")
    return {
        "schemaVersion": 1,
        "kind": "reference_report_metrics_conformance_comparison",
        "reference": {
            "normalizationSha256": sha256_bytes(reference_raw),
            "rawReportSha256": source["rawReportSha256"],
            "manifestSha256": source["manifestSha256"],
            "corpusId": source["corpusId"],
            "playlist": source["playlist"],
            "targetVersions": reference["header"],
        },
        "implementation": {
            "wireOutputSha256": sha256_bytes(implementation_raw),
            "binarySha256": sha256_bytes(binary_raw),
            "wireSchemaVersion": schema_version,
            "toolVersion": tool_version,
            "profile": next(iter(profile_names)),
            "compatibility": next(iter(compatibility_names)),
        },
        "policy": {
            "trackDr": "exact integer token",
            "channelDrDb": "reference two-decimal token with near-zero centi-dB correction",
            "trackPrimaryPeakDbfs": "reference two-decimal/-inf token",
            "trackOverallRmsDbfs": "reference two-decimal/-inf token",
            "channelOverallRmsDbfs": "reference two-decimal/-inf token",
            "numericToleranceDb": 0.0,
            "fixtureSetAndOrder": "exact unique fixture stems in reference order",
        },
        "summary": {
            "status": "match" if not differences else "systematic_difference",
            "trackDrMatched": matched_track_dr,
            "trackDrTotal": len(reference_cases),
            "channelDrMatched": matched_channel_dr,
            "channelDrTotal": total_channel_values,
            "overallPeakMatched": matched_peak,
            "overallPeakTotal": len(reference_cases),
            "overallRmsMatched": matched_rms,
            "overallRmsTotal": len(reference_cases),
            "channelRmsMatched": matched_channel_rms,
            "channelRmsTotal": total_channel_values,
            "differenceCount": len(differences),
            "fixtureSetExact": True,
            "implementationOrderMatchesReference": True,
        },
        "differences": differences,
        "notCompared": [
            {
                "fieldClass": "internal intermediate state",
                "reason": "not observable in the exported reference report",
            },
            {
                "referenceField": "durationToken",
                "reason": "decoded duration is structured, but reference duration text rendering is outside this comparison",
            },
            {
                "referenceFields": ["footer", "album focused aggregation"],
                "reason": "footer metadata and album aggregation have separate evidence and contracts",
            },
        ],
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--implementation-output", required=True, type=Path)
    parser.add_argument("--implementation-binary", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        result = compare(
            args.reference, args.implementation_output, args.implementation_binary
        )
    except (OSError, ComparisonError) as error:
        raise SystemExit(f"error: {error}") from error
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output is None:
        print(rendered, end="")
    else:
        args.output.write_text(rendered, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
