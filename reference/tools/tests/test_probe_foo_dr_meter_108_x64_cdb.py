from __future__ import annotations

import re
import unittest
from pathlib import Path


TOOLS = Path(__file__).resolve().parents[1]
POWERSHELL = TOOLS / "probe_foo_dr_meter_108_x64_cdb.ps1"
CDB_COMMANDS = TOOLS / "probe_foo_dr_meter_108_x64.cdb"

PLUGIN_SHA256 = (
    "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489"
)
FOOBAR_SHA256 = (
    "653cc120c146aaae9e6db9b6f19e5a1588407b8940bc1521f0ced739ff8924b0"
)


class CdbProbeHarnessContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.powershell = POWERSHELL.read_text(encoding="utf-8")
        cls.cdb = CDB_COMMANDS.read_text(encoding="utf-8")

    def test_fixed_binary_identities_and_leaf_command_are_embedded(self) -> None:
        self.assertIn(PLUGIN_SHA256, self.powershell)
        self.assertIn(PLUGIN_SHA256, self.cdb)
        self.assertIn(FOOBAR_SHA256, self.powershell)
        self.assertIn("$contextCommandName = 'Measure Dynamic Range'", self.powershell)
        self.assertIn('"/context_command:$contextCommandName"', self.powershell)
        self.assertNotIn("/context_command:DR Meter/", self.powershell)

    def test_store_cdb_and_live_pid_are_resolved_without_installation(self) -> None:
        self.assertIn("Get-AppxPackage -Name Microsoft.WinDbg -AllUsers", self.powershell)
        self.assertIn("Join-Path $package.InstallLocation 'amd64\\cdb.exe'", self.powershell)
        self.assertIn("[string] $CdbPath", self.powershell)
        self.assertIn("explicit-toolset-source", self.powershell)
        self.assertIn("Copy-CdbToolsetForExecution", self.powershell)
        self.assertIn("verified-private-temp-toolset-copy", self.powershell)
        self.assertIn("Do not execute CDB directly from WindowsApps", self.powershell)
        self.assertIn("cdbExeSha256", self.powershell)
        self.assertIn("sourceToolsetManifest", self.powershell)
        self.assertIn("executionToolsetManifest", self.powershell)
        staging = self.powershell.index(
            "$stageCopy = Copy-CdbToolsetForExecution"
        )
        execution = self.powershell.index(
            "$cdbStart.FileName = $cdbExecutionPath"
        )
        self.assertLess(staging, execution)
        self.assertNotIn(
            "if ($cdbSourceKind -eq 'store-package-read-only-source')",
            self.powershell,
        )
        self.assertIn("Get-Process -Name foobar2000", self.powershell)
        self.assertIn("$targetPid = $foobarProcess.Id", self.powershell)
        forbidden = ("winget", "choco", "Install-Module", "Invoke-WebRequest")
        for token in forbidden:
            self.assertNotIn(token, self.powershell)

    def test_attach_is_guarded_invasive_logged_and_detach_safe(self) -> None:
        self.assertIn("if ($Arm -and -not $ConfirmFoobarIdle)", self.powershell)
        self.assertIn("if (-not $Arm)", self.powershell)
        argument_blocks = re.findall(
            r"foreach \(\$argument in @\((.*?)\)\) \{",
            self.powershell,
            flags=re.DOTALL,
        )
        arguments = next(
            block for block in argument_blocks if "'-logo'" in block
        )
        self.assertIn("'-p'", arguments)
        self.assertNotIn("'-pv'", arguments)
        self.assertIn("'-pd'", arguments)
        self.assertIn("'-logo'", arguments)
        self.assertIn("'-cf'", arguments)
        self.assertIn("Request-CdbDetach", self.powershell)
        self.assertIn("$cdbCommandExecutionPath", self.powershell)
        self.assertIn("cdbCommandSourceSha256", self.powershell)
        self.assertIn("cdbCommandExecutionCopySha256", self.powershell)
        self.assertIn("$cdbCommandGuard = [IO.File]::Open(", self.powershell)
        self.assertIn("guarded CDB command-file copy changed", self.powershell)
        self.assertNotIn("Stop-Process", self.powershell)
        self.assertNotIn(".kill", self.cdb)

    def test_breakpoints_are_module_rvas_with_expected_lifecycle(self) -> None:
        breakpoints = re.findall(
            r"^bp(?P<id>[0-3]) "
            r"(?P<address>foo_dr_meter\+0x[0-9a-f]+) \"(?P<command>.*)\"$",
            self.cdb,
            flags=re.MULTILINE,
        )
        self.assertEqual(
            [(identifier, address) for identifier, address, _ in breakpoints],
            [
                ("0", "foo_dr_meter+0x8410"),
                ("1", "foo_dr_meter+0x89f0"),
                ("2", "foo_dr_meter+0x8df0"),
                ("3", "foo_dr_meter+0x91f0"),
            ],
        )
        for _, _, command in breakpoints[:-1]:
            self.assertTrue(command.endswith("; gc"))
        terminal = breakpoints[-1][2]
        self.assertIn("@@MM_CDB_TERMINAL_BEGIN event=TRACK_PUBLISHED", terminal)
        self.assertIn("session=%p result=%p", terminal)
        self.assertIn("@r13, @rbp", terminal)
        self.assertIn("@@MM_CDB_SNAPSHOT_COMPLETE event=TRACK_PUBLISHED", terminal)
        self.assertIn("@$t1 < 1", terminal)
        self.assertIn("@$t1 > 64", terminal)
        self.assertIn("@@MM_CDB_INVALID", terminal)
        self.assertIn("@$t0 < @$t1", terminal)
        self.assertNotIn("@$t0 < dwo(@rbp+0xc)", terminal)
        self.assertTrue(terminal.endswith("; bc *; qd } }"))
        self.assertNotRegex(self.cdb, r"\b0x1800[0-9a-f]+\b")
        self.assertTrue(self.cdb.rstrip().endswith("g"))

    def test_exception_policy_precedes_breakpoints_and_go(self) -> None:
        clear = self.cdb.index('sx- -c "" -c2 "" e06d7363')
        handling = self.cdb.index("sxn -h e06d7363")
        policy = self.cdb.index("sxn e06d7363")
        query = self.cdb.index("\nsx\n")
        first_breakpoint = self.cdb.index("bp0 foo_dr_meter+0x8410")
        go = self.cdb.rindex("\ng")
        self.assertLess(clear, handling)
        self.assertLess(handling, policy)
        self.assertLess(policy, query)
        self.assertLess(policy, first_breakpoint)
        self.assertLess(policy, go)
        self.assertIn(
            "@@MM_CDB_EXCEPTION_POLICY_QUERY_BEGIN code=e06d7363",
            self.cdb,
        )
        self.assertIn(
            "@@MM_CDB_EXCEPTION_POLICY_QUERY_END code=e06d7363",
            self.cdb,
        )
        self.assertNotIn("POLICY_CONFIRMED", self.cdb)
        self.assertIn("Assert-CdbExceptionPolicy", self.powershell)
        self.assertIn(
            "@@MM_CDB_EXCEPTION_POLICY_CONFIRMED",
            self.powershell,
        )
        self.assertIn("confirmed-from-sx-query-before-trigger", self.powershell)

    def test_preflight_checks_hash_architecture_collision_and_existing_debugger(
        self,
    ) -> None:
        self.assertIn("Get-PeMachine", self.powershell)
        self.assertGreaterEqual(self.powershell.count("0x8664"), 4)
        self.assertIn("Find-ExactContextCommandOwners", self.powershell)
        self.assertIn("CheckRemoteDebuggerPresent", self.powershell)
        self.assertIn("Refusing to overwrite", self.powershell)
        self.assertIn("exactly one foobar2000 process", self.powershell)
        self.assertIn("exactly one loaded foo_dr_meter.dll", self.powershell)

    def test_terminal_probe_captures_raw_result_and_channel_bits(self) -> None:
        self.assertIn("track_dr_bits=0x%08x", self.cdb)
        self.assertIn("channel_count=%u", self.cdb)
        self.assertIn("frames=0x%I64x", self.cdb)
        self.assertIn("MM_CDB_CHANNEL index=%u", self.cdb)
        self.assertIn("dr_bits=0x%08x", self.cdb)
        self.assertIn("peak_bits=0x%08x", self.cdb)
        self.assertIn("rms_bits=0x%08x", self.cdb)

    def test_input_is_locked_and_rehashed_before_forwarding(self) -> None:
        reparse_check = self.powershell.index(
            "Assert-InputFileIsNotReparsePoint "
            "-Attributes $inputItem.Attributes"
        )
        initial_hash = self.powershell.index(
            "$inputSha256 = Get-LowerSha256 -LiteralPath $inputItem.FullName"
        )
        guard = self.powershell.index("$inputGuard = [IO.File]::Open(")
        armed = self.powershell.index("Assert-CdbBreakpointsArmed")
        forwarder = self.powershell.index(
            "$triggerStart.ArgumentList.Add($contextCommandArgument)"
        )
        self.assertLess(reparse_check, initial_hash)
        self.assertLess(guard, forwarder)
        self.assertLess(armed, forwarder)
        self.assertIn("[IO.FileShare]::Read", self.powershell)
        self.assertGreaterEqual(self.powershell.count("Get-StreamSha256"), 3)
        self.assertIn("function New-GuardedStagedInput", self.powershell)
        self.assertIn(
            "-SourceStream $inputGuard",
            self.powershell,
        )
        self.assertIn(
            "$triggerStart.ArgumentList.Add($stagedInputPath)",
            self.powershell,
        )
        self.assertNotIn(
            "$triggerStart.ArgumentList.Add($inputItem.FullName)",
            self.powershell,
        )
        self.assertIn("privateStagedCopy", self.powershell)
        self.assertIn("forwarderInput = 'private-staged-copy'", self.powershell)
        self.assertIn("heldReadLock = $true", self.powershell)
        self.assertIn(
            "Source-handle and private staged-input evidence diverged",
            self.powershell,
        )
        for extension in (".wav", ".flac", ".aif", ".aiff"):
            self.assertIn(f"'{extension}'", self.powershell)
        self.assertIn("[IO.FileAttributes]::ReparsePoint", self.powershell)
        self.assertIn(
            "InputFile itself must not be a reparse point or symbolic link",
            self.powershell,
        )

    def test_raw_log_is_private_and_normalized_output_is_allowlisted(self) -> None:
        self.assertIn("Write-NormalizedCdbMarkers", self.powershell)
        self.assertIn("Assert-NoAbsolutePathText", self.powershell)
        self.assertIn("private-local-raw-input-do-not-commit", self.powershell)
        self.assertIn("strict allowlist; path-scanned", self.powershell)
        self.assertNotIn("name = $inputItem.Name", self.powershell)
        self.assertIn("fixtureId = $FixtureId", self.powershell)
        self.assertIn("rawCdbLog", self.powershell)
        self.assertIn("recordCount", self.powershell)
        self.assertIn("harnessScriptSha256", self.powershell)
        self.assertIn("powershellVersion", self.powershell)
        self.assertIn("osVersion", self.powershell)

    def test_completion_requires_snapshot_and_detached_live_target(self) -> None:
        self.assertIn("Assert-CdbFixtureLifecycle -LogText $logText", self.powershell)
        self.assertIn("Assert-CdbSnapshotComplete -LogText $logText", self.powershell)
        self.assertIn("Assert-TargetDetachedAndAlive", self.powershell)
        self.assertIn("completed-and-detached", self.powershell)
        self.assertIn("failed-and-detached", self.powershell)
        self.assertIn("failed-detach-unconfirmed", self.powershell)
        self.assertIn("CDB exited with code", self.powershell)
        self.assertIn("@@MM_CDB_INVALID", self.powershell)
        self.assertIn("@@MM_CDB_SNAPSHOT_COMPLETE", self.powershell)

    def test_lifecycle_validator_is_strict_and_pointer_bound(self) -> None:
        self.assertIn("function Assert-CdbFixtureLifecycle", self.powershell)
        self.assertIn("exactly one ARMED marker", self.powershell)
        self.assertIn("lifecycle evidence before ARMED", self.powershell)
        self.assertIn(
            "no pre-FINISH data PUSH_ENTRY",
            self.powershell,
        )
        self.assertIn(
            "data PUSH had a null PCM pointer or zero frames",
            self.powershell,
        )
        self.assertIn(
            "post-FINISH flush PUSH_ENTRY event",
            self.powershell,
        )
        self.assertIn(
            "post-FINISH flush PUSH was not pcm=0, frames=0",
            self.powershell,
        )
        self.assertIn("PUSH session did not match INIT session", self.powershell)
        self.assertIn("FINISH session did not match INIT session", self.powershell)
        self.assertIn("terminal session did not match INIT session", self.powershell)
        self.assertIn(
            "FINISH result did not match terminal result pointer",
            self.powershell,
        )
        self.assertIn(
            "extra markers after the terminal lifecycle",
            self.powershell,
        )

    def test_bl_parser_accepts_legacy_and_modern_formats(self) -> None:
        self.assertIn("'legacy-bl-format'", self.powershell)
        self.assertIn("'modern-bl-disable-clear-format'", self.powershell)
        self.assertIn("(?:Disable\\s+Clear\\s+)?", self.powershell)
        self.assertIn(
            "0 e 00000001`80008410 0001 (0001)",
            self.powershell,
        )
        self.assertIn(
            "0 e Disable Clear 00000001`80008410 0001 (0001)",
            self.powershell,
        )

    def test_self_test_mode_is_synthetic_and_precedes_live_preflight(self) -> None:
        self.assertIn("ParameterSetName = 'SelfTest'", self.powershell)
        self.assertIn("function Invoke-CdbHarnessSelfTest", self.powershell)
        self.assertIn("status = 'self-tests-passed'", self.powershell)
        self_test_gate = self.powershell.index("if ($SelfTest)")
        live_process = self.powershell.index(
            "$processes = @(Get-Process -Name foobar2000"
        )
        self.assertLess(self_test_gate, live_process)
        for case in (
            "exception-policy-command-rejection",
            "lifecycle-order-rejection",
            "post-finish-flush-rejection",
            "finish-terminal-pointer-mismatch",
            "invalid-terminal-rejection",
            "memory-error-rejection",
            "absolute-path-rejection",
            "input-reparse-point-rejection",
            "input-extension-allowlist",
            "guarded-staged-input-copy-and-lock",
        ):
            self.assertIn(case, self.powershell)


if __name__ == "__main__":
    unittest.main()
