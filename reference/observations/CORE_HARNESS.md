# Isolated x64 core harness

[`run_foo_dr_meter_108_core.py`](../tools/run_foo_dr_meter_108_core.py) is the
cross-platform parent for the isolated Windows x64 core worker. It does not
start foobar2000 and never loads `foo_dr_meter.dll` in the parent process.
Exactly one worker process receives exactly one PCM stream.

## Evidence boundary

The harness can observe the fixed component DLL's analyzer session, channel
state and result for caller-supplied finite interleaved binary64 PCM. This is a
cleaner algorithm-core boundary than a foobar component run, but it deliberately
does not exercise:

- foobar decoding or sample conversion;
- foobar component registration or host lifecycle;
- metadata, album grouping or report rendering.

The real pinned `shared.dll` does participate in PE load and unload. That narrow
DLL lifecycle is not foobar component registration or host-lifecycle evidence.

Accordingly every record states `compatibility: none` and
`foobarParity: not_assessed`. A successful record is not by itself a
compatibility or E3 claim.

## Input modes

`fixture` selects one case from a manifest and rechecks the WAVE file length,
file/data SHA-256, encoding, sample rate, channels and frame count. The parent
strictly parses integer or IEEE-float RIFF/WAVE and deterministically converts
each sample to little-endian binary64. This conversion belongs to the harness;
it is not an observation of foobar's decoder.

`pcm` accepts an already interleaved `f64le` file. The input ID, SHA-256,
sample rate, channels and frame count are all mandatory. The parent rejects
misaligned or non-finite PCM.

## Runtime identity

The target is permanently gated to
`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`. The caller must also supply the
expected SHA-256 for the worker and each exact runtime artifact:

- `shared.dll`
- `msvcp140.dll`
- `vcruntime140.dll`
- `vcruntime140_1.dll`

The transport request contains local source paths because the worker must open,
lock and privately stage those bytes. The emitted record contains only names,
hashes and lengths. It never copies local paths or worker stderr.

On Windows, the parent rehashes its staged worker and PCM and rechecks its
canonical request while holding no-write, no-delete, non-reparse handles for
the staging directory, worker, PCM and request through process completion. The
native worker creates its own 128-bit-random directory with a current-user +
Local System protected DACL, holds a no-delete directory handle through target
unload, and holds every staged file against write/delete. It rechecks
volume/file IDs and SHA-256 before and after load. These controls close ordinary
filesystem path-substitution races; they are not an OS sandbox against process
injection, token theft or kernel access.

The accepted runtime profile is `fixed_foobar_2_25_10`. The worker verifies the
four-item allowlist and retains the fixed real `shared.dll` for normal PE
load/unload lifecycle. During core execution, all 13 ordinary target IAT
entries for `shared.dll` are replaced by a fail-fast tripwire and restored
before unload.
No foobar service return value is emulated through those slots. The tripwire
does not by itself intercept a cached pointer, dynamically resolved export, or
call routed through another module; such paths require separate static or
dynamic evidence. Every successful response records this exact boundary as:

```json
{
  "loadLifecycle": "real_shared",
  "coreExecution": "fail_fast_iat_tripwire",
  "armedImportCount": 13
}
```

A separately identifiable fail-fast `shared.dll` remains a negative lifecycle
probe. The fixed DLL reaches it during `LoadLibraryExW`, before init can run,
so it is deliberately not offered as a successful parent runtime profile.

The worker also verifies the floating-point environment. The default block
size is 512 frames, and the request ID binds the block size, PCM, target,
runtime and worker identities without depending on transport paths.

## Accepted safe-master observation

The first accepted suite record is
[`OBS-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719`](obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/record.md).
It binds target
`ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`
and the complete-v2 manifest, starts a fresh worker for each safe input, and
completed 39 of 39 core executions successfully.

Each execution directly calls init/push/finish and captures raw result, session,
channel-state and floating-point-control bits. During those calls all 13
`shared.dll` IAT entries are armed with the fail-fast tripwire. The observation
therefore provides accepted dynamic evidence for this isolated analyzer-core
boundary only. It contains no foobar decoder, registration, metadata, album or
renderer evidence and retains `compatibility: none` and
`foobarParity: not_assessed`.

A separate
[`core-to-report comparison`](../conformance/conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/record.md)
reconstructs only four exported field classes from those raw result bits.
Track DR 39/39, channel DR 62/62, channel RMS 62/62 and overall peak 39/39
match the already registered x64 foobar report exactly. That cross-check does
not expand the harness boundary or turn the result into full component parity.

## Invocation shape

The complete command is intentionally explicit:

```text
python reference/tools/run_foo_dr_meter_108_core.py \
  --worker <worker.exe> --worker-sha256 <sha256> \
  --target-dll <foo_dr_meter.dll> \
  --shared-dll <shared.dll> --shared-sha256 <sha256> \
  --msvcp140-dll <msvcp140.dll> --msvcp140-sha256 <sha256> \
  --vcruntime140-dll <vcruntime140.dll> --vcruntime140-sha256 <sha256> \
  --vcruntime140-1-dll <vcruntime140_1.dll> \
  --vcruntime140-1-sha256 <sha256> \
  fixture --manifest <manifest.json> --corpus-root <corpus> \
  --fixture-id <fixture-id>
```

Use `pcm --pcm <input.f64le> --pcm-sha256 <sha256> --input-id <id>
--sample-rate <hz> --channels <n> --frames <n>` for explicit PCM. `--output`
writes the same path-free canonical JSON that would otherwise be written to
stdout.

The serial safe-master suite uses the same explicit identities:

```text
python reference/tools/run_foo_dr_meter_108_core_suite.py \
  --manifest <manifest.json> --corpus-root <corpus> \
  --worker <worker.exe> --worker-sha256 <sha256> \
  --target-dll <foo_dr_meter.dll> \
  --shared-dll <shared.dll> --shared-sha256 <sha256> \
  --msvcp140-dll <msvcp140.dll> --msvcp140-sha256 <sha256> \
  --vcruntime140-dll <vcruntime140.dll> --vcruntime140-sha256 <sha256> \
  --vcruntime140-1-dll <vcruntime140_1.dll> \
  --vcruntime140-1-sha256 <sha256> \
  --output <suite.json>
```

The suite continues after per-item input or worker failure, emits tagged
outcomes in manifest order, and still creates exactly one worker process per
input. All cases are prepared from the one manifest byte snapshot loaded at
suite start, so replacing the manifest path during execution cannot mix
identities. A successful suite record does not add a foobar-parity claim.

The parent requires one strict UTF-8 JSON line on worker stdout, enforces a
timeout, recursively rejects duplicate JSON keys, rejects unknown fields and
path leakage, and verifies all echoed identities, stream geometry, raw result
bits, session snapshots, channel state and floating-point control records.
stdout and diagnostic-only stderr are bounded while the child is running;
timeout or overflow terminates the direct worker. The current cross-platform
parent does not use a Windows Job Object to clean up a hypothetical descendant
process tree.
