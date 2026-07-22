#!/usr/bin/env python3
"""Compare a MacinMeter WireEnvelope with a normalized reference report.

The comparison is intentionally limited to fields with the same public
semantics: integer track DR and two-decimal per-channel DR tokens. Reference
overall peak/RMS fields are not compared with differently defined internal
MacinMeter measurements.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
from typing import Any

LEGACY_PROFILE = "foo_dr_meter_1_0_8_candidate_v1"


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


def finite_number(value: Any, context: str) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise ComparisonError(f"{context} must be a number")
    result = float(value)
    if not math.isfinite(result):
        raise ComparisonError(f"{context} must be finite")
    return result


def implementation_channel_token(
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
    return f"{value:.2f}"


def compare(
    reference_path: Path, implementation_path: Path, binary_path: Path
) -> dict[str, Any]:
    reference_raw, reference = load_json_bytes(reference_path)
    implementation_raw, implementation = load_json_bytes(implementation_path)
    try:
        binary_raw = binary_path.read_bytes()
    except OSError as error:
        raise ComparisonError(f"cannot read implementation binary: {error}") from error

    if reference.get("kind") != "foo_dr_meter_report_normalization":
        raise ComparisonError("reference input has the wrong kind")
    if implementation.get("kind") != "batch":
        raise ComparisonError("implementation input must be one batch WireEnvelope")

    reference_cases = reference.get("cases")
    data = require_dict(implementation.get("data"), "implementation.data")
    implementation_items = data.get("items")
    if not isinstance(reference_cases, list) or not isinstance(implementation_items, list):
        raise ComparisonError("reference cases and implementation items must be arrays")

    implementation_by_stem: dict[str, dict[str, Any]] = {}
    implementation_order: list[str] = []
    algorithm_identities: set[str] = set()
    for index, item_value in enumerate(implementation_items):
        item = require_dict(item_value, f"implementation.items[{index}]")
        display_path = item.get("displayPath")
        if not isinstance(display_path, str):
            raise ComparisonError(f"implementation.items[{index}].displayPath is invalid")
        stem = Path(display_path.replace("\\", "/")).stem
        if stem in implementation_by_stem:
            raise ComparisonError(f"implementation repeats stem {stem!r}")
        implementation_by_stem[stem] = item
        implementation_order.append(stem)

        outcome = require_dict(item.get("outcome"), f"implementation item {stem}.outcome")
        if outcome.get("status") != "success":
            raise ComparisonError(f"implementation item {stem!r} did not succeed")
        report = require_dict(outcome.get("report"), f"implementation item {stem}.report")
        analysis = require_dict(report.get("analysis"), f"implementation item {stem}.analysis")
        algorithm = require_dict(
            analysis.get("algorithm"), f"implementation item {stem}.algorithm"
        )
        legacy_profile = algorithm.get("profile")
        legacy_profile_version = algorithm.get("profileVersion")
        if (legacy_profile, legacy_profile_version) not in {
            (None, None),
            (LEGACY_PROFILE, 1),
        }:
            raise ComparisonError(
                f"implementation item {stem!r} has an unknown legacy analysis identity"
            )
        if "compatibility" in algorithm:
            raise ComparisonError(
                f"implementation item {stem!r} attaches compatibility to its report"
            )
        require_dict(
            algorithm.get("parameters"),
            f"implementation item {stem}.algorithm.parameters",
        )
        current_algorithm = {
            key: value
            for key, value in algorithm.items()
            if key not in {"profile", "profileVersion"}
        }
        algorithm_identities.add(
            json.dumps(current_algorithm, sort_keys=True, separators=(",", ":"))
        )

    expected_stems: list[str] = []
    differences: list[dict[str, Any]] = []
    matched_track_dr = 0
    matched_channel_dr = 0
    total_channel_dr = 0

    for index, case_value in enumerate(reference_cases):
        case = require_dict(case_value, f"reference.cases[{index}]")
        fixture_id = case.get("fixtureId")
        stem = case.get("stem")
        reference_track_dr = case.get("trackDr")
        reference_channel_tokens = case.get("channelDrDbTokens")
        if (
            not isinstance(fixture_id, str)
            or not isinstance(stem, str)
            or not isinstance(reference_track_dr, int)
            or not isinstance(reference_channel_tokens, list)
            or not all(isinstance(token, str) for token in reference_channel_tokens)
        ):
            raise ComparisonError(f"reference case {index} is invalid")
        expected_stems.append(stem)

        item = implementation_by_stem.get(stem)
        if item is None:
            raise ComparisonError(f"implementation is missing reference stem {stem!r}")
        outcome = require_dict(item["outcome"], f"implementation item {stem}.outcome")
        report = require_dict(outcome["report"], f"implementation item {stem}.report")
        analysis = require_dict(report.get("analysis"), f"implementation item {stem}.analysis")
        aggregates = require_dict(
            analysis.get("aggregates"), f"implementation item {stem}.aggregates"
        )
        track = require_dict(aggregates.get("track"), f"implementation item {stem}.track")
        implementation_track_dr = track.get("roundedDr")
        if implementation_track_dr == reference_track_dr:
            matched_track_dr += 1
        else:
            differences.append(
                {
                    "fixtureId": fixture_id,
                    "stem": stem,
                    "field": "trackDr",
                    "reference": reference_track_dr,
                    "implementation": implementation_track_dr,
                }
            )

        algorithm = require_dict(
            analysis.get("algorithm"), f"implementation item {stem}.algorithm"
        )
        parameters = require_dict(
            algorithm.get("parameters"), f"implementation item {stem}.parameters"
        )
        silent_dr_db = finite_number(
            parameters.get("silentChannelDrDb"),
            f"implementation item {stem}.silentChannelDrDb",
        )
        implementation_channels = analysis.get("channels")
        if not isinstance(implementation_channels, list):
            raise ComparisonError(f"implementation item {stem}.channels must be an array")
        if len(implementation_channels) != len(reference_channel_tokens):
            raise ComparisonError(
                f"implementation item {stem!r} has {len(implementation_channels)} channels; "
                f"reference has {len(reference_channel_tokens)}"
            )

        for channel_index, (channel_value, reference_token) in enumerate(
            zip(implementation_channels, reference_channel_tokens, strict=True)
        ):
            channel = require_dict(
                channel_value, f"implementation item {stem}.channels[{channel_index}]"
            )
            if channel.get("channelIndex") != channel_index:
                raise ComparisonError(
                    f"implementation item {stem!r} has non-canonical channel order"
                )
            implementation_token = implementation_channel_token(
                channel, silent_dr_db, f"implementation item {stem}.channels[{channel_index}]"
            )
            total_channel_dr += 1
            if implementation_token == reference_token:
                matched_channel_dr += 1
            else:
                differences.append(
                    {
                        "fixtureId": fixture_id,
                        "stem": stem,
                        "field": "channelDrDbToken",
                        "channelIndex": channel_index,
                        "reference": reference_token,
                        "implementation": implementation_token,
                    }
                )

    extra_stems = sorted(set(implementation_by_stem) - set(expected_stems))
    if extra_stems:
        raise ComparisonError(f"implementation has extra stems: {extra_stems}")
    if len(algorithm_identities) != 1:
        raise ComparisonError("implementation batch mixes algorithm identities")

    return {
        "schemaVersion": 1,
        "kind": "reference_conformance_comparison",
        "reference": {
            "normalizationSha256": sha256_bytes(reference_raw),
            "rawReportSha256": reference["source"]["rawReportSha256"],
            "corpusId": reference["source"]["corpusId"],
            "playlist": reference["source"]["playlist"],
            "targetVersions": reference["header"],
        },
        "implementation": {
            "wireOutputSha256": sha256_bytes(implementation_raw),
            "binarySha256": sha256_bytes(binary_raw),
            "wireSchemaVersion": implementation.get("schemaVersion"),
            "toolVersion": implementation.get("toolVersion"),
        },
        "policy": {
            "trackDr": "exact integer token",
            "channelDrDb": "exact exported two-decimal token",
            "numericToleranceDb": 0.0,
            "itemOrder": "compare by unique fixture stem",
        },
        "summary": {
            "status": "match" if not differences else "systematic_difference",
            "trackDrMatched": matched_track_dr,
            "trackDrTotal": len(reference_cases),
            "channelDrMatched": matched_channel_dr,
            "channelDrTotal": total_channel_dr,
            "differenceCount": len(differences),
            "fixtureSetExact": True,
            "implementationOrderMatchesReference": implementation_order == expected_stems,
        },
        "differences": differences,
        "notCompared": [
            {
                "referenceFields": ["peakDbfsToken", "rmsDbfsToken"],
                "reason": "MacinMeter WireEnvelope does not expose the same report-level primary-peak and overall-channel-RMS fields"
            },
            {
                "referenceField": "channelRmsDbfsTokens",
                "reason": "MacinMeter loudWindowRms has different semantics from the reference report channel overall RMS"
            },
            {
                "fieldClass": "internal intermediate state",
                "reason": "not observable in the exported reference report"
            }
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
