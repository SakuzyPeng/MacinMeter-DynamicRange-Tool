#!/usr/bin/env python3
"""Compare construction-model diagnostics with a normalized observation.

This is a model-validation aid, not a reference oracle. The observation remains
the reference fact; the model file is independently generated diagnostic data.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path
from typing import Any


class ModelComparisonError(ValueError):
    """An input does not satisfy the fixed model-comparison contract."""


def load_json_bytes(path: Path) -> tuple[bytes, dict[str, Any]]:
    try:
        raw = path.read_bytes()
        value = json.loads(raw)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ModelComparisonError(f"cannot read JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise ModelComparisonError(f"{path} must contain one JSON object")
    return raw, value


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def finite_hex(value: Any, context: str) -> float:
    if not isinstance(value, str):
        raise ModelComparisonError(f"{context} must be a hexadecimal float string")
    try:
        parsed = float.fromhex(value)
    except ValueError as error:
        raise ModelComparisonError(f"{context} is not a hexadecimal float") from error
    if not math.isfinite(parsed):
        raise ModelComparisonError(f"{context} must be finite")
    return parsed


def db_token(linear: float) -> str:
    if linear == 0.0:
        return "-inf"
    return f"{f32(20.0 * math.log10(linear)):.2f}"


def compare(observation_path: Path, model_path: Path) -> dict[str, Any]:
    observation_raw, observation = load_json_bytes(observation_path)
    model_raw, model = load_json_bytes(model_path)
    if observation.get("kind") != "foo_dr_meter_report_normalization":
        raise ModelComparisonError("observation input has the wrong kind")
    if model.get("kind") != "model_prediction":
        raise ModelComparisonError("model input has the wrong kind")

    observation_cases = observation.get("cases")
    model_cases = model.get("cases")
    if not isinstance(observation_cases, list) or not isinstance(model_cases, list):
        raise ModelComparisonError("both inputs must contain cases arrays")
    model_by_id: dict[str, dict[str, Any]] = {}
    for case in model_cases:
        if not isinstance(case, dict) or not isinstance(case.get("id"), str):
            raise ModelComparisonError("model contains an invalid case")
        if case["id"] in model_by_id:
            raise ModelComparisonError(f"model repeats case {case['id']!r}")
        model_by_id[case["id"]] = case

    counts = {
        "trackDr": [0, 0],
        "channelDr": [0, 0],
        "overallPeak": [0, 0],
        "overallRms": [0, 0],
        "channelRms": [0, 0],
    }
    differences: list[dict[str, Any]] = []
    for observation_case in observation_cases:
        if not isinstance(observation_case, dict):
            raise ModelComparisonError("observation contains an invalid case")
        fixture_id = observation_case.get("fixtureId")
        if not isinstance(fixture_id, str):
            raise ModelComparisonError("observation case lacks fixtureId")
        model_case = model_by_id.get(fixture_id)
        if model_case is None:
            raise ModelComparisonError(f"model is missing case {fixture_id!r}")
        diagnostics = model_case.get("candidateDiagnostics")
        if not isinstance(diagnostics, dict):
            raise ModelComparisonError(f"model case {fixture_id!r} lacks diagnostics")

        def observe(field: str, reference: Any, predicted: Any) -> None:
            counts[field][1] += 1
            if reference == predicted:
                counts[field][0] += 1
            else:
                differences.append(
                    {
                        "fixtureId": fixture_id,
                        "field": field,
                        "reference": reference,
                        "model": predicted,
                    }
                )

        observe("trackDr", observation_case["trackDr"], diagnostics["roundedTrackDr"])
        observe(
            "overallPeak",
            observation_case["peakDbfsToken"],
            db_token(
                finite_hex(
                    diagnostics["reportPeakF64Hex"],
                    f"{fixture_id}.reportPeakF64Hex",
                )
            ),
        )
        observe(
            "overallRms",
            observation_case["rmsDbfsToken"],
            db_token(
                finite_hex(
                    diagnostics["reportRmsF64Hex"],
                    f"{fixture_id}.reportRmsF64Hex",
                )
            ),
        )

        channel_diagnostics = diagnostics.get("channels")
        reference_channel_dr = observation_case.get("channelDrDbTokens")
        reference_channel_rms = observation_case.get("channelRmsDbfsTokens")
        if (
            not isinstance(channel_diagnostics, list)
            or not isinstance(reference_channel_dr, list)
            or not isinstance(reference_channel_rms, list)
            or len(channel_diagnostics) != len(reference_channel_dr)
            or len(channel_diagnostics) != len(reference_channel_rms)
        ):
            raise ModelComparisonError(f"case {fixture_id!r} has inconsistent channels")
        for channel_index, channel in enumerate(channel_diagnostics):
            if not isinstance(channel, dict):
                raise ModelComparisonError(
                    f"case {fixture_id!r} channel {channel_index} is invalid"
                )
            observe(
                "channelDr",
                reference_channel_dr[channel_index],
                f"{finite_hex(channel['drF32Hex'], f'{fixture_id}.drF32Hex'):.2f}",
            )
            observe(
                "channelRms",
                reference_channel_rms[channel_index],
                db_token(
                    finite_hex(
                        channel["channelRmsF32Hex"],
                        f"{fixture_id}.channelRmsF32Hex",
                    )
                ),
            )

    expected_ids = {
        case["fixtureId"]
        for case in observation_cases
        if isinstance(case, dict) and isinstance(case.get("fixtureId"), str)
    }
    extras = sorted(set(model_by_id) - expected_ids)
    return {
        "schemaVersion": 1,
        "kind": "construction_model_observation_comparison",
        "factBoundary": {
            "reference": "normalized fixed-target observation",
            "model": "construction diagnostics, not a golden",
        },
        "inputs": {
            "observationNormalizationSha256": sha256_bytes(observation_raw),
            "rawReportSha256": observation["source"]["rawReportSha256"],
            "modelPredictionsSha256": sha256_bytes(model_raw),
            "modelName": model.get("model"),
            "compatibilityClaim": model.get("compatibilityClaim"),
        },
        "summary": {
            "status": "match" if not differences else "difference",
            "trackDrMatched": counts["trackDr"][0],
            "trackDrTotal": counts["trackDr"][1],
            "channelDrMatched": counts["channelDr"][0],
            "channelDrTotal": counts["channelDr"][1],
            "overallPeakMatched": counts["overallPeak"][0],
            "overallPeakTotal": counts["overallPeak"][1],
            "overallRmsMatched": counts["overallRms"][0],
            "overallRmsTotal": counts["overallRms"][1],
            "channelRmsMatched": counts["channelRms"][0],
            "channelRmsTotal": counts["channelRms"][1],
            "differenceCount": len(differences),
            "ignoredModelCasesOutsideObservation": extras,
        },
        "differences": differences,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--observation", required=True, type=Path)
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        result = compare(args.observation, args.model)
    except (OSError, ModelComparisonError) as error:
        raise SystemExit(f"error: {error}") from error
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output is None:
        print(rendered, end="")
    else:
        args.output.write_text(rendered, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
