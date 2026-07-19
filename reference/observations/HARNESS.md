# Reference observation import harness

## Purpose and boundary

[`build_reference_observation.py`](../tools/build_reference_observation.py)
turns one fixed corpus, one unchanged `foo_dr_meter` report and explicit capture
metadata into a deterministic observation package. It is an offline evidence
importer, not an experiment runner:

- it does not start foobar2000 or require a network connection;
- it never reads MacinMeter output or model predictions;
- it does not infer target identity or plugin settings from the local machine;
- it preserves the raw report and derives normalization only through
  [`normalize_foo_dr_meter_report.py`](../tools/normalize_foo_dr_meter_report.py).

The capture metadata binds component hashes to an already fixed target record.
The harness validates their syntax, byte lengths, architecture and version
cross-links, then independently checks the report header. It does not claim to
rehash installed target binaries. Binary identity must already have been fixed
using the target-record procedure.

## Inputs

`build` requires five independent local inputs:

1. a path-free capture metadata JSON object;
2. the canonical repository manifest;
3. the generated corpus root containing `FILES.sha256` and the fixture files;
4. the original, unchanged report;
5. an absent or empty output directory.

For complete-v2, the canonical manifest is
[`foo-dr-meter-108-complete-v2.manifest.json`](../fixtures/foo-dr-meter-108-complete-v2.manifest.json).
The generated corpus must first pass its own `--verify` command described by
[`EXP-foo-dr-meter-108-complete-v2`](../experiments/foo-dr-meter-108-complete-v2.md).
The importer then rehashes every fixture named by the selected playlist instead
of trusting that earlier result.

The following is a run-2 x64 metadata skeleton. Replace angle-bracket values;
do not add local paths:

```json
{
  "schemaVersion": 1,
  "kind": "foo_dr_meter_observation_capture",
  "observationId": "<OBS-ID>",
  "status": "safe_master_repeat",
  "target": {
    "id": "TARGET-foo-dr-meter-1.0.8-foobar2000-2.25.10-x64-win10-19045",
    "architecture": "x86_64",
    "expectedHeader": {
      "foobar2000Version": "2.25.10",
      "drMeterVersion": "1.0.8"
    },
    "identities": [
      {
        "role": "foo_dr_meter",
        "version": "1.0.8",
        "architecture": "x86_64",
        "sha256": "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489",
        "byteLength": 424448,
        "binding": "fixed_target_record"
      },
      {
        "role": "foobar2000",
        "version": "2.25.10",
        "architecture": "x86_64",
        "sha256": "653cc120c146aaae9e6db9b6f19e5a1588407b8940bc1521f0ced739ff8924b0",
        "byteLength": 4789128,
        "binding": "fixed_target_record"
      },
      {
        "role": "foo_input_std",
        "version": "fixed-2.25.10-installation-component",
        "architecture": "x86_64",
        "sha256": "46a4b9c4515fae55add895e12d30602f73944959f0e0f7acf7122e6562b51651",
        "byteLength": 2505616,
        "binding": "fixed_target_record"
      }
    ]
  },
  "experimentId": "EXP-foo-dr-meter-108-complete-v2",
  "corpus": {
    "id": "foo-dr-meter-108-complete-v2",
    "repositoryManifestPath": "reference/fixtures/foo-dr-meter-108-complete-v2.manifest.json",
    "manifestSha256": "479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8",
    "filesSha256FileSha256": "b5692b3165d82d7d189c38573a48fd0b5dd24750e82aa706d6c9eb45ce5d7595",
    "playlist": "00-safe-master"
  },
  "run": {
    "repeat": 2,
    "repeatOfObservationId": "OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718",
    "procedure": "foobar2000 GUI Measure Dynamic Range followed by Save Log",
    "timezone": {
      "name": "China Standard Time",
      "utcOffset": "+08:00"
    },
    "settings": {
      "automaticallySaveTags": {
        "value": false,
        "source": "operator_attested"
      },
      "stereoPerChannelStats": {
        "value": true,
        "source": "operator_attested_and_report_corroborated"
      },
      "albumLengthWeighting": {
        "value": false,
        "source": "operator_attested"
      },
      "multichannelLoudnessWeighting": {
        "value": false,
        "source": "operator_attested"
      }
    },
    "operatorNotes": "none",
    "repeatConsistency": "not_assessed_by_this_import"
  },
  "rawReport": {
    "group": "safe-master",
    "outputName": "<portable-report-basename>.txt",
    "sha256": "<lowercase-SHA-256>",
    "byteLength": 12345
  },
  "limitations": [
    "Only exported final report fields are dynamically observable."
  ],
  "claims": {
    "scope": "safe-master exported report text for this exact runtime target only",
    "compatibility": "none",
    "appliesToVersion": "foo_dr_meter 1.0.8 x64 under foobar2000 2.25.10 only"
  }
}
```

`repeatConsistency` is deliberately not inferred by this importer and must
remain `not_assessed_by_this_import` for a repeat capture (`not_assessed` for
run 1). Likewise, `claims.compatibility` must remain `none`. A second report has
a different log date even when every measured field is stable, so a separate
comparison must define which normalized fields constitute repeatability or
compatibility.

## Build and verify

First calculate the raw report identity without editing it:

```bash
shasum -a 256 "${REPORT}"
wc -c < "${REPORT}"
```

Build:

```bash
python3 reference/tools/build_reference_observation.py build \
  --metadata "${CAPTURE_METADATA}" \
  --manifest reference/fixtures/foo-dr-meter-108-complete-v2.manifest.json \
  --corpus-root "${CORPUS_ROOT}" \
  --report "${REPORT}" \
  --output "${OBSERVATION_PACKAGE}"
```

Reconstruct and verify every package artifact:

```bash
python3 reference/tools/build_reference_observation.py verify \
  --package "${OBSERVATION_PACKAGE}" \
  --manifest reference/fixtures/foo-dr-meter-108-complete-v2.manifest.json \
  --corpus-root "${CORPUS_ROOT}"
```

Both commands print one compact JSON summary. `verify` rejects missing, extra,
modified or symlinked package files.

## Validation performed

Before writing anything, `build` checks:

- capture schema, required target identities, complete-v2 settings and
  path-free metadata;
- the supplied manifest path resolves to the declared canonical path inside
  the current repository checkout;
- canonical manifest and `FILES.sha256` identities;
- selected playlist membership, uniqueness, fixture order and isolated
  exclusion;
- every selected fixture's byte length and SHA-256 against both manifest and
  `FILES.sha256`;
- raw report byte length, SHA-256, US-ASCII encoding and CRLF endings;
- report header versions against target metadata;
- exact fixture stem/order/channel counts and footer track count through the
  existing normalizer.
- normalization source identities still match the report and manifest byte
  snapshots already validated by the importer.

The package contains exactly:

```text
capture.json
observation.json
raw/<portable-report-basename>.txt
normalized/<portable-report-basename>.json
```

`capture.json` is canonicalized JSON, not a copy of its local input filename.
All CLI input paths remain local process state. Absolute POSIX paths (including
single-component paths), Windows drive/UNC paths, home-relative paths and
`file://` paths are rejected in capture metadata and raw report text. Portable
output basenames also reject Windows device names and trailing dots. Generated
records use only portable repository/package-relative paths.

The observation records the harness and normalizer source hashes. To verify a
package after either tool changes, check out the recorded tool revision first;
the package remains intentionally bound to the importer that created it.
