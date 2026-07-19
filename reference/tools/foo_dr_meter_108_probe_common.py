"""Pure validation helpers for the foo_dr_meter 1.0.8 x64 debugger probes."""

from __future__ import annotations

import math
import re
import struct
from dataclasses import dataclass
from typing import Any


EXPECTED_TARGET_SHA256 = (
    "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489"
)
CHANNEL_STATE_SIZE = 0x28
TRACK_RESULT_SIZE = 0x58
HISTOGRAM_BINS = 10001
MAX_CHANNELS = 64
MAX_ALBUM_TRACKS = 4096
MIN_USER_POINTER = 0x10000
MAX_USER_POINTER = 0x00007FFFFFFFFFFF
IDENTIFIER_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}")
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}")
CORE_EVENTS = frozenset(
    {
        "INIT_ENTRY",
        "PUSH_ENTRY",
        "PEAK_RANKED",
        "HIST_INC_PRE",
        "WINDOW_COMMIT_PRE_RESET",
        "FINISH_ENTRY",
        "LOUD_SELECTED",
        "PEAK_SELECTED",
        "NEGATIVE_FALLBACK",
        "CHANNEL_FINAL",
        "CHANNEL_PUBLISHED",
        "TRACK_INTERNAL",
        "TRACK_PUBLISHED",
    }
)
POST_FINISH_CORE_EVENTS = frozenset(
    {
        "LOUD_SELECTED",
        "PEAK_SELECTED",
        "NEGATIVE_FALLBACK",
        "CHANNEL_FINAL",
        "CHANNEL_PUBLISHED",
        "TRACK_INTERNAL",
        "TRACK_PUBLISHED",
    }
)
RESULT_BOUND_CORE_EVENTS = frozenset(
    {
        "FINISH_ENTRY",
        "CHANNEL_PUBLISHED",
        "TRACK_INTERNAL",
        "TRACK_PUBLISHED",
    }
)


class ProbeBoundsError(ValueError):
    """A debugger value cannot be safely dereferenced by the probe."""


@dataclass
class CoreCaptureLifecycle:
    """Fail-closed single-session lifecycle for one fixed fixture."""

    active_session: int | None = None
    active_result: int | None = None
    initializer_count: int = 0
    data_push_count: int = 0
    finish_count: int = 0
    tail_flush_count: int = 0
    track_published_count: int = 0

    def validate_event(
        self,
        event: str,
        session: int | None,
        result: int | None,
        *,
        push_frames: int | None = None,
        push_pcm: int | None = None,
    ) -> None:
        if event not in CORE_EVENTS:
            if self.track_published_count != 1:
                raise ProbeBoundsError(
                    "peripheral_event_before_track_published"
                )
            return

        if self.track_published_count != 0:
            raise ProbeBoundsError("core_event_after_track_published")
        if session is None:
            raise ProbeBoundsError("core_event_has_no_session")

        if event == "INIT_ENTRY":
            if self.initializer_count != 0 or self.active_session is not None:
                raise ProbeBoundsError(
                    "more_than_one_analyzer_for_single_fixture_capture"
                )
            if result is not None:
                raise ProbeBoundsError("initializer_unexpectedly_has_result")
            if push_frames is not None or push_pcm is not None:
                raise ProbeBoundsError("initializer_unexpectedly_has_push_data")
            return

        if self.initializer_count != 1 or self.active_session is None:
            raise ProbeBoundsError("core_event_before_analyzer_initializer")
        if session != self.active_session:
            raise ProbeBoundsError(
                "core_event_session_does_not_match_initializer"
            )

        if event == "PUSH_ENTRY":
            if push_frames is None or push_pcm is None:
                raise ProbeBoundsError("push_event_has_no_push_contract")
            if result is not None:
                raise ProbeBoundsError("push_event_unexpectedly_has_result")
            if self.finish_count == 0:
                if push_frames <= 0 or push_pcm == 0:
                    raise ProbeBoundsError(
                        "pre_finish_push_must_contain_nonempty_pcm"
                    )
            else:
                if self.finish_count != 1 or self.tail_flush_count != 0:
                    raise ProbeBoundsError(
                        "more_than_one_post_finish_tail_flush"
                    )
                if push_frames != 0 or push_pcm != 0:
                    raise ProbeBoundsError(
                        "post_finish_push_is_not_zero_tail_flush"
                    )
            return

        if push_frames is not None or push_pcm is not None:
            raise ProbeBoundsError("non_push_event_has_push_contract")

        if event == "FINISH_ENTRY":
            if self.finish_count != 0 or self.active_result is not None:
                raise ProbeBoundsError(
                    "more_than_one_finish_for_single_fixture_capture"
                )
            if result is None:
                raise ProbeBoundsError("finish_result_is_null")
            if self.data_push_count < 1:
                raise ProbeBoundsError("finish_before_any_nonempty_push")
            return

        if self.finish_count == 0 and self.data_push_count < 1:
            raise ProbeBoundsError("core_event_before_any_nonempty_push")
        if self.finish_count == 1 and self.tail_flush_count != 1:
            raise ProbeBoundsError("core_event_before_zero_tail_flush")

        if event in POST_FINISH_CORE_EVENTS and (
            self.finish_count != 1 or self.active_result is None
        ):
            raise ProbeBoundsError("finalization_event_before_finish")

        if event in RESULT_BOUND_CORE_EVENTS:
            if result is None:
                raise ProbeBoundsError("result_bound_event_has_null_result")
            if result != self.active_result:
                raise ProbeBoundsError(
                    "core_result_does_not_match_finish_result"
                )
        elif result is not None:
            raise ProbeBoundsError("core_event_unexpectedly_has_result")

    def accept_event(
        self,
        event: str,
        session: int | None,
        result: int | None,
        *,
        push_frames: int | None = None,
        push_pcm: int | None = None,
    ) -> None:
        self.validate_event(
            event,
            session,
            result,
            push_frames=push_frames,
            push_pcm=push_pcm,
        )
        if event == "INIT_ENTRY":
            assert session is not None
            self.active_session = session
            self.initializer_count = 1
        elif event == "PUSH_ENTRY":
            if self.finish_count == 0:
                self.data_push_count += 1
            else:
                self.tail_flush_count = 1
        elif event == "FINISH_ENTRY":
            assert result is not None
            self.active_result = result
            self.finish_count = 1
        elif event == "TRACK_PUBLISHED":
            self.track_published_count = 1

    def is_complete(self) -> bool:
        return (
            self.initializer_count == 1
            and self.data_push_count >= 1
            and self.finish_count == 1
            and self.tail_flush_count == 1
            and self.track_published_count == 1
            and self.active_session is not None
            and self.active_result is not None
        )

    def reset(self) -> None:
        self.active_session = None
        self.active_result = None
        self.initializer_count = 0
        self.data_push_count = 0
        self.finish_count = 0
        self.tail_flush_count = 0
        self.track_published_count = 0


def validate_attested_sha256(value: str) -> str:
    normalized = validate_sha256(value, "attested loaded-module")
    if normalized != EXPECTED_TARGET_SHA256:
        raise ProbeBoundsError(
            "attested loaded-module SHA-256 does not match the fixed target"
        )
    return normalized


def validate_sha256(value: str, label: str) -> str:
    normalized = value.strip().casefold()
    if not SHA256_PATTERN.fullmatch(normalized):
        raise ProbeBoundsError(f"{label} SHA-256 is not 64 hexadecimal digits")
    return normalized


def validate_identifier(value: str, label: str) -> str:
    normalized = value.strip()
    if not IDENTIFIER_PATTERN.fullmatch(normalized):
        raise ProbeBoundsError(
            f"{label} must be path-free and match {IDENTIFIER_PATTERN.pattern}"
        )
    return normalized


def validate_input_attestation(expected: str, attested: str) -> tuple[str, str]:
    normalized_expected = validate_sha256(expected, "expected input")
    normalized_attested = validate_sha256(attested, "attested input")
    if normalized_expected != normalized_attested:
        raise ProbeBoundsError(
            "attested input SHA-256 does not match expected fixture SHA-256"
        )
    return normalized_expected, normalized_attested


def temporary_exception_flags(
    old_flags: int,
    *,
    break_flag: int,
    handle_flag: int,
    message_flag: int,
    silent_flag: int,
) -> int:
    """Return a silent pass-through policy without discarding unrelated bits."""

    return (
        old_flags & ~(break_flag | handle_flag | message_flag)
    ) | silent_flag


def exception_policy_is_applied(
    flags: int,
    *,
    break_flag: int,
    handle_flag: int,
    message_flag: int,
    silent_flag: int,
) -> bool:
    """Verify every flag changed by :func:`temporary_exception_flags`."""

    return (
        not bool(flags & break_flag)
        and not bool(flags & handle_flag)
        and not bool(flags & message_flag)
        and bool(flags & silent_flag)
    )


def restored_exception_matches(
    saved: tuple[int, str, str] | None,
    observed: tuple[int, str, str] | None,
) -> bool:
    """An absent entry stays absent; an existing entry is restored exactly."""

    return observed == saved


def checked_pointer(value: int, label: str) -> int:
    if not MIN_USER_POINTER <= value <= MAX_USER_POINTER:
        raise ProbeBoundsError(f"{label} is not a bounded Windows user pointer")
    return value


def checked_channels(value: int) -> int:
    if not 1 <= value <= MAX_CHANNELS:
        raise ProbeBoundsError(
            f"channel count must be in 1..{MAX_CHANNELS}, got {value}"
        )
    return value


def checked_array(pointer: int, capacity: int, count: int, label: str) -> int:
    checked_pointer(pointer, f"{label} pointer")
    if not 0 <= count <= capacity <= MAX_CHANNELS:
        raise ProbeBoundsError(
            f"{label} capacity/count is invalid: capacity={capacity}, count={count}"
        )
    return pointer


def histogram_bin(flat_index: int, channel: int, channels: int) -> int:
    checked_channels(channels)
    if not 0 <= channel < channels:
        raise ProbeBoundsError(f"channel index out of range: {channel}")
    maximum = channels * HISTOGRAM_BINS
    if not 0 <= flat_index < maximum:
        raise ProbeBoundsError(f"flattened histogram index out of range: {flat_index}")
    result = flat_index - channel * HISTOGRAM_BINS
    if not 0 <= result < HISTOGRAM_BINS:
        raise ProbeBoundsError(
            f"histogram index does not belong to channel {channel}: {flat_index}"
        )
    return result


def album_record_from_offset(table: int, offset: int) -> tuple[int, int]:
    checked_pointer(table, "album track table")
    if offset < 0 or offset % TRACK_RESULT_SIZE:
        raise ProbeBoundsError(
            f"album record offset is not {TRACK_RESULT_SIZE:#x}-aligned: {offset}"
        )
    index = offset // TRACK_RESULT_SIZE
    if index >= MAX_ALBUM_TRACKS:
        raise ProbeBoundsError(f"album record index exceeds safety bound: {index}")
    address = table + offset
    checked_pointer(address, "album record")
    return address, index


def album_group_indices(start: int, count: int) -> range:
    if not 0 <= start < MAX_ALBUM_TRACKS:
        raise ProbeBoundsError(f"album group start exceeds safety bound: {start}")
    if not 1 <= count <= MAX_ALBUM_TRACKS:
        raise ProbeBoundsError(f"album group count exceeds safety bound: {count}")
    end = start + count
    if end > MAX_ALBUM_TRACKS:
        raise ProbeBoundsError(
            f"album group end exceeds safety bound: start={start}, count={count}"
        )
    return range(start, end)


def float_record(raw: bytes) -> dict[str, Any]:
    if len(raw) == 4:
        bits = f"0x{struct.unpack('<I', raw)[0]:08x}"
        value = struct.unpack("<f", raw)[0]
    elif len(raw) == 8:
        bits = f"0x{struct.unpack('<Q', raw)[0]:016x}"
        value = struct.unpack("<d", raw)[0]
    else:
        raise ProbeBoundsError(f"float width must be 4 or 8 bytes, got {len(raw)}")

    if math.isnan(value):
        decoded: float | str = "nan"
    elif math.isinf(value):
        decoded = "inf" if value > 0 else "-inf"
    else:
        decoded = value
    return {"bits": bits, "value": decoded}
