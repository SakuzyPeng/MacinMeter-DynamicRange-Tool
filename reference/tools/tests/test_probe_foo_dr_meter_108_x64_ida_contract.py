from __future__ import annotations

import ast
import unittest
from pathlib import Path


SCRIPT = (
    Path(__file__).resolve().parents[1]
    / "probe_foo_dr_meter_108_x64_ida.py"
)
SOURCE = SCRIPT.read_text(encoding="utf-8")
TREE = ast.parse(SOURCE, filename=str(SCRIPT))


def function_source(name: str) -> str:
    for node in ast.walk(TREE):
        if isinstance(node, ast.FunctionDef) and node.name == name:
            segment = ast.get_source_segment(SOURCE, node)
            assert segment is not None
            return segment
    raise AssertionError(f"missing function: {name}")


def literal_assignment(name: str) -> object:
    for node in TREE.body:
        if not isinstance(node, ast.Assign):
            continue
        if any(
            isinstance(target, ast.Name) and target.id == name
            for target in node.targets
        ):
            return ast.literal_eval(node.value)
    raise AssertionError(f"missing literal assignment: {name}")


class IdaProbeOfflineContractTests(unittest.TestCase):
    def test_breakpoint_rvas_are_unique_and_avoid_shared_joins(self) -> None:
        probes = literal_assignment("PROBES")
        self.assertIsInstance(probes, dict)
        assert isinstance(probes, dict)
        self.assertEqual(len(probes), 20)
        self.assertEqual(len(set(probes)), len(probes))
        self.assertEqual(probes[0x8D86], "WINDOW_COMMIT_PRE_RESET")
        self.assertEqual(probes[0xEA33], "ALBUM_EMPTY_PRE_WRITE")
        self.assertNotIn(0x8D8A, probes)
        self.assertNotIn(0xEA3B, probes)

    def test_breakpoint_arm_and_dispatch_are_all_or_none(self) -> None:
        install_source = function_source("_install_breakpoints")
        verify_source = function_source("_verify_owned_breakpoints")
        breakpoint_source = function_source("dbg_bpt")
        completion_source = function_source("complete_capture")
        for contract in (
            "conflicts = sorted(",
            "if not ida_dbg.add_bpt(",
            "rollback_failed",
            "_verify_owned_breakpoints(",
            "if not _emit_status(",
            '"breakpoints_armed"',
            "_arm_confirmed = True",
        ):
            self.assertIn(contract, install_source)
        for live_contract in (
            "breakpoint = ida_dbg.bpt_t()",
            "ida_dbg.get_bpt(address, breakpoint)",
            "breakpoint.enabled()",
            "breakpoint.type",
            "ida_idd.BPT_SOFT",
            "_owned_breakpoints != expected",
        ):
            self.assertIn(live_contract, verify_source)
        self.assertIn(
            '_verify_owned_breakpoints("dispatch")',
            breakpoint_source,
        )
        self.assertIn(
            '_verify_owned_breakpoints("completion")',
            completion_source,
        )
        self.assertIn('action="process_left_paused"', breakpoint_source)

    def test_invalid_or_aborted_capture_cannot_arm_or_dispatch(self) -> None:
        arm_gate_source = function_source("_arm_block_reason")
        install_source = function_source("_install_breakpoints")
        capture_source = function_source("_capture_event")
        callback_source = function_source("_arm_callback_allowed")
        for state in ("_invalid_reasons", "_capture_aborted"):
            self.assertIn(state, arm_gate_source)
            self.assertIn(state, capture_source)
        self.assertIn("_arm_block_reason()", install_source)
        self.assertIn("_arm_block_reason()", callback_source)

    def test_module_scan_requires_unique_leaf_and_base_is_immutable(self) -> None:
        scan_source = function_source("_scan_loaded_modules")
        install_source = function_source("_install_breakpoints")
        load_source = function_source("dbg_library_load")
        self.assertIn("matches: list[int] = []", scan_source)
        self.assertIn("len(matches) > 1", scan_source)
        self.assertIn('"target_module_ambiguous"', scan_source)
        self.assertIn("matches[0]", scan_source)
        self.assertIn("_module_base != module_base", install_source)
        self.assertIn('"target_module_base_changed_while_owned"', install_source)
        self.assertIn("modinfo_base != _module_base", load_source)
        self.assertIn('"second_target_module_base_loaded"', load_source)

    def test_exception_policy_write_is_saved_verified_and_restored(self) -> None:
        apply_source = function_source("_apply_cpp_exception_policy")
        restore_source = function_source("_restore_cpp_exception_policy")
        self.assertIn("_saved_cpp_exception =", apply_source)
        self.assertIn("ida_dbg.store_exceptions()", apply_source)
        self.assertIn("exception_policy_is_applied(", apply_source)
        self.assertIn("_restore_cpp_exception_policy()", apply_source)
        self.assertLess(
            apply_source.index("_exception_policy_applied = True"),
            apply_source.index("verification = ida_dbg.retrieve_exceptions()"),
        )
        self.assertIn("restored_exception_matches(", restore_source)
        self.assertIn("reason=\"restored_policy_verification_failed\"", restore_source)

    def test_track_published_exposes_only_core_valid_fields(self) -> None:
        published_source = function_source("_track_published")
        for field in (
            '"trackDr"',
            '"channels"',
            '"sampleRate"',
            '"frames"',
            '"channelDr"',
            '"primaryPeak"',
            '"overallRms"',
        ):
            self.assertIn(field, published_source)
        for invalid_at_core_publish in (
            '"effective"',
            '"unweighted"',
            '"hostMetadata"',
            "result + 0x04",
            "result + 0x08",
            "result + 0x10",
            "result + 0x18",
        ):
            self.assertNotIn(invalid_at_core_publish, published_source)

    def test_records_require_completion_and_reset_requires_export(self) -> None:
        records_source = function_source("records_jsonl")
        failed_records_source = function_source("failed_records_jsonl")
        reset_source = function_source("reset_capture")
        self.assertIn("not _export_ready or not _capture_completed", records_source)
        self.assertIn("_records_exported = True", records_source)
        self.assertIn("_capture_aborted", failed_records_source)
        self.assertIn("_invalid_reasons", failed_records_source)
        for required_guard in (
            "completed_exported",
            "aborted_exported",
            "_owned_breakpoints",
            "_exception_policy_applied",
        ):
            self.assertIn(required_guard, reset_source)
        for reset_action in (
            "_records.clear()",
            "_sequence = 0",
            "_invalid_reasons.clear()",
            "_core_lifecycle.reset()",
            "_run_id = None",
            "_fixture_id = None",
        ):
            self.assertIn(reset_action, reset_source)

    def test_single_fixture_state_machine_is_wired_to_every_event(self) -> None:
        binding_source = function_source("_event_bindings")
        capture_source = function_source("_capture_event")
        completion_source = function_source("complete_capture")
        status_source = function_source("capture_status")
        self.assertIn('"TRACK_PUBLISHED"', binding_source)
        self.assertIn('session_register = "R13"', binding_source)
        self.assertIn('result = checked_pointer(_reg("RBP")', binding_source)
        self.assertIn("push_frames = _reg(\"R8\")", binding_source)
        self.assertIn("push_pcm = _reg(\"RDX\")", binding_source)
        self.assertIn("_core_lifecycle.validate_event(", capture_source)
        self.assertIn("_core_lifecycle.accept_event(", capture_source)
        self.assertIn("_core_lifecycle.is_complete()", completion_source)
        for field in (
            '"activeSession"',
            '"activeResult"',
            '"initializerCount"',
            '"dataPushCount"',
            '"finishCount"',
            '"tailFlushCount"',
            '"trackPublishedCount"',
            '"armBlockedReason"',
            '"breakpointIntegrity"',
            '"moduleScan"',
            '"targetModuleBase"',
        ):
            self.assertIn(field, status_source)

    def test_failure_latch_blocks_completion_and_preserves_abort_evidence(self) -> None:
        completion_source = function_source("complete_capture")
        abort_source = function_source("abort_capture")
        capture_source = function_source("_capture_event")
        breakpoint_source = function_source("dbg_bpt")
        request_error_source = function_source("dbg_request_error")
        self.assertIn("if _invalid_reasons:", completion_source)
        self.assertIn('"capture_is_permanently_invalid"', completion_source)
        self.assertIn("_latch_invalid(", capture_source)
        self.assertIn("_latch_invalid(", breakpoint_source)
        self.assertIn("_latch_invalid(", request_error_source)
        self.assertIn("_remove_owned_breakpoints()", abort_source)
        self.assertIn("_restore_cpp_exception_policy()", abort_source)
        self.assertIn("_capture_aborted", SOURCE)

    def test_evidence_boundary_is_operator_attested_diagnostic(self) -> None:
        identity_source = function_source("_identity")
        capture_source = function_source("_capture_event")
        self.assertIn(
            '"evidenceClass": "operator_attested_diagnostic"',
            identity_source,
        )
        self.assertIn(
            '"consistencyLevel": "internally_consistent"',
            identity_source,
        )
        self.assertIn(
            '"internally_correlated_core_diagnostic"',
            capture_source,
        )

    def test_snapshot_or_log_failure_never_requests_continue(self) -> None:
        capture_source = function_source("_capture_event")
        breakpoint_source = function_source("dbg_bpt")
        self.assertNotIn("request_continue_process", capture_source)
        capture_call = "if not _capture_event(tid, rva, event):"
        continue_call = "if not ida_dbg.request_continue_process():"
        self.assertIn(capture_call, breakpoint_source)
        self.assertIn(continue_call, breakpoint_source)
        self.assertLess(
            breakpoint_source.index(capture_call),
            breakpoint_source.index(continue_call),
        )
        self.assertIn('base_record["outcome"] = "captured"', capture_source)
        self.assertIn("if not _write_record(base_record):", capture_source)


if __name__ == "__main__":
    unittest.main()
