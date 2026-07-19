#requires -Version 7.3

<#
.SYNOPSIS
Preflights or deliberately runs the fixed foo_dr_meter 1.0.8 x64 CDB probe.

.DESCRIPTION
Without -Arm this script is read-only: it resolves the live foobar2000 PID and
either an explicit executable CDB copy or the installed Microsoft Store CDB
source, verifies the fixed foobar2000 and plugin hashes, verifies x64 PE
identity, checks the input hash, and rejects an ambiguous
"Measure Dynamic Range" leaf command.

With both -Arm and -ConfirmFoobarIdle it invasively attaches CDB to the already
running foobar2000 process. An explicit -CdbPath is preferred. Its complete
parent tool directory, or otherwise the installed Store package's complete
amd64 tool directory, is copied to a private, random per-run temporary
directory before CDB is executed; WindowsApps is never used as an executable
location. Source and execution-copy manifests must match. The bundled CDB
command file uses module-plus-RVA breakpoints, logs through -logo,
auto-continues nonterminal probes, and executes qd at TRACK_PUBLISHED so
foobar2000 remains running.

Use exactly one small, fixed input per run. This is not a batch harness.
In armed mode, input bytes are copied from the verified source handle into a
private ordinary file with an allowlisted audio extension. That staged copy is
flushed, rehashed, held read-locked, and is the only path passed to foobar2000.
The -logo file is private raw debugger input: never commit or publish it. Only
the strict allowlisted marker file and its path-free metadata sidecar are
candidate repository artifacts.

.EXAMPLE
.\probe_foo_dr_meter_108_x64_cdb.ps1 -SelfTest

Runs only parser/validator self-tests over synthetic logs. It does not inspect
foobar2000, copy a debugger toolset, attach CDB, or write probe artifacts.

.EXAMPLE
.\probe_foo_dr_meter_108_x64_cdb.ps1 `
  -RunId exact-window-01 `
  -FixtureId exact-window-f64-stereo `
  -InputFile 'D:\corpus\110_exact_window_f64_stereo.wav' `
  -ExpectedInputSha256 '<64 lowercase hex characters>' `
  -LogPath 'D:\probe-output\exact-window-01.cdb.log'

Runs only the read-only preflight.

.EXAMPLE
.\probe_foo_dr_meter_108_x64_cdb.ps1 `
  -RunId exact-window-01 `
  -FixtureId exact-window-f64-stereo `
  -InputFile 'D:\corpus\110_exact_window_f64_stereo.wav' `
  -ExpectedInputSha256 '<64 lowercase hex characters>' `
  -LogPath 'D:\probe-output\exact-window-01.cdb.log' `
  -Arm -ConfirmFoobarIdle

After deliberate operator confirmation, invokes the equivalent of:

  foobar2000.exe /context_command:"Measure Dynamic Range" <one-file>

and performs the invasive probe.

.NOTES
Run from an elevated PowerShell 7.3 or newer session in the same user profile
as foobar2000. Do not run while another scan or modal foobar2000 dialog is
active. The command-line launcher must be able to forward from the invoking
session to the existing interactive foobar2000 process.

If -CdbPath is supplied, it must name x64 cdb.exe outside WindowsApps in a
dedicated debugger-tool directory; its entire parent directory is staged and
bound. If it is omitted, the script locates Store WinDbg read-only and stages
its complete amd64 directory under the invoking account's temporary directory.
No package is installed or modified.

The normal and timeout paths request qd. The -pd CDB option is also present so
an unexpected debugger exit does not intentionally terminate foobar2000. If
the scripted detach itself fails, do not terminate foobar2000; record the CDB
PID from the error and coordinate recovery in the interactive Windows session.
Never use q without -pd and never use .kill.
#>

[CmdletBinding(DefaultParameterSetName = 'Probe')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Probe')]
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$')]
    [string] $RunId,

    [Parameter(Mandatory, ParameterSetName = 'Probe')]
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$')]
    [string] $FixtureId,

    [Parameter(Mandatory, ParameterSetName = 'Probe')]
    [ValidateNotNullOrEmpty()]
    [string] $InputFile,

    [Parameter(Mandatory, ParameterSetName = 'Probe')]
    [ValidatePattern('^[0-9A-Fa-f]{64}$')]
    [string] $ExpectedInputSha256,

    [Parameter(Mandatory, ParameterSetName = 'Probe')]
    [ValidateNotNullOrEmpty()]
    [string] $LogPath,

    [Parameter(ParameterSetName = 'Probe')]
    [ValidateNotNullOrEmpty()]
    [string] $NormalizedPath,

    [Parameter(ParameterSetName = 'Probe')]
    [ValidateNotNullOrEmpty()]
    [string] $CdbPath,

    [Parameter(ParameterSetName = 'Probe')]
    [ValidateRange(5, 120)]
    [int] $AttachTimeoutSeconds = 30,

    [Parameter(ParameterSetName = 'Probe')]
    [ValidateRange(10, 3600)]
    [int] $CompletionTimeoutSeconds = 300,

    [Parameter(ParameterSetName = 'Probe')]
    [switch] $Arm,

    [Parameter(ParameterSetName = 'Probe')]
    [switch] $ConfirmFoobarIdle,

    [Parameter(Mandatory, ParameterSetName = 'SelfTest')]
    [switch] $SelfTest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$expectedFoobarSha256 =
    '653cc120c146aaae9e6db9b6f19e5a1588407b8940bc1521f0ced739ff8924b0'
$expectedPluginSha256 =
    'ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489'
$contextCommandName = 'Measure Dynamic Range'
$contextCommandArgument = "/context_command:$contextCommandName"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class MacinMeterCdbProbeNative {
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool CheckRemoteDebuggerPresent(
        IntPtr process,
        [MarshalAs(UnmanagedType.Bool)] ref bool present);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool DebugBreakProcess(IntPtr process);
}
'@

function Get-LowerSha256 {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    (Get-FileHash -LiteralPath $LiteralPath -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-StreamSha256 {
    param(
        [Parameter(Mandatory)]
        [IO.Stream] $Stream
    )

    $originalPosition = $Stream.Position
    $hasher = [Security.Cryptography.SHA256]::Create()
    try {
        $Stream.Position = 0
        [Convert]::ToHexString(
            $hasher.ComputeHash($Stream)
        ).ToLowerInvariant()
    }
    finally {
        $Stream.Position = $originalPosition
        $hasher.Dispose()
    }
}

function Get-ArtifactEvidence {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    $item = Get-Item -LiteralPath $LiteralPath
    [ordered]@{
        bytes = $item.Length
        sha256 = Get-LowerSha256 -LiteralPath $item.FullName
    }
}

function Assert-InputFileIsNotReparsePoint {
    param(
        [Parameter(Mandatory)]
        [IO.FileAttributes] $Attributes
    )

    if (($Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw (
            'InputFile itself must not be a reparse point or symbolic link; ' +
            'foobar2000 must reopen the same ordinary file that was hashed.'
        )
    }
}

function Get-ProbeInputExtension {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    $extension = [IO.Path]::GetExtension($LiteralPath).ToLowerInvariant()
    if ($extension -notin @('.wav', '.flac', '.aif', '.aiff')) {
        throw (
            'InputFile must use one of the restricted probe extensions: ' +
            '.wav, .flac, .aif, or .aiff.'
        )
    }
    $extension
}

function New-GuardedStagedInput {
    param(
        [Parameter(Mandatory)]
        [IO.Stream] $SourceStream,

        [Parameter(Mandatory)]
        [string] $StageDirectory,

        [Parameter(Mandatory)]
        [string] $Extension,

        [Parameter(Mandatory)]
        [Int64] $ExpectedBytes,

        [Parameter(Mandatory)]
        [ValidatePattern('^[0-9a-f]{64}$')]
        [string] $ExpectedSha256
    )

    if ($Extension -notin @('.wav', '.flac', '.aif', '.aiff')) {
        throw 'The staged input extension is not allowlisted.'
    }
    $inputDirectory = Join-Path $StageDirectory 'input'
    [void] [IO.Directory]::CreateDirectory($inputDirectory)
    if (
        ((Get-Item -LiteralPath $inputDirectory).Attributes -band
            [IO.FileAttributes]::ReparsePoint) -ne 0
    ) {
        throw 'The private staged-input directory became a reparse point.'
    }

    $stagedPath = Join-Path $inputDirectory "fixture$Extension"
    $copyStream = $null
    $stagedGuard = $null
    $sourcePosition = $SourceStream.Position
    try {
        $copyStream = [IO.File]::Open(
            $stagedPath,
            [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write,
            [IO.FileShare]::None
        )
        $SourceStream.Position = 0
        $SourceStream.CopyTo($copyStream)
        $copyStream.Flush($true)
        $copyStream.Dispose()
        $copyStream = $null

        $stagedItem = Get-Item -LiteralPath $stagedPath
        if (
            ($stagedItem.Attributes -band
                [IO.FileAttributes]::ReparsePoint) -ne 0
        ) {
            throw 'The private staged input became a reparse point.'
        }
        $stagedGuard = [IO.File]::Open(
            $stagedPath,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::Read
        )
        $stagedSha256 = Get-StreamSha256 -Stream $stagedGuard
        if (
            $stagedGuard.Length -ne $ExpectedBytes -or
            $stagedSha256 -ne $ExpectedSha256
        ) {
            throw 'The private staged input did not match the source handle.'
        }

        [pscustomobject]@{
            path = $stagedPath
            guard = $stagedGuard
            bytes = $stagedGuard.Length
            sha256 = $stagedSha256
            extension = $Extension
        }
        $stagedGuard = $null
    }
    catch {
        if ($null -ne $stagedGuard) {
            $stagedGuard.Dispose()
        }
        Remove-Item -LiteralPath $stagedPath -Force `
            -ErrorAction SilentlyContinue
        throw
    }
    finally {
        $SourceStream.Position = $sourcePosition
        if ($null -ne $copyStream) {
            $copyStream.Dispose()
        }
    }
}

function Get-DirectoryManifestEvidence {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    $root = [IO.Path]::GetFullPath($LiteralPath)
    $files = @(
        [IO.Directory]::EnumerateFiles(
            $root,
            '*',
            [IO.SearchOption]::AllDirectories
        ) | Sort-Object
    )
    if ($files.Count -lt 1) {
        throw "CDB tool directory is empty: $root"
    }

    $builder = [Text.StringBuilder]::new()
    [Int64] $totalBytes = 0
    foreach ($file in $files) {
        $item = Get-Item -LiteralPath $file
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "CDB tool directory contains a reparse-point file: $file"
        }
        $relative = [IO.Path]::GetRelativePath($root, $item.FullName)
        $relative = $relative.Replace('\', '/')
        $sha256 = Get-LowerSha256 -LiteralPath $item.FullName
        [void] $builder.Append($relative)
        [void] $builder.Append([char] 0)
        [void] $builder.Append($item.Length)
        [void] $builder.Append([char] 0)
        [void] $builder.Append($sha256)
        [void] $builder.Append("`n")
        $totalBytes += $item.Length
    }

    $hasher = [Security.Cryptography.SHA256]::Create()
    try {
        $manifestBytes = [Text.Encoding]::UTF8.GetBytes($builder.ToString())
        $manifestSha256 = [Convert]::ToHexString(
            $hasher.ComputeHash($manifestBytes)
        ).ToLowerInvariant()
    }
    finally {
        $hasher.Dispose()
    }

    [ordered]@{
        fileCount = $files.Count
        totalBytes = $totalBytes
        manifestSha256 = $manifestSha256
    }
}

function Set-PrivateDirectoryAcl {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    if (-not $IsWindows) {
        throw 'The CDB execution-copy ACL can only be created on Windows.'
    }

    $acl = Get-Acl -LiteralPath $LiteralPath
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($existingRule in @($acl.Access)) {
        [void] $acl.RemoveAccessRuleSpecific($existingRule)
    }

    $identities = @(
        [Security.Principal.WindowsIdentity]::GetCurrent().User,
        [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
    )
    $inheritance =
        [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
        [Security.AccessControl.InheritanceFlags]::ObjectInherit
    foreach ($identity in $identities) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $identity,
            [Security.AccessControl.FileSystemRights]::FullControl,
            $inheritance,
            [Security.AccessControl.PropagationFlags]::None,
            [Security.AccessControl.AccessControlType]::Allow
        )
        [void] $acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $LiteralPath -AclObject $acl
}

function Copy-CdbToolsetForExecution {
    param(
        [Parameter(Mandatory)]
        [string] $SourceCdbPath,

        [Parameter(Mandatory)]
        [string] $RunId
    )

    $sourceRoot = Split-Path -Parent $SourceCdbPath
    $sourceEvidence = Get-DirectoryManifestEvidence -LiteralPath $sourceRoot
    $privateRoot = Join-Path ([IO.Path]::GetTempPath()) 'MacinMeterCdbProbe'
    [void] [IO.Directory]::CreateDirectory($privateRoot)
    $stageRoot = Join-Path $privateRoot (
        "$RunId-$([Guid]::NewGuid().ToString('N'))"
    )
    [void] [IO.Directory]::CreateDirectory($stageRoot)

    try {
        Set-PrivateDirectoryAcl -LiteralPath $stageRoot
        $toolsetRoot = Join-Path $stageRoot 'toolset'
        [void] [IO.Directory]::CreateDirectory($toolsetRoot)
        foreach ($directory in [IO.Directory]::EnumerateDirectories(
            $sourceRoot,
            '*',
            [IO.SearchOption]::AllDirectories
        )) {
            $item = Get-Item -LiteralPath $directory
            if (
                ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
            ) {
                throw (
                    'CDB tool directory contains a reparse-point directory: ' +
                    $directory
                )
            }
            $relative = [IO.Path]::GetRelativePath(
                $sourceRoot,
                $item.FullName
            )
            [void] [IO.Directory]::CreateDirectory(
                (Join-Path $toolsetRoot $relative)
            )
        }
        foreach ($file in [IO.Directory]::EnumerateFiles(
            $sourceRoot,
            '*',
            [IO.SearchOption]::AllDirectories
        )) {
            $item = Get-Item -LiteralPath $file
            if (
                ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
            ) {
                throw "CDB tool directory contains a reparse-point file: $file"
            }
            $relative = [IO.Path]::GetRelativePath(
                $sourceRoot,
                $item.FullName
            )
            [IO.File]::Copy(
                $item.FullName,
                (Join-Path $toolsetRoot $relative),
                $false
            )
        }

        $stagedEvidence =
            Get-DirectoryManifestEvidence -LiteralPath $toolsetRoot
        if (
            $stagedEvidence.fileCount -ne $sourceEvidence.fileCount -or
            $stagedEvidence.totalBytes -ne $sourceEvidence.totalBytes -or
            $stagedEvidence.manifestSha256 -ne
                $sourceEvidence.manifestSha256
        ) {
            throw 'The private CDB toolset copy did not match its source.'
        }

        $relativeCdb = [IO.Path]::GetRelativePath(
            $sourceRoot,
            ([IO.Path]::GetFullPath($SourceCdbPath))
        )
        [pscustomobject]@{
            root = $stageRoot
            cdbPath = Join-Path $toolsetRoot $relativeCdb
            sourceManifest = $sourceEvidence
            stagedManifest = $stagedEvidence
        }
    }
    catch {
        Remove-Item -LiteralPath $stageRoot -Recurse -Force `
            -ErrorAction SilentlyContinue
        throw
    }
}

function Get-PeMachine {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    $stream = [IO.File]::Open(
        $LiteralPath,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::ReadWrite
    )
    try {
        $reader = [IO.BinaryReader]::new($stream)
        if ($reader.ReadUInt16() -ne 0x5a4d) {
            throw "Not an MZ image: $LiteralPath"
        }
        $stream.Position = 0x3c
        $peOffset = $reader.ReadUInt32()
        if ($peOffset -gt ($stream.Length - 6)) {
            throw "Invalid PE header offset in: $LiteralPath"
        }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "Missing PE signature in: $LiteralPath"
        }
        $reader.ReadUInt16()
    }
    finally {
        $stream.Dispose()
    }
}

function Get-StoreWinDbgPackages {
    $windowsPowerShell = Join-Path $env:SystemRoot (
        'System32\WindowsPowerShell\v1.0\powershell.exe'
    )
    if (-not (Test-Path -LiteralPath $windowsPowerShell -PathType Leaf)) {
        throw "Windows PowerShell was not found: $windowsPowerShell"
    }

    # The inbox Appx module is not loadable in PowerShell 7. Query it through
    # Windows PowerShell 5.1 and return only the two fields needed here.
    $query = @'
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Get-AppxPackage -Name Microsoft.WinDbg -AllUsers |
    Select-Object `
        @{ Name = 'Version'; Expression = { $_.Version.ToString() } },
        InstallLocation |
    ConvertTo-Json -Compress
'@
    $encodedQuery = [Convert]::ToBase64String(
        [Text.Encoding]::Unicode.GetBytes($query)
    )
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $windowsPowerShell
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @(
        '-NoLogo',
        '-NoProfile',
        '-NonInteractive',
        '-EncodedCommand',
        $encodedQuery
    )) {
        [void] $start.ArgumentList.Add($argument)
    }

    $process = [Diagnostics.Process]::Start($start)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $outputText = $stdout.GetAwaiter().GetResult()
    $errorText = $stderr.GetAwaiter().GetResult()
    $exitCode = $process.ExitCode
    $process.Dispose()

    if ($exitCode -ne 0) {
        throw (
            "Windows PowerShell Get-AppxPackage failed with exit code " +
            "${exitCode}: $errorText"
        )
    }
    if ([string]::IsNullOrWhiteSpace($outputText)) {
        return @()
    }
    @($outputText | ConvertFrom-Json)
}

function Test-DebuggerPresent {
    param(
        [Parameter(Mandatory)]
        [Diagnostics.Process] $Process
    )

    $present = $false
    $ok = [MacinMeterCdbProbeNative]::CheckRemoteDebuggerPresent(
        $Process.Handle,
        [ref] $present
    )
    if (-not $ok) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "CheckRemoteDebuggerPresent failed with Win32 error $errorCode."
    }
    $present
}

function Find-ExactContextCommandOwners {
    param(
        [Parameter(Mandatory)]
        [Diagnostics.ProcessModule[]] $Modules,

        [Parameter(Mandatory)]
        [string] $CommandName
    )

    $needle = $CommandName + [char] 0
    $owners = [Collections.Generic.List[object]]::new()

    foreach ($module in $Modules) {
        $bytes = [IO.File]::ReadAllBytes($module.FileName)
        $text = [Text.Encoding]::ASCII.GetString($bytes)
        $offset = 0
        $occurrences = 0
        while (
            ($offset = $text.IndexOf(
                $needle,
                $offset,
                [StringComparison]::Ordinal
            )) -ge 0
        ) {
            $occurrences += 1
            $offset += $needle.Length
        }

        if ($occurrences -gt 0) {
            $owners.Add([pscustomobject]@{
                moduleName = $module.ModuleName
                sha256 = Get-LowerSha256 -LiteralPath $module.FileName
                exactNullTerminatedOccurrences = $occurrences
            })
        }
    }

    $owners.ToArray()
}

function Get-LogText {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    if (-not (Test-Path -LiteralPath $LiteralPath -PathType Leaf)) {
        return ''
    }

    try {
        [IO.File]::ReadAllText($LiteralPath)
    }
    catch [IO.IOException] {
        ''
    }
}

function Assert-NoAbsolutePathText {
    param(
        [Parameter(Mandatory)]
        [string] $Text,

        [Parameter(Mandatory)]
        [string] $ArtifactKind
    )

    $pathPattern = @(
        '(?i)[a-z]:[\\/]',
        '(?i)\\\\[^\\\r\n]+\\',
        '(?i)/(?:Users|home|tmp|private|var|opt|Volumes)/',
        '(?i)\b(?:WindowsApps|AppData|Program Files)\b'
    ) -join '|'
    if ([regex]::IsMatch($Text, $pathPattern)) {
        throw "$ArtifactKind contains text resembling an absolute path."
    }
}

function Write-NormalizedCdbMarkers {
    param(
        [Parameter(Mandatory)]
        [string] $RawLogPath,

        [Parameter(Mandatory)]
        [string] $OutputPath,

        [switch] $ExceptionPolicyConfirmed
    )

    $allowedPatterns = @(
        '^@@MM_CDB_PROBE schema=1 target_sha256=[0-9a-f]{64}$',
        (
            '^@@MM_CDB_EXCEPTION_POLICY_QUERY_(?:BEGIN|END)' +
            ' code=e06d7363$'
        ),
        (
            '^@@MM_CDB_MODULE base=[0-9a-f`]+ init=[0-9a-f`]+' +
            ' push=[0-9a-f`]+ finish=[0-9a-f`]+' +
            ' published=[0-9a-f`]+$'
        ),
        (
            '^@@MM_CDB_EVENT event=(?:INIT_ENTRY|PUSH_ENTRY|FINISH_ENTRY)' +
            ' rva=0x(?:8410|89f0|8df0)$'
        ),
        (
            '^@@MM_CDB_INIT session=[0-9a-f`]+' +
            ' sample_rate=[0-9]+ channels=[0-9]+$'
        ),
        (
            '^@@MM_CDB_PUSH session=[0-9a-f`]+ pcm=[0-9a-f`]+' +
            ' frames=[0-9]+$'
        ),
        (
            '^@@MM_CDB_FINISH session=[0-9a-f`]+' +
            ' result=[0-9a-f`]+ weighting=[0-9]+$'
        ),
        (
            '^@@MM_CDB_TERMINAL_BEGIN event=TRACK_PUBLISHED rva=0x91f0' +
            ' session=[0-9a-f`]+ result=[0-9a-f`]+$'
        ),
        (
            '^@@MM_CDB_RESULT track_dr_bits=0x[0-9a-f]{8}' +
            ' channel_count=[0-9]+ sample_rate=[0-9]+' +
            ' frames=0x[0-9a-f]+$'
        ),
        (
            '^@@MM_CDB_CHANNEL index=[0-9]+ dr_bits=0x[0-9a-f]{8}' +
            ' peak_bits=0x[0-9a-f]{8} rms_bits=0x[0-9a-f]{8}$'
        ),
        (
            '^@@MM_CDB_INVALID reason=(?:null-result-pointer|' +
            'channel-count(?: value=[0-9]+)?)$'
        ),
        '^@@MM_CDB_SNAPSHOT_COMPLETE event=TRACK_PUBLISHED$',
        '^@@MM_CDB_ARMED$'
    )

    $markers = [Collections.Generic.List[string]]::new()
    foreach ($line in [IO.File]::ReadAllLines($RawLogPath)) {
        if (-not $line.StartsWith('@@MM_CDB_', [StringComparison]::Ordinal)) {
            continue
        }
        $allowed = $false
        foreach ($pattern in $allowedPatterns) {
            if ([regex]::IsMatch(
                $line,
                $pattern,
                [Text.RegularExpressions.RegexOptions]::IgnoreCase
            )) {
                $allowed = $true
                break
            }
        }
        if (-not $allowed) {
            throw "Raw CDB log contains an unknown marker-schema record."
        }
        $markers.Add($line)
        if (
            $ExceptionPolicyConfirmed -and
            $line -eq
                '@@MM_CDB_EXCEPTION_POLICY_QUERY_END code=e06d7363'
        ) {
            $markers.Add(
                '@@MM_CDB_EXCEPTION_POLICY_CONFIRMED code=e06d7363 ' +
                'action=notify-no-break-no-handle commands=empty'
            )
        }
    }
    if ($markers.Count -lt 1) {
        throw 'Raw CDB log contains no allowlisted marker records.'
    }

    $normalizedText = [string]::Join("`n", $markers) + "`n"
    Assert-NoAbsolutePathText `
        -Text $normalizedText `
        -ArtifactKind 'Normalized CDB marker artifact'

    $stream = [IO.File]::Open(
        $OutputPath,
        [IO.FileMode]::CreateNew,
        [IO.FileAccess]::Write,
        [IO.FileShare]::None
    )
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($normalizedText)
        $stream.Write($bytes, 0, $bytes.Length)
    }
    finally {
        $stream.Dispose()
    }

    $evidence = Get-ArtifactEvidence -LiteralPath $OutputPath
    $evidence['recordCount'] = $markers.Count
    $evidence
}

function Write-SafeMetadata {
    param(
        [Parameter(Mandatory)]
        [Collections.IDictionary] $Metadata,

        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    $json = $Metadata | ConvertTo-Json -Depth 12
    Assert-NoAbsolutePathText `
        -Text $json `
        -ArtifactKind 'Probe metadata sidecar'
    [IO.File]::WriteAllText(
        $LiteralPath,
        $json,
        [Text.UTF8Encoding]::new($false)
    )
}

function Test-LogMarkerLine {
    param(
        [Parameter(Mandatory)]
        [string] $LogText,

        [Parameter(Mandatory)]
        [string] $Marker
    )

    $pattern = '(?m)^' + [regex]::Escape($Marker) + '\r?$'
    [regex]::IsMatch($LogText, $pattern)
}

function Convert-CdbHexAddress {
    param(
        [Parameter(Mandatory)]
        [string] $Text
    )

    $hex = [regex]::Replace($Text, '[^0-9A-Fa-f]', '')
    [Convert]::ToUInt64($hex, 16)
}

function Assert-CdbBreakpointsArmed {
    param(
        [Parameter(Mandatory)]
        [string] $LogText
    )

    $moduleMatches = [regex]::Matches(
        $LogText,
        '(?im)^@@MM_CDB_MODULE base=(?<base>[0-9a-f`]+) .*\r?$'
    )
    if ($moduleMatches.Count -ne 1) {
        throw 'CDB did not emit exactly one resolved module-base record.'
    }
    $moduleBase = Convert-CdbHexAddress `
        -Text $moduleMatches[0].Groups['base'].Value

    $breakpointMatches = [regex]::Matches(
        $LogText,
        '(?im)^\s*(?<id>[0-3])\s+e\s+' +
            '(?:Disable\s+Clear\s+)?' +
            '(?<address>[0-9a-f`]+)\s+0001\s+\(0001\).*$'
    )
    if ($breakpointMatches.Count -ne 4) {
        throw (
            'CDB did not list exactly four enabled, resolved owned ' +
            'breakpoints.'
        )
    }

    $expectedRvas = @(0x8410L, 0x89f0L, 0x8df0L, 0x91f0L)
    $seenIds = [Collections.Generic.HashSet[int]]::new()
    foreach ($match in $breakpointMatches) {
        $id = [int] $match.Groups['id'].Value
        if (-not $seenIds.Add($id)) {
            throw "CDB listed duplicate breakpoint ID $id."
        }
        $actual = Convert-CdbHexAddress `
            -Text $match.Groups['address'].Value
        $expected = $moduleBase + $expectedRvas[$id]
        if ($actual -ne $expected) {
            throw (
                "CDB breakpoint $id resolved to 0x$($actual.ToString('x')), " +
                "expected 0x$($expected.ToString('x'))."
            )
        }
    }
}

function Assert-CdbExceptionPolicy {
    param(
        [Parameter(Mandatory)]
        [string] $LogText
    )

    $beginMatches = [regex]::Matches(
        $LogText,
        '(?m)^@@MM_CDB_EXCEPTION_POLICY_QUERY_BEGIN code=e06d7363\r?$'
    )
    $endMatches = [regex]::Matches(
        $LogText,
        '(?m)^@@MM_CDB_EXCEPTION_POLICY_QUERY_END code=e06d7363\r?$'
    )
    $armedMatches = [regex]::Matches(
        $LogText,
        '(?m)^@@MM_CDB_ARMED\r?$'
    )
    if (
        $beginMatches.Count -ne 1 -or
        $endMatches.Count -ne 1 -or
        $armedMatches.Count -ne 1
    ) {
        throw (
            'CDB did not emit one exception-policy query followed by one ' +
            'ARMED marker.'
        )
    }
    if (
        $endMatches[0].Index -le $beginMatches[0].Index -or
        $armedMatches[0].Index -le $endMatches[0].Index
    ) {
        throw 'CDB exception-policy query markers are out of order.'
    }

    $queryText = $LogText.Substring(
        $beginMatches[0].Index,
        $endMatches[0].Index - $beginMatches[0].Index
    )
    $policyMatches = [regex]::Matches(
        $queryText,
        '(?im)^\s*(?:e06d7363|eh)\s+-[^\r\n]*-\s+notify\s+-\s+' +
            'not handled\s*$'
    )
    if ($policyMatches.Count -ne 1) {
        throw (
            'The sx query did not confirm exactly one e06d7363/eh policy ' +
            'with Notify and Not Handled status.'
        )
    }

    $afterPolicy = $queryText.Substring(
        $policyMatches[0].Index + $policyMatches[0].Length
    )
    $nextFilter = [regex]::Match(
        $afterPolicy,
        '(?im)^\s*[a-z0-9*][a-z0-9*:\[\].-]*\s+-\s+'
    )
    $policyBlock = if ($nextFilter.Success) {
        $afterPolicy.Substring(0, $nextFilter.Index)
    }
    else {
        $afterPolicy
    }
    if ([regex]::IsMatch(
        $policyBlock,
        '(?im)^\s*(?:Command|Command2|First[^:\r\n]*command|' +
            'Second[^:\r\n]*command)\s*:'
    )) {
        throw 'The e06d7363 exception policy still has an automatic command.'
    }
}

function Get-RequiredCdbMarkerMatch {
    param(
        [Parameter(Mandatory)]
        [object[]] $Records,

        [Parameter(Mandatory)]
        [int] $Index,

        [Parameter(Mandatory)]
        [string] $Pattern,

        [Parameter(Mandatory)]
        [string] $Description
    )

    if ($Index -ge $Records.Count) {
        throw "CDB lifecycle ended before $Description."
    }
    $match = [regex]::Match(
        $Records[$Index].line,
        $Pattern,
        [Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if (-not $match.Success) {
        throw (
            "CDB lifecycle expected $Description, got: " +
            $Records[$Index].line
        )
    }
    $match
}

function Assert-CdbFixtureLifecycle {
    param(
        [Parameter(Mandatory)]
        [string] $LogText
    )

    $records = @(
        foreach ($match in [regex]::Matches(
            $LogText,
            '(?m)^(?<line>@@MM_CDB_[^\r\n]+)\r?$'
        )) {
            [pscustomobject]@{
                line = $match.Groups['line'].Value
                index = $match.Index
            }
        }
    )
    $armedRecords = @(
        $records | Where-Object line -EQ '@@MM_CDB_ARMED'
    )
    if ($armedRecords.Count -ne 1) {
        throw 'CDB lifecycle requires exactly one ARMED marker.'
    }
    $armedIndex = $armedRecords[0].index
    $preArmLifecycle = @(
        $records | Where-Object {
            $_.index -lt $armedIndex -and
            $_.line -match (
                '^@@MM_CDB_(?:EVENT|INIT|PUSH|FINISH|TERMINAL_BEGIN|' +
                'RESULT|CHANNEL|SNAPSHOT_COMPLETE|INVALID)\b'
            )
        }
    )
    if ($preArmLifecycle.Count -ne 0) {
        throw 'CDB emitted lifecycle evidence before ARMED.'
    }

    $afterArm = @($records | Where-Object index -GT $armedIndex)
    $cursor = 0
    [void] (
        Get-RequiredCdbMarkerMatch `
            -Records $afterArm `
            -Index $cursor `
            -Pattern '^@@MM_CDB_EVENT event=INIT_ENTRY rva=0x8410$' `
            -Description 'INIT_ENTRY event'
    )
    $cursor += 1
    $init = Get-RequiredCdbMarkerMatch `
        -Records $afterArm `
        -Index $cursor `
        -Pattern (
            '^@@MM_CDB_INIT session=(?<session>[0-9a-f`]+) ' +
            'sample_rate=[0-9]+ channels=[0-9]+$'
        ) `
        -Description 'INIT payload'
    $cursor += 1
    $session = Convert-CdbHexAddress -Text $init.Groups['session'].Value
    if ($session -eq 0) {
        throw 'CDB INIT session pointer was null.'
    }

    $dataPushCount = 0
    while (
        $cursor -lt $afterArm.Count -and
        $afterArm[$cursor].line -eq
            '@@MM_CDB_EVENT event=PUSH_ENTRY rva=0x89f0'
    ) {
        $cursor += 1
        $push = Get-RequiredCdbMarkerMatch `
            -Records $afterArm `
            -Index $cursor `
            -Pattern (
                '^@@MM_CDB_PUSH session=(?<session>[0-9a-f`]+) ' +
                'pcm=(?<pcm>[0-9a-f`]+) frames=(?<frames>[0-9]+)$'
            ) `
            -Description 'data PUSH payload'
        if (
            (Convert-CdbHexAddress -Text $push.Groups['session'].Value) -ne
                $session
        ) {
            throw 'CDB PUSH session did not match INIT session.'
        }
        if (
            (Convert-CdbHexAddress -Text $push.Groups['pcm'].Value) -eq 0 -or
            [UInt64] $push.Groups['frames'].Value -eq 0
        ) {
            throw 'CDB data PUSH had a null PCM pointer or zero frames.'
        }
        $dataPushCount += 1
        $cursor += 1
    }
    if ($dataPushCount -lt 1) {
        throw 'CDB lifecycle contained no pre-FINISH data PUSH_ENTRY event.'
    }

    [void] (
        Get-RequiredCdbMarkerMatch `
            -Records $afterArm `
            -Index $cursor `
            -Pattern '^@@MM_CDB_EVENT event=FINISH_ENTRY rva=0x8df0$' `
            -Description 'FINISH_ENTRY event'
    )
    $cursor += 1
    $finish = Get-RequiredCdbMarkerMatch `
        -Records $afterArm `
        -Index $cursor `
        -Pattern (
            '^@@MM_CDB_FINISH session=(?<session>[0-9a-f`]+) ' +
            'result=(?<result>[0-9a-f`]+) weighting=[0-9]+$'
        ) `
        -Description 'FINISH payload'
    $cursor += 1
    if (
        (Convert-CdbHexAddress -Text $finish.Groups['session'].Value) -ne
            $session
    ) {
        throw 'CDB FINISH session did not match INIT session.'
    }
    $finishResult =
        Convert-CdbHexAddress -Text $finish.Groups['result'].Value
    if ($finishResult -eq 0) {
        throw 'CDB FINISH result pointer was null.'
    }

    [void] (
        Get-RequiredCdbMarkerMatch `
            -Records $afterArm `
            -Index $cursor `
            -Pattern '^@@MM_CDB_EVENT event=PUSH_ENTRY rva=0x89f0$' `
            -Description 'post-FINISH flush PUSH_ENTRY event'
    )
    $cursor += 1
    $flushPush = Get-RequiredCdbMarkerMatch `
        -Records $afterArm `
        -Index $cursor `
        -Pattern (
            '^@@MM_CDB_PUSH session=(?<session>[0-9a-f`]+) ' +
            'pcm=(?<pcm>[0-9a-f`]+) frames=(?<frames>[0-9]+)$'
        ) `
        -Description 'post-FINISH flush PUSH payload'
    $cursor += 1
    if (
        (Convert-CdbHexAddress -Text $flushPush.Groups['session'].Value) -ne
            $session
    ) {
        throw 'CDB flush PUSH session did not match INIT session.'
    }
    if (
        (Convert-CdbHexAddress -Text $flushPush.Groups['pcm'].Value) -ne 0 -or
        [UInt64] $flushPush.Groups['frames'].Value -ne 0
    ) {
        throw 'CDB post-FINISH flush PUSH was not pcm=0, frames=0.'
    }

    $terminal = Get-RequiredCdbMarkerMatch `
        -Records $afterArm `
        -Index $cursor `
        -Pattern (
            '^@@MM_CDB_TERMINAL_BEGIN event=TRACK_PUBLISHED rva=0x91f0 ' +
            'session=(?<session>[0-9a-f`]+) ' +
            'result=(?<result>[0-9a-f`]+)$'
        ) `
        -Description 'TRACK_PUBLISHED terminal payload'
    $cursor += 1
    if (
        (Convert-CdbHexAddress -Text $terminal.Groups['session'].Value) -ne
            $session
    ) {
        throw 'CDB terminal session did not match INIT session.'
    }
    if (
        (Convert-CdbHexAddress -Text $terminal.Groups['result'].Value) -ne
            $finishResult
    ) {
        throw 'CDB FINISH result did not match terminal result pointer.'
    }

    [void] (
        Get-RequiredCdbMarkerMatch `
            -Records $afterArm `
            -Index $cursor `
            -Pattern (
                '^@@MM_CDB_RESULT track_dr_bits=0x[0-9a-f]{8} ' +
                'channel_count=[0-9]+ sample_rate=[0-9]+ ' +
                'frames=0x[0-9a-f]+$'
            ) `
            -Description 'terminal result'
    )
    $cursor += 1

    $channelCount = 0
    while (
        $cursor -lt $afterArm.Count -and
        $afterArm[$cursor].line -match '^@@MM_CDB_CHANNEL\b'
    ) {
        [void] (
            Get-RequiredCdbMarkerMatch `
                -Records $afterArm `
                -Index $cursor `
                -Pattern (
                    '^@@MM_CDB_CHANNEL index=[0-9]+ ' +
                    'dr_bits=0x[0-9a-f]{8} peak_bits=0x[0-9a-f]{8} ' +
                    'rms_bits=0x[0-9a-f]{8}$'
                ) `
                -Description 'terminal channel row'
        )
        $channelCount += 1
        $cursor += 1
    }
    if ($channelCount -lt 1) {
        throw 'CDB lifecycle contained no terminal channel rows.'
    }

    [void] (
        Get-RequiredCdbMarkerMatch `
            -Records $afterArm `
            -Index $cursor `
            -Pattern (
                '^@@MM_CDB_SNAPSHOT_COMPLETE event=TRACK_PUBLISHED$'
            ) `
            -Description 'terminal snapshot completion'
    )
    $cursor += 1
    if ($cursor -ne $afterArm.Count) {
        throw 'CDB emitted extra markers after the terminal lifecycle.'
    }
}

function Assert-CdbSnapshotComplete {
    param(
        [Parameter(Mandatory)]
        [string] $LogText
    )

    if ([regex]::IsMatch($LogText, '(?m)^@@MM_CDB_INVALID .*\r?$')) {
        throw 'CDB rejected an invalid terminal result layout.'
    }

    $beginMatches = [regex]::Matches(
        $LogText,
        '(?m)^@@MM_CDB_TERMINAL_BEGIN event=TRACK_PUBLISHED ' +
            'rva=0x91f0 session=[0-9a-f`]+ result=[0-9a-f`]+\r?$'
    )
    $completeMatches = [regex]::Matches(
        $LogText,
        '(?m)^@@MM_CDB_SNAPSHOT_COMPLETE event=TRACK_PUBLISHED\r?$'
    )
    if ($beginMatches.Count -ne 1 -or $completeMatches.Count -ne 1) {
        throw 'CDB did not emit one complete TRACK_PUBLISHED snapshot.'
    }
    if ($completeMatches[0].Index -le $beginMatches[0].Index) {
        throw 'CDB snapshot-complete marker preceded its terminal begin marker.'
    }

    $resultMatches = [regex]::Matches(
        $LogText,
        '(?im)^@@MM_CDB_RESULT track_dr_bits=0x[0-9a-f]{8} ' +
            'channel_count=(?<channels>[0-9]+) sample_rate=[0-9]+ ' +
            'frames=0x[0-9a-f]+\r?$'
    )
    if ($resultMatches.Count -ne 1) {
        throw 'CDB did not emit exactly one valid result record.'
    }
    $channelCount = [int] $resultMatches[0].Groups['channels'].Value
    if ($channelCount -lt 1 -or $channelCount -gt 64) {
        throw "CDB emitted implausible channel count $channelCount."
    }

    $channelMatches = [regex]::Matches(
        $LogText,
        '(?im)^@@MM_CDB_CHANNEL index=(?<index>[0-9]+) ' +
            'dr_bits=0x[0-9a-f]{8} peak_bits=0x[0-9a-f]{8} ' +
            'rms_bits=0x[0-9a-f]{8}\r?$'
    )
    if ($channelMatches.Count -ne $channelCount) {
        throw (
            "CDB emitted $($channelMatches.Count) channel rows for " +
            "$channelCount channels."
        )
    }
    for ($index = 0; $index -lt $channelCount; $index += 1) {
        if ([int] $channelMatches[$index].Groups['index'].Value -ne $index) {
            throw "CDB channel rows are not contiguous at index $index."
        }
    }

    $snapshotLength =
        $completeMatches[0].Index - $beginMatches[0].Index
    $snapshotText = $LogText.Substring(
        $beginMatches[0].Index,
        $snapshotLength
    )
    if ([regex]::IsMatch(
        $snapshotText,
        '(?im)(memory access error|unable to read memory|' +
            "couldn't resolve|syntax error|bad register|^\s*\*\*\*)"
    )) {
        throw 'CDB reported a debugger or memory error in the snapshot.'
    }
}

function Wait-CdbMarker {
    param(
        [Parameter(Mandatory)]
        [Diagnostics.Process] $CdbProcess,

        [Parameter(Mandatory)]
        [string] $LiteralPath,

        [Parameter(Mandatory)]
        [string] $Marker,

        [Parameter(Mandatory)]
        [int] $TimeoutSeconds
    )

    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($timer.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        if ($CdbProcess.HasExited) {
            return $false
        }
        $logText = Get-LogText -LiteralPath $LiteralPath
        if (Test-LogMarkerLine -LogText $logText -Marker $Marker) {
            return $true
        }
        Start-Sleep -Milliseconds 100
    }
    $false
}

function Request-CdbDetach {
    param(
        [Parameter(Mandatory)]
        [Diagnostics.Process] $CdbProcess,

        [Parameter(Mandatory)]
        [int] $TargetPid
    )

    if ($CdbProcess.HasExited) {
        return
    }

    $target = Get-Process -Id $TargetPid -ErrorAction SilentlyContinue
    if ($null -ne $target) {
        $breakRequested = [MacinMeterCdbProbeNative]::DebugBreakProcess(
            $target.Handle
        )
        if (-not $breakRequested) {
            $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            Write-Warning "DebugBreakProcess failed with Win32 error $errorCode."
        }
    }

    Start-Sleep -Milliseconds 250
    if (-not $CdbProcess.HasExited) {
        $CdbProcess.StandardInput.WriteLine('bc *; qd')
        $CdbProcess.StandardInput.Flush()
    }

    if (-not $CdbProcess.WaitForExit(10000)) {
        try {
            $CdbProcess.StandardInput.Write([char] 2)
            $CdbProcess.StandardInput.WriteLine()
            $CdbProcess.StandardInput.Flush()
        }
        catch [InvalidOperationException] {
            # Preserve the more useful recovery error below.
        }
    }

    if (-not $CdbProcess.WaitForExit(5000)) {
        throw (
            "CDB PID $($CdbProcess.Id) did not confirm detach. Do not " +
            'terminate foobar2000; coordinate recovery in the interactive ' +
            'Windows session.'
        )
    }
}

function Assert-TargetDetachedAndAlive {
    param(
        [Parameter(Mandatory)]
        [int] $TargetPid,

        [Parameter(Mandatory)]
        [DateTime] $ExpectedStartTimeUtc,

        [Parameter(Mandatory)]
        [string] $ExpectedFoobarSha256,

        [Parameter(Mandatory)]
        [string] $ExpectedPluginSha256
    )

    $process = Get-Process -Id $TargetPid -ErrorAction Stop
    if ($process.StartTime.ToUniversalTime() -ne $ExpectedStartTimeUtc) {
        throw 'The foobar2000 PID was reused by a different process.'
    }
    if (Test-DebuggerPresent -Process $process) {
        throw 'CDB exited but foobar2000 still reports a debugger attached.'
    }
    if (
        (Get-LowerSha256 -LiteralPath $process.MainModule.FileName) -ne
            $ExpectedFoobarSha256
    ) {
        throw 'The live foobar2000 image changed during the probe.'
    }
    $plugin = @(
        $process.Modules |
            Where-Object ModuleName -IEQ 'foo_dr_meter.dll'
    )
    if (
        $plugin.Count -ne 1 -or
        (Get-LowerSha256 -LiteralPath $plugin[0].FileName) -ne
            $ExpectedPluginSha256
    ) {
        throw 'The live foo_dr_meter module changed during the probe.'
    }
}

function Assert-CdbHarnessSelfTestThrows {
    param(
        [Parameter(Mandatory)]
        [scriptblock] $Action,

        [Parameter(Mandatory)]
        [string] $Name
    )

    $threw = $false
    try {
        & $Action
    }
    catch {
        $threw = $true
    }
    if (-not $threw) {
        throw "Self-test '$Name' expected a fail-closed exception."
    }
}

function Invoke-CdbHarnessSelfTest {
    $passed = [Collections.Generic.List[string]]::new()

    $legacyBreakpointLog = @'
@@MM_CDB_MODULE base=00000001`80000000 init=00000001`80008410 push=00000001`800089f0 finish=00000001`80008df0 published=00000001`800091f0
 0 e 00000001`80008410 0001 (0001)  0:**** foo_dr_meter+0x8410
 1 e 00000001`800089f0 0001 (0001)  0:**** foo_dr_meter+0x89f0
 2 e 00000001`80008df0 0001 (0001)  0:**** foo_dr_meter+0x8df0
 3 e 00000001`800091f0 0001 (0001)  0:**** foo_dr_meter+0x91f0
'@
    Assert-CdbBreakpointsArmed -LogText $legacyBreakpointLog
    $passed.Add('legacy-bl-format')

    $modernBreakpointLog = @'
@@MM_CDB_MODULE base=00000001`80000000 init=00000001`80008410 push=00000001`800089f0 finish=00000001`80008df0 published=00000001`800091f0
 0 e Disable Clear 00000001`80008410 0001 (0001)  0:**** foo_dr_meter+0x8410
 1 e Disable Clear 00000001`800089f0 0001 (0001)  0:**** foo_dr_meter+0x89f0
 2 e Disable Clear 00000001`80008df0 0001 (0001)  0:**** foo_dr_meter+0x8df0
 3 e Disable Clear 00000001`800091f0 0001 (0001)  0:**** foo_dr_meter+0x91f0
'@
    Assert-CdbBreakpointsArmed -LogText $modernBreakpointLog
    $passed.Add('modern-bl-disable-clear-format')

    $validPolicyLog = @'
@@MM_CDB_EXCEPTION_POLICY_QUERY_BEGIN code=e06d7363
 e06d7363 - Microsoft C++ EH exception - notify - not handled
 av - Access violation - break - not handled
@@MM_CDB_EXCEPTION_POLICY_QUERY_END code=e06d7363
@@MM_CDB_ARMED
'@
    Assert-CdbExceptionPolicy -LogText $validPolicyLog
    $passed.Add('exception-policy-notify-not-handled-no-commands')

    $commandPolicyLog = @'
@@MM_CDB_EXCEPTION_POLICY_QUERY_BEGIN code=e06d7363
 e06d7363 - Microsoft C++ EH exception - notify - not handled
       Command: "g"
 av - Access violation - break - not handled
@@MM_CDB_EXCEPTION_POLICY_QUERY_END code=e06d7363
@@MM_CDB_ARMED
'@
    Assert-CdbHarnessSelfTestThrows `
        -Name 'exception-policy-command-rejection' `
        -Action {
            Assert-CdbExceptionPolicy -LogText $commandPolicyLog
        }
    $passed.Add('exception-policy-command-rejection')

    $validLifecycleLog = @'
@@MM_CDB_PROBE schema=1 target_sha256=ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489
@@MM_CDB_EXCEPTION_POLICY_QUERY_BEGIN code=e06d7363
@@MM_CDB_EXCEPTION_POLICY_QUERY_END code=e06d7363
@@MM_CDB_MODULE base=00000001`80000000 init=00000001`80008410 push=00000001`800089f0 finish=00000001`80008df0 published=00000001`800091f0
@@MM_CDB_ARMED
@@MM_CDB_EVENT event=INIT_ENTRY rva=0x8410
@@MM_CDB_INIT session=00000000`10001000 sample_rate=44100 channels=2
@@MM_CDB_EVENT event=PUSH_ENTRY rva=0x89f0
@@MM_CDB_PUSH session=00000000`10001000 pcm=00000000`30001000 frames=1024
@@MM_CDB_EVENT event=PUSH_ENTRY rva=0x89f0
@@MM_CDB_PUSH session=00000000`10001000 pcm=00000000`30002000 frames=976
@@MM_CDB_EVENT event=FINISH_ENTRY rva=0x8df0
@@MM_CDB_FINISH session=00000000`10001000 result=00000000`20002000 weighting=0
@@MM_CDB_EVENT event=PUSH_ENTRY rva=0x89f0
@@MM_CDB_PUSH session=00000000`10001000 pcm=00000000`00000000 frames=0
@@MM_CDB_TERMINAL_BEGIN event=TRACK_PUBLISHED rva=0x91f0 session=00000000`10001000 result=00000000`20002000
@@MM_CDB_RESULT track_dr_bits=0x40e00000 channel_count=2 sample_rate=44100 frames=0x7d0
@@MM_CDB_CHANNEL index=0 dr_bits=0x40e00000 peak_bits=0x3f000000 rms_bits=0x3e800000
@@MM_CDB_CHANNEL index=1 dr_bits=0x40e00000 peak_bits=0x3f000000 rms_bits=0x3e800000
@@MM_CDB_SNAPSHOT_COMPLETE event=TRACK_PUBLISHED
'@
    Assert-CdbFixtureLifecycle -LogText $validLifecycleLog
    Assert-CdbSnapshotComplete -LogText $validLifecycleLog
    $passed.Add('valid-strict-fixture-lifecycle')

    $pointerMismatchLog = $validLifecycleLog.Replace(
        (
            'rva=0x91f0 session=00000000`10001000 ' +
            'result=00000000`20002000'
        ),
        (
            'rva=0x91f0 session=00000000`10001000 ' +
            'result=00000000`20003000'
        )
    )
    Assert-CdbHarnessSelfTestThrows `
        -Name 'finish-terminal-pointer-mismatch' `
        -Action {
            Assert-CdbFixtureLifecycle -LogText $pointerMismatchLog
        }
    $passed.Add('finish-terminal-pointer-mismatch')

    $outOfOrderLog = $validLifecycleLog.Replace(
        '@@MM_CDB_EVENT event=PUSH_ENTRY rva=0x89f0',
        '@@MM_CDB_EVENT event=FINISH_ENTRY rva=0x8df0'
    )
    Assert-CdbHarnessSelfTestThrows `
        -Name 'lifecycle-order-rejection' `
        -Action {
            Assert-CdbFixtureLifecycle -LogText $outOfOrderLog
        }
    $passed.Add('lifecycle-order-rejection')

    $invalidFlushLog = $validLifecycleLog.Replace(
        (
            '@@MM_CDB_PUSH session=00000000`10001000 ' +
            'pcm=00000000`00000000 frames=0'
        ),
        (
            '@@MM_CDB_PUSH session=00000000`10001000 ' +
            'pcm=00000000`30003000 frames=1'
        )
    )
    Assert-CdbHarnessSelfTestThrows `
        -Name 'post-finish-flush-rejection' `
        -Action {
            Assert-CdbFixtureLifecycle -LogText $invalidFlushLog
        }
    $passed.Add('post-finish-flush-rejection')

    $preArmLog = $validLifecycleLog.Replace(
        '@@MM_CDB_ARMED',
        (
            '@@MM_CDB_EVENT event=PUSH_ENTRY rva=0x89f0' + "`n" +
            '@@MM_CDB_ARMED'
        )
    )
    Assert-CdbHarnessSelfTestThrows `
        -Name 'pre-arm-lifecycle-rejection' `
        -Action {
            Assert-CdbFixtureLifecycle -LogText $preArmLog
        }
    $passed.Add('pre-arm-lifecycle-rejection')

    $invalidSnapshotLog = $validLifecycleLog.Replace(
        (
            '@@MM_CDB_RESULT track_dr_bits=0x40e00000 ' +
            'channel_count=2 sample_rate=44100 frames=0x7d0'
        ),
        '@@MM_CDB_INVALID reason=channel-count value=0'
    )
    Assert-CdbHarnessSelfTestThrows `
        -Name 'invalid-terminal-rejection' `
        -Action {
            Assert-CdbSnapshotComplete -LogText $invalidSnapshotLog
        }
    $passed.Add('invalid-terminal-rejection')

    $memoryErrorLog = $validLifecycleLog.Replace(
        '@@MM_CDB_CHANNEL index=0',
        'Memory access error' + "`n" + '@@MM_CDB_CHANNEL index=0'
    )
    Assert-CdbHarnessSelfTestThrows `
        -Name 'memory-error-rejection' `
        -Action {
            Assert-CdbSnapshotComplete -LogText $memoryErrorLog
        }
    $passed.Add('memory-error-rejection')

    Assert-CdbHarnessSelfTestThrows `
        -Name 'absolute-path-rejection' `
        -Action {
            Assert-NoAbsolutePathText `
                -Text 'privateSource=D:\debuggers\amd64\cdb.exe' `
                -ArtifactKind 'Synthetic normalized artifact'
        }
    $passed.Add('absolute-path-rejection')

    Assert-CdbHarnessSelfTestThrows `
        -Name 'input-reparse-point-rejection' `
        -Action {
            Assert-InputFileIsNotReparsePoint -Attributes (
                [IO.FileAttributes]::Normal -bor
                [IO.FileAttributes]::ReparsePoint
            )
        }
    $passed.Add('input-reparse-point-rejection')

    if ((Get-ProbeInputExtension -LiteralPath 'fixture.WAV') -ne '.wav') {
        throw 'Self-test did not normalize an allowlisted input extension.'
    }
    Assert-CdbHarnessSelfTestThrows `
        -Name 'input-extension-rejection' `
        -Action {
            [void] (
                Get-ProbeInputExtension -LiteralPath 'fixture.m3u8'
            )
        }
    $passed.Add('input-extension-allowlist')

    $stagingTestRoot = Join-Path ([IO.Path]::GetTempPath()) (
        "MacinMeterInputSelfTest-$([Guid]::NewGuid().ToString('N'))"
    )
    [void] [IO.Directory]::CreateDirectory($stagingTestRoot)
    $testBytes = [byte[]] @(0, 1, 2, 3, 0x7f, 0x80, 0xfe, 0xff)
    $testSha256 = [Convert]::ToHexString(
        [Security.Cryptography.SHA256]::HashData($testBytes)
    ).ToLowerInvariant()
    $testSource = [IO.MemoryStream]::new($testBytes, $false)
    $testStaged = $null
    try {
        $testStaged = New-GuardedStagedInput `
            -SourceStream $testSource `
            -StageDirectory $stagingTestRoot `
            -Extension '.wav' `
            -ExpectedBytes $testBytes.Length `
            -ExpectedSha256 $testSha256
        if (
            $testStaged.bytes -ne $testBytes.Length -or
            $testStaged.sha256 -ne $testSha256 -or
            [IO.Path]::GetExtension($testStaged.path) -ne '.wav' -or
            ((Get-Item -LiteralPath $testStaged.path).Attributes -band
                [IO.FileAttributes]::ReparsePoint) -ne 0
        ) {
            throw 'Synthetic staged-input evidence did not match its source.'
        }
        Assert-CdbHarnessSelfTestThrows `
            -Name 'staged-input-write-lock' `
            -Action {
                $writeAttempt = [IO.File]::Open(
                    $testStaged.path,
                    [IO.FileMode]::Open,
                    [IO.FileAccess]::Write,
                    [IO.FileShare]::ReadWrite
                )
                try {
                    $writeAttempt.Dispose()
                }
                finally {
                    if ($null -ne $writeAttempt) {
                        $writeAttempt.Dispose()
                    }
                }
            }
    }
    finally {
        if ($null -ne $testStaged) {
            $testStaged.guard.Dispose()
        }
        $testSource.Dispose()
        Remove-Item -LiteralPath $stagingTestRoot -Recurse -Force `
            -ErrorAction SilentlyContinue
    }
    $passed.Add('guarded-staged-input-copy-and-lock')

    [pscustomobject]@{
        status = 'self-tests-passed'
        testCount = $passed.Count
        tests = $passed.ToArray()
    } | ConvertTo-Json -Depth 4
}

if ($SelfTest) {
    Invoke-CdbHarnessSelfTest
    return
}

if ($Arm -and -not $ConfirmFoobarIdle) {
    throw '-Arm also requires -ConfirmFoobarIdle.'
}
if ($ConfirmFoobarIdle -and -not $Arm) {
    throw '-ConfirmFoobarIdle is only valid together with -Arm.'
}

$inputItem = Get-Item -LiteralPath $InputFile
if ($inputItem.PSIsContainer) {
    throw "Input must be exactly one file: $InputFile"
}
Assert-InputFileIsNotReparsePoint -Attributes $inputItem.Attributes
$inputExtension = Get-ProbeInputExtension -LiteralPath $inputItem.FullName
$inputSha256 = Get-LowerSha256 -LiteralPath $inputItem.FullName
if ($inputSha256 -ne $ExpectedInputSha256.ToLowerInvariant()) {
    throw (
        "Input SHA-256 mismatch: expected $ExpectedInputSha256, " +
        "got $inputSha256."
    )
}

$processes = @(Get-Process -Name foobar2000 -ErrorAction Stop)
if ($processes.Count -ne 1) {
    throw "Expected exactly one foobar2000 process, found $($processes.Count)."
}
$foobarProcess = $processes[0]
$targetPid = $foobarProcess.Id
$targetStartTimeUtc = $foobarProcess.StartTime.ToUniversalTime()
$foobarPath = $foobarProcess.MainModule.FileName
$modules = @($foobarProcess.Modules | Sort-Object FileName -Unique)
$pluginModules = @(
    $modules | Where-Object ModuleName -IEQ 'foo_dr_meter.dll'
)
if ($pluginModules.Count -ne 1) {
    throw (
        'Expected exactly one loaded foo_dr_meter.dll module, found ' +
        "$($pluginModules.Count)."
    )
}
$pluginModule = $pluginModules[0]

$foobarSha256 = Get-LowerSha256 -LiteralPath $foobarPath
if ($foobarSha256 -ne $expectedFoobarSha256) {
    throw (
        "foobar2000 SHA-256 mismatch: expected $expectedFoobarSha256, " +
        "got $foobarSha256."
    )
}
$pluginSha256 = Get-LowerSha256 -LiteralPath $pluginModule.FileName
if ($pluginSha256 -ne $expectedPluginSha256) {
    throw (
        "foo_dr_meter SHA-256 mismatch: expected $expectedPluginSha256, " +
        "got $pluginSha256."
    )
}
if ((Get-PeMachine -LiteralPath $foobarPath) -ne 0x8664) {
    throw 'The running foobar2000 image is not x86-64 PE.'
}
if ((Get-PeMachine -LiteralPath $pluginModule.FileName) -ne 0x8664) {
    throw 'The loaded foo_dr_meter image is not x86-64 PE.'
}
if (Test-DebuggerPresent -Process $foobarProcess) {
    throw 'foobar2000 already has a debugger attached.'
}

$commandOwners = @(
    Find-ExactContextCommandOwners `
        -Modules $modules `
        -CommandName $contextCommandName
)
if (
    $commandOwners.Count -ne 1 -or
    $commandOwners[0].moduleName -ine 'foo_dr_meter.dll' -or
    $commandOwners[0].sha256 -ne $expectedPluginSha256 -or
    $commandOwners[0].exactNullTerminatedOccurrences -ne 1
) {
    throw (
        "The context-command leaf '$contextCommandName' is missing or " +
        'ambiguous among loaded modules.'
    )
}

$logFullPath = [IO.Path]::GetFullPath($LogPath)
$normalizedFullPath = if (
    $PSBoundParameters.ContainsKey('NormalizedPath')
) {
    [IO.Path]::GetFullPath($NormalizedPath)
}
else {
    "$logFullPath.markers.log"
}
$metadataPath = "$normalizedFullPath.metadata.json"
$artifactPaths = @($logFullPath, $normalizedFullPath, $metadataPath)
if ((@($artifactPaths | Sort-Object -Unique)).Count -ne 3) {
    throw 'Raw log, normalized marker, and metadata paths must be distinct.'
}
foreach ($artifactPath in $artifactPaths) {
    $outputDirectory = Split-Path -Parent $artifactPath
    if (-not (Test-Path -LiteralPath $outputDirectory -PathType Container)) {
        throw "Output directory does not exist: $outputDirectory"
    }
}
if (
    (Test-Path -LiteralPath $logFullPath) -or
    (Test-Path -LiteralPath $normalizedFullPath) -or
    (Test-Path -LiteralPath $metadataPath)
) {
    throw (
        'Refusing to overwrite an existing raw log, normalized marker ' +
        'artifact, or metadata sidecar.'
    )
}

$cdbPackageVersion = $null
if ($PSBoundParameters.ContainsKey('CdbPath')) {
    $cdbSourceItem = Get-Item -LiteralPath $CdbPath
    if ($cdbSourceItem.PSIsContainer -or $cdbSourceItem.Name -ine 'cdb.exe') {
        throw '-CdbPath must name an x64 cdb.exe file.'
    }
    $cdbSourcePath = $cdbSourceItem.FullName
    if ($cdbSourcePath -match '(?i)[\\/]WindowsApps[\\/]') {
        throw (
            'Do not execute CDB directly from WindowsApps. Omit -CdbPath ' +
            'to request a verified private toolset copy.'
        )
    }
    $cdbSourceKind = 'explicit-toolset-source'
    $cdbExecutionMode = if ($Arm) {
        'private-temp-copy-pending'
    }
    else {
        'not-executed; arm mode will create a private verified copy'
    }
}
else {
    $cdbPackages = @(
        Get-StoreWinDbgPackages |
            Sort-Object { [version] $_.Version } -Descending
    )
    $cdbCandidates = @(
        foreach ($package in $cdbPackages) {
            $candidate = Join-Path $package.InstallLocation 'amd64\cdb.exe'
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                [pscustomobject]@{
                    package = $package
                    path = $candidate
                }
            }
        }
    )
    if ($cdbCandidates.Count -lt 1) {
        throw 'Microsoft Store WinDbg amd64\cdb.exe was not found.'
    }
    $cdbPackage = $cdbCandidates[0].package
    $cdbPackageVersion = $cdbPackage.Version.ToString()
    $cdbSourcePath = $cdbCandidates[0].path
    $cdbSourceKind = 'store-package-read-only-source'
    $cdbExecutionMode = if ($Arm) {
        'private-temp-copy-pending'
    }
    else {
        'not-executed; arm mode will create a private verified copy'
    }
}
if ((Get-PeMachine -LiteralPath $cdbSourcePath) -ne 0x8664) {
    throw 'The resolved CDB source is not x86-64 PE.'
}
$cdbSourceSha256 = Get-LowerSha256 -LiteralPath $cdbSourcePath
$cdbFileVersion =
    [Diagnostics.FileVersionInfo]::GetVersionInfo($cdbSourcePath).FileVersion

$preflight = [ordered]@{
    schemaVersion = 1
    mode = if ($Arm) { 'armed' } else { 'preflight-only' }
    runId = $RunId
    contextCommand = $contextCommandName
    input = [ordered]@{
        fixtureId = $FixtureId
        bytes = $inputItem.Length
        sha256 = $inputSha256
    }
    target = [ordered]@{
        pid = $targetPid
        processStartTimeUtc = $targetStartTimeUtc.ToString('o')
        foobar2000 = [ordered]@{
            sha256 = $foobarSha256
            peMachine = '0x8664'
        }
        fooDrMeter = [ordered]@{
            sha256 = $pluginSha256
            peMachine = '0x8664'
            commandOccurrences = 1
        }
    }
    debugger = [ordered]@{
        source = $cdbSourceKind
        packageVersion = $cdbPackageVersion
        executionMode = $cdbExecutionMode
        cdbExeSha256 = $cdbSourceSha256
        fileVersion = $cdbFileVersion
        peMachine = '0x8664'
        attachMode = if ($Arm) { 'invasive -p with -pd' } else { 'none' }
        exceptionPolicy = [ordered]@{
            code = 'e06d7363'
            requiredAction =
                'sxn: notify, do not break; not handled; commands empty'
            validation = 'pending-sx-query'
        }
        sourceToolsetManifest = $null
        executionToolsetManifest = $null
    }
    runtime = [ordered]@{
        powershellVersion = $PSVersionTable.PSVersion.ToString()
        powershellEdition = $PSVersionTable.PSEdition
        osVersion = [Environment]::OSVersion.VersionString
        osDescription =
            [Runtime.InteropServices.RuntimeInformation]::OSDescription
        processArchitecture = (
            [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture
        ).ToString()
    }
}

if (-not $Arm) {
    $preflight | ConvertTo-Json -Depth 8
    Write-Warning (
        'Preflight passed; no debugger was attached. Re-run with both ' +
        '-Arm and -ConfirmFoobarIdle only after coordinating the GUI session.'
    )
    return
}

$cdbProcess = $null
$inputGuard = $null
$stagedInputGuard = $null
$cdbCommandGuard = $null
$triggerProcess = $null
$stageDirectory = $null
$exceptionPolicyConfirmed = $false
try {
    $stageCopy = Copy-CdbToolsetForExecution `
        -SourceCdbPath $cdbSourcePath `
        -RunId $RunId
    $stageDirectory = $stageCopy.root
    $cdbExecutionPath = $stageCopy.cdbPath
    $preflight['debugger']['executionMode'] =
        'verified-private-temp-toolset-copy'
    $preflight['debugger']['sourceToolsetManifest'] =
        $stageCopy.sourceManifest
    $preflight['debugger']['executionToolsetManifest'] =
        $stageCopy.stagedManifest
    if (
        (Get-PeMachine -LiteralPath $cdbExecutionPath) -ne 0x8664 -or
        (Get-LowerSha256 -LiteralPath $cdbExecutionPath) -ne
            $cdbSourceSha256
    ) {
        throw 'The executable CDB copy does not match the verified x64 source.'
    }

    $cdbCommandPath =
        Join-Path $PSScriptRoot 'probe_foo_dr_meter_108_x64.cdb'
    if (-not (Test-Path -LiteralPath $cdbCommandPath -PathType Leaf)) {
        throw "Bundled CDB command file not found: $cdbCommandPath"
    }
    $cdbCommandExecutionPath =
        Join-Path $stageDirectory 'probe_foo_dr_meter_108_x64.cdb'
    $sourceCommandGuard = [IO.File]::Open(
        $cdbCommandPath,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::Read
    )
    $commandCopyStream = $null
    try {
        $cdbCommandSourceSha256 =
            Get-StreamSha256 -Stream $sourceCommandGuard
        $commandCopyStream = [IO.File]::Open(
            $cdbCommandExecutionPath,
            [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write,
            [IO.FileShare]::None
        )
        $sourceCommandGuard.Position = 0
        $sourceCommandGuard.CopyTo($commandCopyStream)
        $commandCopyStream.Flush($true)
    }
    finally {
        if ($null -ne $commandCopyStream) {
            $commandCopyStream.Dispose()
        }
        $sourceCommandGuard.Dispose()
    }
    $cdbCommandExecutionSha256 =
        Get-LowerSha256 -LiteralPath $cdbCommandExecutionPath
    if ($cdbCommandExecutionSha256 -ne $cdbCommandSourceSha256) {
        throw 'The private CDB command-file copy did not match its source.'
    }

    $liveProcess = Get-Process -Id $targetPid -ErrorAction Stop
    if (
        $liveProcess.StartTime.ToUniversalTime() -ne $targetStartTimeUtc -or
        (Get-LowerSha256 -LiteralPath $liveProcess.MainModule.FileName) -ne
            $expectedFoobarSha256 -or
        (Test-DebuggerPresent -Process $liveProcess)
    ) {
        throw 'The foobar2000 process identity changed after preflight.'
    }
    $livePlugin = @(
        $liveProcess.Modules |
            Where-Object ModuleName -IEQ 'foo_dr_meter.dll'
    )
    if (
        $livePlugin.Count -ne 1 -or
        (Get-LowerSha256 -LiteralPath $livePlugin[0].FileName) -ne
            $expectedPluginSha256
    ) {
        throw 'The loaded foo_dr_meter identity changed after preflight.'
    }

    $phase = 'prepared-not-attached'
    $metadata = [ordered]@{
        schemaVersion = 1
        status = $phase
        preflight = $preflight
        probeStartedUtc = [DateTime]::UtcNow.ToString('o')
        harnessScriptSha256 = Get-LowerSha256 -LiteralPath $PSCommandPath
        cdbCommandSourceSha256 = $cdbCommandSourceSha256
        cdbCommandExecutionCopySha256 = $cdbCommandExecutionSha256
        evidencePolicy = [ordered]@{
            rawCdbLog = 'private local raw input; do not commit or publish'
            normalizedMarkers = 'strict allowlist; path-scanned'
            metadata = 'path-free provenance and evidence bindings'
        }
    }
    Write-SafeMetadata -Metadata $metadata -LiteralPath $metadataPath

    $cdbStart = [Diagnostics.ProcessStartInfo]::new()
    $cdbStart.FileName = $cdbExecutionPath
    $cdbStart.WorkingDirectory = Split-Path -Parent $cdbExecutionPath
    $cdbStart.UseShellExecute = $false
    $cdbStart.CreateNoWindow = $true
    $cdbStart.RedirectStandardInput = $true
    $cdbStart.Environment['_NT_SYMBOL_PATH'] = ''
    $cdbStart.Environment['_NT_ALT_SYMBOL_PATH'] = ''
    foreach ($argument in @(
        '-pd',
        '-ee',
        'masm',
        '-p',
        $targetPid.ToString(),
        '-logo',
        $logFullPath,
        '-cf',
        $cdbCommandExecutionPath
    )) {
        [void] $cdbStart.ArgumentList.Add($argument)
    }

    try {
        $phase = 'guarding-cdb-command-copy'
        $cdbCommandGuard = [IO.File]::Open(
            $cdbCommandExecutionPath,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::Read
        )
        if (
            (Get-StreamSha256 -Stream $cdbCommandGuard) -ne
                $cdbCommandExecutionSha256
        ) {
            throw 'The private CDB command-file copy changed before attach.'
        }

        $phase = 'guarding-input'
        $inputGuard = [IO.File]::Open(
            $inputItem.FullName,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::Read
        )
        $lockedInputSha256 = Get-StreamSha256 -Stream $inputGuard
        if (
            $inputGuard.Length -ne $inputItem.Length -or
            $lockedInputSha256 -ne $inputSha256
        ) {
            throw 'The input changed between preflight and the guarded open.'
        }
        $phase = 'staging-guarded-input-copy'
        $stagedInput = New-GuardedStagedInput `
            -SourceStream $inputGuard `
            -StageDirectory $stageDirectory `
            -Extension $inputExtension `
            -ExpectedBytes $inputGuard.Length `
            -ExpectedSha256 $lockedInputSha256
        $stagedInputGuard = $stagedInput.guard
        $stagedInputPath = $stagedInput.path
        $metadata['inputBinding'] = [ordered]@{
            sourceHandle = [ordered]@{
                bytes = $inputGuard.Length
                sha256 = $lockedInputSha256
            }
            privateStagedCopy = [ordered]@{
                bytes = $stagedInput.bytes
                sha256 = $stagedInput.sha256
                extension = $stagedInput.extension
                ordinaryFile = $true
                heldReadLock = $true
            }
            forwarderInput = 'private-staged-copy'
        }
        Write-SafeMetadata -Metadata $metadata -LiteralPath $metadataPath

        $phase = 'attaching-cdb'
        $cdbProcess = [Diagnostics.Process]::Start($cdbStart)
        $armed = Wait-CdbMarker `
            -CdbProcess $cdbProcess `
            -LiteralPath $logFullPath `
            -Marker '@@MM_CDB_ARMED' `
            -TimeoutSeconds $AttachTimeoutSeconds
        if (-not $armed) {
            throw 'CDB exited or timed out before confirming armed breakpoints.'
        }
        $armedLogText = Get-LogText -LiteralPath $logFullPath
        Assert-CdbBreakpointsArmed -LogText $armedLogText
        Assert-CdbExceptionPolicy -LogText $armedLogText
        $exceptionPolicyConfirmed = $true
        $preflight['debugger']['exceptionPolicy']['validation'] =
            'confirmed-from-sx-query-before-trigger'
        Write-SafeMetadata -Metadata $metadata -LiteralPath $metadataPath
        if (
            (Get-StreamSha256 -Stream $cdbCommandGuard) -ne
                $cdbCommandExecutionSha256
        ) {
            throw 'The guarded CDB command-file copy changed during startup.'
        }
        if ((Get-StreamSha256 -Stream $inputGuard) -ne $inputSha256) {
            throw 'The guarded source input hash changed after CDB armed.'
        }
        if (
            (Get-StreamSha256 -Stream $stagedInputGuard) -ne
                $stagedInput.sha256
        ) {
            throw 'The guarded staged input hash changed after CDB armed.'
        }

        $phase = 'triggering-context-command'
        $triggerStart = [Diagnostics.ProcessStartInfo]::new()
        $triggerStart.FileName = $foobarPath
        $triggerStart.UseShellExecute = $false
        [void] $triggerStart.ArgumentList.Add($contextCommandArgument)
        [void] $triggerStart.ArgumentList.Add($stagedInputPath)
        $triggerProcess = [Diagnostics.Process]::Start($triggerStart)
        if (-not $triggerProcess.WaitForExit(30000)) {
            throw (
                'The foobar2000 command-line forwarder did not exit within 30 ' +
                'seconds; the command may not have reached the interactive process.'
            )
        }
        if ($triggerProcess.ExitCode -ne 0) {
            throw (
                'The foobar2000 context-command forwarder exited with code ' +
                "$($triggerProcess.ExitCode)."
            )
        }
        $forwardedTargets = @(Get-Process -Name foobar2000 -ErrorAction Stop)
        if (
            $forwardedTargets.Count -ne 1 -or
            $forwardedTargets[0].Id -ne $targetPid -or
            $forwardedTargets[0].StartTime.ToUniversalTime() -ne
                $targetStartTimeUtc
        ) {
            throw (
                'The context-command launcher did not leave exactly the attached ' +
                'foobar2000 process as the sole target.'
            )
        }

        $phase = 'waiting-for-terminal-breakpoint'
        if (-not $cdbProcess.WaitForExit($CompletionTimeoutSeconds * 1000)) {
            throw (
                "The terminal breakpoint did not fire within " +
                "$CompletionTimeoutSeconds seconds."
            )
        }
        if ($cdbProcess.ExitCode -ne 0) {
            throw "CDB exited with code $($cdbProcess.ExitCode)."
        }

        $phase = 'validating-input-binding'
        if (
            $inputGuard.Length -ne $stagedInputGuard.Length -or
            (Get-StreamSha256 -Stream $inputGuard) -ne $inputSha256 -or
            (Get-StreamSha256 -Stream $stagedInputGuard) -ne $inputSha256
        ) {
            throw (
                'Source-handle and private staged-input evidence diverged ' +
                'before result validation.'
            )
        }

        $phase = 'validating-detached-result'
        $logText = Get-LogText -LiteralPath $logFullPath
        Assert-CdbFixtureLifecycle -LogText $logText
        $metadata['fixtureLifecycle'] = [ordered]@{
            validation = 'strict-chain-confirmed'
            sequence =
                'ARMED>INIT>DATA_PUSH+>FINISH>ZERO_FLUSH_PUSH>TRACK'
            sessionPointerConsistency = $true
            finishTerminalResultPointerConsistency = $true
        }
        Assert-CdbSnapshotComplete -LogText $logText
        Assert-TargetDetachedAndAlive `
            -TargetPid $targetPid `
            -ExpectedStartTimeUtc $targetStartTimeUtc `
            -ExpectedFoobarSha256 $expectedFoobarSha256 `
            -ExpectedPluginSha256 $expectedPluginSha256

        $phase = 'normalizing-evidence'
        $normalizedEvidence = Write-NormalizedCdbMarkers `
            -RawLogPath $logFullPath `
            -OutputPath $normalizedFullPath `
            -ExceptionPolicyConfirmed
        $rawEvidence = Get-ArtifactEvidence -LiteralPath $logFullPath
        $metadata['evidence'] = [ordered]@{
            rawCdbLog = [ordered]@{
                classification = 'private-local-raw-input-do-not-commit'
                bytes = $rawEvidence.bytes
                sha256 = $rawEvidence.sha256
            }
            normalizedMarkers = [ordered]@{
                classification = 'allowlisted-path-scanned'
                bytes = $normalizedEvidence.bytes
                sha256 = $normalizedEvidence.sha256
                recordCount = $normalizedEvidence.recordCount
            }
        }
        $metadata['probeCompletedUtc'] = [DateTime]::UtcNow.ToString('o')
        $metadata['status'] = 'completed-and-detached'
        Write-SafeMetadata -Metadata $metadata -LiteralPath $metadataPath

        Write-Warning (
            'The raw -logo file is private debugger input. Do not commit or ' +
            'publish it; use only the normalized marker artifact and sidecar.'
        )
        [pscustomobject]@{
            status = 'completed-and-detached'
            runId = $RunId
            fixtureId = $FixtureId
            targetPid = $targetPid
            inputSha256 = $inputSha256
            rawLogPath = $logFullPath
            rawLogClassification = 'private-do-not-commit'
            normalizedPath = $normalizedFullPath
            metadataPath = $metadataPath
        } | ConvertTo-Json -Depth 4
    }
    catch {
        $originalError = $_
        $detachError = $null
        if ($null -ne $cdbProcess -and -not $cdbProcess.HasExited) {
            try {
                Request-CdbDetach `
                    -CdbProcess $cdbProcess `
                    -TargetPid $targetPid
            }
            catch {
                $detachError = $_
            }
        }

        $targetDetached = $false
        $postconditionError = $null
        try {
            Assert-TargetDetachedAndAlive `
                -TargetPid $targetPid `
                -ExpectedStartTimeUtc $targetStartTimeUtc `
                -ExpectedFoobarSha256 $expectedFoobarSha256 `
                -ExpectedPluginSha256 $expectedPluginSha256
            $targetDetached = $true
        }
        catch {
            $postconditionError = $_
        }

        try {
            $metadata['probeFailedUtc'] = [DateTime]::UtcNow.ToString('o')
            $metadata['status'] = if ($targetDetached) {
                'failed-and-detached'
            }
            else {
                'failed-detach-unconfirmed'
            }
            $metadata['failure'] = [ordered]@{
                phase = $phase
                primaryErrorType =
                    $originalError.Exception.GetType().FullName
                detachErrorType = if ($null -ne $detachError) {
                    $detachError.Exception.GetType().FullName
                }
                else {
                    $null
                }
                postconditionErrorType = if ($null -ne $postconditionError) {
                    $postconditionError.Exception.GetType().FullName
                }
                else {
                    $null
                }
            }
            if (
                (Test-Path -LiteralPath $logFullPath -PathType Leaf) -and
                ($null -eq $cdbProcess -or $cdbProcess.HasExited)
            ) {
                $rawEvidence = Get-ArtifactEvidence -LiteralPath $logFullPath
                $normalizedEvidence = if (
                    Test-Path -LiteralPath $normalizedFullPath -PathType Leaf
                ) {
                    $normalizedText = [IO.File]::ReadAllText(
                        $normalizedFullPath
                    )
                    Assert-NoAbsolutePathText `
                        -Text $normalizedText `
                        -ArtifactKind 'Normalized CDB marker artifact'
                    $existingEvidence =
                        Get-ArtifactEvidence -LiteralPath $normalizedFullPath
                    $existingEvidence['recordCount'] = @(
                        $normalizedText -split '\r?\n' |
                            Where-Object Length -GT 0
                    ).Count
                    $existingEvidence
                }
                else {
                    Write-NormalizedCdbMarkers `
                        -RawLogPath $logFullPath `
                        -OutputPath $normalizedFullPath `
                        -ExceptionPolicyConfirmed:$exceptionPolicyConfirmed
                }
                $metadata['evidence'] = [ordered]@{
                    rawCdbLog = [ordered]@{
                        classification =
                            'private-local-raw-input-do-not-commit'
                        bytes = $rawEvidence.bytes
                        sha256 = $rawEvidence.sha256
                    }
                    normalizedMarkers = [ordered]@{
                        classification = 'allowlisted-path-scanned'
                        bytes = $normalizedEvidence.bytes
                        sha256 = $normalizedEvidence.sha256
                        recordCount = $normalizedEvidence.recordCount
                    }
                }
            }
            else {
                $metadata['evidence'] = [ordered]@{
                    rawCdbLog = [ordered]@{
                        classification =
                            'private-local-raw-input-do-not-commit'
                        binding = 'unavailable-or-still-mutable'
                    }
                    normalizedMarkers = [ordered]@{
                        binding = 'not-produced'
                    }
                }
            }
            Write-SafeMetadata -Metadata $metadata -LiteralPath $metadataPath
        }
        catch {
            Write-Warning (
                'Could not finalize path-free failure metadata: ' +
                $_.Exception.GetType().FullName
            )
        }

        if ($null -ne $detachError) {
            throw (
                "$($originalError.Exception.Message) Automatic qd detach also " +
                "failed: $($detachError.Exception.Message)"
            )
        }
        throw $originalError
    }
    finally {
        if ($null -ne $cdbCommandGuard) {
            $cdbCommandGuard.Dispose()
        }
        if ($null -ne $stagedInputGuard) {
            $stagedInputGuard.Dispose()
        }
        if ($null -ne $inputGuard) {
            $inputGuard.Dispose()
        }
        if ($null -ne $triggerProcess -and $triggerProcess.HasExited) {
            $triggerProcess.Dispose()
        }
        if ($null -ne $cdbProcess -and $cdbProcess.HasExited) {
            $cdbProcess.Dispose()
        }
    }
}
finally {
    if ($null -ne $stageDirectory) {
        if ($null -eq $cdbProcess -or $cdbProcess.HasExited) {
            Remove-Item -LiteralPath $stageDirectory -Recurse -Force `
                -ErrorAction SilentlyContinue
        }
        else {
            Write-Warning (
                'The private CDB toolset copy was retained because CDB may ' +
                'still be attached. Coordinate debugger recovery first.'
            )
        }
    }
}
