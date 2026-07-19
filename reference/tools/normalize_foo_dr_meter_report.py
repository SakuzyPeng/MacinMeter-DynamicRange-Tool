#!/usr/bin/env python3
"""Normalize one foo_dr_meter text report against a fixed corpus manifest.

The raw report remains the source of truth. This tool preserves every exported
numeric token as text and only adds deterministic structure plus input-order
validation. It does not apply a candidate algorithm or create a golden from
MacinMeter output.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any


HEADER_RE = re.compile(
    r"^foobar2000 v(?P<foobar>\S+) / DR Meter v(?P<plugin>\S+)$"
)
TRACK_RE = re.compile(
    r"^DR(?P<track_dr>\d+)\s+"
    r"(?P<peak>-inf|[-+]?\d+\.\d+) dBFS\s+"
    r"(?P<rms>-inf|[-+]?\d+\.\d+) dBFS\s+"
    r"(?P<duration>\d+:\d+)\s+\?-(?P<rest>.*)$"
)
CHANNEL_TOKEN_RE = re.compile(
    r"(?P<value>-inf|[-+]?\d+\.\d+)\s+dB(?P<full_scale>FS)?"
)


class ReportError(ValueError):
    """The report does not satisfy the fixed normalization contract."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ReportError(f"cannot read JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise ReportError(f"{path} must contain one JSON object")
    return value


def require_crlf_ascii(path: Path) -> tuple[bytes, str]:
    try:
        raw = path.read_bytes()
    except OSError as error:
        raise ReportError(f"cannot read report {path}: {error}") from error
    try:
        text = raw.decode("ascii")
    except UnicodeDecodeError as error:
        raise ReportError("report must be ASCII for this fixed observation") from error
    without_crlf = raw.replace(b"\r\n", b"")
    if b"\n" in without_crlf or b"\r" in without_crlf:
        raise ReportError("report contains a line ending other than CRLF")
    return raw, text


def footer_value(lines: list[str], label: str) -> str:
    prefix = f"{label}:"
    matches = [line[len(prefix) :].strip() for line in lines if line.startswith(prefix)]
    if len(matches) != 1:
        raise ReportError(f"expected exactly one {label!r} footer line")
    return matches[0]


def normalize(report_path: Path, manifest_path: Path, playlist: str) -> dict[str, Any]:
    raw, text = require_crlf_ascii(report_path)
    manifest_raw = manifest_path.read_bytes()
    manifest = load_json(manifest_path)

    playlists = manifest.get("playlists")
    cases = manifest.get("cases")
    if not isinstance(playlists, dict) or not isinstance(cases, list):
        raise ReportError("manifest must contain object playlists and array cases")
    expected_ids = playlists.get(playlist)
    if not isinstance(expected_ids, list) or not all(
        isinstance(case_id, str) for case_id in expected_ids
    ):
        raise ReportError(f"manifest playlist {playlist!r} is missing or invalid")

    cases_by_id: dict[str, dict[str, Any]] = {}
    for case in cases:
        if not isinstance(case, dict) or not isinstance(case.get("id"), str):
            raise ReportError("manifest contains a case without a string id")
        case_id = case["id"]
        if case_id in cases_by_id:
            raise ReportError(f"manifest repeats case id {case_id!r}")
        cases_by_id[case_id] = case

    lines = text.split("\r\n")
    if lines and lines[-1] == "":
        lines.pop()
    if len(lines) < 3:
        raise ReportError("report is too short")

    header_match = HEADER_RE.fullmatch(lines[0])
    if header_match is None:
        raise ReportError("unexpected report header")
    if not lines[1].startswith("log date: "):
        raise ReportError("missing report log date")
    reported_log_date = lines[1].removeprefix("log date: ")

    track_lines = [line for line in lines if re.match(r"^DR\d+\s", line)]
    if len(track_lines) != len(expected_ids):
        raise ReportError(
            f"report has {len(track_lines)} track rows; expected {len(expected_ids)}"
        )

    normalized_cases: list[dict[str, Any]] = []
    observed_stems: list[str] = []
    total_channel_values = 0
    for index, (case_id, line) in enumerate(zip(expected_ids, track_lines, strict=True), 1):
        case = cases_by_id.get(case_id)
        if case is None:
            raise ReportError(f"playlist refers to unknown case {case_id!r}")
        case_path = case.get("path")
        channels = case.get("channels")
        if not isinstance(case_path, str) or not isinstance(channels, int) or channels < 1:
            raise ReportError(f"case {case_id!r} has invalid path or channel count")
        expected_stem = Path(case_path).stem

        match = TRACK_RE.fullmatch(line)
        if match is None:
            raise ReportError(f"cannot parse track row {index}: {line!r}")
        rest = match.group("rest")
        stem_suffix = (
            rest[len(expected_stem) :]
            if rest.startswith(expected_stem)
            else ""
        )
        if not stem_suffix or not stem_suffix[0].isspace():
            observed = rest.split(maxsplit=1)[0] if rest else ""
            raise ReportError(
                f"track row {index} is {observed!r}; expected {expected_stem!r}"
            )

        tokens = list(CHANNEL_TOKEN_RE.finditer(stem_suffix))
        if len(tokens) != channels * 2:
            raise ReportError(
                f"track {expected_stem!r} has {len(tokens)} channel tokens; "
                f"expected {channels * 2}"
            )
        if any(token.group("full_scale") is not None for token in tokens[:channels]):
            raise ReportError(f"track {expected_stem!r} labels channel DR as dBFS")
        if any(token.group("full_scale") != "FS" for token in tokens[channels:]):
            raise ReportError(f"track {expected_stem!r} does not label channel RMS as dBFS")

        observed_stems.append(expected_stem)
        total_channel_values += channels
        normalized_cases.append(
            {
                "fixtureId": case_id,
                "manifestOrder": case["order"],
                "path": case_path,
                "stem": expected_stem,
                "channels": channels,
                "trackDr": int(match.group("track_dr")),
                "peakDbfsToken": match.group("peak"),
                "rmsDbfsToken": match.group("rms"),
                "durationToken": match.group("duration"),
                "channelDrDbTokens": [
                    token.group("value") for token in tokens[:channels]
                ],
                "channelRmsDbfsTokens": [
                    token.group("value") for token in tokens[channels:]
                ],
            }
        )

    track_count = footer_value(lines, "Number of tracks")
    if track_count != str(len(expected_ids)):
        raise ReportError(
            f"footer track count is {track_count!r}; expected {len(expected_ids)}"
        )
    if len(set(observed_stems)) != len(observed_stems):
        raise ReportError("report repeats one or more track stems")

    return {
        "schemaVersion": 1,
        "kind": "foo_dr_meter_report_normalization",
        "source": {
            "rawReportSha256": sha256_bytes(raw),
            "rawReportByteLength": len(raw),
            "encoding": "US-ASCII",
            "lineEndings": "CRLF",
            "manifestSha256": sha256_bytes(manifest_raw),
            "corpusId": manifest.get("corpusId"),
            "playlist": playlist,
        },
        "header": {
            "foobar2000Version": header_match.group("foobar"),
            "drMeterVersion": header_match.group("plugin"),
            "reportedLogDate": reported_log_date,
        },
        "cases": normalized_cases,
        "footer": {
            "numberOfTracksToken": track_count,
            "officialDrToken": footer_value(lines, "Official DR value"),
            "sampleRateToken": footer_value(lines, "Samplerate"),
            "channelsToken": footer_value(lines, "Channels"),
            "bitsPerSampleToken": footer_value(lines, "Bits per sample"),
            "bitrateToken": footer_value(lines, "Bitrate"),
            "codecToken": footer_value(lines, "Codec"),
        },
        "validation": {
            "expectedTrackCount": len(expected_ids),
            "observedTrackCount": len(normalized_cases),
            "expectedChannelValueCount": sum(
                int(cases_by_id[case_id]["channels"]) for case_id in expected_ids
            ),
            "observedChannelValueCount": total_channel_values,
            "manifestStemsExactlyOnce": True,
            "manifestOrderExact": True,
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--playlist", default="00-safe-master")
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        result = normalize(args.report, args.manifest, args.playlist)
    except (OSError, ReportError) as error:
        raise SystemExit(f"error: {error}") from error
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output is None:
        print(rendered, end="")
    else:
        args.output.write_text(rendered, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
