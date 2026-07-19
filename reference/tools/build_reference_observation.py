#!/usr/bin/env python3
"""Build or verify a path-free foo_dr_meter reference observation package.

The harness is intentionally offline. It binds one fixed corpus manifest, the
manifest-ordered fixture bytes, one unchanged raw report, and explicit
target/run metadata into a deterministic package:

    capture.json
    raw/<configured-name>
    normalized/<configured-stem>.json
    observation.json

It never executes foobar2000, MacinMeter, or a candidate model.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import sys
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, Iterable


SCHEMA_VERSION = 1
HARNESS_VERSION = 1
HARNESS_REPOSITORY_PATH = "reference/tools/build_reference_observation.py"
NORMALIZER_REPOSITORY_PATH = "reference/tools/normalize_foo_dr_meter_report.py"
REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
UTC_OFFSET_RE = re.compile(r"^[+-](?:0\d|1[0-4]):[0-5]\d$")
SAFE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
WINDOWS_RESERVED_STEMS = {
    "aux",
    "con",
    "nul",
    "prn",
    *(f"com{index}" for index in range(1, 10)),
    *(f"lpt{index}" for index in range(1, 10)),
}
PRIVATE_PATH_RE = re.compile(
    r"(?i)(?:"
    r"file://|"
    r"(?<![A-Za-z0-9_])[A-Z]:[\\/]|"
    r"\\\\[^\\\s]+\\|"
    r"(?:^|[\s\"'=({])~[/\\]|"
    r"(?<![A-Za-z0-9_.-])/(?!/)[^\s\"']+"
    r")"
)

NORMALIZER_PATH = Path(__file__).with_name("normalize_foo_dr_meter_report.py")
NORMALIZER_SPEC = importlib.util.spec_from_file_location(
    "_foo_dr_meter_report_normalizer", NORMALIZER_PATH
)
if NORMALIZER_SPEC is None or NORMALIZER_SPEC.loader is None:
    raise RuntimeError(f"cannot load {NORMALIZER_PATH}")
NORMALIZER = importlib.util.module_from_spec(NORMALIZER_SPEC)
sys.modules[NORMALIZER_SPEC.name] = NORMALIZER
NORMALIZER_SPEC.loader.exec_module(NORMALIZER)


class HarnessError(ValueError):
    """An input or package violates the observation harness contract."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json_bytes(value: Any) -> bytes:
    try:
        rendered = json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            indent=2,
            allow_nan=False,
        )
    except (TypeError, ValueError) as error:
        raise HarnessError(f"value is not finite canonical JSON: {error}") from error
    return (rendered + "\n").encode("utf-8")


def load_json_object(path: Path, context: str) -> tuple[dict[str, Any], bytes]:
    try:
        raw = path.read_bytes()
        value = json.loads(raw)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise HarnessError(f"cannot read {context} JSON: {error}") from error
    if not isinstance(value, dict):
        raise HarnessError(f"{context} must contain one JSON object")
    return value, raw


def require_object(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise HarnessError(f"{context} must be an object")
    return value


def require_string(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise HarnessError(f"{context} must be a non-empty string")
    return value


def require_integer(value: Any, context: str, *, minimum: int = 0) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise HarnessError(f"{context} must be an integer >= {minimum}")
    return value


def require_sha256(value: Any, context: str) -> str:
    digest = require_string(value, context)
    if SHA256_RE.fullmatch(digest) is None:
        raise HarnessError(f"{context} must be one lowercase SHA-256")
    return digest


def require_exact_keys(
    value: dict[str, Any],
    required: Iterable[str],
    optional: Iterable[str],
    context: str,
) -> None:
    required_set = set(required)
    allowed = required_set | set(optional)
    missing = sorted(required_set - value.keys())
    extra = sorted(value.keys() - allowed)
    if missing or extra:
        raise HarnessError(f"{context} keys differ: missing={missing}, extra={extra}")


def require_portable_relative_path(value: Any, context: str) -> str:
    text = require_string(value, context)
    posix = PurePosixPath(text)
    if (
        posix.is_absolute()
        or PureWindowsPath(text).is_absolute()
        or "\\" in text
        or ":" in text
        or "." in posix.parts
        or ".." in posix.parts
        or posix.as_posix() != text
    ):
        raise HarnessError(f"{context} must be a canonical POSIX relative path")
    return text


def require_portable_basename(value: Any, context: str) -> str:
    text = require_string(value, context)
    stem = text.split(".", 1)[0].casefold()
    if (
        SAFE_NAME_RE.fullmatch(text) is None
        or text.endswith(".")
        or stem in WINDOWS_RESERVED_STEMS
    ):
        raise HarnessError(f"{context} must be a portable file basename")
    return text


def assert_path_free(value: Any, context: str = "capture metadata") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            assert_path_free(child, f"{context}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            assert_path_free(child, f"{context}[{index}]")
    elif isinstance(value, str) and PRIVATE_PATH_RE.search(value):
        raise HarnessError(f"{context} contains an absolute/private path")


def validate_capture(capture: dict[str, Any]) -> dict[str, Any]:
    require_exact_keys(
        capture,
        {
            "schemaVersion",
            "kind",
            "observationId",
            "status",
            "target",
            "experimentId",
            "corpus",
            "run",
            "rawReport",
            "limitations",
            "claims",
        },
        set(),
        "capture",
    )
    if capture["schemaVersion"] != SCHEMA_VERSION:
        raise HarnessError(f"capture.schemaVersion must be {SCHEMA_VERSION}")
    if capture["kind"] != "foo_dr_meter_observation_capture":
        raise HarnessError("capture.kind is not foo_dr_meter_observation_capture")
    for key in ("observationId", "status", "experimentId"):
        require_string(capture[key], f"capture.{key}")

    target = require_object(capture["target"], "capture.target")
    require_exact_keys(
        target,
        {"id", "architecture", "expectedHeader", "identities"},
        set(),
        "capture.target",
    )
    require_string(target["id"], "capture.target.id")
    if target["architecture"] not in {"x86", "x86_64"}:
        raise HarnessError("capture.target.architecture must be x86 or x86_64")
    header = require_object(target["expectedHeader"], "capture.target.expectedHeader")
    require_exact_keys(
        header,
        {"foobar2000Version", "drMeterVersion"},
        set(),
        "capture.target.expectedHeader",
    )
    foobar_version = require_string(
        header["foobar2000Version"],
        "capture.target.expectedHeader.foobar2000Version",
    )
    plugin_version = require_string(
        header["drMeterVersion"],
        "capture.target.expectedHeader.drMeterVersion",
    )
    identities = target["identities"]
    if not isinstance(identities, list) or not identities:
        raise HarnessError("capture.target.identities must be a non-empty array")
    identity_roles: set[str] = set()
    identity_versions: dict[str, str] = {}
    for index, identity_value in enumerate(identities):
        identity = require_object(
            identity_value, f"capture.target.identities[{index}]"
        )
        require_exact_keys(
            identity,
            {"role", "version", "architecture", "sha256", "byteLength", "binding"},
            set(),
            f"capture.target.identities[{index}]",
        )
        role = require_string(
            identity["role"], f"capture.target.identities[{index}].role"
        )
        if SAFE_NAME_RE.fullmatch(role) is None or role in identity_roles:
            raise HarnessError(f"invalid or repeated target identity role {role!r}")
        identity_roles.add(role)
        identity_versions[role] = require_string(
            identity["version"], f"capture.target.identities[{index}].version"
        )
        if identity["architecture"] != target["architecture"]:
            raise HarnessError(f"target identity {role!r} architecture differs")
        require_sha256(
            identity["sha256"], f"capture.target.identities[{index}].sha256"
        )
        require_integer(
            identity["byteLength"],
            f"capture.target.identities[{index}].byteLength",
            minimum=1,
        )
        if identity["binding"] != "fixed_target_record":
            raise HarnessError(
                f"target identity {role!r} binding must be fixed_target_record"
            )
    required_roles = {"foo_dr_meter", "foobar2000", "foo_input_std"}
    if not required_roles.issubset(identity_roles):
        raise HarnessError(
            f"target identities omit roles {sorted(required_roles - identity_roles)}"
        )
    if identity_versions["foo_dr_meter"] != plugin_version:
        raise HarnessError("foo_dr_meter identity version differs from report header")
    if identity_versions["foobar2000"] != foobar_version:
        raise HarnessError("foobar2000 identity version differs from report header")

    corpus = require_object(capture["corpus"], "capture.corpus")
    require_exact_keys(
        corpus,
        {
            "id",
            "repositoryManifestPath",
            "manifestSha256",
            "filesSha256FileSha256",
            "playlist",
        },
        set(),
        "capture.corpus",
    )
    require_string(corpus["id"], "capture.corpus.id")
    require_portable_relative_path(
        corpus["repositoryManifestPath"], "capture.corpus.repositoryManifestPath"
    )
    require_sha256(corpus["manifestSha256"], "capture.corpus.manifestSha256")
    require_sha256(
        corpus["filesSha256FileSha256"],
        "capture.corpus.filesSha256FileSha256",
    )
    playlist = require_string(corpus["playlist"], "capture.corpus.playlist")
    if SAFE_NAME_RE.fullmatch(playlist) is None:
        raise HarnessError("capture.corpus.playlist must be a safe identifier")

    run = require_object(capture["run"], "capture.run")
    require_exact_keys(
        run,
        {
            "repeat",
            "procedure",
            "timezone",
            "settings",
            "operatorNotes",
            "repeatConsistency",
        },
        {"repeatOfObservationId"},
        "capture.run",
    )
    repeat = require_integer(run["repeat"], "capture.run.repeat", minimum=1)
    require_string(run["procedure"], "capture.run.procedure")
    require_string(run["operatorNotes"], "capture.run.operatorNotes")
    repeat_consistency = require_string(
        run["repeatConsistency"], "capture.run.repeatConsistency"
    )
    expected_repeat_consistency = (
        "not_assessed" if repeat == 1 else "not_assessed_by_this_import"
    )
    if repeat_consistency != expected_repeat_consistency:
        raise HarnessError(
            "capture.run.repeatConsistency must remain "
            f"{expected_repeat_consistency!r}; this importer does not compare runs"
        )
    if repeat == 1 and "repeatOfObservationId" in run:
        raise HarnessError("run1 must not name repeatOfObservationId")
    if repeat > 1:
        require_string(
            run.get("repeatOfObservationId"), "capture.run.repeatOfObservationId"
        )
    timezone = require_object(run["timezone"], "capture.run.timezone")
    require_exact_keys(timezone, {"name", "utcOffset"}, set(), "capture.run.timezone")
    require_string(timezone["name"], "capture.run.timezone.name")
    if UTC_OFFSET_RE.fullmatch(str(timezone["utcOffset"])) is None:
        raise HarnessError("capture.run.timezone.utcOffset must be ±HH:MM")
    settings = require_object(run["settings"], "capture.run.settings")
    expected_settings = {
        "automaticallySaveTags": False,
        "stereoPerChannelStats": True,
        "albumLengthWeighting": False,
        "multichannelLoudnessWeighting": False,
    }
    require_exact_keys(settings, expected_settings, set(), "capture.run.settings")
    for name, expected in expected_settings.items():
        setting = require_object(settings[name], f"capture.run.settings.{name}")
        require_exact_keys(
            setting, {"value", "source"}, set(), f"capture.run.settings.{name}"
        )
        if setting["value"] is not expected:
            raise HarnessError(f"capture.run.settings.{name}.value must be {expected}")
        require_string(setting["source"], f"capture.run.settings.{name}.source")

    raw_report = require_object(capture["rawReport"], "capture.rawReport")
    require_exact_keys(
        raw_report,
        {"group", "outputName", "sha256", "byteLength"},
        set(),
        "capture.rawReport",
    )
    require_string(raw_report["group"], "capture.rawReport.group")
    output_name = require_portable_basename(
        raw_report["outputName"], "capture.rawReport.outputName"
    )
    if output_name in {
        ".",
        "..",
        "observation.json",
        "capture.json",
    }:
        raise HarnessError("capture.rawReport.outputName is not a safe basename")
    require_sha256(raw_report["sha256"], "capture.rawReport.sha256")
    require_integer(
        raw_report["byteLength"], "capture.rawReport.byteLength", minimum=1
    )

    limitations = capture["limitations"]
    if not isinstance(limitations, list) or not all(
        isinstance(item, str) and item.strip() for item in limitations
    ):
        raise HarnessError("capture.limitations must be an array of non-empty strings")
    claims = require_object(capture["claims"], "capture.claims")
    require_exact_keys(
        claims,
        {"scope", "compatibility", "appliesToVersion"},
        set(),
        "capture.claims",
    )
    for key in claims:
        require_string(claims[key], f"capture.claims.{key}")
    if claims["compatibility"] != "none":
        raise HarnessError(
            "capture.claims.compatibility must remain 'none'; "
            "this importer cannot establish compatibility"
        )

    assert_path_free(capture)
    return capture


def parse_checksums(path: Path) -> tuple[dict[str, str], bytes]:
    try:
        raw = path.read_bytes()
        text = raw.decode("ascii")
    except (OSError, UnicodeError) as error:
        raise HarnessError(f"cannot read FILES.sha256: {error}") from error
    checksums: dict[str, str] = {}
    for line_number, line in enumerate(text.splitlines(), 1):
        digest, separator, relative = line.partition("  ")
        if not separator:
            raise HarnessError(f"FILES.sha256 line {line_number} is malformed")
        require_sha256(digest, f"FILES.sha256 line {line_number}")
        canonical = require_portable_relative_path(
            relative, f"FILES.sha256 line {line_number} path"
        )
        if canonical in checksums:
            raise HarnessError(f"FILES.sha256 repeats {canonical!r}")
        checksums[canonical] = digest
    return checksums, raw


def validate_manifest_and_fixtures(
    capture: dict[str, Any],
    manifest_path: Path,
    corpus_root: Path,
    repository_root: Path,
) -> tuple[dict[str, Any], list[dict[str, Any]], bytes]:
    manifest, manifest_raw = load_json_object(manifest_path, "corpus manifest")
    corpus = capture["corpus"]
    actual_manifest_sha = sha256_bytes(manifest_raw)
    if actual_manifest_sha != corpus["manifestSha256"]:
        raise HarnessError("corpus manifest SHA-256 differs from capture metadata")
    if manifest.get("corpusId") != corpus["id"]:
        raise HarnessError("corpusId differs between manifest and capture metadata")
    repository_manifest = (
        repository_root
        / Path(*PurePosixPath(corpus["repositoryManifestPath"]).parts)
    )
    try:
        supplied_manifest = manifest_path.resolve(strict=True)
        expected_manifest = repository_manifest.resolve(strict=True)
    except OSError as error:
        raise HarnessError(f"cannot resolve canonical repository manifest: {error}") from error
    if supplied_manifest != expected_manifest:
        raise HarnessError(
            "manifest input is not the declared canonical repository manifest"
        )
    generated_manifest_path = corpus_root / "manifest.json"
    if generated_manifest_path.is_symlink() or not generated_manifest_path.is_file():
        raise HarnessError("corpus root manifest.json is missing or not a regular file")
    if generated_manifest_path.read_bytes() != manifest_raw:
        raise HarnessError("corpus root manifest.json differs from canonical manifest")

    checksum_path = corpus_root / "FILES.sha256"
    checksums, checksum_raw = parse_checksums(checksum_path)
    if sha256_bytes(checksum_raw) != corpus["filesSha256FileSha256"]:
        raise HarnessError("FILES.sha256 SHA-256 differs from capture metadata")
    if checksums.get("manifest.json") != actual_manifest_sha:
        raise HarnessError("FILES.sha256 does not bind the supplied manifest")

    cases = manifest.get("cases")
    playlists = manifest.get("playlists")
    if not isinstance(cases, list) or not isinstance(playlists, dict):
        raise HarnessError("manifest must contain cases array and playlists object")
    playlist_ids = playlists.get(corpus["playlist"])
    if not isinstance(playlist_ids, list) or not all(
        isinstance(case_id, str) for case_id in playlist_ids
    ):
        raise HarnessError(f"manifest playlist {corpus['playlist']!r} is invalid")
    if not playlist_ids or len(set(playlist_ids)) != len(playlist_ids):
        raise HarnessError("manifest playlist must contain unique fixture IDs")

    cases_by_id: dict[str, dict[str, Any]] = {}
    paths: set[str] = set()
    orders: set[int] = set()
    for index, case_value in enumerate(cases):
        case = require_object(case_value, f"manifest.cases[{index}]")
        case_id = require_string(case.get("id"), f"manifest.cases[{index}].id")
        if case_id in cases_by_id:
            raise HarnessError(f"manifest repeats fixture ID {case_id!r}")
        case_path = require_portable_relative_path(
            case.get("path"), f"manifest case {case_id!r} path"
        )
        if case_path in paths:
            raise HarnessError(f"manifest repeats fixture path {case_path!r}")
        order = require_integer(
            case.get("order"), f"manifest case {case_id!r} order", minimum=1
        )
        if order in orders:
            raise HarnessError(f"manifest repeats fixture order {order}")
        require_sha256(case.get("fileSha256"), f"manifest case {case_id!r} fileSha256")
        require_integer(
            case.get("byteLength"),
            f"manifest case {case_id!r} byteLength",
            minimum=1,
        )
        require_integer(
            case.get("channels"), f"manifest case {case_id!r} channels", minimum=1
        )
        paths.add(case_path)
        orders.add(order)
        cases_by_id[case_id] = case

    selected: list[dict[str, Any]] = []
    for case_id in playlist_ids:
        try:
            selected.append(cases_by_id[case_id])
        except KeyError as error:
            raise HarnessError(f"playlist names unknown fixture {case_id!r}") from error
    selected_orders = [case["order"] for case in selected]
    if selected_orders != sorted(selected_orders):
        raise HarnessError("playlist order differs from manifest fixture order")
    playlist_relative = f"playlists/{corpus['playlist']}.m3u8"
    playlist_path = corpus_root / playlist_relative
    if playlist_path.is_symlink() or not playlist_path.is_file():
        raise HarnessError("selected M3U8 is missing or not a regular file")
    try:
        playlist_raw = playlist_path.read_bytes()
        playlist_text = playlist_raw.decode("utf-8")
    except (OSError, UnicodeError) as error:
        raise HarnessError(f"cannot read selected M3U8: {error}") from error
    if checksums.get(playlist_relative) != sha256_bytes(playlist_raw):
        raise HarnessError("FILES.sha256 does not bind the selected M3U8")
    playlist_entries = [
        line
        for line in playlist_text.splitlines()
        if line and not line.startswith("#")
    ]
    expected_entries = [f"../{case['path']}" for case in selected]
    if playlist_entries != expected_entries:
        raise HarnessError("selected M3U8 membership/order differs from manifest")

    expected_safe_count = (
        manifest.get("budgets", {}).get("expectedSafeMasterEntries")
        if corpus["playlist"] == "00-safe-master"
        else None
    )
    if expected_safe_count is not None and len(selected) != expected_safe_count:
        raise HarnessError("safe-master fixture count differs from manifest budget")
    isolated = manifest.get("isolatedFixtureIds", [])
    if not isinstance(isolated, list):
        raise HarnessError("manifest isolatedFixtureIds must be an array")
    if set(playlist_ids).intersection(isolated):
        raise HarnessError("isolated fixture leaked into selected playlist")

    root = corpus_root.resolve()
    verified: list[dict[str, Any]] = []
    for case in selected:
        relative = case["path"]
        fixture = corpus_root / relative
        if fixture.is_symlink() or not fixture.is_file():
            raise HarnessError(f"fixture is missing or not a regular file: {relative}")
        try:
            fixture.resolve(strict=True).relative_to(root)
        except (OSError, ValueError) as error:
            raise HarnessError(f"fixture escapes corpus root: {relative}") from error
        try:
            fixture_raw = fixture.read_bytes()
        except OSError as error:
            raise HarnessError(f"cannot read fixture {relative}: {error}") from error
        actual_sha = sha256_bytes(fixture_raw)
        if actual_sha != case["fileSha256"]:
            raise HarnessError(f"fixture SHA-256 mismatch: {relative}")
        if len(fixture_raw) != case["byteLength"]:
            raise HarnessError(f"fixture byte length mismatch: {relative}")
        if checksums.get(relative) != actual_sha:
            raise HarnessError(f"FILES.sha256 does not bind fixture: {relative}")
        verified.append(
            {
                "fixtureId": case["id"],
                "manifestOrder": case["order"],
                "path": relative,
                "sha256": actual_sha,
                "byteLength": len(fixture_raw),
            }
        )
    return manifest, verified, manifest_raw


def normalize_report(
    capture: dict[str, Any],
    report_path: Path,
    manifest_path: Path,
    manifest_raw: bytes,
) -> tuple[dict[str, Any], bytes]:
    try:
        report_raw = report_path.read_bytes()
    except OSError as error:
        raise HarnessError(f"cannot read raw report: {error}") from error
    expected = capture["rawReport"]
    if sha256_bytes(report_raw) != expected["sha256"]:
        raise HarnessError("raw report SHA-256 differs from capture metadata")
    if len(report_raw) != expected["byteLength"]:
        raise HarnessError("raw report byte length differs from capture metadata")
    try:
        report_text = report_raw.decode("ascii")
    except UnicodeDecodeError as error:
        raise HarnessError("raw report must be US-ASCII") from error
    if PRIVATE_PATH_RE.search(report_text):
        raise HarnessError("raw report contains an absolute/private path")
    try:
        normalized = NORMALIZER.normalize(
            report_path, manifest_path, capture["corpus"]["playlist"]
        )
    except (OSError, NORMALIZER.ReportError) as error:
        raise HarnessError(f"raw report normalization failed: {error}") from error
    source = require_object(normalized.get("source"), "normalized report source")
    expected_source = {
        "rawReportSha256": sha256_bytes(report_raw),
        "rawReportByteLength": len(report_raw),
        "manifestSha256": sha256_bytes(manifest_raw),
        "corpusId": capture["corpus"]["id"],
        "playlist": capture["corpus"]["playlist"],
    }
    for field, expected_value in expected_source.items():
        if source.get(field) != expected_value:
            raise HarnessError(
                f"normalized report source {field} differs from validated input"
            )
    header = normalized["header"]
    expected_header = capture["target"]["expectedHeader"]
    if header["foobar2000Version"] != expected_header["foobar2000Version"]:
        raise HarnessError("report foobar2000 version differs from target metadata")
    if header["drMeterVersion"] != expected_header["drMeterVersion"]:
        raise HarnessError("report DR Meter version differs from target metadata")
    return normalized, report_raw


def artifact_paths(capture: dict[str, Any]) -> dict[str, str]:
    raw_name = capture["rawReport"]["outputName"]
    normalized_name = f"{Path(raw_name).stem}.json"
    return {
        "capture": "capture.json",
        "raw": f"raw/{raw_name}",
        "normalized": f"normalized/{normalized_name}",
        "observation": "observation.json",
    }


def make_observation(
    capture: dict[str, Any],
    capture_raw: bytes,
    manifest: dict[str, Any],
    verified_fixtures: list[dict[str, Any]],
    report_raw: bytes,
    normalized: dict[str, Any],
    normalized_raw: bytes,
) -> dict[str, Any]:
    paths = artifact_paths(capture)
    harness_raw = Path(__file__).read_bytes()
    normalizer_raw = NORMALIZER_PATH.read_bytes()
    validation = normalized["validation"]
    return {
        "schemaVersion": 1,
        "kind": "reference_observation",
        "factClass": "reference-observation",
        "observationId": capture["observationId"],
        "status": capture["status"],
        "targetId": capture["target"]["id"],
        "experimentId": capture["experimentId"],
        "corpus": {
            "id": capture["corpus"]["id"],
            "manifestPath": capture["corpus"]["repositoryManifestPath"],
            "manifestSha256": capture["corpus"]["manifestSha256"],
            "filesSha256FileSha256": capture["corpus"][
                "filesSha256FileSha256"
            ],
            "generatorName": manifest.get("generator", {}).get("name"),
            "generatorSha256": manifest.get("generator", {}).get("sourceSha256"),
            "playlist": capture["corpus"]["playlist"],
        },
        "targetIdentity": {
            "architecture": capture["target"]["architecture"],
            "components": capture["target"]["identities"],
            "headerVersionsMatchTarget": True,
        },
        "run": {
            **capture["run"],
            "reportedLogDate": normalized["header"]["reportedLogDate"],
        },
        "inputVerification": {
            "manifestSha256Exact": True,
            "filesSha256FileSha256Exact": True,
            "fixtureCount": len(verified_fixtures),
            "fixtureFilesRehashed": True,
            "fixtureHashesExact": True,
            "manifestOrderExact": True,
            "fixtures": verified_fixtures,
        },
        "rawReports": [
            {
                "group": capture["rawReport"]["group"],
                "path": paths["raw"],
                "sha256": sha256_bytes(report_raw),
                "byteLength": len(report_raw),
                "encoding": "US-ASCII",
                "lineEndings": "CRLF",
                "capturedVerbatim": True,
            }
        ],
        "normalization": {
            "path": paths["normalized"],
            "sha256": sha256_bytes(normalized_raw),
            "parserPath": NORMALIZER_REPOSITORY_PATH,
            "parserSha256": sha256_bytes(normalizer_raw),
            "numericTokensPreservedAsText": True,
        },
        "validation": {
            "fixtureCount": validation["observedTrackCount"],
            "channelValueCount": validation["observedChannelValueCount"],
            "fixtureStemsExactlyOnce": validation["manifestStemsExactlyOnce"],
            "manifestOrderExact": validation["manifestOrderExact"],
            "footerTrackCount": int(normalized["footer"]["numberOfTracksToken"]),
            "repeatConsistency": capture["run"]["repeatConsistency"],
        },
        "provenance": {
            "captureMetadataPath": paths["capture"],
            "captureMetadataSha256": sha256_bytes(capture_raw),
            "harnessPath": HARNESS_REPOSITORY_PATH,
            "harnessVersion": HARNESS_VERSION,
            "harnessSha256": sha256_bytes(harness_raw),
            "offline": True,
            "candidateOutputConsumed": False,
        },
        "limitations": capture["limitations"],
        "claims": capture["claims"],
    }


def prepare_artifacts(
    capture: dict[str, Any],
    manifest_path: Path,
    corpus_root: Path,
    report_path: Path,
    repository_root: Path = REPOSITORY_ROOT,
) -> dict[str, bytes]:
    validate_capture(capture)
    capture_raw = canonical_json_bytes(capture)
    manifest, verified, manifest_raw = validate_manifest_and_fixtures(
        capture, manifest_path, corpus_root, repository_root
    )
    normalized, report_raw = normalize_report(
        capture, report_path, manifest_path, manifest_raw
    )
    normalized_raw = canonical_json_bytes(normalized)
    observation = make_observation(
        capture,
        capture_raw,
        manifest,
        verified,
        report_raw,
        normalized,
        normalized_raw,
    )
    artifacts = {
        artifact_paths(capture)["capture"]: capture_raw,
        artifact_paths(capture)["raw"]: report_raw,
        artifact_paths(capture)["normalized"]: normalized_raw,
        artifact_paths(capture)["observation"]: canonical_json_bytes(observation),
    }
    for relative, raw in artifacts.items():
        if PRIVATE_PATH_RE.search(raw.decode("ascii", errors="ignore")):
            raise HarnessError(f"generated artifact {relative} contains a private path")
    return artifacts


def write_artifacts(output_dir: Path, artifacts: dict[str, bytes]) -> None:
    if output_dir.exists():
        if output_dir.is_symlink() or not output_dir.is_dir() or any(output_dir.iterdir()):
            raise HarnessError("output directory must be absent or empty")
    else:
        output_dir.mkdir(parents=True)
    for relative, raw in artifacts.items():
        path = output_dir / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)


def build_package(
    metadata_path: Path,
    manifest_path: Path,
    corpus_root: Path,
    report_path: Path,
    output_dir: Path,
    repository_root: Path = REPOSITORY_ROOT,
) -> dict[str, Any]:
    capture, _ = load_json_object(metadata_path, "capture metadata")
    artifacts = prepare_artifacts(
        capture,
        manifest_path,
        corpus_root,
        report_path,
        repository_root,
    )
    write_artifacts(output_dir, artifacts)
    observation = json.loads(artifacts["observation.json"])
    return {
        "observationId": observation["observationId"],
        "fixtureCount": observation["validation"]["fixtureCount"],
        "packageFileCount": len(artifacts),
        "observationSha256": sha256_bytes(artifacts["observation.json"]),
    }


def verify_package(
    package_dir: Path,
    manifest_path: Path,
    corpus_root: Path,
    repository_root: Path = REPOSITORY_ROOT,
) -> dict[str, Any]:
    if package_dir.is_symlink() or not package_dir.is_dir():
        raise HarnessError("package path must be a regular directory")
    capture_path = package_dir / "capture.json"
    capture, _ = load_json_object(capture_path, "package capture metadata")
    validate_capture(capture)
    paths = artifact_paths(capture)
    expected_paths = set(paths.values())
    actual_paths: set[str] = set()
    for path in package_dir.rglob("*"):
        if path.is_symlink():
            raise HarnessError("observation package must not contain symbolic links")
        if path.is_file():
            actual_paths.add(path.relative_to(package_dir).as_posix())
    if actual_paths != expected_paths:
        raise HarnessError(
            "package path set differs: "
            f"missing={sorted(expected_paths - actual_paths)}, "
            f"extra={sorted(actual_paths - expected_paths)}"
        )
    artifacts = prepare_artifacts(
        capture,
        manifest_path,
        corpus_root,
        package_dir / paths["raw"],
        repository_root,
    )
    for relative, expected in artifacts.items():
        actual = (package_dir / relative).read_bytes()
        if actual != expected:
            raise HarnessError(f"package artifact differs from reconstruction: {relative}")
    observation = json.loads(artifacts["observation.json"])
    return {
        "observationId": observation["observationId"],
        "fixtureCount": observation["validation"]["fixtureCount"],
        "packageFileCount": len(artifacts),
        "observationSha256": sha256_bytes(artifacts["observation.json"]),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    build = subparsers.add_parser("build", help="build a new observation package")
    build.add_argument("--metadata", required=True, type=Path)
    build.add_argument("--manifest", required=True, type=Path)
    build.add_argument("--corpus-root", required=True, type=Path)
    build.add_argument("--report", required=True, type=Path)
    build.add_argument("--output", required=True, type=Path)
    verify = subparsers.add_parser("verify", help="reconstruct and verify a package")
    verify.add_argument("--package", required=True, type=Path)
    verify.add_argument("--manifest", required=True, type=Path)
    verify.add_argument("--corpus-root", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "build":
            summary = build_package(
                args.metadata,
                args.manifest,
                args.corpus_root,
                args.report,
                args.output,
            )
        else:
            summary = verify_package(args.package, args.manifest, args.corpus_root)
    except (HarnessError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
    print(json.dumps(summary, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
