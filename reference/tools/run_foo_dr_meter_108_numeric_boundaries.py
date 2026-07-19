#!/usr/bin/env python3
"""Run the fixed x64 numeric-boundary matrix without starting foobar2000.

The matrix deliberately combines the last high-information per-track rules:

* the isolated duration formatter leaf at half-second boundaries;
* the optional multichannel loudness-weighting branch;
* both endpoint clamps of the per-channel RMS histogram.

Every vector gets a fresh hardened worker process.  The output is a path-free,
canonical record of the fixed target's behavior, not a compatibility claim.
"""

from __future__ import annotations

import argparse
import importlib.util
import math
import os
import struct
import sys
import tempfile
from array import array
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Iterator, Sequence


PARENT_PATH = Path(__file__).with_name("run_foo_dr_meter_108_core.py")
PARENT_SPEC = importlib.util.spec_from_file_location(
    "_foo_dr_meter_108_numeric_parent", PARENT_PATH
)
if PARENT_SPEC is None or PARENT_SPEC.loader is None:
    raise RuntimeError(f"cannot import {PARENT_PATH}")
PARENT = importlib.util.module_from_spec(PARENT_SPEC)
sys.modules[PARENT_SPEC.name] = PARENT
PARENT_SPEC.loader.exec_module(PARENT)


SCHEMA_VERSION = 1
RECORD_KIND = "foo_dr_meter_108_numeric_boundaries_record"
SUITE_ID = "foo-dr-meter-108-x64-numeric-boundaries-v1"
RUNTIME_PROFILE = "fixed_foobar_2_25_10"
WINDOW_DURATION_COEFFICIENT = 3.0040816326530613
WEIGHTING_SAMPLE_RATE = 8_000
WEIGHTING_WINDOW_COUNT = 10


@dataclass(frozen=True)
class DurationCase:
    case_id: str
    decoded_frames: int
    sample_rate_hz: int
    expected_text: str
    boundary: str


@dataclass(frozen=True)
class WeightingScenario:
    scenario_id: str
    channels: tuple[array, ...]
    channel_dr_db: tuple[float, ...]
    overall_rms: tuple[float, ...]
    expected_branch: str


@dataclass(frozen=True)
class HistogramCase:
    case_id: str
    rms_db: float
    rms: float
    peak: float
    expected_minus_100_db_count: int
    expected_zero_db_count: int
    boundary: str


def _duration_cases() -> tuple[DurationCase, ...]:
    return (
        DurationCase("duration-ms-0-below", 499, 1_000, "0:00", "0.5s-below"),
        DurationCase("duration-ms-0-half", 1, 2, "0:01", "0.5s-exact"),
        DurationCase("duration-ms-0-above", 501, 1_000, "0:01", "0.5s-above"),
        DurationCase("duration-ms-1-below", 1_499, 1_000, "0:01", "1.5s-below"),
        DurationCase("duration-ms-1-half", 3, 2, "0:02", "1.5s-exact"),
        DurationCase("duration-ms-1-above", 1_501, 1_000, "0:02", "1.5s-above"),
        DurationCase(
            "duration-44100-below", 22_049, 44_100, "0:00", "44.1k-half-below"
        ),
        DurationCase(
            "duration-44100-half", 22_050, 44_100, "0:01", "44.1k-half-exact"
        ),
        DurationCase(
            "duration-44100-above", 22_051, 44_100, "0:01", "44.1k-half-above"
        ),
        DurationCase(
            "duration-48000-below", 23_999, 48_000, "0:00", "48k-half-below"
        ),
        DurationCase(
            "duration-48000-half", 24_000, 48_000, "0:01", "48k-half-exact"
        ),
        DurationCase(
            "duration-48000-above", 24_001, 48_000, "0:01", "48k-half-above"
        ),
        DurationCase(
            "duration-minute-below", 59_499, 1_000, "0:59", "minute-below"
        ),
        DurationCase("duration-minute-half", 119, 2, "1:00", "minute-half"),
        DurationCase(
            "duration-minute-above", 59_501, 1_000, "1:00", "minute-above"
        ),
        DurationCase(
            "duration-hour-below", 3_599_499, 1_000, "59:59", "hour-below"
        ),
        DurationCase(
            "duration-hour-half", 7_199, 2, "1:00:00", "hour-half"
        ),
        DurationCase(
            "duration-hour-above", 3_599_501, 1_000, "1:00:00", "hour-above"
        ),
        DurationCase(
            "duration-day-below",
            86_399_499,
            1_000,
            "23:59:59",
            "day-below",
        ),
        DurationCase(
            "duration-day-half", 172_799, 2, "1d 0:00:00", "day-half"
        ),
        DurationCase(
            "duration-day-above",
            86_399_501,
            1_000,
            "1d 0:00:00",
            "day-above",
        ),
        DurationCase(
            "duration-week-below",
            604_799_499,
            1_000,
            "6d 23:59:59",
            "week-below",
        ),
        DurationCase(
            "duration-week-half",
            1_209_599,
            2,
            "1wk 0d 0:00:00",
            "week-half",
        ),
        DurationCase(
            "duration-week-above",
            604_799_501,
            1_000,
            "1wk 0d 0:00:00",
            "week-above",
        ),
    )


def _format_duration(whole_seconds: int) -> str:
    if whole_seconds < 0:
        raise ValueError("duration must be non-negative")
    weeks, remainder = divmod(whole_seconds, 7 * 24 * 60 * 60)
    days, remainder = divmod(remainder, 24 * 60 * 60)
    hours, remainder = divmod(remainder, 60 * 60)
    minutes, seconds = divmod(remainder, 60)
    if weeks:
        return f"{weeks}wk {days}d {hours}:{minutes:02d}:{seconds:02d}"
    if days:
        return f"{days}d {hours}:{minutes:02d}:{seconds:02d}"
    if hours:
        return f"{hours}:{minutes:02d}:{seconds:02d}"
    return f"{minutes}:{seconds:02d}"


def _expected_duration_text(decoded_frames: int, sample_rate_hz: int) -> str:
    seconds = float(decoded_frames) / float(sample_rate_hz)
    rounded = math.floor(seconds + 0.5)
    return _format_duration(rounded)


def _window_frames(sample_rate_hz: int) -> int:
    return math.floor(sample_rate_hz * WINDOW_DURATION_COEFFICIENT)


def _shaped_window(frames: int, rms: float, peak: float) -> array:
    if frames < 2 or not math.isfinite(rms) or not math.isfinite(peak):
        raise ValueError("invalid shaped-window geometry")
    required_sum_squares = rms * rms * frames / 2.0
    remainder = required_sum_squares - peak * peak
    if remainder < 0.0:
        raise ValueError("requested peak is incompatible with requested RMS")
    floor = math.sqrt(remainder / (frames - 1))
    output = array("d", [peak])
    output.extend(floor if frame & 1 else -floor for frame in range(1, frames))
    return output


def _repeat(window: array, count: int) -> array:
    output = array("d")
    for _ in range(count):
        output.extend(window)
    return output


def _zeros(frames: int) -> array:
    return array("d", [0.0]) * frames


def _dr_channel(target_dr_db: float, nonzero_windows: int) -> tuple[array, float]:
    width = _window_frames(WEIGHTING_SAMPLE_RATE)
    window_rms = 10.0 ** (-target_dr_db / 20.0)
    nonzero = _repeat(
        _shaped_window(width, window_rms, 1.0), nonzero_windows
    )
    zero_windows = WEIGHTING_WINDOW_COUNT - nonzero_windows
    if zero_windows:
        nonzero.extend(_zeros(width * zero_windows))
    overall_rms = window_rms * math.sqrt(
        nonzero_windows / WEIGHTING_WINDOW_COUNT
    )
    return nonzero, overall_rms


def _weighting_scenarios() -> Iterator[WeightingScenario]:
    a0, a0_rms = _dr_channel(10.0, 10)
    a1, a1_rms = _dr_channel(20.0, 10)
    a2, a2_rms = _dr_channel(30.0, 10)
    yield WeightingScenario(
        "weighting-balanced-3ch",
        (a0, a1, a2),
        (10.0, 20.0, 30.0),
        (a0_rms, a1_rms, a2_rms),
        "weighted_for_more_than_two_channels",
    )

    b0, b0_rms = _dr_channel(10.0, 10)
    b1, b1_rms = _dr_channel(20.0, 1)
    b2, b2_rms = _dr_channel(30.0, 1)
    yield WeightingScenario(
        "weighting-overall-rms-source-3ch",
        (b0, b1, b2),
        (10.0, 20.0, 30.0),
        (b0_rms, b1_rms, b2_rms),
        "overall_channel_rms_not_loud_rms_or_power",
    )

    c0, c0_rms = _dr_channel(10.0, 10)
    c1, c1_rms = _dr_channel(30.0, 10)
    yield WeightingScenario(
        "weighting-gate-2ch",
        (c0, c1),
        (10.0, 30.0),
        (c0_rms, c1_rms),
        "two_channel_gate_keeps_arithmetic_mean",
    )

    d0, d0_rms = _dr_channel(10.0, 10)
    d1, d1_rms = _dr_channel(30.0, 10)
    silent = _zeros(len(d0))
    yield WeightingScenario(
        "weighting-partial-silence-3ch",
        (d0, d1, silent),
        (10.0, 30.0, 0.0),
        (d0_rms, d1_rms, 0.0),
        "silent_channel_has_zero_weight_and_finite_denominator",
    )


def _interleaved_f64le(channels: Sequence[Sequence[float]]) -> bytes:
    if not channels or any(len(channel) != len(channels[0]) for channel in channels):
        raise ValueError("channels must be non-empty and frame-aligned")
    output = array("d")
    for frame in range(len(channels[0])):
        output.extend(channel[frame] for channel in channels)
    if sys.byteorder != "little":
        output.byteswap()
    return output.tobytes()


def _prepared_scenario(scenario: WeightingScenario) -> Any:
    pcm = _interleaved_f64le(scenario.channels)
    identity = PARENT.FileIdentity(PARENT.sha256_bytes(pcm), len(pcm))
    return PARENT.PreparedPcm(
        input_id=scenario.scenario_id,
        source_kind="generated_numeric_boundary_pcm",
        source_encoding="f64le-interleaved",
        conversion="identity",
        source_identity=identity,
        pcm=pcm,
        sample_rate=WEIGHTING_SAMPLE_RATE,
        channels=len(scenario.channels),
        frames=len(scenario.channels[0]),
    )


def _f32_bits(value: float) -> str:
    narrowed = struct.unpack("<f", struct.pack("<f", value))[0]
    return f"{struct.unpack('<I', struct.pack('<f', narrowed))[0]:08x}"


def _f64_bits(value: float) -> str:
    return f"{struct.unpack('<Q', struct.pack('<d', value))[0]:016x}"


def _prepared_identity(prepared: Any) -> dict[str, Any]:
    return {
        "pcmSha256": prepared.pcm_identity.sha256,
        "pcmByteLength": prepared.pcm_identity.byte_length,
        "sampleRateHz": prepared.sample_rate,
        "channels": prepared.channels,
        "frames": prepared.frames,
    }


def _expected_channel_bits(
    scenario: WeightingScenario,
) -> list[dict[str, Any]]:
    return [
        {
            "index": index,
            "drBits": _f32_bits(dr),
            "rmsBits": _f32_bits(rms),
        }
        for index, (dr, rms) in enumerate(
            zip(scenario.channel_dr_db, scenario.overall_rms)
        )
    ]


def _observed_channel_bits(result: dict[str, Any]) -> list[dict[str, Any]]:
    channels = result.get("channelResults")
    if not isinstance(channels, list):
        return []
    observed: list[dict[str, Any]] = []
    for item in channels:
        if not isinstance(item, dict):
            return []
        observed.append(
            {
                "index": item.get("index"),
                "drBits": item.get("drBits"),
                "rmsBits": item.get("rmsBits"),
            }
        )
    return observed


def _expected_weighted_track_bits(
    scenario: WeightingScenario, enabled: bool
) -> str:
    if not enabled or len(scenario.channels) <= 2:
        value = sum(scenario.channel_dr_db) / len(scenario.channel_dr_db)
    else:
        denominator = sum(scenario.overall_rms)
        value = sum(
            rms * dr
            for rms, dr in zip(scenario.overall_rms, scenario.channel_dr_db)
        ) / denominator
    return _f32_bits(value)


def _histogram_cases() -> tuple[HistogramCase, ...]:
    return (
        HistogramCase(
            "histogram-lower-beyond",
            -101.0,
            10.0 ** (-101.0 / 20.0),
            1.0e-4,
            1,
            0,
            "below_minus_100_db_clamps_to_bin_0",
        ),
        HistogramCase(
            "histogram-lower-exact",
            -100.0,
            1.0e-5,
            1.0e-4,
            1,
            0,
            "minus_100_db_is_bin_0",
        ),
        HistogramCase(
            "histogram-lower-inside",
            -99.0,
            10.0 ** (-99.0 / 20.0),
            1.0e-4,
            0,
            0,
            "above_minus_100_db_is_interior",
        ),
        HistogramCase(
            "histogram-upper-inside",
            -1.0,
            10.0 ** (-1.0 / 20.0),
            2.0,
            0,
            0,
            "below_zero_db_is_interior",
        ),
        HistogramCase(
            "histogram-upper-exact",
            0.0,
            1.0,
            2.0,
            0,
            1,
            "zero_db_is_bin_10000",
        ),
        HistogramCase(
            "histogram-upper-beyond",
            1.0,
            10.0 ** (1.0 / 20.0),
            2.0,
            0,
            1,
            "above_zero_db_clamps_to_bin_10000",
        ),
    )


def _prepared_histogram(case: HistogramCase) -> Any:
    width = _window_frames(WEIGHTING_SAMPLE_RATE)
    channel = _shaped_window(width, case.rms, case.peak)
    pcm = _interleaved_f64le((channel,))
    identity = PARENT.FileIdentity(PARENT.sha256_bytes(pcm), len(pcm))
    return PARENT.PreparedPcm(
        input_id=case.case_id,
        source_kind="generated_numeric_boundary_pcm",
        source_encoding="f64le-interleaved",
        conversion="identity",
        source_identity=identity,
        pcm=pcm,
        sample_rate=WEIGHTING_SAMPLE_RATE,
        channels=1,
        frames=width,
    )


def _runtime_sources(args: argparse.Namespace) -> dict[str, tuple[Path, str]]:
    return {
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
    }


def _common_worker_kwargs(args: argparse.Namespace) -> dict[str, Any]:
    return {
        "worker_path": args.worker,
        "worker_sha256": args.worker_sha256,
        "target_path": args.target_dll,
        "runtime_artifact_sources": _runtime_sources(args),
        "runtime_profile": RUNTIME_PROFILE,
        "timeout_seconds": args.timeout_seconds,
    }


def _core_result(record: dict[str, Any]) -> dict[str, Any]:
    result = record.get("result")
    if not isinstance(result, dict):
        raise PARENT.CoreHarnessError("core record result is absent")
    return result


def _duration_result(record: dict[str, Any]) -> dict[str, Any]:
    result = record.get("result")
    if not isinstance(result, dict):
        raise PARENT.CoreHarnessError("duration record result is absent")
    return result


def _paired_state(result: dict[str, Any]) -> dict[str, Any]:
    return {
        key: result[key]
        for key in (
            "channelResults",
            "sessionBeforeFinish",
            "sessionAfterFinish",
            "channelStateAfterFinish",
            "histogramAfterFinish",
        )
    }


def run_suite(args: argparse.Namespace) -> dict[str, Any]:
    common = _common_worker_kwargs(args)
    duration_records: list[dict[str, Any]] = []
    duration_matches = 0
    for case in _duration_cases():
        local_expected = _expected_duration_text(
            case.decoded_frames, case.sample_rate_hz
        )
        if local_expected != case.expected_text:
            raise PARENT.CoreHarnessError(
                f"duration case {case.case_id} has an inconsistent expectation"
            )
        record = PARENT.run_duration_worker(
            decoded_frames=case.decoded_frames,
            sample_rate_hz=case.sample_rate_hz,
            fractional_digits=0,
            **common,
        )
        actual = _duration_result(record).get("text")
        matched = actual == case.expected_text
        duration_matches += int(matched)
        duration_records.append(
            {
                "caseId": case.case_id,
                "boundary": case.boundary,
                "decodedFrames": case.decoded_frames,
                "sampleRateHz": case.sample_rate_hz,
                "expectedText": case.expected_text,
                "actualText": actual,
                "matched": matched,
                "record": record,
            }
        )

    weighting_records: list[dict[str, Any]] = []
    weighting_bits_matched = 0
    weighting_run_total = 0
    weighting_channel_preconditions_matched = 0
    weighting_pair_invariants = 0
    weighting_scenario_total = 0
    for scenario in _weighting_scenarios():
        weighting_scenario_total += 1
        prepared = _prepared_scenario(scenario)
        input_identity = _prepared_identity(prepared)
        expected_channels = _expected_channel_bits(scenario)
        pair: dict[bool, dict[str, Any]] = {}
        expected: dict[bool, str] = {}
        for enabled in (False, True):
            weighting_run_total += 1
            expected_bits = _expected_weighted_track_bits(scenario, enabled)
            expected[enabled] = expected_bits
            record = PARENT.run_core_worker(
                prepared,
                multichannel_loudness_weighting=enabled,
                block_frames=args.block_frames,
                **common,
            )
            result = _core_result(record)
            actual_bits = result.get("trackDrBits")
            matched = actual_bits == expected_bits
            actual_channels = _observed_channel_bits(result)
            channel_preconditions_matched = actual_channels == expected_channels
            weighting_bits_matched += int(matched)
            weighting_channel_preconditions_matched += int(
                channel_preconditions_matched
            )
            pair[enabled] = record
            weighting_records.append(
                {
                    "scenarioId": scenario.scenario_id,
                    "enabled": enabled,
                    "expectedBranch": scenario.expected_branch,
                    "expectedTrackDrBits": expected_bits,
                    "actualTrackDrBits": actual_bits,
                    "matched": matched,
                    "expectedChannelBits": expected_channels,
                    "actualChannelBits": actual_channels,
                    "channelPreconditionsMatched": (
                        channel_preconditions_matched
                    ),
                    "inputIdentity": input_identity,
                    "record": record,
                }
            )
        disabled_result = _core_result(pair[False])
        enabled_result = _core_result(pair[True])
        state_equal = _paired_state(disabled_result) == _paired_state(enabled_result)
        track_should_change = len(scenario.channels) > 2
        track_changed = (
            disabled_result.get("trackDrBits") != enabled_result.get("trackDrBits")
        )
        pair_matched = state_equal and track_changed == track_should_change
        weighting_pair_invariants += int(pair_matched)
        weighting_records.append(
            {
                "scenarioId": scenario.scenario_id,
                "kind": "pairedAssertion",
                "channelAndSessionStateEqual": state_equal,
                "trackShouldChange": track_should_change,
                "trackChanged": track_changed,
                "matched": pair_matched,
                "disabledExpectedTrackDrBits": expected[False],
                "enabledExpectedTrackDrBits": expected[True],
            }
        )

    histogram_records: list[dict[str, Any]] = []
    histogram_matches = 0
    for case in _histogram_cases():
        prepared = _prepared_histogram(case)
        input_identity = _prepared_identity(prepared)
        record = PARENT.run_core_worker(
            prepared,
            multichannel_loudness_weighting=False,
            block_frames=args.block_frames,
            **common,
        )
        result = _core_result(record)
        histogram = result.get("histogramAfterFinish")
        channels = histogram.get("channels") if isinstance(histogram, dict) else None
        channel = channels[0] if isinstance(channels, list) and len(channels) == 1 else {}
        matched = (
            isinstance(channel, dict)
            and channel.get("totalCount") == 1
            and channel.get("nonzeroBinCount") == 1
            and channel.get("minus100DbCount")
            == case.expected_minus_100_db_count
            and channel.get("zeroDbCount") == case.expected_zero_db_count
        )
        histogram_matches += int(matched)
        histogram_records.append(
            {
                "caseId": case.case_id,
                "boundary": case.boundary,
                "rmsDb": case.rms_db,
                "expectedMinus100DbCount": case.expected_minus_100_db_count,
                "expectedZeroDbCount": case.expected_zero_db_count,
                "matched": matched,
                "inputIdentity": input_identity,
                "record": record,
            }
        )

    summary = {
        "durationMatched": duration_matches,
        "durationTotal": len(duration_records),
        "weightingTrackBitsMatched": weighting_bits_matched,
        "weightingTrackBitsTotal": weighting_run_total,
        "weightingChannelPreconditionsMatched": (
            weighting_channel_preconditions_matched
        ),
        "weightingChannelPreconditionsTotal": weighting_run_total,
        "weightingPairInvariantsMatched": weighting_pair_invariants,
        "weightingPairInvariantsTotal": weighting_scenario_total,
        "histogramMatched": histogram_matches,
        "histogramTotal": len(histogram_records),
    }
    all_matched = all(
        summary[key] == summary[key.replace("Matched", "Total")]
        for key in (
            "durationMatched",
            "weightingTrackBitsMatched",
            "weightingChannelPreconditionsMatched",
            "weightingPairInvariantsMatched",
            "histogramMatched",
        )
    )
    semantic_manifest = {
        "suiteId": SUITE_ID,
        "generator": {
            "windowDurationCoefficientF64Hex": (
                WINDOW_DURATION_COEFFICIENT.hex()
            ),
            "weightingSampleRateHz": WEIGHTING_SAMPLE_RATE,
            "weightingWindowCount": WEIGHTING_WINDOW_COUNT,
            "weightingWindowFrames": _window_frames(
                WEIGHTING_SAMPLE_RATE
            ),
        },
        "duration": [
            {
                "caseId": item.case_id,
                "decodedFrames": item.decoded_frames,
                "sampleRateHz": item.sample_rate_hz,
                "expectedText": item.expected_text,
            }
            for item in _duration_cases()
        ],
        "weighting": [
            {
                "scenarioId": item["scenarioId"],
                "enabled": item.get("enabled"),
                "expectedTrackDrBits": item.get("expectedTrackDrBits"),
                "expectedChannelBits": item.get("expectedChannelBits"),
                "inputIdentity": item.get("inputIdentity"),
            }
            for item in weighting_records
            if "enabled" in item
        ],
        "histogram": [
            {
                "caseId": item["caseId"],
                "rmsDb": item["rmsDb"],
                "rmsBits": _f64_bits(
                    next(
                        case.rms
                        for case in _histogram_cases()
                        if case.case_id == item["caseId"]
                    )
                ),
                "peakBits": _f64_bits(
                    next(
                        case.peak
                        for case in _histogram_cases()
                        if case.case_id == item["caseId"]
                    )
                ),
                "expectedMinus100DbCount": item[
                    "expectedMinus100DbCount"
                ],
                "expectedZeroDbCount": item["expectedZeroDbCount"],
                "inputIdentity": item["inputIdentity"],
            }
            for item in histogram_records
        ],
    }
    record = {
        "schemaVersion": SCHEMA_VERSION,
        "kind": RECORD_KIND,
        "suiteId": SUITE_ID,
        "semanticManifestSha256": PARENT.sha256_bytes(
            PARENT.canonical_json_bytes(semantic_manifest)
        ),
        "target": {
            "id": "TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad",
            "sha256": PARENT.EXPECTED_TARGET_SHA256,
            "byteLength": PARENT.EXPECTED_TARGET_BYTE_LENGTH,
        },
        "execution": {
            "workerSha256": PARENT.require_sha256(
                args.worker_sha256, "worker SHA-256"
            ),
            "runtimeProfile": RUNTIME_PROFILE,
            "processModel": "one_worker_process_per_vector",
            "foobarStarted": False,
        },
        "duration": duration_records,
        "multichannelWeighting": weighting_records,
        "histogramClamp": histogram_records,
        "summary": {**summary, "allMatched": all_matched},
        "claims": {
            "scope": (
                "isolated fixed x64 duration numeric leaf and analyzer-core "
                "boundary observations"
            ),
            "compatibility": "none",
            "foobarParity": "not_assessed",
        },
        "limitations": [
            "No foobar decoder, component lifecycle, metadata, album grouping, or full report renderer was exercised.",
            "Generated binary64 PCM enters the isolated analyzer boundary directly.",
            "The duration observation covers the fixed numeric leaf and its static renderer call contract, not full report byte parity.",
        ],
    }
    PARENT.assert_path_free(record, "numeric-boundary record")
    PARENT.canonical_json_bytes(record)
    return record


def _write_record(record: dict[str, Any], output: Path | None) -> None:
    raw = PARENT.canonical_json_bytes(record)
    if output is None:
        sys.stdout.buffer.write(raw)
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=output.parent, prefix=f".{output.name}.", delete=False
    ) as temporary:
        temporary.write(raw)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)
    try:
        os.replace(temporary_path, output)
    finally:
        if temporary_path.exists():
            temporary_path.unlink()


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
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
        record = run_suite(args)
        _write_record(record, args.output)
        return 0 if record["summary"]["allMatched"] else 1
    except (PARENT.CoreHarnessError, OSError, ValueError) as error:
        print(f"numeric boundary suite failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
