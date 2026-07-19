"""Fail-closed IDA remote probe template for foo_dr_meter 1.0.8 x64.

This template has not yet been dynamically executed. It does not launch,
attach, detach, terminate, patch application data, or save the IDB. The
debugger decides how transient software breakpoints are implemented.

Usage inside IDA:

1. Execute this file to install lifecycle hooks.
2. Before the target module loads, call configure() with one path-free fixture
   ID, expected/attested input hashes, and the attested remote DLL hash.
3. Start or attach the remote debugger.
4. After the intended one-fixture capture, pause and call mark_complete().

Any breakpoint conflict, partial install, bounds violation, snapshot failure,
log failure, or continue-request failure leaves capture unarmed or the process
paused. The script never silently continues after a failed evidence snapshot.
"""

from __future__ import annotations

import json
import struct
import sys
import builtins
from pathlib import Path
from typing import Any

import ida_dbg
import ida_idd
import ida_kernwin


TOOLS_DIR = str(Path(__file__).resolve().parent)
if TOOLS_DIR not in sys.path:
    sys.path.insert(0, TOOLS_DIR)

from foo_dr_meter_108_probe_common import (  # noqa: E402
    CORE_EVENTS,
    EXPECTED_TARGET_SHA256,
    HISTOGRAM_BINS,
    MAX_ALBUM_TRACKS,
    TRACK_RESULT_SIZE,
    CoreCaptureLifecycle,
    ProbeBoundsError,
    album_group_indices,
    album_record_from_offset,
    checked_array,
    checked_channels,
    checked_pointer,
    exception_policy_is_applied,
    float_record,
    histogram_bin,
    restored_exception_matches,
    temporary_exception_flags,
    validate_attested_sha256,
    validate_identifier,
    validate_input_attestation,
)


TARGET_MODULE = "foo_dr_meter.dll"
LOG_PREFIX = "MM108PROBE "
REGISTRY_KEY = "_macinmeter_foo_dr_meter_108_probe"
MAX_RECORDS = 100_000
MAX_INVALID_REASONS = 64
CPP_EXCEPTION_CODE = 0xE06D7363

# Runtime address = attested loaded-module base + RVA.
PROBES = {
    0x8410: "INIT_ENTRY",
    0x89F0: "PUSH_ENTRY",
    0x8CEA: "PEAK_RANKED",
    0x8D52: "HIST_INC_PRE",
    0x8D86: "WINDOW_COMMIT_PRE_RESET",
    0x8DF0: "FINISH_ENTRY",
    0x9026: "LOUD_SELECTED",
    0x904C: "PEAK_SELECTED",
    0x909A: "NEGATIVE_FALLBACK",
    0x90B8: "CHANNEL_FINAL",
    0x9116: "CHANNEL_PUBLISHED",
    0x9190: "TRACK_INTERNAL",
    0x91F0: "TRACK_PUBLISHED",
    0xE540: "ALBUM_ENTRY",
    0xE8CC: "ALBUM_INCLUDED",
    0xE960: "ALBUM_COMPUTED",
    0xEA1B: "ALBUM_WRITTEN",
    0xEA33: "ALBUM_EMPTY_PRE_WRITE",
    0x4111E: "RENDER_UNWEIGHTED",
    0x411B2: "RENDER_WEIGHTED",
}

_previous_registry = getattr(builtins, REGISTRY_KEY, None)
if isinstance(_previous_registry, dict) and _previous_registry.get("active"):
    raise RuntimeError(
        "foo_dr_meter probe is already active; use the persistent registry "
        "instead of re-executing the script"
    )

_module_base: int | None = None
_owned_breakpoints: set[int] = set()
_last_breakpoint_integrity: dict[str, Any] = {
    "context": "not_armed",
    "valid": False,
    "issues": [],
}
_last_module_scan: dict[str, Any] = {
    "matchCount": 0,
    "moduleBases": [],
}
_sequence = 0
_hooks: "ProbeHooks | None" = None
_run_id: str | None = None
_fixture_id: str | None = None
_expected_input_sha256: str | None = None
_attested_input_sha256: str | None = None
_attested_loaded_module_sha256: str | None = None
_capture_started = False
_capture_completed = False
_capture_aborted = False
_capture_process_id: int | None = None
_capture_process_epoch: int | None = None
_arm_confirmed = False
_process_id: int | None = None
_process_epoch = 0
_core_lifecycle = CoreCaptureLifecycle()
_records: list[str] = []
_record_overflow = False
_invalid_reasons: list[str] = []
_export_ready = False
_records_exported = False
_EXCEPTION_NOT_SAVED = object()
_saved_cpp_exception: tuple[int, str, str] | None | object = (
    _EXCEPTION_NOT_SAVED
)
_exception_policy_applied = False


def _module_leaf(name: str) -> str:
    return name.replace("\\", "/").rsplit("/", 1)[-1].casefold()


def _hex(value: int) -> str:
    return f"0x{value:x}"


def _next_sequence() -> int:
    global _sequence
    _sequence += 1
    return _sequence


def _latch_invalid(reason: str) -> None:
    normalized = reason.strip() or "unspecified_failure"
    if normalized in _invalid_reasons:
        return
    if len(_invalid_reasons) < MAX_INVALID_REASONS:
        _invalid_reasons.append(normalized)
    elif "additional_invalid_reasons_omitted" not in _invalid_reasons:
        _invalid_reasons.append("additional_invalid_reasons_omitted")


def _identity() -> dict[str, Any]:
    return {
        "evidenceClass": "operator_attested_diagnostic",
        "consistencyLevel": "internally_consistent",
        "expectedTargetSha256": EXPECTED_TARGET_SHA256,
        "attestedLoadedModuleSha256": _attested_loaded_module_sha256,
        "fixtureId": _fixture_id,
        "expectedInputSha256": _expected_input_sha256,
        "attestedInputSha256": _attested_input_sha256,
        "processId": _process_id,
        "processEpoch": _process_epoch,
    }


def _write_record(record: dict[str, Any]) -> bool:
    global _record_overflow
    try:
        encoded = json.dumps(record, allow_nan=False, sort_keys=True)
        if len(_records) >= MAX_RECORDS:
            _record_overflow = True
            _latch_invalid("record_limit_exceeded")
            ida_kernwin.msg(
                LOG_PREFIX
                + json.dumps(
                    {
                        "schemaVersion": 1,
                        "recordType": "status",
                        **_identity(),
                        "runId": _run_id,
                        "status": "record_limit_exceeded",
                        "maxRecords": MAX_RECORDS,
                        "action": "process_left_paused",
                    },
                    allow_nan=False,
                    sort_keys=True,
                )
                + "\n"
            )
            return False
        _records.append(encoded)
        ida_kernwin.msg(LOG_PREFIX + encoded + "\n")
        return True
    except Exception as error:
        _latch_invalid(f"log_failed:{type(error).__name__}")
        fallback = {
            "schemaVersion": 1,
            "recordType": "status",
            **_identity(),
            "runId": _run_id,
            "sequence": _next_sequence(),
            "status": "log_failed",
            "errorType": type(error).__name__,
            "error": str(error),
        }
        try:
            ida_kernwin.msg(
                LOG_PREFIX
                + json.dumps(fallback, allow_nan=False, sort_keys=True)
                + "\n"
            )
        except Exception:
            pass
        return False


def _emit_status(status: str, **fields: Any) -> bool:
    return _write_record(
        {
            "schemaVersion": 1,
            "recordType": "status",
            **_identity(),
            "runId": _run_id,
            "sequence": _next_sequence(),
            "status": status,
            **fields,
        }
    )


def _configured() -> bool:
    return all(
        value is not None
        for value in (
            _run_id,
            _fixture_id,
            _expected_input_sha256,
            _attested_input_sha256,
            _attested_loaded_module_sha256,
        )
    )


def _arm_block_reason() -> str | None:
    if _invalid_reasons:
        return "invalid_capture_must_be_aborted_exported_and_reset"
    if _capture_aborted:
        return "aborted_capture_must_be_exported_and_reset"
    if _capture_completed or _export_ready:
        return "completed_capture_must_be_exported_and_reset"
    if not _configured():
        return "configure_identity_and_attestations_first"
    return None


def _exception_copy(
    flags: int, name: str, description: str
) -> ida_idd.exception_info_t:
    item = ida_idd.exception_info_t()
    item.code = CPP_EXCEPTION_CODE
    item.flags = flags
    item.name = name
    item.desc = description
    return item


def _apply_cpp_exception_policy() -> bool:
    global _exception_policy_applied, _saved_cpp_exception
    if _exception_policy_applied:
        return True
    exceptions = ida_dbg.retrieve_exceptions()
    if exceptions is None:
        _latch_invalid("exception_policy_table_unavailable")
        _emit_status(
            "exception_policy_failed",
            reason="debugger_exception_table_unavailable",
        )
        return False

    found: ida_idd.exception_info_t | None = None
    for item in exceptions:
        if int(item.code) == CPP_EXCEPTION_CODE:
            found = item
            break
    if found is None:
        _saved_cpp_exception = None
        name = "Microsoft C++ exception"
        description = "C++ EH exception used by the fixed host/plugin runtime"
        old_flags = 0
        exceptions.append(
            _exception_copy(ida_idd.EXC_SILENT, name, description)
        )
        new_flags = ida_idd.EXC_SILENT
    else:
        old_flags = int(found.flags)
        _saved_cpp_exception = (
            old_flags,
            str(found.name),
            str(found.desc),
        )
        new_flags = temporary_exception_flags(
            old_flags,
            break_flag=ida_idd.EXC_BREAK,
            handle_flag=ida_idd.EXC_HANDLE,
            message_flag=ida_idd.EXC_MSG,
            silent_flag=ida_idd.EXC_SILENT,
        )
        found.flags = new_flags

    if not ida_dbg.store_exceptions():
        # The retrieved vector was mutated in place. Retain restoration
        # ownership and attempt an exact rollback even when the store request
        # itself reports failure.
        _exception_policy_applied = True
        _latch_invalid("exception_policy_store_failed")
        _emit_status(
            "exception_policy_failed",
            reason="store_exceptions_failed",
        )
        _restore_cpp_exception_policy()
        return False
    # From this point the debugger's table has been mutated, even if the
    # verification read fails. Mark it applied first so every failure path can
    # restore the exact saved entry instead of silently leaving the override.
    _exception_policy_applied = True
    verification = ida_dbg.retrieve_exceptions()
    verified = None
    if verification is not None:
        for item in verification:
            if int(item.code) == CPP_EXCEPTION_CODE:
                verified = item
                break
    if verified is None or not exception_policy_is_applied(
        int(verified.flags),
        break_flag=ida_idd.EXC_BREAK,
        handle_flag=ida_idd.EXC_HANDLE,
        message_flag=ida_idd.EXC_MSG,
        silent_flag=ida_idd.EXC_SILENT,
    ):
        _latch_invalid("exception_policy_verification_failed")
        _emit_status(
            "exception_policy_failed",
            reason="stored_policy_verification_failed",
        )
        _restore_cpp_exception_policy()
        return False
    if _emit_status(
        "exception_policy_applied",
        exceptionCode=_hex(CPP_EXCEPTION_CODE),
        breakOn=False,
        debuggerHandles=False,
        silent=True,
        oldFlags=_hex(old_flags),
        newFlags=_hex(new_flags),
    ):
        return True
    _restore_cpp_exception_policy()
    return False


def _restore_cpp_exception_policy() -> bool:
    global _exception_policy_applied, _saved_cpp_exception
    if not _exception_policy_applied:
        return True
    if _saved_cpp_exception is _EXCEPTION_NOT_SAVED:
        _latch_invalid("exception_policy_original_state_not_saved")
        _emit_status(
            "exception_policy_restore_failed",
            reason="original_exception_state_was_not_saved",
        )
        return False

    exceptions = ida_dbg.retrieve_exceptions()
    if exceptions is None:
        _latch_invalid("exception_policy_restore_table_unavailable")
        _emit_status(
            "exception_policy_restore_failed",
            reason="debugger_exception_table_unavailable",
        )
        return False
    retained: list[tuple[int, int, str, str]] = []
    for item in exceptions:
        if int(item.code) != CPP_EXCEPTION_CODE:
            retained.append(
                (int(item.code), int(item.flags), str(item.name), str(item.desc))
            )
    exceptions.clear()
    for code, flags, name, description in retained:
        item = ida_idd.exception_info_t()
        item.code = code
        item.flags = flags
        item.name = name
        item.desc = description
        exceptions.append(item)
    if isinstance(_saved_cpp_exception, tuple):
        flags, name, description = _saved_cpp_exception
        exceptions.append(_exception_copy(flags, name, description))
    if not ida_dbg.store_exceptions():
        _latch_invalid("exception_policy_restore_store_failed")
        _emit_status(
            "exception_policy_restore_failed",
            reason="store_exceptions_failed",
        )
        return False
    verification = ida_dbg.retrieve_exceptions()
    current = None
    if verification is not None:
        for item in verification:
            if int(item.code) == CPP_EXCEPTION_CODE:
                current = item
                break
    observed = (
        None
        if current is None
        else (int(current.flags), str(current.name), str(current.desc))
    )
    assert _saved_cpp_exception is None or isinstance(
        _saved_cpp_exception, tuple
    )
    restored = restored_exception_matches(_saved_cpp_exception, observed)
    if not restored:
        _latch_invalid("exception_policy_restore_verification_failed")
        _emit_status(
            "exception_policy_restore_failed",
            reason="restored_policy_verification_failed",
        )
        return False
    _exception_policy_applied = False
    _saved_cpp_exception = _EXCEPTION_NOT_SAVED
    return _emit_status(
        "exception_policy_restored",
        exceptionCode=_hex(CPP_EXCEPTION_CODE),
    )


def _reg(name: str) -> int:
    value = ida_dbg.get_reg_val(name)
    if not isinstance(value, int):
        raise TypeError(f"{name} is not an integer register")
    return value


def _xmm_bytes(name: str) -> bytes:
    value = ida_dbg.get_reg_val(name)
    if not isinstance(value, (bytes, bytearray)) or len(value) < 8:
        raise TypeError(f"{name} did not return vector bytes")
    return bytes(value)


def _read(address: int, size: int) -> bytes:
    checked_pointer(address, "debugger memory")
    if not 1 <= size <= 0x100000:
        raise ProbeBoundsError(f"debugger read size is invalid: {size}")
    checked_pointer(address + size - 1, "debugger memory end")
    result = ida_idd.dbg_read_memory(address, size)
    if not isinstance(result, bytes) or len(result) != size:
        raise RuntimeError(
            f"could not read {size} debugger bytes at {_hex(address)}"
        )
    return result


def _u32(address: int) -> int:
    return struct.unpack("<I", _read(address, 4))[0]


def _u64(address: int) -> int:
    return struct.unpack("<Q", _read(address, 8))[0]


def _f32(address: int) -> dict[str, Any]:
    return float_record(_read(address, 4))


def _f64(address: int) -> dict[str, Any]:
    return float_record(_read(address, 8))


def _xmm_f32(name: str) -> dict[str, Any]:
    return float_record(_xmm_bytes(name)[:4])


def _xmm_f64(name: str) -> dict[str, Any]:
    return float_record(_xmm_bytes(name)[:8])


def _checked_vector(
    owner: int,
    begin_offset: int,
    end_offset: int,
    capacity_offset: int,
    expected_bytes: int,
    label: str,
) -> int:
    begin = checked_pointer(_u64(owner + begin_offset), f"{label} begin")
    end = _u64(owner + end_offset)
    capacity = _u64(owner + capacity_offset)
    if end != begin + expected_bytes:
        raise ProbeBoundsError(
            f"{label} size mismatch: expected={expected_bytes}, "
            f"actual={end - begin if end >= begin else -1}"
        )
    if capacity < end:
        raise ProbeBoundsError(f"{label} capacity precedes end")
    checked_pointer(end - 1, f"{label} end")
    checked_pointer(capacity - 1, f"{label} capacity")
    return begin


def _session(address: int) -> dict[str, Any]:
    address = checked_pointer(address, "session")
    sample_rate = _u32(address + 0x08)
    channels = checked_channels(_u32(address + 0x0C))
    window_frames = _u32(address + 0x10)
    current_frames = _u32(address + 0x14)
    if sample_rate == 0 or window_frames == 0 or current_frames > window_frames:
        raise ProbeBoundsError(
            "session sample/window/current frame fields violate core bounds"
        )
    current_base = _checked_vector(
        address, 0x20, 0x28, 0x30, channels * 0x10, "current accumulator"
    )
    state_base = _checked_vector(
        address, 0x38, 0x40, 0x48, channels * 0x28, "channel state"
    )
    histogram_base = _checked_vector(
        address,
        0x50,
        0x58,
        0x60,
        channels * HISTOGRAM_BINS * 4,
        "histogram",
    )
    return {
        "address": _hex(address),
        "sampleRate": sample_rate,
        "channels": channels,
        "windowFrames": window_frames,
        "currentFrames": current_frames,
        "windowCount": _u64(address + 0x18),
        "currentBase": _hex(current_base),
        "channelStateBase": _hex(state_base),
        "histogramBase": _hex(histogram_base),
        "submittedFrames": _u64(address + 0x68),
    }


def _checked_channel(session: int, channel: int) -> tuple[int, int]:
    snapshot = _session(session)
    channels = snapshot["channels"]
    if not isinstance(channels, int) or not 0 <= channel < channels:
        raise ProbeBoundsError(f"channel index out of range: {channel}")
    state_base = _u64(session + 0x38)
    return channels, state_base + channel * 0x28


def _channel_state(session: int, channel: int) -> dict[str, Any]:
    _, base = _checked_channel(session, channel)
    return {
        "address": _hex(base),
        "rmsSquareSum": _f64(base),
        "primaryAmplitude": _f64(base + 0x08),
        "secondaryAmplitude": _f64(base + 0x10),
        "primaryKeyDb": _f64(base + 0x18),
        "secondaryKeyDb": _f64(base + 0x20),
    }


def _current_accumulators(session: int) -> list[dict[str, Any]]:
    snapshot = _session(session)
    channels = snapshot["channels"]
    assert isinstance(channels, int)
    base = _u64(session + 0x20)
    return [
        {
            "channel": channel,
            "sumSquares": _f64(base + channel * 0x10),
            "currentPeak": _f64(base + channel * 0x10 + 0x08),
        }
        for channel in range(channels)
    ]


def _channel_states(session: int) -> list[dict[str, Any]]:
    snapshot = _session(session)
    channels = snapshot["channels"]
    assert isinstance(channels, int)
    return [
        {"channel": channel, **_channel_state(session, channel)}
        for channel in range(channels)
    ]


def _published_arrays(result: int, channels: int) -> dict[str, Any]:
    result = checked_pointer(result, "track result")
    channels = checked_channels(channels)
    dr_base = checked_array(
        _u64(result + 0x28), _u64(result + 0x30), channels, "channel DR"
    )
    peak_base = checked_array(
        _u64(result + 0x38), _u64(result + 0x40), channels, "channel peak"
    )
    rms_base = checked_array(
        _u64(result + 0x48), _u64(result + 0x50), channels, "channel RMS"
    )
    return {
        "channelDr": [_f32(dr_base + channel * 4) for channel in range(channels)],
        "primaryPeak": [
            _f32(peak_base + channel * 4) for channel in range(channels)
        ],
        "overallRms": [
            _f32(rms_base + channel * 4) for channel in range(channels)
        ],
    }


def _track_published(result: int) -> dict[str, Any]:
    result = checked_pointer(result, "track result")
    channels = checked_channels(_u32(result + 0x0C))
    sample_rate = _u32(result + 0x14)
    if sample_rate == 0:
        raise ProbeBoundsError("published track result has zero sample rate")
    return {
        "address": _hex(result),
        "validAtThisProbe": [
            "trackDr",
            "channels",
            "sampleRate",
            "frames",
            "channelDr",
            "primaryPeak",
            "overallRms",
        ],
        "trackDr": _f32(result),
        "channels": channels,
        "sampleRate": sample_rate,
        "frames": _u64(result + 0x20),
        "arrays": _published_arrays(result, channels),
    }


def _album_input_record(address: int, index: int) -> dict[str, Any]:
    address = checked_pointer(address, "album input record")
    sample_rate = _u32(address + 0x14)
    if sample_rate == 0:
        raise ProbeBoundsError("album input record has zero sample rate")
    return {
        "index": index,
        "address": _hex(address),
        "trackDr": _f32(address),
        "sampleRate": sample_rate,
        "frames": _u64(address + 0x20),
    }


def _album_member(state: int, index: int) -> dict[str, Any]:
    state = checked_pointer(state, "album state")
    indices = album_group_indices(index, 1)
    index = indices.start
    table = checked_pointer(_u64(state + 0x28), "album track table")
    record, _ = album_record_from_offset(table, index * TRACK_RESULT_SIZE)
    return {
        **_album_input_record(record, index),
        "effective": _f32(record + 0x04),
        "unweighted": _f32(record + 0x08),
    }


def _event_bindings(
    event: str,
) -> tuple[int | None, int | None, int | None, int | None]:
    """Read only stable correlation registers before any event snapshot."""

    if event not in CORE_EVENTS:
        return None, None, None, None
    if event in {"INIT_ENTRY", "PUSH_ENTRY", "FINISH_ENTRY"}:
        session_register = "RCX"
    elif event in {
        "PEAK_RANKED",
        "HIST_INC_PRE",
        "WINDOW_COMMIT_PRE_RESET",
    }:
        session_register = "RBX"
    else:
        session_register = "R13"
    session = checked_pointer(
        _reg(session_register), f"{event} correlation session"
    )

    if event == "FINISH_ENTRY":
        result = checked_pointer(_reg("RDX"), "FINISH_ENTRY result")
    elif event in {
        "CHANNEL_PUBLISHED",
        "TRACK_INTERNAL",
        "TRACK_PUBLISHED",
    }:
        result = checked_pointer(_reg("RBP"), f"{event} result")
    else:
        result = None
    if event == "PUSH_ENTRY":
        push_frames = _reg("R8") & 0xFFFFFFFF
        push_pcm = _reg("RDX")
    else:
        push_frames = None
        push_pcm = None
    return session, result, push_frames, push_pcm


def _snapshot(event: str) -> dict[str, Any]:
    fields: dict[str, Any]
    if event == "INIT_ENTRY":
        session = checked_pointer(_reg("RCX"), "new session")
        sample_rate = _reg("RDX") & 0xFFFFFFFF
        channels = checked_channels(_reg("R8") & 0xFFFFFFFF)
        if sample_rate == 0:
            raise ProbeBoundsError("initializer received zero sample rate")
        fields = {
            "session": _hex(session),
            "sampleRate": sample_rate,
            "channels": channels,
            "opaqueF64": _xmm_f64("XMM3"),
        }
    elif event == "PUSH_ENTRY":
        session = _reg("RCX")
        frames = _reg("R8") & 0xFFFFFFFF
        pcm = _reg("RDX")
        if frames:
            checked_pointer(pcm, "interleaved PCM")
        elif pcm != 0:
            raise ProbeBoundsError("zero-frame push has a non-null PCM pointer")
        fields = {
            "session": _session(session),
            "pcm": _hex(pcm),
            "frames": frames,
            "currentAccumulators": _current_accumulators(session),
        }
    elif event in {"PEAK_RANKED", "HIST_INC_PRE"}:
        session = _reg("RBX")
        channel = _reg("RSI") & 0xFFFFFFFF
        channels, _ = _checked_channel(session, channel)
        fields = {
            "session": _hex(session),
            "channel": channel,
            "channelState": _channel_state(session, channel),
        }
        if event == "PEAK_RANKED":
            current_peak = _xmm_f64("XMM6")
            fields["currentPeak"] = current_peak
            current_value = current_peak["value"]
            fields["candidateKeyValid"] = (
                isinstance(current_value, float) and current_value > 0.0
            )
            if fields["candidateKeyValid"]:
                fields["candidateKeyDb"] = _xmm_f64("XMM0")
        else:
            flat_index = _reg("RCX") & 0xFFFFFFFF
            local_bin = histogram_bin(flat_index, channel, channels)
            histogram = checked_pointer(
                _u64(session + 0x50), "histogram begin"
            )
            fields.update(
                {
                    "windowRms": _xmm_f64("XMM8"),
                    "flatIndex": flat_index,
                    "bin": local_bin,
                    "counterBeforeIncrement": _u32(
                        histogram + flat_index * 4
                    ),
                }
            )
    elif event == "WINDOW_COMMIT_PRE_RESET":
        session = _reg("RBX")
        fields = {
            "timing": (
                "window/channel/histogram/count writes complete; "
                "currentFrames reset instruction has not executed"
            ),
            "session": _session(session),
            "channelStates": _channel_states(session),
        }
    elif event == "FINISH_ENTRY":
        session = _reg("RCX")
        result = checked_pointer(_reg("RDX"), "track result")
        fields = {
            "session": _session(session),
            "result": _hex(result),
            "multichannelWeighting": bool(_reg("R8") & 0xFF),
            "currentAccumulators": _current_accumulators(session),
        }
    elif event in {
        "LOUD_SELECTED",
        "PEAK_SELECTED",
        "NEGATIVE_FALLBACK",
        "CHANNEL_FINAL",
    }:
        session = _reg("R13")
        channel = _reg("R12") & 0xFFFFFFFF
        _checked_channel(session, channel)
        fields = {
            "session": _hex(session),
            "channel": channel,
            "channelState": _channel_state(session, channel),
        }
        if event == "LOUD_SELECTED":
            target = _reg("R14")
            included = _reg("RSI")
            windows = _u64(session + 0x18)
            if not 1 <= target <= max(1, windows):
                raise ProbeBoundsError("loud target is out of range")
            if not 0 <= included <= windows:
                raise ProbeBoundsError("included loud-window count is out of range")
            if included != 0 and included < target:
                raise ProbeBoundsError(
                    "nonzero included loud-window count is below target"
                )
            raw_boundary = _reg("RBX") & 0xFFFFFFFF
            fields.update(
                {
                    "targetWindows": target,
                    "includedWindows": included,
                    "boundaryBin": (
                        -1 if raw_boundary == 0xFFFFFFFF else raw_boundary
                    ),
                    "loudPowerSum": _xmm_f64("XMM6"),
                    "overallRms": _xmm_f64("XMM10"),
                }
            )
        elif event == "PEAK_SELECTED":
            fields["selectedPeak"] = _xmm_f64("XMM1")
        elif event == "NEGATIVE_FALLBACK":
            fields["candidateDr"] = _xmm_f64("XMM7")
            fields["loudRms"] = _xmm_f64("XMM6")
        else:
            fields["finalDr"] = _xmm_f64("XMM7")
            fields["overallRms"] = _xmm_f64("XMM10")
    elif event == "CHANNEL_PUBLISHED":
        session = _reg("R13")
        channel = _reg("R12") & 0xFFFFFFFF
        session_snapshot = _session(session)
        channels = session_snapshot["channels"]
        assert isinstance(channels, int)
        if not 0 <= channel < channels:
            raise ProbeBoundsError("published channel index is out of range")
        result = checked_pointer(_reg("RBP"), "track result")
        arrays = _published_arrays(result, channels)
        fields = {
            "result": _hex(result),
            "channel": channel,
            "dr": arrays["channelDr"][channel],
            "primaryPeak": arrays["primaryPeak"][channel],
            "overallRms": arrays["overallRms"][channel],
        }
    elif event == "TRACK_INTERNAL":
        fields = {
            "session": _hex(checked_pointer(_reg("R13"), "session")),
            "result": _hex(checked_pointer(_reg("RBP"), "track result")),
            "trackDrF64": _xmm_f64("XMM9"),
        }
    elif event == "TRACK_PUBLISHED":
        fields = {
            "phase": "analyzer_session_complete",
            "session": _hex(
                checked_pointer(_reg("R13"), "published track session")
            ),
            "result": _track_published(_reg("RBP")),
        }
    elif event == "ALBUM_ENTRY":
        state = checked_pointer(_reg("RCX"), "album state")
        fields = {
            "state": _hex(state),
            "trackTable": _hex(
                checked_pointer(_u64(state + 0x28), "album track table")
            ),
            "lengthWeighting": bool(_read(state + 0x10C, 1)[0]),
        }
    elif event == "ALBUM_INCLUDED":
        state = checked_pointer(_reg("RSI"), "album state")
        table = checked_pointer(_u64(state + 0x28), "album track table")
        length_weighting = bool(_read(state + 0x10C, 1)[0])
        offset = _reg("R15")
        record, index = album_record_from_offset(table, offset)
        count = _reg("RDI")
        if not 1 <= count <= MAX_ALBUM_TRACKS:
            raise ProbeBoundsError("album group count exceeds safety bound")
        fields = {
            "state": _hex(state),
            "recordOffset": _hex(offset),
            "currentRecord": _album_input_record(record, index),
            "groupCount": count,
            "lengthWeighting": length_weighting,
            "unweightedSum": _xmm_f64("XMM6"),
            "weightedFieldsValid": length_weighting,
        }
        if length_weighting:
            fields["weightedNumerator"] = _xmm_f64("XMM7")
            fields["durationSum"] = _xmm_f64("XMM8")
    elif event == "ALBUM_COMPUTED":
        state = checked_pointer(_reg("RSI"), "album state")
        length_weighting = bool(_read(state + 0x10C, 1)[0])
        start = _reg("RBX")
        count = _reg("RDI")
        album_group_indices(start, count)
        fields = {
            "state": _hex(state),
            "groupStart": start,
            "groupCount": count,
            "lengthWeighting": length_weighting,
            "unweightedF64": _xmm_f64("XMM6"),
            "effectiveF32": _xmm_f32("XMM0"),
            "unweightedF32": _xmm_f32("XMM1"),
        }
        if length_weighting:
            duration = _xmm_f64("XMM8")
            fields["durationSum"] = duration
            duration_value = duration["value"]
            if not isinstance(duration_value, float):
                raise ProbeBoundsError("album duration sum is non-finite")
            weighted_valid = duration_value > 0.0
            fields["weightedFieldsValid"] = weighted_valid
            if weighted_valid:
                fields["weightedF64"] = _xmm_f64("XMM7")
                fields["effectiveSource"] = "weighted"
            else:
                fields["effectiveSource"] = "unweighted_fallback_zero_duration"
        else:
            fields["weightedFieldsValid"] = False
            fields["effectiveSource"] = "unweighted_weighting_disabled"
    elif event == "ALBUM_WRITTEN":
        state = checked_pointer(_reg("RSI"), "album state")
        start = _reg("RBX")
        count = _reg("RDI")
        indices = album_group_indices(start, count)
        fields = {
            "state": _hex(state),
            "groupStart": start,
            "groupCount": count,
            "members": [_album_member(state, index) for index in indices],
        }
    elif event == "ALBUM_EMPTY_PRE_WRITE":
        state = checked_pointer(_reg("RSI"), "album state")
        index = _reg("RBX")
        album_group_indices(index, 1)
        if _reg("RDI") != 0:
            raise ProbeBoundsError("empty-group probe has nonzero group count")
        table = checked_pointer(_u64(state + 0x28), "album track table")
        record, _ = album_record_from_offset(
            table, index * TRACK_RESULT_SIZE
        )
        fields = {
            "state": _hex(state),
            "index": index,
            "record": _hex(record),
            "timing": "effective sentinel store has not executed",
            "pendingEffectiveSentinelBits": "0xbf800000",
        }
    elif event == "RENDER_UNWEIGHTED":
        record = checked_pointer(
            _reg("RAX") + _reg("RCX"), "renderer record"
        )
        fields = {"record": _hex(record), "unweightedF32": _xmm_f32("XMM0")}
    elif event == "RENDER_WEIGHTED":
        record = checked_pointer(
            _reg("RAX") + _reg("RCX"), "renderer record"
        )
        fields = {"record": _hex(record), "effectiveF32": _xmm_f32("XMM1")}
    else:
        raise RuntimeError(f"unhandled probe event: {event}")
    return fields


def _capture_event(tid: int, rva: int, event: str) -> bool:
    global _capture_process_epoch, _capture_process_id, _capture_started
    if _invalid_reasons or _capture_aborted:
        _emit_status(
            "capture_rejected",
            reason=(
                "capture_is_permanently_invalid"
                if _invalid_reasons
                else "capture_was_aborted"
            ),
            invalidReasons=list(_invalid_reasons),
            action="process_left_paused",
        )
        return False
    if not _configured():
        _emit_status(
            "capture_rejected",
            reason="identity_not_configured",
            action="process_left_paused",
        )
        return False
    if _capture_started and (
        _process_id != _capture_process_id
        or _process_epoch != _capture_process_epoch
    ):
        _latch_invalid("capture_process_identity_changed")
        _emit_status(
            "event_correlation_rejected",
            event=event,
            reason="capture_process_identity_changed",
            captureProcessId=_capture_process_id,
            captureProcessEpoch=_capture_process_epoch,
            currentProcessId=_process_id,
            currentProcessEpoch=_process_epoch,
            action="process_left_paused",
        )
        return False
    if not _exception_policy_applied:
        _emit_status(
            "capture_rejected",
            reason="exception_policy_is_not_applied",
            action="process_left_paused",
        )
        return False
    try:
        session, result, push_frames, push_pcm = _event_bindings(event)
        _core_lifecycle.validate_event(
            event,
            session,
            result,
            push_frames=push_frames,
            push_pcm=push_pcm,
        )
    except Exception as error:
        _latch_invalid(f"event_correlation_rejected:{event}:{error}")
        _emit_status(
            "event_correlation_rejected",
            event=event,
            rva=_hex(rva),
            threadId=tid,
            reason=str(error),
            errorType=type(error).__name__,
            activeSession=(
                None
                if _core_lifecycle.active_session is None
                else _hex(_core_lifecycle.active_session)
            ),
            activeResult=(
                None
                if _core_lifecycle.active_result is None
                else _hex(_core_lifecycle.active_result)
            ),
            action="process_left_paused",
        )
        return False
    if not _capture_started:
        if event != "INIT_ENTRY":
            _emit_status(
                "capture_rejected",
                reason="first_event_is_not_analyzer_initializer",
                event=event,
                action="process_left_paused",
            )
            return False
        if _process_id is None:
            _latch_invalid("capture_started_without_process_identity")
            _emit_status(
                "capture_rejected",
                reason="process_identity_is_unavailable",
                action="process_left_paused",
            )
            return False
        if not _emit_status(
            "capture_started",
            moduleBase=_hex(_module_base or 0),
            breakpointCount=len(_owned_breakpoints),
        ):
            return False
        _capture_process_id = _process_id
        _capture_process_epoch = _process_epoch
        _capture_started = True

    base_record = {
        "schemaVersion": 1,
        "recordType": "event",
        **_identity(),
        "runId": _run_id,
        "sequence": _next_sequence(),
        "threadId": tid,
        "event": event,
        "moduleBase": _hex(_module_base or 0),
        "rva": _hex(rva),
        "correlation": {
            "bindingClass": (
                "internally_correlated_core_diagnostic"
                if event in CORE_EVENTS
                else "unbound_peripheral_diagnostic"
            ),
            "session": None if session is None else _hex(session),
            "result": None if result is None else _hex(result),
            "pushFrames": push_frames,
            "pushPcm": None if push_pcm is None else _hex(push_pcm),
        },
    }
    try:
        base_record["fields"] = _snapshot(event)
    except Exception as error:
        _latch_invalid(f"snapshot_failed:{event}:{type(error).__name__}")
        base_record.update(
            {
                "outcome": "snapshot_failed",
                "errorType": type(error).__name__,
                "error": str(error),
            }
        )
        _write_record(base_record)
        _emit_status(
            "snapshot_failed",
            event=event,
            rva=_hex(rva),
            threadId=tid,
            action="process_left_paused",
        )
        return False
    base_record["outcome"] = "captured"
    if not _write_record(base_record):
        return False
    try:
        _core_lifecycle.accept_event(
            event,
            session,
            result,
            push_frames=push_frames,
            push_pcm=push_pcm,
        )
    except Exception as error:
        _latch_invalid(f"lifecycle_commit_failed:{event}")
        _emit_status(
            "lifecycle_commit_failed",
            event=event,
            reason=str(error),
            errorType=type(error).__name__,
            action="process_left_paused",
        )
        return False
    return True


def _verify_owned_breakpoints(
    context: str, *, require_arm_confirmed: bool = True
) -> bool:
    global _last_breakpoint_integrity
    issues: list[dict[str, Any]] = []
    if _module_base is None:
        issues.append({"reason": "module_base_unset"})
        expected: set[int] = set()
    else:
        expected = {_module_base + rva for rva in PROBES}
    if require_arm_confirmed and not _arm_confirmed:
        issues.append({"reason": "arm_not_confirmed"})
    if _owned_breakpoints != expected:
        issues.append(
            {
                "reason": "python_ownership_set_mismatch",
                "expected": [_hex(address) for address in sorted(expected)],
                "owned": [
                    _hex(address)
                    for address in sorted(_owned_breakpoints)
                ],
            }
        )

    for address in sorted(expected):
        breakpoint = ida_dbg.bpt_t()
        if not ida_dbg.get_bpt(address, breakpoint):
            issues.append(
                {"address": _hex(address), "reason": "missing"}
            )
            continue
        if not breakpoint.enabled():
            issues.append(
                {"address": _hex(address), "reason": "disabled"}
            )
        if int(breakpoint.type) != int(ida_idd.BPT_SOFT):
            issues.append(
                {
                    "address": _hex(address),
                    "reason": "wrong_type",
                    "actualType": int(breakpoint.type),
                    "expectedType": int(ida_idd.BPT_SOFT),
                }
            )

    _last_breakpoint_integrity = {
        "context": context,
        "valid": not issues,
        "issues": issues,
    }
    if not issues:
        return True
    _latch_invalid(f"breakpoint_integrity_failed:{context}")
    _emit_status(
        "breakpoint_integrity_failed",
        context=context,
        issues=issues,
        action="process_left_paused",
    )
    return False


def _remove_owned_breakpoints() -> bool:
    global _arm_confirmed, _module_base
    _arm_confirmed = False
    failed: list[str] = []
    for address in tuple(_owned_breakpoints):
        if ida_dbg.exist_bpt(address) and not ida_dbg.del_bpt(address):
            failed.append(_hex(address))
            continue
        _owned_breakpoints.discard(address)
    if failed:
        _latch_invalid("breakpoint_cleanup_failed")
        _emit_status(
            "breakpoint_cleanup_failed",
            addresses=failed,
            action="ownership_retained",
        )
        return False
    _module_base = None
    return True


def _install_breakpoints(module_base: int) -> bool:
    global _arm_confirmed, _module_base
    arm_block = _arm_block_reason()
    if arm_block is not None:
        _emit_status(
            "arm_rejected",
            reason=arm_block,
            moduleBase=_hex(module_base),
        )
        return False
    checked_pointer(module_base, "loaded module base")
    if _module_base is not None and _module_base != module_base:
        _latch_invalid("target_module_base_changed_while_owned")
        _emit_status(
            "arm_rejected",
            reason="target_module_base_changed_while_owned",
            armedModuleBase=_hex(_module_base),
            moduleBase=_hex(module_base),
        )
        return False
    if not _apply_cpp_exception_policy():
        return False

    if _owned_breakpoints:
        return _verify_owned_breakpoints("idempotent_arm")

    _arm_confirmed = False
    addresses = {module_base + rva for rva in PROBES}
    conflicts = sorted(address for address in addresses if ida_dbg.exist_bpt(address))
    if conflicts:
        _latch_invalid("breakpoint_conflict")
        _emit_status(
            "breakpoint_conflict",
            moduleBase=_hex(module_base),
            addresses=[_hex(address) for address in conflicts],
            action="no_breakpoints_installed",
        )
        return False

    added: list[int] = []
    for address in sorted(addresses):
        if not ida_dbg.add_bpt(address, 0, ida_idd.BPT_SOFT):
            _latch_invalid("breakpoint_install_failed")
            rollback_failed: list[str] = []
            for owned in reversed(added):
                if not ida_dbg.del_bpt(owned):
                    rollback_failed.append(_hex(owned))
                else:
                    _owned_breakpoints.discard(owned)
            if rollback_failed:
                _module_base = module_base
            _emit_status(
                "breakpoint_install_failed",
                failedAddress=_hex(address),
                rollbackFailed=rollback_failed,
                action="capture_unarmed",
            )
            return False
        added.append(address)
        _owned_breakpoints.add(address)

    _module_base = module_base
    if not _verify_owned_breakpoints(
        "post_install", require_arm_confirmed=False
    ):
        _remove_owned_breakpoints()
        return False

    if not _emit_status(
        "breakpoints_armed",
        moduleBase=_hex(module_base),
        breakpointCount=len(_owned_breakpoints),
        rvas=[_hex(rva) for rva in sorted(PROBES)],
    ):
        _remove_owned_breakpoints()
        return False
    _arm_confirmed = True
    return True


def _scan_loaded_modules() -> bool:
    global _last_module_scan
    arm_block = _arm_block_reason()
    if arm_block is not None:
        _emit_status("module_scan_rejected", reason=arm_block)
        return False

    matches: list[int] = []
    module = ida_idd.modinfo_t()
    if ida_dbg.get_first_module(module):
        while True:
            if _module_leaf(module.name) == TARGET_MODULE:
                matches.append(int(module.base))
            if not ida_dbg.get_next_module(module):
                break
    _last_module_scan = {
        "matchCount": len(matches),
        "moduleBases": [_hex(base) for base in sorted(matches)],
    }
    if len(matches) > 1:
        _latch_invalid("target_module_leaf_is_not_unique")
        _emit_status(
            "target_module_ambiguous",
            **_last_module_scan,
            action="no_breakpoints_installed_or_moved",
        )
        return False
    if not matches:
        return True
    return _install_breakpoints(matches[0])


def configure(
    run_id: str,
    fixture_id: str,
    expected_input_sha256: str,
    attested_input_sha256: str,
    attested_loaded_module_sha256: str,
) -> bool:
    """Configure explicit run identity, then arm an already loaded target."""

    global _attested_input_sha256, _attested_loaded_module_sha256
    global _expected_input_sha256, _fixture_id, _run_id
    if _configured() or _capture_started or _owned_breakpoints:
        _emit_status(
            "configuration_rejected",
            reason="capture_identity_is_immutable_for_one_fixture_run",
        )
        return False
    try:
        normalized_run_id = validate_identifier(run_id, "run ID")
        normalized_fixture_id = validate_identifier(fixture_id, "fixture ID")
        expected_input, attested_input = validate_input_attestation(
            expected_input_sha256, attested_input_sha256
        )
        attested = validate_attested_sha256(attested_loaded_module_sha256)
    except ProbeBoundsError as error:
        _emit_status(
            "configuration_rejected",
            reason=str(error),
        )
        return False
    _run_id = normalized_run_id
    _fixture_id = normalized_fixture_id
    _expected_input_sha256 = expected_input
    _attested_input_sha256 = attested_input
    _attested_loaded_module_sha256 = attested
    if not _apply_cpp_exception_policy():
        _run_id = None
        _fixture_id = None
        _expected_input_sha256 = None
        _attested_input_sha256 = None
        _attested_loaded_module_sha256 = None
        return False
    if not _emit_status("configured"):
        _restore_cpp_exception_policy()
        _run_id = None
        _fixture_id = None
        _expected_input_sha256 = None
        _attested_input_sha256 = None
        _attested_loaded_module_sha256 = None
        return False
    return _scan_loaded_modules()


def complete_capture() -> bool:
    """Emit explicit completion while paused, then remove owned breakpoints."""

    global _capture_completed, _export_ready
    if not _capture_started:
        _emit_status("completion_rejected", reason="capture_never_started")
        return False
    if ida_dbg.get_process_state() != ida_dbg.DSTATE_SUSP:
        _emit_status(
            "completion_rejected",
            reason="debugger_process_is_not_suspended",
        )
        return False
    if not _verify_owned_breakpoints("completion"):
        return False
    if _invalid_reasons:
        _emit_status(
            "completion_rejected",
            reason="capture_is_permanently_invalid",
            invalidReasons=list(_invalid_reasons),
            action="process_left_paused",
        )
        return False
    if (
        _process_id != _capture_process_id
        or _process_epoch != _capture_process_epoch
    ):
        _latch_invalid("completion_process_identity_changed")
        _emit_status(
            "completion_rejected",
            reason="completion_process_identity_changed",
            action="process_left_paused",
        )
        return False
    if not _core_lifecycle.is_complete():
        _emit_status(
            "completion_rejected",
            reason="single_fixture_lifecycle_is_incomplete",
            initializerCount=_core_lifecycle.initializer_count,
            dataPushCount=_core_lifecycle.data_push_count,
            finishCount=_core_lifecycle.finish_count,
            tailFlushCount=_core_lifecycle.tail_flush_count,
            trackPublishedCount=_core_lifecycle.track_published_count,
            action="process_left_paused",
        )
        return False
    if _record_overflow:
        _emit_status("completion_rejected", reason="record_limit_was_exceeded")
        return False
    if not _remove_owned_breakpoints():
        _emit_status(
            "completion_rejected",
            reason="breakpoint_cleanup_failed",
        )
        return False
    if not _restore_cpp_exception_policy():
        _emit_status(
            "completion_rejected",
            reason="exception_policy_restore_failed",
        )
        return False
    if not _emit_status(
        "capture_completed",
        eventCountSequence=_sequence,
        initializerCount=_core_lifecycle.initializer_count,
        dataPushCount=_core_lifecycle.data_push_count,
        finishCount=_core_lifecycle.finish_count,
        tailFlushCount=_core_lifecycle.tail_flush_count,
        trackPublishedCount=_core_lifecycle.track_published_count,
        activeSession=_hex(_core_lifecycle.active_session),
        activeResult=_hex(_core_lifecycle.active_result),
    ):
        return False
    _capture_completed = True
    _export_ready = True
    return True


def mark_complete() -> bool:
    return complete_capture()


def records_jsonl() -> str:
    global _records_exported
    if not _export_ready or not _capture_completed:
        raise RuntimeError(
            "diagnostic capture is not internally complete; "
            "call mark_complete() first"
        )
    _records_exported = True
    return "\n".join(_records) + ("\n" if _records else "")


def abort_capture() -> bool:
    """Stabilize an invalid run without discarding its machine records."""

    global _capture_aborted
    if not _invalid_reasons:
        _emit_status("abort_rejected", reason="capture_is_not_invalid")
        return False
    if (
        _process_id is not None
        and ida_dbg.get_process_state() != ida_dbg.DSTATE_SUSP
    ):
        _emit_status(
            "abort_rejected",
            reason="debugger_process_is_not_suspended",
        )
        return False
    if not _remove_owned_breakpoints():
        return False
    if not _restore_cpp_exception_policy():
        return False
    if not _emit_status(
        "capture_aborted",
        invalidReasons=list(_invalid_reasons),
    ):
        return False
    _capture_aborted = True
    return True


def failed_records_jsonl() -> str:
    """Export a stabilized invalid diagnostic run."""

    global _records_exported
    if not _capture_aborted or not _invalid_reasons:
        raise RuntimeError("invalid capture is not stabilized; call abort_capture()")
    _records_exported = True
    return "\n".join(_records) + ("\n" if _records else "")


def capture_status() -> str:
    return json.dumps(
        {
            "schemaVersion": 1,
            "recordType": "captureStatus",
            **_identity(),
            "runId": _run_id,
            "configured": _configured(),
            "armed": _arm_confirmed,
            "armBlockedReason": _arm_block_reason(),
            "targetModuleBase": (
                None if _module_base is None else _hex(_module_base)
            ),
            "breakpointIntegrity": dict(_last_breakpoint_integrity),
            "moduleScan": dict(_last_module_scan),
            "captureStarted": _capture_started,
            "captureCompleted": _capture_completed,
            "captureAborted": _capture_aborted,
            "captureInvalid": bool(_invalid_reasons),
            "invalidReasons": list(_invalid_reasons),
            "captureProcessId": _capture_process_id,
            "captureProcessEpoch": _capture_process_epoch,
            "exportReady": _export_ready,
            "recordCount": len(_records),
            "maxRecords": MAX_RECORDS,
            "recordOverflow": _record_overflow,
            "recordsExported": _records_exported,
            "ownedBreakpointCount": len(_owned_breakpoints),
            "exceptionPolicyApplied": _exception_policy_applied,
            "activeSession": (
                None
                if _core_lifecycle.active_session is None
                else _hex(_core_lifecycle.active_session)
            ),
            "activeResult": (
                None
                if _core_lifecycle.active_result is None
                else _hex(_core_lifecycle.active_result)
            ),
            "initializerCount": _core_lifecycle.initializer_count,
            "dataPushCount": _core_lifecycle.data_push_count,
            "finishCount": _core_lifecycle.finish_count,
            "tailFlushCount": _core_lifecycle.tail_flush_count,
            "trackPublishedCount": (
                _core_lifecycle.track_published_count
            ),
        },
        allow_nan=False,
        sort_keys=True,
    )


def reset_capture() -> bool:
    """Reset only after completed or explicitly aborted records were exported."""

    global _attested_input_sha256, _attested_loaded_module_sha256
    global _capture_aborted, _capture_completed, _capture_process_epoch
    global _capture_process_id, _capture_started, _expected_input_sha256
    global _export_ready, _fixture_id, _record_overflow, _records_exported
    global _last_breakpoint_integrity, _last_module_scan, _run_id, _sequence
    completed_exported = (
        _capture_completed and _export_ready and _records_exported
    )
    aborted_exported = (
        _capture_aborted and bool(_invalid_reasons) and _records_exported
    )
    if (
        not (completed_exported or aborted_exported)
        or _owned_breakpoints
        or _exception_policy_applied
    ):
        _emit_status(
            "reset_rejected",
            reason=(
                "capture must be completed-or-aborted, exported, disarmed, "
                "and have its exception policy restored"
            ),
        )
        return False
    _records.clear()
    _sequence = 0
    _record_overflow = False
    _invalid_reasons.clear()
    _records_exported = False
    _export_ready = False
    _capture_started = False
    _capture_completed = False
    _capture_aborted = False
    _capture_process_id = None
    _capture_process_epoch = None
    _core_lifecycle.reset()
    _last_breakpoint_integrity = {
        "context": "not_armed",
        "valid": False,
        "issues": [],
    }
    _last_module_scan = {
        "matchCount": 0,
        "moduleBases": [],
    }
    _run_id = None
    _fixture_id = None
    _expected_input_sha256 = None
    _attested_input_sha256 = None
    _attested_loaded_module_sha256 = None
    return True


def _cleanup_lifecycle(status: str, **fields: Any) -> None:
    global _process_id
    if (
        (_capture_started or _arm_confirmed)
        and not _capture_completed
        and not _capture_aborted
    ):
        _latch_invalid(f"lifecycle_ended_while_armed:{status}")
    _emit_status(status, **fields)
    breakpoints_cleaned = _remove_owned_breakpoints()
    exception_restored = _restore_cpp_exception_policy()
    if not breakpoints_cleaned or not exception_restored:
        _latch_invalid(f"lifecycle_cleanup_failed:{status}")
        _emit_status(
            "lifecycle_cleanup_failed",
            trigger=status,
            breakpointCleanupSucceeded=breakpoints_cleaned,
            exceptionPolicyRestored=exception_restored,
        )
    _process_id = None


def _arm_callback_allowed(callback: str, process_id: int | None) -> bool:
    arm_block = _arm_block_reason()
    if arm_block is not None:
        _emit_status(
            f"{callback}_not_armed",
            processId=process_id,
            reason=arm_block,
        )
        if not _configured():
            _restore_cpp_exception_policy()
        return False
    if _capture_started and (
        process_id != _capture_process_id
        or _process_epoch != _capture_process_epoch
    ):
        _latch_invalid("arm_callback_process_identity_changed")
        _emit_status(
            f"{callback}_not_armed",
            processId=process_id,
            reason="arm_callback_process_identity_changed",
            captureProcessId=_capture_process_id,
            captureProcessEpoch=_capture_process_epoch,
            currentProcessEpoch=_process_epoch,
        )
        return False
    return True


class ProbeHooks(ida_dbg.DBG_Hooks):
    def dbg_process_start(
        self,
        pid: int,
        tid: int,
        ea: int,
        modinfo_name: str,
        modinfo_base: int,
        modinfo_size: int,
    ) -> None:
        global _process_epoch, _process_id
        del tid, ea, modinfo_size
        _process_epoch += 1
        _process_id = pid
        if not _arm_callback_allowed("process_start", pid):
            return
        if not _apply_cpp_exception_policy():
            return
        if not _emit_status("process_started", processId=pid):
            return
        del modinfo_name, modinfo_base
        _scan_loaded_modules()

    def dbg_process_attach(
        self,
        pid: int,
        tid: int,
        ea: int,
        modinfo_name: str,
        modinfo_base: int,
        modinfo_size: int,
    ) -> None:
        global _process_epoch, _process_id
        del tid, ea, modinfo_size
        _process_epoch += 1
        _process_id = pid
        if not _arm_callback_allowed("process_attach", pid):
            return
        if not _apply_cpp_exception_policy():
            return
        if not _emit_status("process_attached", processId=pid):
            return
        del modinfo_name, modinfo_base
        _scan_loaded_modules()

    def dbg_process_exit(
        self, pid: int, tid: int, ea: int, exit_code: int
    ) -> None:
        del tid, ea
        _cleanup_lifecycle(
            "process_exited", processId=pid, exitCode=exit_code
        )

    def dbg_process_detach(self, pid: int, tid: int, ea: int) -> None:
        del tid, ea
        _cleanup_lifecycle("process_detached", processId=pid)

    def dbg_library_load(
        self,
        pid: int,
        tid: int,
        ea: int,
        modinfo_name: str,
        modinfo_base: int,
        modinfo_size: int,
    ) -> None:
        del tid, ea, modinfo_size
        if _module_leaf(modinfo_name) == TARGET_MODULE:
            if not _arm_callback_allowed("target_module", pid):
                return
            if _module_base is not None and modinfo_base != _module_base:
                _latch_invalid("second_target_module_base_loaded")
                _emit_status(
                    "target_module_not_armed",
                    reason="second_target_module_base_loaded",
                    armedModuleBase=_hex(_module_base),
                    loadedModuleBase=_hex(modinfo_base),
                )
                return
            if not _apply_cpp_exception_policy():
                return
            if not _emit_status(
                "target_module_loaded", moduleBase=_hex(modinfo_base)
            ):
                return
            _scan_loaded_modules()

    def dbg_library_unload(
        self, pid: int, tid: int, ea: int, info: str
    ) -> None:
        del pid, tid, ea
        if TARGET_MODULE in info.casefold():
            _cleanup_lifecycle("target_module_unloaded")

    def dbg_bpt(self, tid: int, bptea: int) -> int:
        if _module_base is None or bptea not in _owned_breakpoints:
            return 0
        if not _verify_owned_breakpoints("dispatch"):
            return 0
        current_thread = ida_dbg.get_current_thread()
        if current_thread != tid:
            _latch_invalid("breakpoint_thread_context_mismatch")
            _emit_status(
                "capture_rejected",
                reason="breakpoint_thread_context_mismatch",
                callbackThreadId=tid,
                currentThreadId=current_thread,
                action="process_left_paused",
            )
            return 0
        rva = bptea - _module_base
        event = PROBES.get(rva)
        if event is None:
            _latch_invalid("owned_breakpoint_has_no_probe_event")
            _emit_status(
                "owned_breakpoint_unknown",
                address=_hex(bptea),
                action="process_left_paused",
            )
            return 0
        if not _capture_event(tid, rva, event):
            return 0
        if not ida_dbg.request_continue_process():
            _latch_invalid(f"continue_request_failed:{event}")
            _emit_status(
                "continue_failed",
                event=event,
                rva=_hex(rva),
                threadId=tid,
                action="process_left_paused",
            )
        return 0

    def dbg_request_error(
        self, failed_command: int, failed_dbg_notification: int
    ) -> None:
        _latch_invalid("asynchronous_debugger_request_failed")
        _emit_status(
            "debugger_request_failed",
            failedCommand=failed_command,
            failedNotification=failed_dbg_notification,
            action="do_not_assume_execution_continued",
        )


def install_hooks() -> bool:
    global _hooks
    if _hooks is not None:
        return True
    if not _apply_cpp_exception_policy():
        return False
    _hooks = ProbeHooks()
    if not _hooks.hook():
        _hooks = None
        _emit_status("hook_install_failed")
        _restore_cpp_exception_policy()
        return False
    if not _emit_status(
        "hooks_installed",
        nextAction=(
            "call configure(run_id, fixture_id, expected_input_sha256, "
            "attested_input_sha256, attested_loaded_module_sha256) before target load"
        ),
    ):
        if not _hooks.unhook():
            _latch_invalid("hook_rollback_unhook_failed")
            return False
        _hooks = None
        _restore_cpp_exception_policy()
        return False
    if _configured():
        _scan_loaded_modules()
    return True


def uninstall() -> bool:
    global _capture_aborted, _hooks
    if _capture_started and not _capture_completed:
        _latch_invalid("operator_uninstall_before_completion")
        _capture_aborted = True
        _emit_status(
            "capture_aborted",
            reason="operator_uninstall_before_completion",
            invalidReasons=list(_invalid_reasons),
        )
    cleaned = _remove_owned_breakpoints()
    restored = _restore_cpp_exception_policy()
    if not cleaned or not restored:
        _emit_status(
            "uninstall_failed",
            breakpointCleanupSucceeded=cleaned,
            exceptionPolicyRestored=restored,
        )
        return False
    if _hooks is not None:
        if not _hooks.unhook():
            _latch_invalid("hook_uninstall_failed")
            _emit_status("uninstall_failed", reason="hook_uninstall_failed")
            return False
        _hooks = None
    registry = getattr(builtins, REGISTRY_KEY, None)
    if isinstance(registry, dict):
        registry["active"] = False
    _emit_status(
        "hooks_uninstalled",
        breakpointCleanupSucceeded=True,
        exceptionPolicyRestored=True,
    )
    return True


if not install_hooks():
    raise RuntimeError("failed to install fail-closed probe hooks/policy")
setattr(
    builtins,
    REGISTRY_KEY,
    {
        "active": True,
        "configure": configure,
        "mark_complete": mark_complete,
        "records_jsonl": records_jsonl,
        "abort_capture": abort_capture,
        "failed_records_jsonl": failed_records_jsonl,
        "capture_status": capture_status,
        "reset_capture": reset_capture,
        "uninstall": uninstall,
    },
)
