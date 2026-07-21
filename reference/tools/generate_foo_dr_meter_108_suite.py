#!/usr/bin/env python3
"""Generate deterministic PCM probes for foo_dr_meter 1.0.8.

The generated WAV files are experimental inputs, not correctness goldens.
Their questions were chosen to distinguish the historical M0 ProvisionalV1
rules from behavior recovered statically from foo_dr_meter 1.0.8.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import struct
import subprocess
import sys
from array import array
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Sequence


CORPUS_ID = "foo-dr-meter-108-discriminating-v1"
GENERATOR_VERSION = 1
SAMPLE_RATE = 8_000
WINDOW_DURATION_COEFFICIENT = 3.0040816326530613
WINDOW_FRAMES = math.floor(SAMPLE_RATE * WINDOW_DURATION_COEFFICIENT)
IEEE_FLOAT_SUBFORMAT = bytes.fromhex(
    "03000000" "0000" "1000" "8000" "00aa00389b71"
)


@dataclass(frozen=True)
class Fixture:
    fixture_id: str
    filename: str
    report_group: str
    question: str
    alternative_hypotheses: Sequence[str]
    channels: Sequence[array]
    waveform: dict[str, Any]
    channel_mask: int | None = None


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def samples(values: Iterable[float]) -> array:
    output = array("f", (f32(value) for value in values))
    if output.itemsize != 4:
        raise RuntimeError("the fixture generator requires 32-bit C floats")
    return output


def constant(frames: int, amplitude: float) -> array:
    return samples(amplitude for _ in range(frames))


def shaped_window(frames: int, rms: float, peak: float) -> array:
    """Build one window with one peak and an alternating constant floor.

    The requested RMS uses the reference formula sqrt(2 * sum(x^2) / N).
    Values are rounded to float32 before they are written or summarized.
    """

    if frames < 2:
        raise ValueError("a shaped window requires at least two frames")
    peak_f32 = f32(peak)
    required_sum_squares = rms * rms * frames / 2.0
    remainder = required_sum_squares - peak_f32 * peak_f32
    if remainder < 0.0:
        raise ValueError(
            f"RMS {rms} is too small for peak {peak_f32} over {frames} frames"
        )
    floor_f32 = f32(math.sqrt(remainder / (frames - 1)))
    output = array("f", [peak_f32])
    output.extend(
        floor_f32 if frame % 2 else -floor_f32 for frame in range(1, frames)
    )
    return output


def join(*parts: array) -> array:
    output = array("f")
    for part in parts:
        output.extend(part)
    return output


def repeated_shaped_windows(
    count: int, rms: float, peak: float = 1.0
) -> array:
    window = shaped_window(WINDOW_FRAMES, rms, peak)
    output = array("f")
    for _ in range(count):
        output.extend(window)
    return output


def peak_order_case(first_peak_db: float, second_peak_db: float) -> array:
    rms = 10.0 ** (-14.5 / 20.0)
    peaks_db = [first_peak_db, second_peak_db, -3.0980391997148637]
    peaks_db.extend([-3.0980391997148637, -3.0980391997148637])
    return join(
        *(
            shaped_window(WINDOW_FRAMES, rms, 10.0 ** (peak_db / 20.0))
            for peak_db in peaks_db
        )
    )


def build_fixtures() -> list[Fixture]:
    quarter_rms = 0.25
    low_peak_db = -2.0035
    high_peak_db = -1.9965

    core = [
        Fixture(
            fixture_id="window-minus-one-control",
            filename="101_window_minus_one_control.wav",
            report_group="01-core",
            question="Does a non-empty W-1 tail form one ordinary analysis window?",
            alternative_hypotheses=[
                "The tail is submitted and measured.",
                "The tail is discarded or treated as insufficient.",
            ],
            channels=[
                shaped_window(WINDOW_FRAMES - 1, quarter_rms, 1.0)
            ],
            waveform={
                "kind": "single-shaped-tail",
                "frames": WINDOW_FRAMES - 1,
                "targetRms": quarter_rms,
                "peak": 1.0,
            },
        ),
        Fixture(
            fixture_id="exact-window-control",
            filename="102_exact_window_control.wav",
            report_group="01-core",
            question=(
                "Does exactly one complete window produce an ordinary numeric "
                "control result? The exported report cannot distinguish "
                "internal virtual-zero behavior."
            ),
            alternative_hypotheses=[
                "The complete window is finalized as an ordinary measured result.",
                "The complete window produces no ordinary numeric result.",
            ],
            channels=[shaped_window(WINDOW_FRAMES, quarter_rms, 1.0)],
            waveform={
                "kind": "single-shaped-window",
                "frames": WINDOW_FRAMES,
                "targetRms": quarter_rms,
                "peak": 1.0,
            },
        ),
        Fixture(
            fixture_id="tail-pair-base",
            filename="103_tail_pair_base.wav",
            report_group="01-core",
            question="What is the control result before appending a one-frame tail?",
            alternative_hypotheses=[
                "The paired input without a tail establishes the baseline.",
                "The paired input is affected by an unrelated boundary rule.",
            ],
            channels=[
                join(
                    shaped_window(WINDOW_FRAMES, 0.1, 1.0),
                    shaped_window(WINDOW_FRAMES, 0.09, 0.9),
                )
            ],
            waveform={
                "kind": "tail-pair-control",
                "windows": [
                    {"targetRms": 0.1, "peak": 1.0},
                    {"targetRms": 0.09, "peak": 0.9},
                ],
            },
        ),
        Fixture(
            fixture_id="tail-pair-plus-one",
            filename="104_tail_pair_plus_one.wav",
            report_group="01-core",
            question="Does appending one nonzero frame create a third window?",
            alternative_hypotheses=[
                "The final 0.5 sample forms a third window.",
                "The final one-frame tail is ignored and matches the paired control.",
            ],
            channels=[
                join(
                    shaped_window(WINDOW_FRAMES, 0.1, 1.0),
                    shaped_window(WINDOW_FRAMES, 0.09, 0.9),
                    samples([0.5]),
                )
            ],
            waveform={
                "kind": "tail-pair-plus-one",
                "windows": [
                    {"targetRms": 0.1, "peak": 1.0},
                    {"targetRms": 0.09, "peak": 0.9},
                ],
                "tailSamples": [0.5],
            },
        ),
        Fixture(
            fixture_id="negative-dr-fallback",
            filename="105_negative_dr_fallback.wav",
            report_group="01-core",
            question="Is a negative secondary-peak DR recomputed with the primary peak?",
            alternative_hypotheses=[
                "The primary peak is used for a positive recomputed result.",
                "The negative result is directly clamped to zero.",
                "The negative result is preserved.",
            ],
            channels=[
                join(
                    shaped_window(WINDOW_FRAMES, 0.8, 1.0),
                    shaped_window(WINDOW_FRAMES, 0.05, 0.1),
                )
            ],
            waveform={
                "kind": "ranked-shaped-windows",
                "windows": [
                    {"targetRms": 0.8, "peak": 1.0},
                    {"targetRms": 0.05, "peak": 0.1},
                ],
            },
        ),
        Fixture(
            fixture_id="histogram-db-domain",
            filename="110_histogram_db_domain.wav",
            report_group="01-core",
            question="Is RMS quantized in 0.01 dB bins or linear 0.0001 bins?",
            alternative_hypotheses=[
                "RMS is rounded in the dB domain before reconstruction.",
                "RMS is truncated in the linear domain.",
            ],
            channels=[
                join(
                    shaped_window(WINDOW_FRAMES, 0.00109, 0.1),
                    *(
                        shaped_window(WINDOW_FRAMES, 0.00101, 0.1)
                        for _ in range(4)
                    ),
                )
            ],
            waveform={
                "kind": "ranked-shaped-windows",
                "windows": [
                    {"count": 1, "targetRms": 0.00109, "peak": 0.1},
                    {"count": 4, "targetRms": 0.00101, "peak": 0.1},
                ],
            },
        ),
        Fixture(
            fixture_id="loud-boundary-bin-ties",
            filename="111_loud_boundary_bin_ties.wav",
            report_group="01-core",
            question="Are all ties in the loudest-20-percent boundary bin included?",
            alternative_hypotheses=[
                "The complete four-window boundary bin is included.",
                "Only enough boundary-bin windows to reach floor(N/5) are used.",
            ],
            channels=[
                join(
                    shaped_window(WINDOW_FRAMES, 0.2, 0.5),
                    *(
                        shaped_window(WINDOW_FRAMES, 0.1, 0.5)
                        for _ in range(4)
                    ),
                    *(
                        shaped_window(WINDOW_FRAMES, 0.02, 0.5)
                        for _ in range(5)
                    ),
                )
            ],
            waveform={
                "kind": "ranked-shaped-windows",
                "windows": [
                    {"count": 1, "targetRms": 0.2, "peak": 0.5},
                    {"count": 4, "targetRms": 0.1, "peak": 0.5},
                    {"count": 5, "targetRms": 0.02, "peak": 0.5},
                ],
            },
        ),
        Fixture(
            fixture_id="peak-order-low-then-high",
            filename="120_peak_order_low_then_high.wav",
            report_group="01-core",
            question=(
                "When the two largest peaks share a 0.01 dB key, does their "
                "arrival order affect the selected second peak?"
            ),
            alternative_hypotheses=[
                "The later, slightly higher peak becomes the selected secondary peak.",
                "Raw-amplitude order statistics select the lower peak.",
            ],
            channels=[peak_order_case(low_peak_db, high_peak_db)],
            waveform={
                "kind": "equal-rms-quantized-peak-order",
                "targetRmsDb": -14.5,
                "peakDbOrder": [
                    low_peak_db,
                    high_peak_db,
                    -3.0980391997148637,
                    -3.0980391997148637,
                    -3.0980391997148637,
                ],
            },
        ),
        Fixture(
            fixture_id="peak-order-high-then-low",
            filename="121_peak_order_high_then_low.wav",
            report_group="01-core",
            question=(
                "Does reversing only the first two quantized-equal peaks change "
                "the selected peak and rounded DR?"
            ),
            alternative_hypotheses=[
                "The selected secondary peak changes with arrival order.",
                "The result is permutation invariant.",
            ],
            channels=[peak_order_case(high_peak_db, low_peak_db)],
            waveform={
                "kind": "equal-rms-quantized-peak-order",
                "targetRmsDb": -14.5,
                "peakDbOrder": [
                    high_peak_db,
                    low_peak_db,
                    -3.0980391997148637,
                    -3.0980391997148637,
                    -3.0980391997148637,
                ],
            },
        ),
    ]

    degenerate = [
        Fixture(
            fixture_id="one-frame-nonzero",
            filename="201_one_frame_nonzero.wav",
            report_group="02-degenerate",
            question="Can a one-frame nonzero file produce a completed DR result?",
            alternative_hypotheses=[
                "One frame is finalized and reported.",
                "The file has insufficient analysis data.",
            ],
            channels=[samples([0.5])],
            waveform={"kind": "literal", "samplesPerChannel": [[0.5]]},
        ),
        Fixture(
            fixture_id="two-frame-negative",
            filename="202_two_frame_constant.wav",
            report_group="02-degenerate",
            question="How is a two-frame constant signal with negative raw DR reported?",
            alternative_hypotheses=[
                "The result is clamped to DR0.",
                "A negative DR is reported.",
            ],
            channels=[samples([0.5, 0.5])],
            waveform={"kind": "literal", "samplesPerChannel": [[0.5, 0.5]]},
        ),
        Fixture(
            fixture_id="silent-mono",
            filename="203_silent_mono.wav",
            report_group="02-degenerate",
            question="Does a silent channel become DR0 or an excluded state?",
            alternative_hypotheses=[
                "Silence is represented as a numeric DR0.",
                "Silence is excluded or reported as insufficient.",
            ],
            channels=[constant(2 * WINDOW_FRAMES, 0.0)],
            waveform={
                "kind": "constant",
                "frames": 2 * WINDOW_FRAMES,
                "amplitude": 0.0,
            },
        ),
    ]

    measured_left = repeated_shaped_windows(
        10, 10.0 ** (-12.0 / 20.0), peak=1.0
    )
    multichannel = [
        Fixture(
            fixture_id="stereo-silent-channel",
            filename="301_stereo_silent_channel.wav",
            report_group="03-multichannel",
            question="Is a silent stereo channel included as DR0 in the track mean?",
            alternative_hypotheses=[
                "The track value averages the measured channel with numeric zero.",
                "The silent channel is excluded from aggregation.",
            ],
            channels=[
                measured_left,
                constant(10 * WINDOW_FRAMES, 0.0),
            ],
            waveform={
                "kind": "per-channel-repeated-shaped-windows",
                "count": 10,
                "channels": [
                    {"targetDrDb": 12.0, "peak": 1.0},
                    {"constant": 0.0},
                ],
            },
        ),
        Fixture(
            fixture_id="three-channel-arithmetic",
            filename="302_three_channel_arithmetic.wav",
            report_group="03-multichannel",
            question="Is default three-channel aggregation an unweighted arithmetic mean?",
            alternative_hypotheses=[
                "Channel DR values are averaged arithmetically.",
                "Channel loudness weights the track DR.",
            ],
            channels=[
                repeated_shaped_windows(
                    10, 10.0 ** (-dr_db / 20.0), peak=1.0
                )
                for dr_db in (10.0, 20.0, 30.0)
            ],
            waveform={
                "kind": "per-channel-repeated-shaped-windows",
                "count": 10,
                "channels": [
                    {"targetDrDb": dr_db, "peak": 1.0}
                    for dr_db in (10.0, 20.0, 30.0)
                ],
            },
            channel_mask=0x0000_0007,
        ),
        Fixture(
            fixture_id="six-channel-lfe",
            filename="303_six_channel_lfe.wav",
            report_group="03-multichannel",
            question="Does the official six-channel result include the LFE channel?",
            alternative_hypotheses=[
                "All six channel DR values, including LFE, are averaged.",
                "The LFE channel is excluded from the official value.",
            ],
            channels=[
                repeated_shaped_windows(
                    10, 10.0 ** (-dr_db / 20.0), peak=1.0
                )
                for dr_db in (6.0, 9.0, 12.0, 30.0, 15.0, 18.0)
            ],
            waveform={
                "kind": "per-channel-repeated-shaped-windows",
                "count": 10,
                "speakerMask": "0x0000003f",
                "channelOrder": ["FL", "FR", "FC", "LFE", "BL", "BR"],
                "channels": [
                    {"targetDrDb": dr_db, "peak": 1.0}
                    for dr_db in (6.0, 9.0, 12.0, 30.0, 15.0, 18.0)
                ],
            },
            channel_mask=0x0000_003F,
        ),
    ]

    return core + degenerate + multichannel


def chunk_bytes(chunk_id: bytes, payload: bytes) -> bytes:
    padding = b"\x00" if len(payload) % 2 else b""
    return chunk_id + struct.pack("<I", len(payload)) + payload + padding


def format_chunk(channel_count: int, channel_mask: int | None) -> bytes:
    bits_per_sample = 32
    block_align = channel_count * bits_per_sample // 8
    byte_rate = SAMPLE_RATE * block_align
    if channel_count <= 2 and channel_mask is None:
        return struct.pack(
            "<HHIIHH",
            3,
            channel_count,
            SAMPLE_RATE,
            byte_rate,
            block_align,
            bits_per_sample,
        )
    if channel_mask is None:
        raise ValueError("multichannel fixtures require an explicit speaker mask")
    return struct.pack(
        "<HHIIHHHHI16s",
        0xFFFE,
        channel_count,
        SAMPLE_RATE,
        byte_rate,
        block_align,
        bits_per_sample,
        22,
        bits_per_sample,
        channel_mask,
        IEEE_FLOAT_SUBFORMAT,
    )


def write_wave(path: Path, fixture: Fixture) -> tuple[str, str, int]:
    channel_count = len(fixture.channels)
    if channel_count == 0:
        raise ValueError(f"{fixture.fixture_id} has no channels")
    frame_count = len(fixture.channels[0])
    if any(len(channel) != frame_count for channel in fixture.channels):
        raise ValueError(f"{fixture.fixture_id} channel lengths differ")
    if any(
        not math.isfinite(value) or abs(value) > 1.0
        for channel in fixture.channels
        for value in channel
    ):
        raise ValueError(f"{fixture.fixture_id} contains invalid PCM")

    data_size = frame_count * channel_count * 4
    fmt = chunk_bytes(b"fmt ", format_chunk(channel_count, fixture.channel_mask))
    fact = chunk_bytes(b"fact", struct.pack("<I", frame_count))
    data_header = b"data" + struct.pack("<I", data_size)
    riff_size = 4 + len(fmt) + len(fact) + len(data_header) + data_size
    if riff_size > 0xFFFF_FFFF:
        raise ValueError(f"{fixture.fixture_id} exceeds RIFF32 limits")

    data_hash = hashlib.sha256()
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as output:
        output.write(b"RIFF")
        output.write(struct.pack("<I", riff_size))
        output.write(b"WAVE")
        output.write(fmt)
        output.write(fact)
        output.write(data_header)

        for start in range(0, frame_count, 4_096):
            end = min(start + 4_096, frame_count)
            interleaved = array("f")
            for frame in range(start, end):
                for channel in fixture.channels:
                    interleaved.append(channel[frame])
            if sys.byteorder != "little":
                interleaved.byteswap()
            encoded = interleaved.tobytes()
            output.write(encoded)
            data_hash.update(encoded)

    file_hash = hashlib.sha256(path.read_bytes()).hexdigest()
    return file_hash, data_hash.hexdigest(), path.stat().st_size


def generated_window_summary(channel: array) -> list[dict[str, Any]]:
    summaries = []
    for start in range(0, len(channel), WINDOW_FRAMES):
        window = channel[start : start + WINDOW_FRAMES]
        if not window:
            continue
        sum_squares = math.fsum(float(value) * float(value) for value in window)
        rms = math.sqrt(2.0 * sum_squares / len(window))
        peak = max(abs(float(value)) for value in window)
        summaries.append(
            {
                "frames": len(window),
                "rmsF64Hex": rms.hex(),
                "peakF64Hex": peak.hex(),
            }
        )
    return summaries


def write_playlist(path: Path, fixtures: Sequence[Fixture]) -> None:
    lines = ["#EXTM3U"]
    for fixture in fixtures:
        lines.append(f"../{fixture.report_group}/{fixture.filename}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def git_identity() -> dict[str, Any]:
    source = Path(__file__).resolve()
    repository = source.parents[2]
    relative_source = source.relative_to(repository)
    try:
        commit = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repository,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        generator_status = subprocess.run(
            ["git", "status", "--porcelain", "--", str(relative_source)],
            cwd=repository,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return {"commit": None, "generatorPathDirty": None}
    return {
        "commit": commit or None,
        "generatorPathDirty": bool(generator_status.strip()),
    }


def metadata_template() -> str:
    return f"""foo_dr_meter reference-run metadata

Corpus
  corpusId: {CORPUS_ID}
  manifestSha256: <SHA-256 of manifest.json>

Run
  architecture: <x86 or x64>
  repeat: <1, 2, or 3>
  localDateTime: <YYYY-MM-DD HH:MM:SS>
  timezone: <Windows timezone name and UTC offset>
  operatorNotes: <exact deviations from HOW_TO_EXPORT.txt, or none>

Environment
  windowsVersion: <edition, version, build>
  foobar2000Path: <absolute path>
  foobar2000Version: <file/product version>
  foobar2000ExeSha256: <SHA-256>
  fooDrMeterPath: <absolute path>
  fooDrMeterVersion: 1.0.8
  fooDrMeterDllSha256: <SHA-256>

Plugin settings
  automaticallySaveTags: false
  stereoPerChannelStats: true
  albumLengthWeighting: false
  multichannelLoudnessWeighting: false
  otherRelevantSettings: <key=value, or none>

Raw reports
  01-core: <relative path and SHA-256>
  02-degenerate: <relative path and SHA-256>
  03-multichannel: <relative path and SHA-256>

PowerShell helpers
  Get-FileHash -Algorithm SHA256 .\\manifest.json
  Get-FileHash -Algorithm SHA256 'C:\\path\\to\\foobar2000.exe'
  Get-FileHash -Algorithm SHA256 'C:\\path\\to\\foo_dr_meter.dll'
  (Get-Item 'C:\\path\\to\\foobar2000.exe').VersionInfo |
    Format-List FileVersion,ProductVersion
  Get-ComputerInfo |
    Select-Object WindowsProductName,WindowsVersion,OsBuildNumber
  Get-TimeZone
"""


def instructions() -> str:
    return f"""foo_dr_meter 1.0.8 discriminating corpus v1

These WAV files are inputs, not golden results.

Before exporting:
  1. Disable "Automatically save tags".
  2. Disable both "Unofficial improvement" weighting options.
  3. Enable "Add per-channel stats also for stereo album logs".
  4. Keep the raw exported log unchanged.

Export three reports:
  A. Open playlists/01-core.m3u8, select all, run "Measure Dynamic Range".
  B. Open playlists/02-degenerate.m3u8 and do the same.
  C. Open playlists/03-multichannel.m3u8 and do the same.

In the result dialog use the 1.0.8 Copy Log action or save the log. Keep x86
and x64 reports separate. For the first x86 pass, use:
  reports/x86-run1-01-core.txt
  reports/x86-run1-02-degenerate.txt
  reports/x86-run1-03-multichannel.txt

For x64 replace "x86" with "x64". For repeat runs replace "run1" with "run2"
or "run3". One pass of 01-core is enough for an initial alignment check; a
reference observation should repeat every group three times and retain all raw
reports.

Fill one copy of RUN_METADATA_TEMPLATE.txt per architecture and repeat. Useful
PowerShell hash and version commands are included in that template.

Sample rate: {SAMPLE_RATE} Hz
Reference window coefficient: {WINDOW_DURATION_COEFFICIENT!r}
Expected window length from the recovered formula: {WINDOW_FRAMES} frames
"""


def generate(output_root: Path) -> None:
    fixtures = build_fixtures()
    case_records = []
    written_paths: list[Path] = []

    for order, fixture in enumerate(fixtures, start=1):
        path = output_root / fixture.report_group / fixture.filename
        file_hash, data_hash, byte_length = write_wave(path, fixture)
        written_paths.append(path)
        case_records.append(
            {
                "id": fixture.fixture_id,
                "order": order,
                "path": path.relative_to(output_root).as_posix(),
                "reportGroup": fixture.report_group,
                "executionSet": (
                    "degenerate-manual"
                    if fixture.report_group == "02-degenerate"
                    else "all-safe"
                ),
                "fileSha256": file_hash,
                "dataChunkSha256": data_hash,
                "byteLength": byte_length,
                "sampleFormat": "ieee-float32-le",
                "sampleRateHz": SAMPLE_RATE,
                "channels": len(fixture.channels),
                "channelMask": (
                    None
                    if fixture.channel_mask is None
                    else f"0x{fixture.channel_mask:08x}"
                ),
                "frames": len(fixture.channels[0]),
                "seed": None,
                "waveform": fixture.waveform,
                "generatedPcmWindowSummary": [
                    generated_window_summary(channel)
                    for channel in fixture.channels
                ],
                "question": fixture.question,
                "alternativeHypotheses": list(fixture.alternative_hypotheses),
            }
        )

    playlists = output_root / "playlists"
    playlists.mkdir(parents=True, exist_ok=True)
    for group in ("01-core", "02-degenerate", "03-multichannel"):
        group_fixtures = [
            fixture for fixture in fixtures if fixture.report_group == group
        ]
        playlist_path = playlists / f"{group}.m3u8"
        write_playlist(playlist_path, group_fixtures)
        written_paths.append(playlist_path)
    safe_playlist = playlists / "all-safe.m3u8"
    write_playlist(
        safe_playlist,
        [
            fixture
            for fixture in fixtures
            if fixture.report_group != "02-degenerate"
        ],
    )
    written_paths.append(safe_playlist)

    how_to = output_root / "HOW_TO_EXPORT.txt"
    how_to.write_text(instructions(), encoding="utf-8", newline="\n")
    written_paths.append(how_to)
    metadata = output_root / "RUN_METADATA_TEMPLATE.txt"
    metadata.write_text(metadata_template(), encoding="utf-8", newline="\n")
    written_paths.append(metadata)
    (output_root / "reports").mkdir(parents=True, exist_ok=True)

    source = Path(__file__).resolve()
    manifest = {
        "schemaVersion": 1,
        "corpusId": CORPUS_ID,
        "generator": {
            "name": source.name,
            "version": GENERATOR_VERSION,
            "sourceSha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "git": git_identity(),
            "runtime": {
                "implementation": platform.python_implementation(),
                "version": platform.python_version(),
                "platform": platform.platform(),
            },
            "command": (
                "python3 reference/tools/"
                "generate_foo_dr_meter_108_suite.py --output <OUTPUT>"
            ),
        },
        "targetFamily": {
            "component": "foo_dr_meter",
            "version": "1.0.8",
            "compatibilityClaim": "none; observations have not yet been collected",
        },
        "window": {
            "sampleRateHz": SAMPLE_RATE,
            "durationCoefficientF64Hex": WINDOW_DURATION_COEFFICIENT.hex(),
            "frames": WINDOW_FRAMES,
        },
        "cases": case_records,
    }
    manifest_path = output_root / "manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    written_paths.append(manifest_path)

    checksums = []
    for path in sorted(written_paths):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        checksums.append(f"{digest}  {path.relative_to(output_root).as_posix()}")
    (output_root / "FILES.sha256").write_text(
        "\n".join(checksums) + "\n", encoding="ascii", newline="\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="directory that will receive the generated corpus",
    )
    args = parser.parse_args()
    if args.output.exists() and not args.output.is_dir():
        parser.error("output path exists and is not a directory")
    if args.output.exists() and any(args.output.iterdir()):
        parser.error("output directory must be empty")
    args.output.mkdir(parents=True, exist_ok=True)
    generate(args.output)
    print(args.output.resolve())


if __name__ == "__main__":
    main()
