from __future__ import annotations

import importlib.util
import math
import struct
import sys
import unittest
from pathlib import Path


MODULE_PATH = (
    Path(__file__).resolve().parents[1] / "foo_dr_meter_108_probe_common.py"
)
SPEC = importlib.util.spec_from_file_location("probe_common", MODULE_PATH)
assert SPEC is not None
assert SPEC.loader is not None
probe = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = probe
SPEC.loader.exec_module(probe)


class ProbeCommonTests(unittest.TestCase):
    def test_attestation_requires_fixed_hash(self) -> None:
        self.assertEqual(
            probe.validate_attested_sha256(
                probe.EXPECTED_TARGET_SHA256.upper()
            ),
            probe.EXPECTED_TARGET_SHA256,
        )
        with self.assertRaises(probe.ProbeBoundsError):
            probe.validate_attested_sha256("0" * 64)

    def test_fixture_identity_is_path_free_and_input_hash_matches(self) -> None:
        self.assertEqual(
            probe.validate_identifier("core:110_histogram", "fixture"),
            "core:110_histogram",
        )
        with self.assertRaises(probe.ProbeBoundsError):
            probe.validate_identifier("../fixture.wav", "fixture")

        expected, attested = probe.validate_input_attestation(
            "a" * 64, "A" * 64
        )
        self.assertEqual(expected, "a" * 64)
        self.assertEqual(attested, "a" * 64)
        with self.assertRaises(probe.ProbeBoundsError):
            probe.validate_input_attestation("a" * 64, "b" * 64)

    def test_exception_policy_save_verify_restore_round_trip(self) -> None:
        break_flag = 0x01
        handle_flag = 0x02
        message_flag = 0x04
        silent_flag = 0x08
        unrelated_flag = 0x20
        saved = (
            break_flag
            | handle_flag
            | message_flag
            | unrelated_flag,
            "Microsoft C++ exception",
            "original description",
        )
        temporary = probe.temporary_exception_flags(
            saved[0],
            break_flag=break_flag,
            handle_flag=handle_flag,
            message_flag=message_flag,
            silent_flag=silent_flag,
        )
        self.assertTrue(
            probe.exception_policy_is_applied(
                temporary,
                break_flag=break_flag,
                handle_flag=handle_flag,
                message_flag=message_flag,
                silent_flag=silent_flag,
            )
        )
        self.assertEqual(temporary & unrelated_flag, unrelated_flag)
        self.assertTrue(
            probe.restored_exception_matches(saved, tuple(saved))
        )
        self.assertTrue(probe.restored_exception_matches(None, None))
        self.assertFalse(
            probe.restored_exception_matches(saved, (temporary, *saved[1:]))
        )

    def test_core_lifecycle_binds_session_result_and_tail_flush(self) -> None:
        lifecycle = probe.CoreCaptureLifecycle()
        session = 0x10000
        result = 0x20000

        with self.assertRaisesRegex(
            probe.ProbeBoundsError,
            "core_event_before_analyzer_initializer",
        ):
            lifecycle.validate_event(
                "PUSH_ENTRY",
                session,
                None,
                push_frames=64,
                push_pcm=0x30000,
            )

        lifecycle.accept_event("INIT_ENTRY", session, None)
        with self.assertRaisesRegex(
            probe.ProbeBoundsError,
            "core_event_session_does_not_match_initializer",
        ):
            lifecycle.validate_event(
                "PUSH_ENTRY",
                session + 8,
                None,
                push_frames=64,
                push_pcm=0x30000,
            )
        lifecycle.accept_event(
            "PUSH_ENTRY",
            session,
            None,
            push_frames=64,
            push_pcm=0x30000,
        )
        with self.assertRaisesRegex(
            probe.ProbeBoundsError, "finish_result_is_null"
        ):
            lifecycle.validate_event("FINISH_ENTRY", session, None)
        lifecycle.accept_event("FINISH_ENTRY", session, result)

        with self.assertRaisesRegex(
            probe.ProbeBoundsError,
            "post_finish_push_is_not_zero_tail_flush",
        ):
            lifecycle.validate_event(
                "PUSH_ENTRY",
                session,
                None,
                push_frames=1,
                push_pcm=0x30000,
            )
        lifecycle.accept_event(
            "PUSH_ENTRY",
            session,
            None,
            push_frames=0,
            push_pcm=0,
        )
        with self.assertRaisesRegex(
            probe.ProbeBoundsError,
            "core_result_does_not_match_finish_result",
        ):
            lifecycle.validate_event(
                "TRACK_INTERNAL", session, result + 8
            )
        lifecycle.accept_event("TRACK_INTERNAL", session, result)
        lifecycle.accept_event("TRACK_PUBLISHED", session, result)
        self.assertTrue(lifecycle.is_complete())

        with self.assertRaisesRegex(
            probe.ProbeBoundsError, "core_event_after_track_published"
        ):
            lifecycle.validate_event(
                "PUSH_ENTRY",
                session,
                None,
                push_frames=1,
                push_pcm=0x30000,
            )
        lifecycle.accept_event("ALBUM_ENTRY", None, None)

        lifecycle.reset()
        self.assertEqual(lifecycle.initializer_count, 0)
        self.assertEqual(lifecycle.data_push_count, 0)
        self.assertEqual(lifecycle.finish_count, 0)
        self.assertEqual(lifecycle.tail_flush_count, 0)
        self.assertEqual(lifecycle.track_published_count, 0)
        self.assertIsNone(lifecycle.active_session)
        self.assertIsNone(lifecycle.active_result)
        self.assertFalse(lifecycle.is_complete())

    def test_histogram_index_is_channel_local(self) -> None:
        self.assertEqual(probe.histogram_bin(10001 + 37, 1, 2), 37)
        with self.assertRaises(probe.ProbeBoundsError):
            probe.histogram_bin(37, 1, 2)
        with self.assertRaises(probe.ProbeBoundsError):
            probe.histogram_bin(20002, 1, 2)

    def test_album_record_uses_table_plus_byte_offset(self) -> None:
        address, index = probe.album_record_from_offset(
            0x0000010000000000, 3 * 0x58
        )
        self.assertEqual(address, 0x0000010000000108)
        self.assertEqual(index, 3)
        with self.assertRaises(probe.ProbeBoundsError):
            probe.album_record_from_offset(0x0000010000000000, 3)

    def test_album_group_is_bounded(self) -> None:
        self.assertEqual(list(probe.album_group_indices(3, 2)), [3, 4])
        with self.assertRaises(probe.ProbeBoundsError):
            probe.album_group_indices(probe.MAX_ALBUM_TRACKS - 1, 2)

    def test_array_requires_pointer_capacity_and_count(self) -> None:
        pointer = 0x0000010000000000
        self.assertEqual(probe.checked_array(pointer, 8, 6, "test"), pointer)
        with self.assertRaises(probe.ProbeBoundsError):
            probe.checked_array(pointer, 5, 6, "test")
        with self.assertRaises(probe.ProbeBoundsError):
            probe.checked_array(0, 8, 6, "test")

    def test_float_record_is_finite_json_or_explicit_string(self) -> None:
        finite = probe.float_record(struct.pack("<f", 1.25))
        self.assertEqual(finite["bits"], "0x3fa00000")
        self.assertEqual(finite["value"], 1.25)

        nonfinite = probe.float_record(struct.pack("<d", math.inf))
        self.assertEqual(nonfinite["value"], "inf")
        with self.assertRaises(probe.ProbeBoundsError):
            probe.float_record(b"\0")


if __name__ == "__main__":
    unittest.main()
