# Isolated foo_dr_meter 1.0.8 x64 core worker

This Windows x64 helper is the worker side of
`reference/tools/run_foo_dr_meter_108_core.py`. It accepts exactly
`--request <request.json>`, privately stages and re-verifies the fixed
`foo_dr_meter.dll` plus the four allowlisted runtime DLLs, loads the target
with DLL-load-directory and System32-only search flags, invokes the fixed
analyzer RVAs, and writes one path-free JSON protocol line.

The staging directory is created with a protected DACL granting access only to
the process token's user SID and Local System. A 128-bit random name is paired
with a non-reparse directory handle acquired immediately after creation; that
handle omits delete sharing and remains open through target unload. Each
verified staged file likewise remains open with read sharing only, which denies
ordinary write, delete, rename, and replacement opens. Immediately before and
after loading, the worker opens the paths again and requires their volume/file
IDs and hashes to match the locked objects. Thus the verified target and
runtime paths cannot be substituted between verification and
`LoadLibraryExW` through normal filesystem APIs.

This is a filesystem TOCTOU boundary, not isolation from code already executing
as the same Windows principal. That principal owns the directory and can
rewrite its DACL; process injection, token theft, kernel access, and a writable
mapping established during the tiny create-to-lock interval are outside this
worker's threat model. The protected DACL, random name, no-delete directory
handle, no-write file handles, and object-ID checks make such a race explicit
rather than treating the temporary path as trustworthy.

The worker is intentionally tied to target SHA-256
`ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`.
It is evidence tooling, not production analysis code and not a compatibility
claim.

## Host-service boundary

Windows still needs the fixed real `shared.dll` during the component DLL's
normal load and unload lifecycle. That does not require a running foobar2000
process. After loading is complete, the worker finds all 13 `shared.dll`
imports in the fixed target and replaces their IAT slots with one fail-fast
tripwire for the complete init/push/finish/cleanup interval. A call made through
any of those 13 ordinary IAT slots therefore terminates that one worker process
instead of receiving an invented return value. The original slots are restored
before `FreeLibrary`.

The tripwire does not cover a function resolved dynamically with
`GetProcAddress`, a pointer cached elsewhere before arming, a delay-load table,
or another indirect transfer that bypasses those fixed ordinary IAT slots.
The worker validates the target's exact 13-entry ordinary `shared.dll` import
set; it does not turn that static fact into a broader claim about every
possible host-service path.

The separately built fail-fast `shared.dll` is a negative lifecycle probe, not
a successful runtime profile. On the fixed target it is reached by
`LoadLibraryExW` before the core can run. The accepted core path therefore uses
the pinned real `shared.dll` for load/unload and the IAT tripwire for the
algorithm interval.

The fourth binary64 init argument is fixed to `0.0`. Static analysis shows that
the fixed core only stores it at session `+0x00`; no meaning is invented for
the corresponding host field.

## Build

From an x64 Visual Studio developer prompt:

```powershell
cmake -S . -B build -A x64
cmake --build build --config Release
ctest --test-dir build -C Release --output-on-failure
```

The worker is linked with the static CRT. Before accepting a build, inspect its
imports and confirm that the worker itself does not preload `shared.dll`,
`MSVCP140.dll`, `VCRUNTIME140.dll`, or `VCRUNTIME140_1.dll`.

The `foo_dr_meter_108_fail_fast_shared` target builds a separately identifiable
`shared.dll`. All 13 plugin imports terminate the process and the only other
export is a fixed marker. It never silently emulates foobar2000 host services.

Two opt-in environment variables are diagnostic-only and never change a
successful record:

- `MACINMETER_CORE_TRACE=1` writes path-free checkpoints to stderr;
- `MACINMETER_CORE_TRIPWIRE_SELF_TEST=1` deliberately invokes one patched IAT
  slot and must terminate the worker.

The parent discards diagnostic stderr and accepts only one strict JSON line on
stdout.
