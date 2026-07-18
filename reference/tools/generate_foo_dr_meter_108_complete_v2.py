#!/usr/bin/env python3
"""Generate and verify the complete foo_dr_meter 1.0.8 v2 WAV probe corpus.

This is a new corpus generator.  It deliberately does not modify or replace
the v1 generator and manifest already named by a recorded observation.

Generation:

    python3 reference/tools/generate_foo_dr_meter_108_complete_v2.py \
      --output /tmp/foo-dr-meter-108-complete-v2

Verification:

    python3 reference/tools/generate_foo_dr_meter_108_complete_v2.py \
      --verify /tmp/foo-dr-meter-108-complete-v2
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import platform
import struct
import sys
from array import array
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, Sequence


CORPUS_ID = "foo-dr-meter-108-complete-v2"
GENERATOR_VERSION = 2
WINDOW_DURATION_COEFFICIENT = 3.0040816326530613
MAX_CORPUS_BYTES = 32 * 1024 * 1024
MAX_FILE_BYTES = 6 * 1024 * 1024
EXPECTED_CASES = 42
EXPECTED_SAFE_CASES = 39
ISOLATED_IDS = frozenset({"zero-frame", "overfull-f32", "overfull-f64"})

IEEE_FLOAT_SUBFORMAT = bytes.fromhex(
    "03000000" "0000" "1000" "8000" "00aa00389b71"
)
PCM_SUBFORMAT = bytes.fromhex(
    "01000000" "0000" "1000" "8000" "00aa00389b71"
)


@dataclass(frozen=True)
class Encoding:
    name: str
    bits: int
    kind: str

    @property
    def bytes_per_sample(self) -> int:
        return self.bits // 8


WAV_U8 = Encoding("wav-pcm-u8", 8, "unsigned")
WAV_S16 = Encoding("wav-pcm-s16le", 16, "signed")
WAV_S24 = Encoding("wav-pcm-s24le", 24, "signed")
WAV_S32 = Encoding("wav-pcm-s32le", 32, "signed")
WAV_F32 = Encoding("wav-ieee-float32le", 32, "float")
WAV_F64 = Encoding("wav-ieee-float64le", 64, "float")


@dataclass
class Fixture:
    fixture_id: str
    filename: str
    storage_group: str
    sample_rate: int
    channels: Sequence[array]
    encoding: Encoding = WAV_F32
    channel_mask: int | None = None
    question: str = ""
    alternatives: Sequence[str] = field(default_factory=tuple)
    discriminator: str = "control"
    waveform: dict[str, Any] = field(default_factory=dict)
    isolated: bool = False
    playlists: set[str] = field(default_factory=set)

    @property
    def relative_path(self) -> Path:
        return Path(self.storage_group) / self.filename

    @property
    def frame_count(self) -> int:
        return len(self.channels[0]) if self.channels else 0


@dataclass(frozen=True)
class ChannelResult:
    dr_internal: float
    dr_f32: float
    rounded_dr: int
    channel_rms: float
    channel_rms_f32: float
    primary_peak: float
    selected_peak: float
    loud_rms: float
    window_count: int
    nonzero_histogram_count: int
    rms_keys: tuple[int | None, ...]
    peak_keys: tuple[int | None, ...]


@dataclass(frozen=True)
class AnalysisResult:
    window_frames: int
    consumed_frames: int
    channels: tuple[ChannelResult, ...]
    track_dr_internal: float
    track_dr_f32: float
    rounded_track_dr: int
    report_peak: float
    report_rms: float


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def lround(value: float) -> int:
    if value >= 0.0:
        return math.floor(value + 0.5)
    return math.ceil(value - 0.5)


def samples(values: Iterable[float]) -> array:
    return array("d", (float(value) for value in values))


def constant(frames: int, amplitude: float) -> array:
    return samples(amplitude for _ in range(frames))


def join(*parts: array) -> array:
    output = array("d")
    for part in parts:
        output.extend(part)
    return output


def window_frames(sample_rate: int) -> int:
    return math.floor(sample_rate * WINDOW_DURATION_COEFFICIENT)


def shaped_window(
    frames: int,
    rms: float,
    peak: float,
    *,
    floor_override: float | None = None,
) -> array:
    if frames < 2:
        raise ValueError("a shaped window requires at least two frames")
    required_sum_squares = rms * rms * frames / 2.0
    remainder = required_sum_squares - peak * peak
    if remainder < 0.0:
        raise ValueError(f"RMS {rms} is too small for peak {peak} over {frames} frames")
    floor = (
        math.sqrt(remainder / (frames - 1))
        if floor_override is None
        else floor_override
    )
    output = array("d", [peak])
    output.extend(floor if frame % 2 else -floor for frame in range(1, frames))
    return output


def repeated_window(window: array, count: int) -> array:
    output = array("d")
    for _ in range(count):
        output.extend(window)
    return output


def quantize_signed(value: float, bits: int) -> int:
    scale = 1 << (bits - 1)
    return max(-scale, min(scale - 1, lround(value * scale)))


def quantize_unsigned_u8(value: float) -> int:
    return max(0, min(255, lround(value * 128.0) + 128))


def materialize_channel(channel: Sequence[float], encoding: Encoding) -> array:
    if encoding.kind == "float" and encoding.bits == 64:
        return samples(channel)
    if encoding.kind == "float" and encoding.bits == 32:
        return samples(f32(value) for value in channel)
    if encoding.kind == "unsigned":
        return samples((quantize_unsigned_u8(value) - 128) / 128.0 for value in channel)
    scale = float(1 << (encoding.bits - 1))
    return samples(quantize_signed(value, encoding.bits) / scale for value in channel)


def materialize_channels(fixture: Fixture) -> list[array]:
    return [materialize_channel(channel, fixture.encoding) for channel in fixture.channels]


def analyze(
    sample_rate: int,
    channels: Sequence[Sequence[float]],
    *,
    target_mode: str = "floor",
    histogram_mode: str = "db",
    lower_clamp: bool = True,
    upper_clamp: bool = True,
    whole_boundary_bin: bool = True,
    submit_tail: bool = True,
    count_all_zero_windows: bool = True,
    peak_rank: str = "quantized",
    negative_mode: str = "primary",
    aggregate_mode: str = "all",
    lfe_index: int | None = None,
    window_override: int | None = None,
) -> AnalysisResult | None:
    if not channels or any(len(channel) != len(channels[0]) for channel in channels):
        raise ValueError("analysis channels must be non-empty and frame-aligned")
    frame_count = len(channels[0])
    width = window_frames(sample_rate) if window_override is None else window_override
    if width <= 0:
        raise ValueError("window length must be positive")
    if frame_count == 0:
        return None

    channel_windows: list[list[tuple[float, float, float]]] = [
        [] for _ in channels
    ]
    common_window_count = 0
    for start in range(0, frame_count, width):
        end = min(start + width, frame_count)
        if end - start < width and not submit_tail:
            break
        all_zero = True
        pending: list[tuple[float, float, float]] = []
        for channel in channels:
            total = 0.0
            peak = 0.0
            for value in channel[start:end]:
                magnitude = abs(value)
                total += magnitude * magnitude
                if magnitude > peak:
                    peak = magnitude
            rms2 = 2.0 * total / (end - start)
            rms = math.sqrt(rms2)
            if peak != 0.0 or rms != 0.0:
                all_zero = False
            pending.append((rms2, rms, peak))
        if all_zero and not count_all_zero_windows:
            continue
        for windows, item in zip(channel_windows, pending):
            windows.append(item)
        common_window_count += 1

    if common_window_count == 0:
        return None

    results: list[ChannelResult] = []
    for windows in channel_windows:
        histogram: dict[int, int] = {}
        rms_keys: list[int | None] = []
        peak_candidates: list[tuple[float, int | float, int]] = []
        sum_rms2 = 0.0
        for order, (rms2, rms, peak) in enumerate(windows):
            sum_rms2 += rms2
            if rms == 0.0:
                rms_keys.append(None)
            else:
                if histogram_mode == "linear-trunc":
                    key = math.trunc(rms * 10_000.0)
                    key = max(0, min(10_000, key))
                else:
                    key = lround(2000.0 * math.log10(rms))
                    if lower_clamp:
                        key = max(-10000, key)
                    if upper_clamp:
                        key = min(0, key)
                rms_keys.append(key)
                histogram[key] = histogram.get(key, 0) + 1
            if peak > 0.0:
                rank: int | float
                if peak_rank == "raw":
                    rank = peak
                else:
                    rank = lround(2000.0 * math.log10(peak))
                peak_candidates.append((peak, rank, order))

        primary: tuple[float, int | float, int] | None = None
        secondary: tuple[float, int | float, int] | None = None
        for candidate in peak_candidates:
            if primary is None or candidate[1] > primary[1]:
                secondary = primary
                primary = candidate
            elif secondary is None or candidate[1] > secondary[1]:
                secondary = candidate

        n = len(windows)
        if target_mode == "ceil":
            target = max(1, math.ceil(n / 5.0))
        elif target_mode == "round":
            target = max(1, lround(n / 5.0))
        else:
            target = max(1, n // 5)
        selected_count = 0
        selected_power = 0.0
        for key in sorted(histogram, reverse=True):
            available = histogram[key]
            take = available
            if not whole_boundary_bin:
                take = min(available, target - selected_count)
            selected_count += take
            if histogram_mode == "linear-trunc":
                selected_power += ((key / 10_000.0) ** 2) * take
            else:
                selected_power += (10.0 ** (key / 1000.0)) * take
            if selected_count >= target:
                break

        primary_peak = primary[0] if primary else 0.0
        selected_peak = (
            secondary[0] if secondary is not None and secondary[0] > 0.0 else primary_peak
        )
        loud_rms = (
            math.sqrt(selected_power / selected_count) if selected_count else 0.0
        )
        dr = 0.0
        if selected_peak > 0.0 and loud_rms > 0.0:
            dr = -20.0 * math.log10(loud_rms / selected_peak)
            if dr < 0.0:
                if negative_mode == "primary" and primary_peak > 0.0:
                    dr = max(-20.0 * math.log10(loud_rms / primary_peak), 0.0)
                elif negative_mode == "zero":
                    dr = 0.0
        dr_public = f32(dr)
        channel_rms = math.sqrt(sum_rms2 / n)
        results.append(
            ChannelResult(
                dr_internal=dr,
                dr_f32=dr_public,
                rounded_dr=math.trunc(dr_public + 0.5),
                channel_rms=channel_rms,
                channel_rms_f32=f32(channel_rms),
                primary_peak=primary_peak,
                selected_peak=selected_peak,
                loud_rms=loud_rms,
                window_count=n,
                nonzero_histogram_count=sum(histogram.values()),
                rms_keys=tuple(rms_keys),
                peak_keys=tuple(
                    None
                    if peak == 0.0
                    else lround(2000.0 * math.log10(peak))
                    for _, _, peak in windows
                ),
            )
        )

    selected_indices = list(range(len(results)))
    if aggregate_mode == "exclude-zero":
        selected_indices = [
            index for index, result in enumerate(results) if result.dr_internal != 0.0
        ]
    elif aggregate_mode == "exclude-lfe" and lfe_index is not None:
        selected_indices = [index for index in selected_indices if index != lfe_index]
    if not selected_indices:
        track_internal = 0.0
    elif aggregate_mode == "rms-weighted":
        denominator = sum(results[index].channel_rms for index in selected_indices)
        track_internal = (
            sum(
                results[index].channel_rms * results[index].dr_internal
                for index in selected_indices
            )
            / denominator
            if denominator
            else 0.0
        )
    else:
        track_internal = (
            sum(results[index].dr_internal for index in selected_indices)
            / len(selected_indices)
        )
    track_public = f32(track_internal)
    report_peak = max(f32(result.primary_peak) for result in results)
    report_rms = math.sqrt(
        sum(
            f32(result.channel_rms_f32 * result.channel_rms_f32)
            for result in results
        )
        / len(results)
    )
    return AnalysisResult(
        window_frames=width,
        consumed_frames=frame_count,
        channels=tuple(results),
        track_dr_internal=track_internal,
        track_dr_f32=track_public,
        rounded_track_dr=math.trunc(track_public + 0.5),
        report_peak=report_peak,
        report_rms=report_rms,
    )


def chunk_bytes(chunk_id: bytes, payload: bytes) -> bytes:
    padding = b"\x00" if len(payload) % 2 else b""
    return chunk_id + struct.pack("<I", len(payload)) + payload + padding


def format_chunk(
    encoding: Encoding,
    channel_count: int,
    sample_rate: int,
    channel_mask: int | None,
) -> bytes:
    block_align = channel_count * encoding.bytes_per_sample
    byte_rate = sample_rate * block_align
    tag = 3 if encoding.kind == "float" else 1
    if channel_count <= 2 and channel_mask is None:
        return struct.pack(
            "<HHIIHH",
            tag,
            channel_count,
            sample_rate,
            byte_rate,
            block_align,
            encoding.bits,
        )
    if channel_mask is None:
        raise ValueError("multichannel WAV requires a channel mask")
    subformat = IEEE_FLOAT_SUBFORMAT if encoding.kind == "float" else PCM_SUBFORMAT
    return struct.pack(
        "<HHIIHHHHI16s",
        0xFFFE,
        channel_count,
        sample_rate,
        byte_rate,
        block_align,
        encoding.bits,
        22,
        encoding.bits,
        channel_mask,
        subformat,
    )


def encode_value(value: float, encoding: Encoding) -> bytes:
    if encoding.kind == "float":
        return struct.pack("<f" if encoding.bits == 32 else "<d", value)
    if encoding.kind == "unsigned":
        return bytes([quantize_unsigned_u8(value)])
    integer = quantize_signed(value, encoding.bits)
    return integer.to_bytes(encoding.bytes_per_sample, "little", signed=True)


def write_wave(path: Path, fixture: Fixture) -> tuple[int, str]:
    channel_count = len(fixture.channels)
    if channel_count == 0:
        raise ValueError(f"{fixture.fixture_id}: no channels")
    if any(len(channel) != fixture.frame_count for channel in fixture.channels):
        raise ValueError(f"{fixture.fixture_id}: channel frame counts differ")
    if any(
        not math.isfinite(value)
        for channel in fixture.channels
        for value in channel
    ):
        raise ValueError(f"{fixture.fixture_id}: non-finite PCM")
    fmt = chunk_bytes(
        b"fmt ",
        format_chunk(
            fixture.encoding,
            channel_count,
            fixture.sample_rate,
            fixture.channel_mask,
        ),
    )
    materialized = materialize_channels(fixture)
    data_size = fixture.frame_count * channel_count * fixture.encoding.bytes_per_sample
    fact = (
        chunk_bytes(b"fact", struct.pack("<I", fixture.frame_count))
        if fixture.encoding.kind == "float"
        else b""
    )
    riff_size = 4 + len(fmt) + len(fact) + 8 + data_size
    if riff_size > 0xFFFF_FFFF:
        raise ValueError(f"{fixture.fixture_id}: RIFF32 overflow")

    path.parent.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256()
    with path.open("wb") as output:
        header = (
            b"RIFF"
            + struct.pack("<I", riff_size)
            + b"WAVE"
            + fmt
            + fact
            + b"data"
            + struct.pack("<I", data_size)
        )
        output.write(header)
        digest.update(header)
        for start in range(0, fixture.frame_count, 4096):
            end = min(start + 4096, fixture.frame_count)
            block = bytearray()
            for frame in range(start, end):
                for channel in materialized:
                    block.extend(encode_value(channel[frame], fixture.encoding))
            output.write(block)
            digest.update(block)
    return path.stat().st_size, digest.hexdigest()


def parse_riff_wave(path: Path) -> dict[str, Any]:
    data = path.read_bytes()
    if len(data) < 12 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError(f"{path}: not RIFF/WAVE")
    declared = struct.unpack_from("<I", data, 4)[0] + 8
    if declared != len(data):
        raise ValueError(f"{path}: RIFF length {declared} != {len(data)}")
    cursor = 12
    fmt_payload: bytes | None = None
    data_payload: bytes | None = None
    while cursor < len(data):
        if cursor + 8 > len(data):
            raise ValueError(f"{path}: truncated chunk header")
        chunk_id = data[cursor : cursor + 4]
        size = struct.unpack_from("<I", data, cursor + 4)[0]
        cursor += 8
        end = cursor + size
        if end > len(data):
            raise ValueError(f"{path}: truncated {chunk_id!r} chunk")
        payload = data[cursor:end]
        if chunk_id == b"fmt ":
            if fmt_payload is not None:
                raise ValueError(f"{path}: duplicate fmt chunk")
            fmt_payload = payload
        elif chunk_id == b"data":
            if data_payload is not None:
                raise ValueError(f"{path}: duplicate data chunk")
            data_payload = payload
        cursor = end + (size & 1)
    if fmt_payload is None or data_payload is None or len(fmt_payload) < 16:
        raise ValueError(f"{path}: missing fmt or data chunk")
    tag, channels, rate, byte_rate, block_align, bits = struct.unpack_from(
        "<HHIIHH", fmt_payload
    )
    if block_align == 0 or len(data_payload) % block_align:
        raise ValueError(f"{path}: non-frame-aligned data")
    return {
        "formatTag": tag,
        "channels": channels,
        "sampleRateHz": rate,
        "byteRate": byte_rate,
        "blockAlign": block_align,
        "bitsPerSample": bits,
        "frames": len(data_payload) // block_align,
        "dataSha256": hashlib.sha256(data_payload).hexdigest(),
        "fileSha256": hashlib.sha256(data).hexdigest(),
        "byteLength": len(data),
    }


def load_v1_fixtures() -> list[Fixture]:
    source = Path(__file__).with_name("generate_foo_dr_meter_108_suite.py")
    spec = importlib.util.spec_from_file_location("_foo_dr_meter_v1_generator", source)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {source}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    fixtures = []
    for legacy in module.build_fixtures():
        fixture = Fixture(
            fixture_id=legacy.fixture_id,
            filename=legacy.filename,
            storage_group=legacy.report_group,
            sample_rate=module.SAMPLE_RATE,
            channels=[samples(channel) for channel in legacy.channels],
            encoding=WAV_F32,
            channel_mask=legacy.channel_mask,
            question=legacy.question,
            alternatives=legacy.alternative_hypotheses,
            discriminator=f"v1:{legacy.fixture_id}",
            waveform=dict(legacy.waveform),
            playlists={legacy.report_group},
        )
        fixtures.append(fixture)
    return fixtures


def f32_cell(value: float) -> tuple[float, float, float]:
    rounded = f32(value)
    bits = struct.unpack("<I", struct.pack("<f", rounded))[0]
    if rounded <= 0.0 or bits == 0 or bits >= 0x7F7F_FFFF:
        raise ValueError("f32_cell currently supports positive normal values")
    previous = struct.unpack("<f", struct.pack("<I", bits - 1))[0]
    following = struct.unpack("<f", struct.pack("<I", bits + 1))[0]
    return (previous + rounded) / 2.0, rounded, (rounded + following) / 2.0


def find_rms_half_collapse(width: int) -> tuple[array, array, dict[str, Any]]:
    """Find two f64 windows across an RMS centi-dB half that collapse to one f32."""

    for centi in range(800, 3200):
        threshold_db = -(centi + 0.5) / 100.0
        target_rms = 10.0 ** (threshold_db / 20.0)
        required = target_rms * target_rms * width / 2.0 - 1.0
        if required <= 0.0:
            continue
        floor_at_threshold = math.sqrt(required / (width - 1))
        lower, rounded, upper = f32_cell(floor_at_threshold)
        margin = min(floor_at_threshold - lower, upper - floor_at_threshold)
        if margin <= abs(floor_at_threshold) * 1e-10:
            continue
        low_floor = floor_at_threshold - margin * 0.6
        high_floor = floor_at_threshold + margin * 0.6
        if f32(low_floor) != f32(high_floor):
            continue
        low = shaped_window(width, target_rms, 1.0, floor_override=low_floor)
        high = shaped_window(width, target_rms, 1.0, floor_override=high_floor)
        probe = analyze(8_000, [repeated_window(low, 2), repeated_window(high, 2)])
        if probe is None:
            continue
        keys = [result.rms_keys[0] for result in probe.channels]
        if keys[0] == keys[1]:
            continue
        downcast = analyze(
            8_000,
            [
                materialize_channel(repeated_window(low, 2), WAV_F32),
                materialize_channel(repeated_window(high, 2), WAV_F32),
            ],
        )
        if downcast is None or downcast.channels[0].rms_keys != downcast.channels[1].rms_keys:
            continue
        return low, high, {
            "thresholdDb": threshold_db,
            "floorF64Hex": floor_at_threshold.hex(),
            "collapsedFloorF32Hex": float(rounded).hex(),
            "sourceKeys": keys,
            "downcastKey": downcast.channels[0].rms_keys[0],
        }
    raise RuntimeError("could not find an RMS half-boundary f64/f32 collapse witness")


def find_peak_half_collapse(width: int) -> tuple[float, float, float, dict[str, Any]]:
    """Find peak amplitudes across a centi-dB half that collapse to one f32."""

    for centi in range(100, 2000):
        threshold_db = -(centi + 0.5) / 100.0
        threshold = 10.0 ** (threshold_db / 20.0)
        lower, rounded, upper = f32_cell(threshold)
        margin = min(threshold - lower, upper - threshold)
        if margin <= threshold * 1e-10:
            continue
        low = threshold - margin * 0.6
        high = threshold + margin * 0.6
        if f32(low) != f32(high):
            continue
        low_key = lround(2000.0 * math.log10(low))
        high_key = lround(2000.0 * math.log10(high))
        if low_key == high_key:
            continue
        # Put the control near the low-key bin's lower-amplitude edge.  When
        # the test peak remains in the low key, strict tie handling preserves
        # this earlier control.  When the test crosses into the high key, it
        # replaces the control.  The nearly one-cent difference remains
        # visible in the plugin's two-decimal per-channel DR column.
        control_db = low_key / 100.0 - 0.0048
        control = 10.0 ** (control_db / 20.0)
        control_key = lround(2000.0 * math.log10(control))
        if control_key != low_key:
            continue
        return low, high, control, {
            "thresholdDb": threshold_db,
            "collapsedPeakF32Hex": float(rounded).hex(),
            "lowKey": low_key,
            "highKey": high_key,
            "controlKey": control_key,
            "controlDb": control_db,
        }
    raise RuntimeError("could not find a peak half-boundary f64/f32 collapse witness")


def dr_signal(
    sample_rate: int,
    target_dr: float,
    *,
    channels: int = 1,
    phase_flip: bool = False,
    channel_mask: int | None = None,
) -> tuple[list[array], int | None]:
    width = window_frames(sample_rate)
    loud_rms = 0.1
    peak = loud_rms * (10.0 ** (target_dr / 20.0))
    window = shaped_window(width, loud_rms, peak)
    if phase_flip:
        window[0] = -window[0]
    payload = repeated_window(window, 2)
    return [array("d", payload) for _ in range(channels)], channel_mask


def build_v2_fixtures() -> list[Fixture]:
    fixtures = load_v1_fixtures()
    width = window_frames(8_000)

    # Histogram lower clamp: candidate rounds to DR39, an unclamped histogram
    # rounds to DR40.  All RMS values remain finite and within normalized PCM.
    clamp_windows = [
        shaped_window(width, 9.65e-6, 0.0010),
        shaped_window(width, 9.40e-6, 0.000927),
        shaped_window(width, 8.80e-6, 0.00085),
        shaped_window(width, 8.60e-6, 0.00082),
        shaped_window(width, 8.40e-6, 0.00080),
    ]
    numeric: list[Fixture] = [
        Fixture(
            "histogram-lower-clamp",
            "401_histogram_lower_clamp.wav",
            "04-numeric",
            8_000,
            [join(*clamp_windows)],
            question="Does RMS below -100 dB enter the endpoint histogram bin?",
            alternatives=("Clamp to -100 dB.", "Retain lower centi-dB keys."),
            discriminator="lower-clamp",
            waveform={"kind": "lower-clamp-ranked-windows", "windowCount": 5},
            playlists={"04-numeric"},
        ),
        Fixture(
            "loud-target-n9",
            "402_loud_target_n9.wav",
            "04-numeric",
            8_000,
            [
                join(
                    shaped_window(width, 0.20, 0.5),
                    shaped_window(width, 0.10, 0.5),
                    *(shaped_window(width, 0.02, 0.5) for _ in range(7)),
                )
            ],
            question="At N=9, is the loud target floor(N/5)=1 rather than 2?",
            alternatives=("floor gives one.", "round or ceil gives two."),
            discriminator="target-n9",
            waveform={"kind": "n9-target-boundary", "windowCount": 9},
            playlists={"04-numeric"},
        ),
        Fixture(
            "sparse-nonzero-among-zero",
            "403_sparse_nonzero_among_zero.wav",
            "04-numeric",
            8_000,
            [
                join(
                    shaped_window(width, 0.10, 1.0),
                    *(constant(width, 0.0) for _ in range(9)),
                )
            ],
            question="Do zero windows count in N while remaining absent from the histogram?",
            alternatives=(
                "N=10 and the one nonzero histogram item is used.",
                "Zero windows are removed from N.",
            ),
            discriminator="sparse-zero",
            waveform={"kind": "one-nonzero-nine-zero", "windowCount": 10},
            playlists={"04-numeric"},
        ),
    ]

    rms_low, rms_high, rms_recipe = find_rms_half_collapse(width)
    rms_source_channels = [
        repeated_window(rms_low, 2),
        repeated_window(rms_high, 2),
    ]
    numeric.extend(
        [
            Fixture(
                "rms-half-f64-stereo",
                "410_rms_half_f64_stereo.wav",
                "04-numeric",
                8_000,
                rms_source_channels,
                encoding=WAV_F64,
                question="Do source-f64 channels straddle an RMS centi-dB half?",
                alternatives=("Distinct adjacent RMS keys.", "One shared key."),
                discriminator="rms-half-f64",
                waveform={"kind": "rms-half-collapse", **rms_recipe},
                playlists={"04-numeric"},
            ),
            Fixture(
                "rms-half-f32-stereo",
                "411_rms_half_f32_stereo.wav",
                "04-numeric",
                8_000,
                rms_source_channels,
                encoding=WAV_F32,
                question="Does f32 encoding collapse the paired RMS half witness?",
                alternatives=("Both channels collapse.", "They remain distinct."),
                discriminator="rms-half-f32",
                waveform={"kind": "rms-half-collapse", **rms_recipe},
                playlists={"04-numeric"},
            ),
        ]
    )

    peak_low, peak_high, peak_control, peak_recipe = find_peak_half_collapse(width)

    def peak_channel(test_peak: float) -> array:
        rms = 0.1
        return join(
            shaped_window(width, rms, 1.0),
            shaped_window(width, rms, peak_control),
            shaped_window(width, rms, test_peak),
            shaped_window(width, rms, 0.5),
            shaped_window(width, rms, 0.4),
        )

    peak_source_channels = [peak_channel(peak_low), peak_channel(peak_high)]
    numeric.extend(
        [
            Fixture(
                "peak-half-f64-stereo",
                "420_peak_half_f64_stereo.wav",
                "04-numeric",
                8_000,
                peak_source_channels,
                encoding=WAV_F64,
                question="Does the peak key use centi-dB lround at the half boundary?",
                alternatives=("The two channels select different secondary peaks.", "Same rank."),
                discriminator="peak-half-f64",
                waveform={"kind": "peak-half-collapse", **peak_recipe},
                playlists={"04-numeric"},
            ),
            Fixture(
                "peak-half-f32-stereo",
                "421_peak_half_f32_stereo.wav",
                "04-numeric",
                8_000,
                peak_source_channels,
                encoding=WAV_F32,
                question="Does f32 encoding collapse the paired peak half witness?",
                alternatives=("Both channels collapse.", "They remain distinct."),
                discriminator="peak-half-f32",
                waveform={"kind": "peak-half-collapse", **peak_recipe},
                playlists={"04-numeric"},
            ),
        ]
    )
    fixtures.extend(numeric)

    for rate, prefix in ((44_100, "44100"), (48_000, "48000")):
        rate_width = window_frames(rate)
        fixtures.extend(
            [
                Fixture(
                    f"sr-{prefix}-w-minus-one",
                    f"50{1 if rate == 44_100 else 3}_sr_{prefix}_w_minus_one.wav",
                    "05-samplerates",
                    rate,
                    [shaped_window(rate_width - 1, 0.25, 1.0)],
                    question=f"Is a W-1 tail submitted at {rate} Hz?",
                    alternatives=("Submitted.", "Discarded."),
                    discriminator="sample-rate-control",
                    waveform={"kind": "w-minus-one", "windowFrames": rate_width},
                    playlists={"05-samplerates"},
                ),
                Fixture(
                    f"sr-{prefix}-w-plus-one",
                    f"50{2 if rate == 44_100 else 4}_sr_{prefix}_w_plus_one.wav",
                    "05-samplerates",
                    rate,
                    [
                        join(
                            shaped_window(rate_width, 0.10, 1.0),
                            samples([0.5]),
                        )
                    ],
                    question=f"Does floor(coefficient*rate) split W+1 at {rate} Hz?",
                    alternatives=("One full window plus one-frame tail.", "One rounded-width window."),
                    discriminator="sample-rate-split",
                    waveform={"kind": "w-plus-one-transition", "windowFrames": rate_width},
                    playlists={"05-samplerates"},
                ),
            ]
        )

    report_channels = []
    report_drs = (4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 28.0, 32.0)
    report_peaks = (0.95, 0.85, 0.75, 0.65, 0.55, 0.45, 0.35, 0.25)
    for target_dr, peak in zip(report_drs, report_peaks):
        loud = peak / (10.0 ** (target_dr / 20.0))
        report_channels.append(repeated_window(shaped_window(width, loud, peak), 2))
    fixtures.append(
        Fixture(
            "eight-channel-report-map",
            "601_eight_channel_report_map.wav",
            "06-precision-report",
            8_000,
            report_channels,
            channel_mask=0x0000_063F,
            question="What are the 7.1 column order, report peak/RMS, and track mean?",
            alternatives=("All eight mapped channels contribute.", "A layout-specific subset contributes."),
            discriminator="eight-channel-report",
            waveform={
                "kind": "distinct-eight-channel",
                "speakerMask": "0x0000063f",
                "channelOrder": ["FL", "FR", "FC", "LFE", "BL", "BR", "SL", "SR"],
                "targetDrDb": list(report_drs),
                "peak": list(report_peaks),
            },
            playlists={"06-precision-report"},
        )
    )

    for fixture_id, filename, target, phase in (
        ("album-10-49-a", "701_album_10_49_a.wav", 10.49, False),
        ("album-10-49-b", "702_album_10_49_b.wav", 10.49, True),
        ("album-11-49", "703_album_11_49.wav", 11.49, False),
        ("display-10-50", "704_display_10_50.wav", 10.50, False),
    ):
        channels, mask = dr_signal(8_000, target, phase_flip=phase)
        fixtures.append(
            Fixture(
                fixture_id,
                filename,
                "07-album",
                8_000,
                channels,
                channel_mask=mask,
                question=f"Track designed around DR{target:.2f} for album/display discrimination.",
                alternatives=("Aggregate precise track values.", "Aggregate displayed integers."),
                discriminator="album-track",
                waveform={"kind": "album-rounding-track", "targetDrDb": target},
                playlists=set(),
            )
        )

    for fixture_id, filename, target in (
        ("aggregate-narrow-low", "610_aggregate_narrow_low.wav", 12.499),
        ("aggregate-narrow-high", "611_aggregate_narrow_high.wav", 12.501),
    ):
        channels, _ = dr_signal(8_000, target, channels=3, channel_mask=0x7)
        fixtures.append(
            Fixture(
                fixture_id,
                filename,
                "06-precision-report",
                8_000,
                channels,
                channel_mask=0x7,
                question="Does nonnegative track DR cross the +0.5 display boundary?",
                alternatives=("Low and high witnesses display differently.", "Both truncate alike."),
                discriminator="aggregate-display-boundary",
                waveform={"kind": "three-channel-display-boundary", "targetDrDb": target},
                playlists={"06-precision-report"},
            )
        )

    codec_width = width
    base_window = array("d", [127.0 / 128.0])
    base_window.extend(
        (32.0 / 128.0) if frame % 2 else -(32.0 / 128.0)
        for frame in range(1, codec_width)
    )
    codec_signal = repeated_window(base_window, 2)
    for order, (suffix, encoding) in enumerate(
        (
            ("u8", WAV_U8),
            ("s16", WAV_S16),
            ("s24", WAV_S24),
            ("s32", WAV_S32),
            ("f32", WAV_F32),
            ("f64", WAV_F64),
        ),
        start=1,
    ):
        fixtures.append(
            Fixture(
                f"host-decode-{suffix}",
                f"80{order}_host_decode_{suffix}.wav",
                "08-host-decode",
                8_000,
                [array("d", codec_signal)],
                encoding=encoding,
                question=f"Does the host deliver the shared dyadic waveform from {encoding.name}?",
                alternatives=("Report-equivalent decoded PCM.", "Format-specific divergence."),
                discriminator="host-decode-equivalence",
                waveform={
                    "kind": "shared-dyadic-codec-signal",
                    "baseCodeBits": 8,
                    "encoding": encoding.name,
                },
                playlists={"08-host-decode"},
            )
        )

    overfull = repeated_window(shaped_window(width, 1.10, 2.0), 2)
    fixtures.extend(
        [
            Fixture(
                "zero-frame",
                "901_zero_frame.wav",
                "99-isolated",
                8_000,
                [array("d")],
                encoding=WAV_F32,
                question="What is the host/UI behavior for a valid zero-frame WAV?",
                alternatives=("Rejected or omitted before analysis.", "Reported as a numeric track."),
                discriminator="zero-frame-host",
                waveform={"kind": "zero-frame"},
                isolated=True,
                playlists={"99-zero-frame"},
            ),
            Fixture(
                "overfull-f32",
                "902_overfull_f32.wav",
                "99-isolated",
                8_000,
                [array("d", overfull)],
                encoding=WAV_F32,
                question="Is the 0 dB histogram clamp observable on finite overfull float32 PCM?",
                alternatives=("Clamp loud RMS to 0 dB.", "Retain positive RMS keys."),
                discriminator="upper-clamp-overfull",
                waveform={"kind": "overfull-upper-clamp", "targetRms": 1.10, "peak": 2.0},
                isolated=True,
                playlists={"99-overfull-f32"},
            ),
            Fixture(
                "overfull-f64",
                "903_overfull_f64.wav",
                "99-isolated",
                8_000,
                [array("d", overfull)],
                encoding=WAV_F64,
                question="Is the 0 dB histogram clamp observable on finite overfull float64 PCM?",
                alternatives=("Clamp loud RMS to 0 dB.", "Retain positive RMS keys."),
                discriminator="upper-clamp-overfull",
                waveform={"kind": "overfull-upper-clamp", "targetRms": 1.10, "peak": 2.0},
                isolated=True,
                playlists={"99-overfull-f64"},
            ),
        ]
    )

    by_id = {fixture.fixture_id: fixture for fixture in fixtures}
    by_id["album-10-49-a"].playlists.update(
        {"07-album-source", "07-album-silence", "07-display-half"}
    )
    by_id["album-10-49-b"].playlists.add("07-album-source")
    by_id["album-11-49"].playlists.add("07-album-source")
    by_id["display-10-50"].playlists.add("07-display-half")
    by_id["silent-mono"].playlists.add("07-album-silence")

    safe_ids = [fixture.fixture_id for fixture in fixtures if not fixture.isolated]
    if len(fixtures) != EXPECTED_CASES:
        raise AssertionError(f"expected {EXPECTED_CASES} cases, built {len(fixtures)}")
    if len(safe_ids) != EXPECTED_SAFE_CASES:
        raise AssertionError(
            f"expected {EXPECTED_SAFE_CASES} safe cases, built {len(safe_ids)}"
        )
    if {fixture.fixture_id for fixture in fixtures if fixture.isolated} != ISOLATED_IDS:
        raise AssertionError("isolated fixture set drifted")
    if len(by_id) != len(fixtures):
        raise AssertionError("duplicate fixture ID")
    return fixtures


def pcm_f64le_sha256(channels: Sequence[Sequence[float]]) -> str:
    digest = hashlib.sha256()
    if not channels:
        return digest.hexdigest()
    for frame in range(len(channels[0])):
        for channel in channels:
            digest.update(struct.pack("<d", float(channel[frame])))
    return digest.hexdigest()


def result_record(result: AnalysisResult | None) -> dict[str, Any] | None:
    if result is None:
        return None
    return {
        "windowFrames": result.window_frames,
        "consumedFrames": result.consumed_frames,
        "trackDrInternalF64Hex": result.track_dr_internal.hex(),
        "trackDrF32Hex": float(result.track_dr_f32).hex(),
        "roundedTrackDr": result.rounded_track_dr,
        "reportPeakF64Hex": result.report_peak.hex(),
        "reportRmsF64Hex": result.report_rms.hex(),
        "channels": [
            {
                "drInternalF64Hex": channel.dr_internal.hex(),
                "drF32Hex": float(channel.dr_f32).hex(),
                "roundedDr": channel.rounded_dr,
                "channelRmsF64Hex": channel.channel_rms.hex(),
                "channelRmsF32Hex": float(channel.channel_rms_f32).hex(),
                "primaryPeakF64Hex": channel.primary_peak.hex(),
                "selectedPeakF64Hex": channel.selected_peak.hex(),
                "loudRmsF64Hex": channel.loud_rms.hex(),
                "windowCount": channel.window_count,
                "nonzeroHistogramCount": channel.nonzero_histogram_count,
                "rmsKeysCentiDb": list(channel.rms_keys),
                "peakKeysCentiDb": list(channel.peak_keys),
            }
            for channel in result.channels
        ],
    }


def analyze_fixture(fixture: Fixture, **options: Any) -> AnalysisResult | None:
    return analyze(
        fixture.sample_rate,
        materialize_channels(fixture),
        lfe_index=3 if fixture.channel_mask == 0x3F else None,
        **options,
    )


def fixture_classification(fixture: Fixture) -> dict[str, str]:
    discriminator = fixture.discriminator
    if discriminator.startswith("v1:"):
        question_class = "algorithm_core"
        architecture_scope = "x86-observed-and-x64-static-control"
        evidence_intent = "previous_observation_and_static_regression"
    elif discriminator in {
        "lower-clamp",
        "target-n9",
        "sparse-zero",
        "sample-rate-control",
        "sample-rate-split",
    }:
        question_class = "numeric_boundary"
        architecture_scope = "x86-and-x64-shared-control"
        evidence_intent = "static_regression"
    elif discriminator.startswith(("rms-half-", "peak-half-")):
        question_class = "architecture_discriminator"
        architecture_scope = (
            "x64-f64-core"
            if discriminator.endswith("-f64")
            else "f32-input-control"
        )
        evidence_intent = "architecture_discriminator"
    elif discriminator == "eight-channel-report":
        question_class = "report_and_track_aggregate"
        architecture_scope = "x86-and-x64-shared-control"
        evidence_intent = "static_regression"
    elif discriminator == "album-track":
        question_class = "album_aggregate"
        architecture_scope = "x86-and-x64-shared-control"
        evidence_intent = "static_regression"
    elif discriminator == "aggregate-display-boundary":
        question_class = "track_aggregate"
        architecture_scope = "x86-and-x64-shared-control"
        evidence_intent = "static_regression"
    elif discriminator == "host-decode-equivalence":
        question_class = "host_decode"
        architecture_scope = "foobar2000-2.0-x64-wave-decoder"
        evidence_intent = "static_decoder_regression"
    elif discriminator in {"zero-frame-host", "upper-clamp-overfull"}:
        question_class = "host_edge"
        architecture_scope = "host-dependent"
        evidence_intent = "isolated_host_edge"
    else:
        raise AssertionError(
            f"{fixture.fixture_id}: unclassified discriminator {discriminator!r}"
        )
    return {
        "discriminatorId": discriminator,
        "questionClass": question_class,
        "architectureScope": architecture_scope,
        "evidenceIntent": evidence_intent,
    }


def assert_discriminators(
    fixtures: Sequence[Fixture],
    candidate: dict[str, AnalysisResult | None],
) -> dict[str, list[dict[str, Any]]]:
    by_id = {fixture.fixture_id: fixture for fixture in fixtures}
    evidence: dict[str, list[dict[str, Any]]] = {
        fixture.fixture_id: [] for fixture in fixtures
    }

    def add(case_id: str, assertion: str, **observed: Any) -> None:
        evidence[case_id].append({"assertion": assertion, "observed": observed})

    def require(condition: bool, message: str) -> None:
        if not condition:
            raise AssertionError(message)

    expected_v1 = {
        "window-minus-one-control": 12,
        "exact-window-control": 12,
        "tail-pair-base": 19,
        "tail-pair-plus-one": 2,
        "negative-dr-fallback": 2,
        "histogram-db-domain": 39,
        "loud-boundary-bin-ties": 12,
        "peak-order-low-then-high": 13,
        "peak-order-high-then-low": 12,
        "one-frame-nonzero": 0,
        "two-frame-negative": 0,
        "silent-mono": 0,
        "stereo-silent-channel": 6,
        "three-channel-arithmetic": 20,
        "six-channel-lfe": 15,
    }
    for case_id, expected in expected_v1.items():
        result = candidate[case_id]
        require(result is not None, f"{case_id}: candidate produced no result")
        require(
            result.rounded_track_dr == expected,
            f"{case_id}: expected DR{expected}, got DR{result.rounded_track_dr}",
        )
        add(case_id, "candidate reproduces the registered v1 track DR", roundedDr=expected)

    for case_id in ("window-minus-one-control", "one-frame-nonzero"):
        discarded = analyze_fixture(by_id[case_id], submit_tail=False)
        require(discarded is None, f"{case_id}: discard-tail alternative remained measurable")
        add(case_id, "submitted tail differs from discarded-tail hypothesis")

    exact = candidate["exact-window-control"]
    require(exact is not None and exact.channels[0].window_count == 1, "exact window drift")
    add("exact-window-control", "exact W submits exactly one window", windowCount=1)

    base = candidate["tail-pair-base"]
    plus = candidate["tail-pair-plus-one"]
    plus_discarded = analyze_fixture(by_id["tail-pair-plus-one"], submit_tail=False)
    require(base is not None and plus is not None and plus_discarded is not None, "tail pair")
    require(
        plus.rounded_track_dr != base.rounded_track_dr
        and plus_discarded.rounded_track_dr == base.rounded_track_dr,
        "one-frame tail does not discriminate",
    )
    add(
        "tail-pair-base",
        "paired control differs only when the appended tail is submitted",
        control=base.rounded_track_dr,
    )
    add(
        "tail-pair-plus-one",
        "one-frame tail changes the result",
        submitted=plus.rounded_track_dr,
        discarded=plus_discarded.rounded_track_dr,
    )

    negative = candidate["negative-dr-fallback"]
    negative_zero = analyze_fixture(by_id["negative-dr-fallback"], negative_mode="zero")
    require(negative is not None and negative_zero is not None, "negative fallback")
    require(
        negative.rounded_track_dr != negative_zero.rounded_track_dr,
        "primary fallback equals direct-zero alternative",
    )
    add(
        "negative-dr-fallback",
        "primary recomputation differs from direct zero clamp",
        primaryFallback=negative.rounded_track_dr,
        directZero=negative_zero.rounded_track_dr,
    )

    linear = analyze_fixture(by_id["histogram-db-domain"], histogram_mode="linear-trunc")
    db_result = candidate["histogram-db-domain"]
    require(linear is not None and db_result is not None, "histogram domain")
    require(
        linear.rounded_track_dr != db_result.rounded_track_dr,
        "histogram-domain fixture does not discriminate",
    )
    add(
        "histogram-db-domain",
        "dB-bin and linear-trunc hypotheses differ",
        dbBins=db_result.rounded_track_dr,
        linearBins=linear.rounded_track_dr,
    )

    partial = analyze_fixture(by_id["loud-boundary-bin-ties"], whole_boundary_bin=False)
    whole = candidate["loud-boundary-bin-ties"]
    require(partial is not None and whole is not None, "boundary ties")
    require(
        partial.rounded_track_dr != whole.rounded_track_dr,
        "boundary tie fixture does not discriminate",
    )
    add(
        "loud-boundary-bin-ties",
        "whole boundary bin differs from taking only the required count",
        wholeBin=whole.rounded_track_dr,
        partialBin=partial.rounded_track_dr,
    )

    peak_low_order = candidate["peak-order-low-then-high"]
    peak_high_order = candidate["peak-order-high-then-low"]
    require(
        peak_low_order is not None
        and peak_high_order is not None
        and peak_low_order.rounded_track_dr != peak_high_order.rounded_track_dr,
        "peak order pair does not discriminate",
    )
    for case_id, result in (
        ("peak-order-low-then-high", peak_low_order),
        ("peak-order-high-then-low", peak_high_order),
    ):
        add(
            case_id,
            "reversing quantized-equal peak arrival order changes the paired result",
            roundedDr=result.rounded_track_dr,
        )

    preserved = analyze_fixture(by_id["two-frame-negative"], negative_mode="preserve")
    clamped = candidate["two-frame-negative"]
    require(
        preserved is not None
        and clamped is not None
        and preserved.track_dr_internal < 0.0
        and clamped.track_dr_internal == 0.0,
        "two-frame negative clamp does not discriminate",
    )
    add(
        "two-frame-negative",
        "negative raw DR differs from the zero floor",
        rawF64Hex=preserved.track_dr_internal.hex(),
        clamped=0,
    )

    silent = candidate["silent-mono"]
    require(
        silent is not None
        and silent.track_dr_internal == 0.0
        and silent.channels[0].nonzero_histogram_count == 0,
        "silent numeric path drifted",
    )
    add("silent-mono", "silent track is a numeric DR0 with no histogram entries")

    excluded_silent = analyze_fixture(
        by_id["stereo-silent-channel"], aggregate_mode="exclude-zero"
    )
    included_silent = candidate["stereo-silent-channel"]
    require(
        excluded_silent is not None
        and included_silent is not None
        and excluded_silent.rounded_track_dr != included_silent.rounded_track_dr,
        "silent-channel aggregate fixture does not discriminate",
    )
    add(
        "stereo-silent-channel",
        "including numeric silent DR0 differs from excluding it",
        included=included_silent.rounded_track_dr,
        excluded=excluded_silent.rounded_track_dr,
    )

    weighted = analyze_fixture(
        by_id["three-channel-arithmetic"], aggregate_mode="rms-weighted"
    )
    arithmetic = candidate["three-channel-arithmetic"]
    require(
        weighted is not None
        and arithmetic is not None
        and weighted.rounded_track_dr != arithmetic.rounded_track_dr,
        "multichannel weighting fixture does not discriminate",
    )
    add(
        "three-channel-arithmetic",
        "arithmetic and channel-RMS-weighted track values differ",
        arithmetic=arithmetic.rounded_track_dr,
        rmsWeighted=weighted.rounded_track_dr,
    )

    without_lfe = analyze_fixture(
        by_id["six-channel-lfe"], aggregate_mode="exclude-lfe"
    )
    with_lfe = candidate["six-channel-lfe"]
    require(
        without_lfe is not None
        and with_lfe is not None
        and without_lfe.rounded_track_dr != with_lfe.rounded_track_dr,
        "LFE fixture does not discriminate",
    )
    add(
        "six-channel-lfe",
        "including and excluding LFE differ",
        included=with_lfe.rounded_track_dr,
        excluded=without_lfe.rounded_track_dr,
    )

    lower_clamped = candidate["histogram-lower-clamp"]
    lower_open = analyze_fixture(by_id["histogram-lower-clamp"], lower_clamp=False)
    require(lower_clamped is not None and lower_open is not None, "lower clamp")
    require(
        lower_clamped.rounded_track_dr != lower_open.rounded_track_dr,
        "lower clamp fixture does not discriminate at report precision",
    )
    add(
        "histogram-lower-clamp",
        "clamped and unclamped lower histogram paths differ",
        clamped=lower_clamped.rounded_track_dr,
        unclamped=lower_open.rounded_track_dr,
    )

    n9_floor = candidate["loud-target-n9"]
    n9_ceil = analyze_fixture(by_id["loud-target-n9"], target_mode="ceil")
    n9_round = analyze_fixture(by_id["loud-target-n9"], target_mode="round")
    require(n9_floor is not None and n9_ceil is not None and n9_round is not None, "N9")
    require(
        n9_floor.rounded_track_dr != n9_ceil.rounded_track_dr
        and n9_ceil.rounded_track_dr == n9_round.rounded_track_dr,
        "N=9 target fixture does not distinguish floor from round/ceil",
    )
    add(
        "loud-target-n9",
        "floor(N/5) differs from round/ceil at N=9",
        floor=n9_floor.rounded_track_dr,
        round=n9_round.rounded_track_dr,
        ceil=n9_ceil.rounded_track_dr,
    )

    sparse = candidate["sparse-nonzero-among-zero"]
    sparse_drop = analyze_fixture(
        by_id["sparse-nonzero-among-zero"], count_all_zero_windows=False
    )
    require(sparse is not None and sparse_drop is not None, "sparse zero")
    require(
        sparse.channels[0].window_count == 10
        and sparse.channels[0].nonzero_histogram_count == 1
        and sparse_drop.channels[0].window_count == 1
        and sparse.report_rms != sparse_drop.report_rms,
        "sparse zero fixture does not distinguish N accounting",
    )
    add(
        "sparse-nonzero-among-zero",
        "zero-window inclusion changes N and channel/report RMS",
        includedWindowCount=10,
        removedWindowCount=1,
    )

    rms_f64 = candidate["rms-half-f64-stereo"]
    rms_f32_result = candidate["rms-half-f32-stereo"]
    require(rms_f64 is not None and rms_f32_result is not None, "RMS half")
    rms_f64_display = [f"{channel.dr_f32:.2f}" for channel in rms_f64.channels]
    rms_f32_display = [f"{channel.dr_f32:.2f}" for channel in rms_f32_result.channels]
    require(
        rms_f64.channels[0].rms_keys != rms_f64.channels[1].rms_keys
        and rms_f32_result.channels[0].rms_keys == rms_f32_result.channels[1].rms_keys
        and rms_f64_display[0] != rms_f64_display[1]
        and rms_f32_display[0] == rms_f32_display[1],
        "RMS f64/f32 collapse fixture drifted",
    )
    for case_id, result in (
        ("rms-half-f64-stereo", rms_f64),
        ("rms-half-f32-stereo", rms_f32_result),
    ):
        add(
            case_id,
            "source-f64 straddles while f32 encoding collapses the RMS key",
            leftKeys=list(result.channels[0].rms_keys),
            rightKeys=list(result.channels[1].rms_keys),
            channelDr2dp=[f"{channel.dr_f32:.2f}" for channel in result.channels],
        )

    peak_f64 = candidate["peak-half-f64-stereo"]
    peak_f32_result = candidate["peak-half-f32-stereo"]
    require(peak_f64 is not None and peak_f32_result is not None, "peak half")
    peak_f64_display = [f"{channel.dr_f32:.2f}" for channel in peak_f64.channels]
    peak_f32_display = [f"{channel.dr_f32:.2f}" for channel in peak_f32_result.channels]
    require(
        peak_f64.channels[0].selected_peak != peak_f64.channels[1].selected_peak
        and peak_f32_result.channels[0].selected_peak
        == peak_f32_result.channels[1].selected_peak
        and peak_f64_display[0] != peak_f64_display[1]
        and peak_f32_display[0] == peak_f32_display[1],
        "peak f64/f32 collapse fixture drifted",
    )
    for case_id, result in (
        ("peak-half-f64-stereo", peak_f64),
        ("peak-half-f32-stereo", peak_f32_result),
    ):
        add(
            case_id,
            "source-f64 straddles while f32 encoding collapses peak ranking",
            selectedPeakHex=[
                result.channels[0].selected_peak.hex(),
                result.channels[1].selected_peak.hex(),
            ],
            channelDr2dp=[f"{channel.dr_f32:.2f}" for channel in result.channels],
        )

    for prefix, rate in (("44100", 44_100), ("48000", 48_000)):
        control_id = f"sr-{prefix}-w-minus-one"
        split_id = f"sr-{prefix}-w-plus-one"
        control = candidate[control_id]
        split = candidate[split_id]
        discarded = analyze_fixture(by_id[control_id], submit_tail=False)
        require(control is not None and discarded is None, f"{control_id}: tail control")
        add(
            control_id,
            "W-1 is a submitted tail",
            candidateWindowFrames=window_frames(rate),
        )
        alternative_width = 3 * rate if rate == 44_100 else round(
            rate * WINDOW_DURATION_COEFFICIENT
        )
        alternate = analyze_fixture(by_id[split_id], window_override=alternative_width)
        require(split is not None and alternate is not None, f"{split_id}: split")
        require(
            split.channels[0].window_count == 2
            and split.rounded_track_dr != alternate.rounded_track_dr,
            f"{split_id}: window formula fixture does not discriminate",
        )
        add(
            split_id,
            "candidate floor window split differs from the competing width",
            candidateWidth=window_frames(rate),
            alternateWidth=alternative_width,
            candidateWindowCount=split.channels[0].window_count,
            alternateWindowCount=alternate.channels[0].window_count,
            candidateDr=split.rounded_track_dr,
            alternateDr=alternate.rounded_track_dr,
        )

    report = candidate["eight-channel-report-map"]
    require(
        report is not None
        and len(report.channels) == 8
        and report.report_peak
        == max(f32(channel.primary_peak) for channel in report.channels),
        "eight-channel report fixture drifted",
    )
    add(
        "eight-channel-report-map",
        "eight distinct channels expose report max/RMS and column order",
        roundedDr=report.rounded_track_dr,
        reportPeakHex=report.report_peak.hex(),
        reportRmsHex=report.report_rms.hex(),
    )

    low_aggregate = candidate["aggregate-narrow-low"]
    high_aggregate = candidate["aggregate-narrow-high"]
    require(
        low_aggregate is not None
        and high_aggregate is not None
        and low_aggregate.rounded_track_dr != high_aggregate.rounded_track_dr,
        "aggregate display boundary pair does not discriminate",
    )
    for case_id, result in (
        ("aggregate-narrow-low", low_aggregate),
        ("aggregate-narrow-high", high_aggregate),
    ):
        add(
            case_id,
            "paired track values fall on opposite sides of +0.5 display conversion",
            trackDrF32Hex=float(result.track_dr_f32).hex(),
            roundedDr=result.rounded_track_dr,
        )

    album_ids = ("album-10-49-a", "album-10-49-b", "album-11-49")
    album_results = [candidate[case_id] for case_id in album_ids]
    require(all(result is not None for result in album_results), "album source")
    album_typed = [result for result in album_results if result is not None]
    precise_album = math.trunc(
        f32(sum(result.track_dr_f32 for result in album_typed) / len(album_typed))
        + 0.5
    )
    displayed_album = math.trunc(
        (sum(result.rounded_track_dr for result in album_typed) / len(album_typed))
        + 0.5
    )
    require(precise_album != displayed_album, "album source fixture does not discriminate")
    for case_id in album_ids:
        add(
            case_id,
            "shared album distinguishes precise track values from displayed integers",
            preciseAlbum=precise_album,
            displayedIntegerAlbum=displayed_album,
        )

    display_low = candidate["album-10-49-a"]
    display_high = candidate["display-10-50"]
    require(
        display_low is not None
        and display_high is not None
        and display_low.rounded_track_dr != display_high.rounded_track_dr,
        "display half pair does not discriminate",
    )
    add(
        "display-10-50",
        "DR10.49 and DR10.50 witnesses display differently",
        low=display_low.rounded_track_dr,
        high=display_high.rounded_track_dr,
    )

    silent_album_member = candidate["silent-mono"]
    require(display_low is not None and silent_album_member is not None, "silent album")
    included_album = math.trunc(
        f32((display_low.track_dr_f32 + silent_album_member.track_dr_f32) / 2.0)
        + 0.5
    )
    excluded_album = display_low.rounded_track_dr
    require(included_album != excluded_album, "silent album fixture does not discriminate")
    add(
        "album-10-49-a",
        "album playlist including a numeric silent DR0 differs from exclusion",
        included=included_album,
        excluded=excluded_album,
    )
    add(
        "silent-mono",
        "same silent fixture is reused in a mixed album discriminator",
        included=included_album,
        excluded=excluded_album,
    )

    codec_ids = (
        "host-decode-u8",
        "host-decode-s16",
        "host-decode-s24",
        "host-decode-s32",
        "host-decode-f32",
        "host-decode-f64",
    )
    codec_results = [candidate[case_id] for case_id in codec_ids]
    require(all(result is not None for result in codec_results), "host decode")
    codec_typed = [result for result in codec_results if result is not None]
    baseline = codec_typed[0]
    require(
        all(
            result.track_dr_f32 == baseline.track_dr_f32
            and result.report_peak == baseline.report_peak
            and result.report_rms == baseline.report_rms
            for result in codec_typed[1:]
        ),
        "host-decode logical PCM is not report-equivalent in the local oracle",
    )
    for case_id in codec_ids:
        add(
            case_id,
            "six WAV encodings materialize the same dyadic analysis PCM",
            roundedDr=baseline.rounded_track_dr,
            reportPeakHex=baseline.report_peak.hex(),
            reportRmsHex=baseline.report_rms.hex(),
        )

    require(candidate["zero-frame"] is None, "zero-frame candidate should have no core result")
    add("zero-frame", "zero-frame is isolated before a numeric core result")

    for case_id in ("overfull-f32", "overfull-f64"):
        clamped_upper = candidate[case_id]
        open_upper = analyze_fixture(by_id[case_id], upper_clamp=False)
        require(clamped_upper is not None and open_upper is not None, case_id)
        require(
            clamped_upper.rounded_track_dr != open_upper.rounded_track_dr,
            f"{case_id}: overfull upper-clamp witness does not discriminate",
        )
        add(
            case_id,
            "finite overfull input exposes the upper histogram clamp",
            clamped=clamped_upper.rounded_track_dr,
            unclamped=open_upper.rounded_track_dr,
        )

    for fixture in fixtures:
        require(evidence[fixture.fixture_id], f"{fixture.fixture_id}: no discriminator assertion")
    return evidence


def write_playlist(path: Path, fixtures: Sequence[Fixture]) -> None:
    lines = ["#EXTM3U"]
    lines.extend(f"../{fixture.relative_path.as_posix()}" for fixture in fixtures)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def write_verify_powershell(path: Path) -> None:
    script = r"""$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
$ExpectedFiles = Get-Content (Join-Path $Root 'FILES.sha256')
$ExpectedPaths = @()
foreach ($Line in $ExpectedFiles) {
    if ($Line -notmatch '^([0-9a-f]{64})  (.+)$') {
        throw "Malformed FILES.sha256 line: $Line"
    }
    $Expected = $Matches[1]
    $Recorded = $Matches[2]
    if ([IO.Path]::IsPathRooted($Recorded) -or
        @($Recorded.Split('/')) -contains '..' -or
        $Recorded.Replace('\', '/') -ne $Recorded) {
        throw "Unsafe/non-canonical FILES.sha256 path: $Recorded"
    }
    if ($ExpectedPaths -contains $Recorded) {
        throw "Duplicate FILES.sha256 path: $Recorded"
    }
    $ExpectedPaths += $Recorded
    $Relative = $Recorded.Replace('/', [IO.Path]::DirectorySeparatorChar)
    $Actual = (Get-FileHash -Algorithm SHA256 (Join-Path $Root $Relative)).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) {
        throw "SHA-256 mismatch: $Relative"
    }
}
$ActualPaths = @(Get-ChildItem -Path $Root -File -Recurse |
    Where-Object { $_.FullName -ne (Join-Path $Root 'FILES.sha256') } |
    ForEach-Object {
        $_.FullName.Substring($RootFull.Length).Replace('\', '/')
    })
if (@(Compare-Object $ExpectedPaths $ActualPaths).Count -ne 0) {
    throw 'FILES.sha256 path set differs from the output tree'
}
$WavCount = @(Get-ChildItem -Path $Root -Filter '*.wav' -File -Recurse).Count
if ($WavCount -ne 42) { throw "Expected 42 WAV files, found $WavCount" }
$MasterPath = Join-Path $Root 'playlists/00-safe-master.m3u8'
$Master = Get-Content $MasterPath |
    Where-Object { $_ -and -not $_.StartsWith('#') }
if (@($Master).Count -ne 39) { throw "Expected 39 safe-master entries" }
foreach ($Forbidden in @('901_zero_frame.wav', '902_overfull_f32.wav', '903_overfull_f64.wav')) {
    if ($Master -match [regex]::Escape($Forbidden)) {
        throw "Isolated fixture leaked into safe master: $Forbidden"
    }
}
$Manifest = Get-Content (Join-Path $Root 'manifest.json') -Raw | ConvertFrom-Json
$CaseIdByPath = @{}
foreach ($Case in $Manifest.cases) { $CaseIdByPath[$Case.path] = $Case.id }
$ActualMasterIds = @()
foreach ($Entry in $Master) {
    $Full = [IO.Path]::GetFullPath((Join-Path (Split-Path $MasterPath -Parent) $Entry))
    if (-not $Full.StartsWith($RootFull, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Master entry escapes corpus root: $Entry"
    }
    $Relative = $Full.Substring($RootFull.Length).Replace('\', '/')
    if (-not $CaseIdByPath.ContainsKey($Relative)) {
        throw "Master entry is not a registered case: $Entry"
    }
    $ActualMasterIds += $CaseIdByPath[$Relative]
}
$ExpectedMasterIds = @($Manifest.playlists.'00-safe-master')
for ($Index = 0; $Index -lt $ExpectedMasterIds.Count; $Index++) {
    if ($ExpectedMasterIds[$Index] -ne $ActualMasterIds[$Index]) {
        throw "Safe-master order differs from manifest at index $Index"
    }
}
Write-Host 'foo_dr_meter complete v2 corpus verification passed.'
"""
    path.write_text(script, encoding="utf-8", newline="\r\n")


def how_to_export(manifest_sha256: str) -> str:
    return f"""foo_dr_meter 1.0.8 complete corpus v2

Corpus: {CORPUS_ID}
Manifest SHA-256: {manifest_sha256}

model-predictions.json contains candidate-model diagnostics only. It is not a
reference observation, accepted conformance result, or golden output.

Before exporting:
  1. Disable Automatically save tags.
  2. Disable Weight album DR by track lengths.
  3. Disable Weight multichannel DR by channel loudness.
  4. Enable per-channel statistics for stereo album logs.
  5. Keep every copied report byte-for-byte unchanged.

Primary pass:
  Open playlists/00-safe-master.m3u8. It contains exactly 39 unique, safe WAV
  inputs. Select all, run Measure Dynamic Range, and save the unchanged raw
  log as reports/<arch>-run1-safe-master.txt.

Optional focused album diagnostics (the album rules are already static):
  playlists/07-album-source.m3u8
  playlists/07-album-silence.m3u8
  playlists/07-display-half.m3u8

Isolated passes (open and run each playlist separately; never add them to the
master selection):
  playlists/99-zero-frame.m3u8
  playlists/99-overfull-f32.m3u8
  playlists/99-overfull-f64.m3u8

For zero-frame, retain the exact host dialog/error/screenshot if no report can
be copied. The overfull files contain finite samples above normalized full
scale and are isolated intentionally.

Verification:
  macOS/Linux:
    python3 reference/tools/generate_foo_dr_meter_108_complete_v2.py --verify <DIR>
  Windows PowerShell:
    powershell -ExecutionPolicy Bypass -File .\\VERIFY.ps1

After the run:
  Fill RUN_METADATA_TEMPLATE.txt once for the fixed target and keep every raw
  report alongside it. Do not copy values from model-predictions.json into a
  reference observation.
"""


def run_metadata_template(manifest_sha256: str) -> str:
    return f"""foo_dr_meter complete-v2 reference-run metadata

Corpus
  corpusId: {CORPUS_ID}
  manifestSha256: {manifest_sha256}

Run
  architecture: <x86 or x64>
  localDateTime: <YYYY-MM-DD HH:MM:SS>
  timezone: <Windows timezone name and UTC offset>
  operatorNotes: <exact deviations from HOW_TO_EXPORT.txt, or none>

Environment
  windowsVersion: <edition, version, build>
  foobar2000Version: <file/product version>
  foobar2000ExeSha256: <SHA-256>
  fooInputStdSha256: <SHA-256>
  fooDrMeterVersion: 1.0.8
  fooDrMeterDllSha256: <SHA-256>

Plugin settings
  automaticallySaveTags: false
  stereoPerChannelStats: true
  albumLengthWeighting: false
  multichannelLoudnessWeighting: false
  otherRelevantSettings: <key=value, or none>

Raw outputs
  safeMasterReport: <relative path and SHA-256>
  albumSourceReport: <relative path and SHA-256, or not run>
  albumSilenceReport: <relative path and SHA-256, or not run>
  displayHalfReport: <relative path and SHA-256, or not run>
  zeroFrameOutcome: <raw UI/error/screenshot path and SHA-256, or not run>
  overfullF32Outcome: <raw report/error path and SHA-256, or not run>
  overfullF64Outcome: <raw report/error path and SHA-256, or not run>

PowerShell helpers
  Get-FileHash -Algorithm SHA256 .\\manifest.json
  Get-FileHash -Algorithm SHA256 <path-to-foobar2000.exe>
  Get-FileHash -Algorithm SHA256 <path-to-foo_input_std.dll>
  Get-FileHash -Algorithm SHA256 <path-to-foo_dr_meter.dll>
  Get-ComputerInfo |
    Select-Object WindowsProductName,WindowsVersion,OsBuildNumber
  Get-TimeZone
"""


def deterministic_manifest(
    source: Path,
    v1_source: Path,
    case_records: Sequence[dict[str, Any]],
    playlists: dict[str, list[str]],
) -> dict[str, Any]:
    return {
        "schemaVersion": 2,
        "corpusId": CORPUS_ID,
        "generator": {
            "name": source.name,
            "version": GENERATOR_VERSION,
            "sourceSha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "v1FixtureSource": {
                "name": v1_source.name,
                "sha256": hashlib.sha256(v1_source.read_bytes()).hexdigest(),
                "usage": "read-only import of the 15 registered v1 fixture recipes",
            },
            "command": (
                "python3 reference/tools/"
                "generate_foo_dr_meter_108_complete_v2.py --output <OUTPUT>"
            ),
        },
        "targetFamily": {
            "component": "foo_dr_meter",
            "version": "1.0.8",
            "compatibilityClaim": "none; generated inputs are not reference goldens",
        },
        "window": {
            "durationCoefficientF64Hex": WINDOW_DURATION_COEFFICIENT.hex(),
            "formula": "floor(sampleRateHz * coefficient)",
        },
        "budgets": {
            "expectedWavFiles": EXPECTED_CASES,
            "expectedSafeMasterEntries": EXPECTED_SAFE_CASES,
            "maximumTotalWavBytes": MAX_CORPUS_BYTES,
            "maximumSingleWavBytes": MAX_FILE_BYTES,
        },
        "isolatedFixtureIds": sorted(ISOLATED_IDS),
        "playlists": playlists,
        "cases": list(case_records),
    }


def generate(output_root: Path) -> None:
    fixtures = build_v2_fixtures()
    source = Path(__file__).resolve()
    v1_source = source.with_name("generate_foo_dr_meter_108_suite.py")
    written_paths: list[Path] = []
    case_records: list[dict[str, Any]] = []
    prediction_records: list[dict[str, Any]] = []
    provenance_cases: list[dict[str, Any]] = []
    candidate: dict[str, AnalysisResult | None] = {}

    for fixture in fixtures:
        path = output_root / fixture.relative_path
        byte_length, file_sha = write_wave(path, fixture)
        riff = parse_riff_wave(path)
        materialized = materialize_channels(fixture)
        result = analyze(
            fixture.sample_rate,
            materialized,
            lfe_index=3 if fixture.channel_mask == 0x3F else None,
        )
        candidate[fixture.fixture_id] = result
        written_paths.append(path)
        case_records.append(
            {
                "id": fixture.fixture_id,
                "order": len(case_records) + 1,
                "path": fixture.relative_path.as_posix(),
                "executionClass": "isolated" if fixture.isolated else "safe",
                "encoding": fixture.encoding.name,
                "sampleRateHz": fixture.sample_rate,
                "windowFrames": window_frames(fixture.sample_rate),
                "channels": len(fixture.channels),
                "channelMask": (
                    None
                    if fixture.channel_mask is None
                    else f"0x{fixture.channel_mask:08x}"
                ),
                "frames": fixture.frame_count,
                "byteLength": byte_length,
                "fileSha256": file_sha,
                "dataSha256": riff["dataSha256"],
                "playlists": sorted(fixture.playlists),
                "waveform": fixture.waveform,
                "question": fixture.question,
                "alternativeHypotheses": list(fixture.alternatives),
                **fixture_classification(fixture),
            }
        )
        prediction_records.append(
            {
                "id": fixture.fixture_id,
                "modelScope": (
                    "x64-core construction model applied to the materialized "
                    "WAV sample values; not an x86 numeric model"
                ),
                "candidateDiagnostics": result_record(result),
            }
        )
        provenance_cases.append(
            {
                "id": fixture.fixture_id,
                "sourcePcmF64LeSha256": pcm_f64le_sha256(fixture.channels),
                "materializedPcmF64LeSha256": pcm_f64le_sha256(materialized),
                "fileSha256": file_sha,
                "dataSha256": riff["dataSha256"],
            }
        )

    discriminator_evidence = assert_discriminators(fixtures, candidate)
    for record in prediction_records:
        record["generationAssertions"] = discriminator_evidence[record["id"]]

    safe_fixtures = [fixture for fixture in fixtures if not fixture.isolated]
    playlist_members: dict[str, list[Fixture]] = {
        "00-safe-master": safe_fixtures,
    }
    for fixture in fixtures:
        for playlist_id in fixture.playlists:
            playlist_members.setdefault(playlist_id, []).append(fixture)
    playlist_records: dict[str, list[str]] = {}
    for playlist_id, members in sorted(playlist_members.items()):
        if playlist_id == "00-safe-master":
            unique_members = members
        else:
            seen: set[str] = set()
            unique_members = []
            for member in members:
                if member.fixture_id not in seen:
                    seen.add(member.fixture_id)
                    unique_members.append(member)
        path = output_root / "playlists" / f"{playlist_id}.m3u8"
        write_playlist(path, unique_members)
        written_paths.append(path)
        playlist_records[playlist_id] = [member.fixture_id for member in unique_members]

    if len(playlist_records["00-safe-master"]) != EXPECTED_SAFE_CASES:
        raise AssertionError("safe master count drifted")
    if ISOLATED_IDS.intersection(playlist_records["00-safe-master"]):
        raise AssertionError("isolated fixture leaked into safe master")

    manifest = deterministic_manifest(
        source,
        v1_source,
        case_records,
        playlist_records,
    )
    manifest_path = output_root / "manifest.json"
    manifest_path.write_text(
        json.dumps(
            manifest,
            indent=2,
            sort_keys=True,
            ensure_ascii=False,
            allow_nan=False,
        )
        + "\n",
        encoding="utf-8",
        newline="\n",
    )
    written_paths.append(manifest_path)
    manifest_sha = hashlib.sha256(manifest_path.read_bytes()).hexdigest()

    predictions = {
        "schemaVersion": 1,
        "kind": "model_prediction",
        "model": (
            "recovered x64-core construction model embedded in the v2 "
            "generator"
        ),
        "compatibilityClaim": "none",
        "evidenceBoundary": (
            "These are deterministic model predictions and generation-time "
            "discriminator checks computed with Python's binary64 math. They "
            "do not model the x86 sample-square/log10f path or guarantee "
            "Windows CRT last-bit behavior. They are not a reference "
            "observation, accepted conformance result, or golden output."
        ),
        "coreManifestSha256": manifest_sha,
        "cases": prediction_records,
    }
    predictions_path = output_root / "model-predictions.json"
    predictions_path.write_text(
        json.dumps(
            predictions,
            indent=2,
            sort_keys=True,
            ensure_ascii=False,
            allow_nan=False,
        )
        + "\n",
        encoding="utf-8",
        newline="\n",
    )
    written_paths.append(predictions_path)

    provenance = {
        "schemaVersion": 1,
        "kind": "generation-provenance",
        "coreManifestSha256": manifest_sha,
        "runtime": {
            "pythonImplementation": platform.python_implementation(),
            "pythonVersion": platform.python_version(),
            "platform": platform.platform(),
            "byteOrder": sys.byteorder,
        },
        "cases": provenance_cases,
    }
    provenance_path = output_root / "generation-provenance.json"
    provenance_path.write_text(
        json.dumps(
            provenance,
            indent=2,
            sort_keys=True,
            ensure_ascii=False,
            allow_nan=False,
        )
        + "\n",
        encoding="utf-8",
        newline="\n",
    )
    written_paths.append(provenance_path)

    how_to = output_root / "HOW_TO_EXPORT.txt"
    how_to.write_text(
        how_to_export(manifest_sha),
        encoding="utf-8",
        newline="\n",
    )
    written_paths.append(how_to)
    metadata_template = output_root / "RUN_METADATA_TEMPLATE.txt"
    metadata_template.write_text(
        run_metadata_template(manifest_sha),
        encoding="utf-8",
        newline="\n",
    )
    written_paths.append(metadata_template)
    verify_ps1 = output_root / "VERIFY.ps1"
    write_verify_powershell(verify_ps1)
    written_paths.append(verify_ps1)
    (output_root / "reports").mkdir(parents=True, exist_ok=True)

    wav_paths = [output_root / fixture.relative_path for fixture in fixtures]
    total_wav_bytes = sum(path.stat().st_size for path in wav_paths)
    largest = max(wav_paths, key=lambda item: item.stat().st_size)
    if total_wav_bytes > MAX_CORPUS_BYTES:
        raise AssertionError(
            f"WAV budget exceeded: {total_wav_bytes} > {MAX_CORPUS_BYTES}"
        )
    if largest.stat().st_size > MAX_FILE_BYTES:
        raise AssertionError(
            f"single WAV budget exceeded: {largest} is {largest.stat().st_size} bytes"
        )

    checksum_lines = []
    for path in sorted(written_paths):
        checksum_lines.append(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  "
            f"{path.relative_to(output_root).as_posix()}"
        )
    (output_root / "FILES.sha256").write_text(
        "\n".join(checksum_lines) + "\n",
        encoding="ascii",
        newline="\n",
    )


def verify(output_root: Path) -> dict[str, Any]:
    output_root_resolved = output_root.resolve()
    manifest_path = output_root / "manifest.json"
    checksums_path = output_root / "FILES.sha256"
    if not manifest_path.is_file() or not checksums_path.is_file():
        raise ValueError("directory does not contain manifest.json and FILES.sha256")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("corpusId") != CORPUS_ID:
        raise ValueError(f"unexpected corpusId: {manifest.get('corpusId')!r}")
    cases = manifest.get("cases")
    if not isinstance(cases, list) or len(cases) != EXPECTED_CASES:
        raise ValueError(f"expected {EXPECTED_CASES} manifest cases")
    predictions_path = output_root / "model-predictions.json"
    predictions = json.loads(predictions_path.read_text(encoding="utf-8"))
    if predictions.get("kind") != "model_prediction":
        raise ValueError("model-predictions.json has the wrong kind")
    manifest_sha = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
    if predictions.get("coreManifestSha256") != manifest_sha:
        raise ValueError("model predictions refer to a different core manifest")
    prediction_cases = predictions.get("cases")
    if (
        not isinstance(prediction_cases, list)
        or [record.get("id") for record in prediction_cases]
        != [record.get("id") for record in cases]
    ):
        raise ValueError("model prediction case order differs from the manifest")

    checked_files = 0
    checksum_paths: set[str] = set()
    for line in checksums_path.read_text(encoding="ascii").splitlines():
        digest, separator, relative = line.partition("  ")
        if not separator or len(digest) != 64:
            raise ValueError(f"malformed FILES.sha256 line: {line!r}")
        posix_path = PurePosixPath(relative)
        if (
            not relative
            or posix_path.is_absolute()
            or Path(relative).is_absolute()
            or ".." in posix_path.parts
            or "." in posix_path.parts
            or "\\" in relative
            or ":" in relative
            or posix_path.as_posix() != relative
        ):
            raise ValueError(f"unsafe/non-canonical FILES.sha256 path: {relative!r}")
        if relative in checksum_paths:
            raise ValueError(f"duplicate FILES.sha256 path: {relative}")
        checksum_paths.add(relative)
        path = output_root / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"checksummed path is not a regular file: {relative}")
        try:
            path.resolve(strict=True).relative_to(output_root_resolved)
        except ValueError as error:
            raise ValueError(
                f"checksummed path escapes the corpus root: {relative}"
            ) from error
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != digest:
            raise ValueError(f"SHA-256 mismatch: {relative}")
        checked_files += 1

    symlink_paths = [
        path.relative_to(output_root).as_posix()
        for path in output_root.rglob("*")
        if path.is_symlink()
    ]
    if symlink_paths:
        raise ValueError(f"corpus tree contains symbolic links: {symlink_paths}")

    actual_paths = {
        path.relative_to(output_root).as_posix()
        for path in output_root.rglob("*")
        if path.is_file()
        and not path.is_symlink()
        and path != checksums_path
    }
    if checksum_paths != actual_paths:
        missing = sorted(actual_paths - checksum_paths)
        extra = sorted(checksum_paths - actual_paths)
        raise ValueError(
            "FILES.sha256 path set differs from the output tree: "
            f"missing={missing}, extra={extra}"
        )

    wav_paths = sorted(output_root.rglob("*.wav"))
    if len(wav_paths) != EXPECTED_CASES:
        raise ValueError(f"expected {EXPECTED_CASES} WAVs, found {len(wav_paths)}")
    case_by_path = {record["path"]: record for record in cases}
    if len(case_by_path) != EXPECTED_CASES:
        raise ValueError("duplicate manifest path")
    total_wav_bytes = 0
    largest_path: Path | None = None
    for path in wav_paths:
        relative = path.relative_to(output_root).as_posix()
        if relative not in case_by_path:
            raise ValueError(f"unregistered WAV: {relative}")
        parsed = parse_riff_wave(path)
        record = case_by_path[relative]
        for key in (
            "sampleRateHz",
            "channels",
            "frames",
            "byteLength",
            "fileSha256",
            "dataSha256",
        ):
            if parsed[key] != record[key]:
                raise ValueError(
                    f"{relative}: manifest {key}={record[key]!r}, actual={parsed[key]!r}"
                )
        total_wav_bytes += parsed["byteLength"]
        if largest_path is None or path.stat().st_size > largest_path.stat().st_size:
            largest_path = path
    if total_wav_bytes > MAX_CORPUS_BYTES:
        raise ValueError("total WAV budget exceeded")
    if largest_path is None or largest_path.stat().st_size > MAX_FILE_BYTES:
        raise ValueError("single WAV budget exceeded")

    master_path = output_root / "playlists" / "00-safe-master.m3u8"
    master_entries = [
        line
        for line in master_path.read_text(encoding="utf-8").splitlines()
        if line and not line.startswith("#")
    ]
    if len(master_entries) != EXPECTED_SAFE_CASES or len(set(master_entries)) != len(
        master_entries
    ):
        raise ValueError("safe master must contain 39 unique entries")
    isolated_names = {
        case_by_path[record["path"]]["path"]
        for record in cases
        if record["id"] in ISOLATED_IDS
    }
    resolved_master = {
        (master_path.parent / entry).resolve().relative_to(output_root.resolve()).as_posix()
        for entry in master_entries
    }
    if resolved_master.intersection(isolated_names):
        raise ValueError("isolated fixture leaked into safe master")

    playlists = manifest.get("playlists")
    if not isinstance(playlists, dict):
        raise ValueError("manifest playlists must be an object")
    path_to_id = {record["path"]: record["id"] for record in cases}
    for playlist_id, expected_ids in playlists.items():
        playlist_path = output_root / "playlists" / f"{playlist_id}.m3u8"
        entries = [
            line
            for line in playlist_path.read_text(encoding="utf-8").splitlines()
            if line and not line.startswith("#")
        ]
        actual_ids = []
        for entry in entries:
            try:
                relative = (
                    (playlist_path.parent / entry)
                    .resolve()
                    .relative_to(output_root.resolve())
                    .as_posix()
                )
            except ValueError as error:
                raise ValueError(
                    f"{playlist_id}: entry escapes corpus root: {entry}"
                ) from error
            try:
                actual_ids.append(path_to_id[relative])
            except KeyError as error:
                raise ValueError(
                    f"{playlist_id}: entry is not a registered WAV: {entry}"
                ) from error
        if actual_ids != expected_ids:
            raise ValueError(
                f"{playlist_id}: manifest membership/order differs from M3U8"
            )

    return {
        "cases": len(cases),
        "safeMasterEntries": len(master_entries),
        "checkedFiles": checked_files,
        "totalWavBytes": total_wav_bytes,
        "largestWav": largest_path.relative_to(output_root).as_posix(),
        "largestWavBytes": largest_path.stat().st_size,
        "manifestSha256": manifest_sha,
        "filesSha256": hashlib.sha256(checksums_path.read_bytes()).hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--output", type=Path, help="empty directory to generate")
    mode.add_argument("--verify", type=Path, help="generated directory to verify")
    args = parser.parse_args()
    if args.output is not None:
        output = args.output
        if output.exists() and not output.is_dir():
            parser.error("output path exists and is not a directory")
        if output.exists() and any(output.iterdir()):
            parser.error("output directory must be empty")
        output.mkdir(parents=True, exist_ok=True)
        generate(output)
        summary = verify(output)
        print(json.dumps(summary, sort_keys=True))
        return
    try:
        summary = verify(args.verify)
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        parser.error(str(error))
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
