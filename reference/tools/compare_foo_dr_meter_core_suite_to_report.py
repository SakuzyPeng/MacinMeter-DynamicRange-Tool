#!/usr/bin/env python3
"""Compare isolated foo_dr_meter core bits with one normalized report.

This comparator has a deliberately narrow contract.  It accepts the fixed
complete-v2 39-item isolated-core suite and the fixed safe-master report
normalization, reconstructs only four exported renderer field classes, and
emits a path-free canonical summary.  It does not exercise or assess foobar's
decoder, component lifecycle, host services, album aggregation, or the rest of
the report renderer.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import struct
import sys
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any


SCHEMA_VERSION = 1
RECORD_KIND = "foo_dr_meter_108_isolated_core_report_comparison"
SUITE_KIND = "foo_dr_meter_108_core_suite_record"
REPORT_KIND = "foo_dr_meter_report_normalization"
EXPECTED_CORPUS_ID = "foo-dr-meter-108-complete-v2"
EXPECTED_MANIFEST_SHA256 = (
    "479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8"
)
EXPECTED_TARGET_SHA256 = (
    "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489"
)
EXPECTED_TARGET_BYTE_LENGTH = 424448
EXPECTED_PLAYLIST = "00-safe-master"
EXPECTED_CASE_IDS = (
    "window-minus-one-control",
    "exact-window-control",
    "tail-pair-base",
    "tail-pair-plus-one",
    "negative-dr-fallback",
    "histogram-db-domain",
    "loud-boundary-bin-ties",
    "peak-order-low-then-high",
    "peak-order-high-then-low",
    "one-frame-nonzero",
    "two-frame-negative",
    "silent-mono",
    "stereo-silent-channel",
    "three-channel-arithmetic",
    "six-channel-lfe",
    "histogram-lower-clamp",
    "loud-target-n9",
    "sparse-nonzero-among-zero",
    "rms-half-f64-stereo",
    "rms-half-f32-stereo",
    "peak-half-f64-stereo",
    "peak-half-f32-stereo",
    "sr-44100-w-minus-one",
    "sr-44100-w-plus-one",
    "sr-48000-w-minus-one",
    "sr-48000-w-plus-one",
    "eight-channel-report-map",
    "album-10-49-a",
    "album-10-49-b",
    "album-11-49",
    "display-10-50",
    "aggregate-narrow-low",
    "aggregate-narrow-high",
    "host-decode-u8",
    "host-decode-s16",
    "host-decode-s24",
    "host-decode-s32",
    "host-decode-f32",
    "host-decode-f64",
)
EXPECTED_FIXTURE_IDENTITIES = {
    "window-minus-one-control": (8000, 1, 24031, "wav-ieee-float32le", "f635e1475fcdef6cbb8c9b2547e5d0ef564348a1a05d6e7cfe0aa2417f52cd27", 96180),
    "exact-window-control": (8000, 1, 24032, "wav-ieee-float32le", "05c18258fe7a062e167ae5e4eadaaa60e302d7315c29396daea37fd1dbfc05c4", 96184),
    "tail-pair-base": (8000, 1, 48064, "wav-ieee-float32le", "c7ce1d1eebee758f95de50f1eba1ab3f0bfe3d5b9485350699ab7c900bdff472", 192312),
    "tail-pair-plus-one": (8000, 1, 48065, "wav-ieee-float32le", "4c0bdf19cd0d206886c40bafe657091b652bde2ca2c5d215a724efcf62dd5229", 192316),
    "negative-dr-fallback": (8000, 1, 48064, "wav-ieee-float32le", "ef0a10555cb589625fdf1c712fefad00afd0abd695dd99f4efde630e962561f0", 192312),
    "histogram-db-domain": (8000, 1, 120160, "wav-ieee-float32le", "c342e1e4fae4487ba79208975edf5047a97691e56a16537cce81d966842309a1", 480696),
    "loud-boundary-bin-ties": (8000, 1, 240320, "wav-ieee-float32le", "5b21bb6f7f497657c5805dd118ad805c31bad6e9effa7a14506a938368edc34a", 961336),
    "peak-order-low-then-high": (8000, 1, 120160, "wav-ieee-float32le", "680ae1a7d9a480a5665330933be9e9b776a25e8726635163505094fa804d08ad", 480696),
    "peak-order-high-then-low": (8000, 1, 120160, "wav-ieee-float32le", "f7884af7a53ffac8915b163fdeb49b01e069858ba2ccf0f9b32db717aebd0df3", 480696),
    "one-frame-nonzero": (8000, 1, 1, "wav-ieee-float32le", "9328c71b54bdcf658fb405a06c32b0ec4819b39ddc16a86a3b0616b07d48ba29", 60),
    "two-frame-negative": (8000, 1, 2, "wav-ieee-float32le", "950f035497a3068b4eb711cac1673b576662467abfd4407d21f7ce1148d29823", 64),
    "silent-mono": (8000, 1, 48064, "wav-ieee-float32le", "0d8b5999e67fc2b3491ef2b5bd319a670af5f7f30f03bcdb10b0764d6eecca3b", 192312),
    "stereo-silent-channel": (8000, 2, 240320, "wav-ieee-float32le", "daa1e30fe6d68cca15d9eae35877caf01a97b49ce6069de63d8e111cb8dc0331", 1922616),
    "three-channel-arithmetic": (8000, 3, 240320, "wav-ieee-float32le", "395d5b96f8f646b7f9400d123fb58175aee48fd7de08b004f28dcad88efa1aa5", 2883920),
    "six-channel-lfe": (8000, 6, 240320, "wav-ieee-float32le", "aab3f7b406e9c136f5cad0a0f0d11f4754a171e3acd2e5ec84640fe36636e4ef", 5767760),
    "histogram-lower-clamp": (8000, 1, 120160, "wav-ieee-float32le", "ab06ac487d20ca0732e4b681b53362d8aa030b14903dcbb4194975e8ef362f85", 480696),
    "loud-target-n9": (8000, 1, 216288, "wav-ieee-float32le", "e1a94d3f3a2fd7efb7f206b54043d27a8329fada67ad105a1c9530dde98b3e31", 865208),
    "sparse-nonzero-among-zero": (8000, 1, 240320, "wav-ieee-float32le", "22db06244ad26f29fbb0622ed5bc21d2d7d6276ba1a992a4c59970d698ecee43", 961336),
    "rms-half-f64-stereo": (8000, 2, 48064, "wav-ieee-float64le", "25aa2ca5173905597c766ca34b61a0d61b1d7d591684b3b3cbce7c07b9ba4cca", 769080),
    "rms-half-f32-stereo": (8000, 2, 48064, "wav-ieee-float32le", "ecf5b3e09f99e9482fe6289e58649eafc6ac43c5644bc57c93f114e7e4408d36", 384568),
    "peak-half-f64-stereo": (8000, 2, 120160, "wav-ieee-float64le", "6a44778a6ca24bf0e7d9267ed2e50f18ea3ffb4a98ad59ff415f935385db7a23", 1922616),
    "peak-half-f32-stereo": (8000, 2, 120160, "wav-ieee-float32le", "46df6c734e1de07b881765c7d251fa1e1c9e28f1f7b9cc7f0e1254ea8b280c91", 961336),
    "sr-44100-w-minus-one": (44100, 1, 132479, "wav-ieee-float32le", "b588c155933215340838cee7f182d025a6a31a8622d8ee15de1043cfbf95a7f4", 529972),
    "sr-44100-w-plus-one": (44100, 1, 132481, "wav-ieee-float32le", "576e49b6080003989f104ddd96726f7a56e635bbcbff25efec9b43c84380c401", 529980),
    "sr-48000-w-minus-one": (48000, 1, 144194, "wav-ieee-float32le", "14d8e58eba2079b5fda1ba72a066c791d5c3958c74bf72a847c894f0a1c67036", 576832),
    "sr-48000-w-plus-one": (48000, 1, 144196, "wav-ieee-float32le", "8fa08e407a1d29f2afacf516e87d2a4da7b547d8a2102b0cec323011948029fd", 576840),
    "eight-channel-report-map": (8000, 8, 48064, "wav-ieee-float32le", "5ed6c28587265097035ebc5c1c342996386a3dca4961433e01bc40511d0fc37d", 1538128),
    "album-10-49-a": (8000, 1, 48064, "wav-ieee-float32le", "587ff25b5092809357f771341e5088c4917c814a0b532fbbb867898eea351f6c", 192312),
    "album-10-49-b": (8000, 1, 48064, "wav-ieee-float32le", "d81ed671d0ea3fc8b6ef6dd1b098e55d3a6faed14618de836dda2390c715d4cd", 192312),
    "album-11-49": (8000, 1, 48064, "wav-ieee-float32le", "8efb335a0c3cfab161d84a612e505962e43bf7e1953f2d340268940996745a93", 192312),
    "display-10-50": (8000, 1, 48064, "wav-ieee-float32le", "cecd4f01acd0912f1b35cd54dfb20a6dffba2e620cfa323e8f6577d61a280cc7", 192312),
    "aggregate-narrow-low": (8000, 3, 48064, "wav-ieee-float32le", "622f674ef98d523bb17c05bdd9998a2723e82cb03966733c63e1c3582185acef", 576848),
    "aggregate-narrow-high": (8000, 3, 48064, "wav-ieee-float32le", "336d1768263305076911544b309c344d05dbb26275a56be5c6f4b13aef88dd01", 576848),
    "host-decode-u8": (8000, 1, 48064, "wav-pcm-u8", "cc5ec4d60254c2c16a4aaff80e577239c3bb4658e830ad8bff20e0fc9608351d", 48108),
    "host-decode-s16": (8000, 1, 48064, "wav-pcm-s16le", "940fa755f429e6a8484ac5e71837c92e2854975bdb51a860f4ee9cd7f25c0b04", 96172),
    "host-decode-s24": (8000, 1, 48064, "wav-pcm-s24le", "6d5d30dc197044fb349cf289ee6c7b43f43046dd6cdf273dc723efb2b7a21dad", 144236),
    "host-decode-s32": (8000, 1, 48064, "wav-pcm-s32le", "503ec351fe1a01c0991959208536974586c3da9c03cd11f65d456dbb418813eb", 192300),
    "host-decode-f32": (8000, 1, 48064, "wav-ieee-float32le", "6020659b806263a5d050d85bc5e7774b18336b641d5706f466a4706b0dd70e62", 192312),
    "host-decode-f64": (8000, 1, 48064, "wav-ieee-float64le", "6f2473481ea65ba751b8f6248719381ce4b1a18e4afa5cc673849d8153c27d9e", 384568),
}
EXPECTED_TRACK_COUNT = 39
EXPECTED_CHANNEL_COUNT = 62
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
F32_BITS_RE = re.compile(r"^[0-9a-f]{8}$")
F64_BITS_RE = re.compile(r"^[0-9a-f]{16}$")
DB_TOKEN_RE = re.compile(r"^(?:-inf|[-+]?\d+\.\d{2})$")
DURATION_TOKEN_RE = re.compile(r"^\d+(?::\d{2}){1,2}$")
WINDOWS_DRIVE_RE = re.compile(r"(?i)(?:^|[^A-Za-z0-9_])[A-Z]:[\\/]")
UNC_RE = re.compile(r"\\\\[^\\\s]+\\")
POSIX_ABSOLUTE_RE = re.compile(r"(?:^|[\s\"'=({])/(?!/)[^\s\"']+")


class ComparisonError(ValueError):
    """An input violates the fixed comparison contract."""


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _reject_constant(value: str) -> None:
    raise ComparisonError(f"JSON contains forbidden numeric constant {value}")


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ComparisonError(f"JSON repeats object key {key!r}")
        result[key] = value
    return result


def load_json_bytes(path: Path) -> tuple[bytes, dict[str, Any]]:
    try:
        raw = path.read_bytes()
        text = raw.decode("utf-8")
        value = json.loads(
            text,
            object_pairs_hook=_unique_object,
            parse_constant=_reject_constant,
        )
    except ComparisonError:
        raise
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ComparisonError("cannot read strict UTF-8 JSON input") from error
    if not isinstance(value, dict):
        raise ComparisonError("JSON input must contain one object")
    return raw, value


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
        raise ComparisonError("comparison is not finite canonical JSON") from error
    return (rendered + "\n").encode("utf-8")


def require_object(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ComparisonError(f"{context} must be an object")
    return value


def require_array(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise ComparisonError(f"{context} must be an array")
    return value


def require_exact_keys(
    value: dict[str, Any], expected: set[str], context: str
) -> None:
    if set(value) != expected:
        raise ComparisonError(f"{context} has unexpected object keys")


def require_string(value: Any, context: str) -> str:
    if not isinstance(value, str):
        raise ComparisonError(f"{context} must be a string")
    return value


def require_int(
    value: Any, context: str, *, minimum: int | None = None
) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise ComparisonError(f"{context} must be an integer")
    if minimum is not None and value < minimum:
        raise ComparisonError(f"{context} is below its minimum")
    return value


def require_sha256(value: Any, context: str) -> str:
    result = require_string(value, context)
    if SHA256_RE.fullmatch(result) is None:
        raise ComparisonError(f"{context} must be a lowercase SHA-256")
    return result


def require_bool(value: Any, context: str) -> bool:
    if not isinstance(value, bool):
        raise ComparisonError(f"{context} must be a boolean")
    return value


def _looks_absolute_or_private_path(value: str) -> bool:
    return bool(
        "file://" in value.casefold()
        or WINDOWS_DRIVE_RE.search(value)
        or UNC_RE.search(value)
        or POSIX_ABSOLUTE_RE.search(value)
        or value.startswith(("/", "~"))
    )


def assert_path_free(value: Any, context: str) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            assert_path_free(child, f"{context}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            assert_path_free(child, f"{context}[{index}]")
    elif isinstance(value, str) and _looks_absolute_or_private_path(value):
        raise ComparisonError(f"{context} contains an absolute/private path")


def f32_from_bits(value: Any, context: str) -> float:
    token = require_string(value, context)
    if F32_BITS_RE.fullmatch(token) is None:
        raise ComparisonError(f"{context} must be eight lowercase hex digits")
    number = struct.unpack("<f", struct.pack("<I", int(token, 16)))[0]
    if not math.isfinite(number):
        raise ComparisonError(f"{context} encodes a non-finite binary32")
    return number


def f64_from_bits(value: Any, context: str) -> float:
    token = require_string(value, context)
    if F64_BITS_RE.fullmatch(token) is None:
        raise ComparisonError(f"{context} must be sixteen lowercase hex digits")
    number = struct.unpack("<d", struct.pack("<Q", int(token, 16)))[0]
    if not math.isfinite(number):
        raise ComparisonError(f"{context} encodes a non-finite binary64")
    return number


def narrow_f32(value: float, context: str) -> float:
    if not math.isfinite(value):
        raise ComparisonError(f"{context} must be finite before binary32 narrowing")
    try:
        narrowed = struct.unpack("<f", struct.pack("<f", value))[0]
    except (OverflowError, struct.error) as error:
        raise ComparisonError(f"{context} cannot be represented as finite binary32") from error
    if not math.isfinite(narrowed):
        raise ComparisonError(f"{context} cannot be represented as finite binary32")
    return narrowed


def lround(value: float) -> int:
    magnitude = abs(value)
    whole = math.floor(magnitude)
    if magnitude - whole >= 0.5:
        whole += 1
    return whole if value >= 0.0 else -whole


def two_decimal_token(value: float, context: str) -> str:
    if not math.isfinite(value):
        raise ComparisonError(f"{context} must be finite")
    # The fixed report renderer applies this explicit centi-dB correction
    # before its C-locale two-decimal formatting.
    if -0.01 < value < 0.01:
        value = lround(100.0 * value) / 100.0
    return f"{value:.2f}"


def linear_f32_db_token(value: float, context: str) -> str:
    if value < 0.0:
        raise ComparisonError(f"{context} must not be negative")
    if value == 0.0:
        return "-inf"
    db_f64 = 20.0 * math.log10(value)
    return two_decimal_token(narrow_f32(db_f64, context), context)


def require_db_token(value: Any, context: str) -> str:
    token = require_string(value, context)
    if DB_TOKEN_RE.fullmatch(token) is None:
        raise ComparisonError(f"{context} is not a canonical dB token")
    return token


def _validate_runtime_artifacts(value: Any, context: str) -> list[dict[str, Any]]:
    artifacts = require_array(value, context)
    expected_names = (
        "shared.dll",
        "msvcp140.dll",
        "vcruntime140.dll",
        "vcruntime140_1.dll",
    )
    if len(artifacts) != len(expected_names):
        raise ComparisonError(f"{context} has the wrong artifact count")
    validated: list[dict[str, Any]] = []
    for index, (raw, expected_name) in enumerate(
        zip(artifacts, expected_names, strict=True)
    ):
        item_context = f"{context}[{index}]"
        item = require_object(raw, item_context)
        require_exact_keys(item, {"name", "sha256", "byteLength"}, item_context)
        if item.get("name") != expected_name:
            raise ComparisonError(f"{item_context}.name is not canonical")
        validated.append(
            {
                "name": expected_name,
                "sha256": require_sha256(item.get("sha256"), f"{item_context}.sha256"),
                "byteLength": require_int(
                    item.get("byteLength"),
                    f"{item_context}.byteLength",
                    minimum=1,
                ),
            }
        )
    return validated


def _validate_session(
    value: Any, context: str, *, frames: int, finalized: bool
) -> None:
    state = require_object(value, context)
    require_exact_keys(
        state,
        {"currentWindowFrames", "windowCount", "submittedFrames"},
        context,
    )
    current = require_int(
        state.get("currentWindowFrames"), f"{context}.currentWindowFrames", minimum=0
    )
    require_int(state.get("windowCount"), f"{context}.windowCount", minimum=0)
    submitted = require_int(
        state.get("submittedFrames"), f"{context}.submittedFrames", minimum=0
    )
    if finalized:
        if current != 0 or submitted != frames:
            raise ComparisonError(f"{context} does not describe finalized geometry")
    elif current + submitted != frames:
        raise ComparisonError(f"{context} does not preserve submitted frame geometry")


def _validate_fp_environment(value: Any, context: str) -> None:
    environment = require_object(value, context)
    require_exact_keys(environment, {"before", "applied", "after", "restored"}, context)
    for name in ("before", "after", "restored"):
        child_context = f"{context}.{name}"
        child = require_object(environment.get(name), child_context)
        require_exact_keys(child, {"x87ControlWordBits", "mxcsrBits"}, child_context)
        if re.fullmatch(
            r"[0-9a-f]{4}", require_string(child.get("x87ControlWordBits"), child_context)
        ) is None:
            raise ComparisonError(f"{child_context} has invalid x87 bits")
        if re.fullmatch(
            r"[0-9a-f]{8}", require_string(child.get("mxcsrBits"), child_context)
        ) is None:
            raise ComparisonError(f"{child_context} has invalid MXCSR bits")
    applied_context = f"{context}.applied"
    applied = require_object(environment.get("applied"), applied_context)
    require_exact_keys(
        applied,
        {
            "x87ControlWordBits",
            "mxcsrBits",
            "rounding",
            "ftz",
            "daz",
            "exceptionsMasked",
        },
        applied_context,
    )
    if re.fullmatch(
        r"[0-9a-f]{4}",
        require_string(
            applied.get("x87ControlWordBits"),
            f"{applied_context}.x87ControlWordBits",
        ),
    ) is None:
        raise ComparisonError(f"{applied_context} has invalid x87 bits")
    if re.fullmatch(
        r"[0-9a-f]{8}",
        require_string(applied.get("mxcsrBits"), f"{applied_context}.mxcsrBits"),
    ) is None:
        raise ComparisonError(f"{applied_context} has invalid MXCSR bits")
    if applied.get("rounding") != "nearest":
        raise ComparisonError(f"{applied_context}.rounding is not nearest")
    for name in ("ftz", "daz", "exceptionsMasked"):
        require_bool(applied.get(name), f"{applied_context}.{name}")
    if applied.get("ftz") or applied.get("daz") or not applied.get("exceptionsMasked"):
        raise ComparisonError(f"{applied_context} has the wrong floating-point mode")


def validate_suite(suite: dict[str, Any]) -> list[dict[str, Any]]:
    require_exact_keys(
        suite,
        {
            "schemaVersion",
            "kind",
            "suiteId",
            "corpus",
            "execution",
            "target",
            "items",
            "summary",
            "claims",
            "limitations",
        },
        "coreSuite",
    )
    if suite.get("schemaVersion") != 1 or suite.get("kind") != SUITE_KIND:
        raise ComparisonError("coreSuite has the wrong schema or kind")
    require_sha256(suite.get("suiteId"), "coreSuite.suiteId")
    assert_path_free(suite, "coreSuite")

    corpus = require_object(suite.get("corpus"), "coreSuite.corpus")
    require_exact_keys(
        corpus,
        {"id", "manifestSha256", "safeCaseCount", "safeCaseIds"},
        "coreSuite.corpus",
    )
    if (
        corpus.get("id") != EXPECTED_CORPUS_ID
        or corpus.get("manifestSha256") != EXPECTED_MANIFEST_SHA256
        or corpus.get("safeCaseCount") != EXPECTED_TRACK_COUNT
        or corpus.get("safeCaseIds") != list(EXPECTED_CASE_IDS)
    ):
        raise ComparisonError("coreSuite.corpus is not the fixed safe-master corpus")

    execution = require_object(suite.get("execution"), "coreSuite.execution")
    require_exact_keys(
        execution,
        {
            "workerSha256",
            "workerByteLength",
            "timeoutSeconds",
            "blockFrames",
            "processModel",
        },
        "coreSuite.execution",
    )
    require_sha256(execution.get("workerSha256"), "coreSuite.execution.workerSha256")
    require_int(
        execution.get("workerByteLength"),
        "coreSuite.execution.workerByteLength",
        minimum=1,
    )
    timeout = execution.get("timeoutSeconds")
    if (
        not isinstance(timeout, (int, float))
        or isinstance(timeout, bool)
        or not math.isfinite(float(timeout))
        or float(timeout) <= 0.0
    ):
        raise ComparisonError("coreSuite.execution.timeoutSeconds is invalid")
    require_int(
        execution.get("blockFrames"), "coreSuite.execution.blockFrames", minimum=1
    )
    if execution.get("processModel") != "one_worker_process_per_input":
        raise ComparisonError("coreSuite.execution.processModel is not isolated")

    target = require_object(suite.get("target"), "coreSuite.target")
    require_exact_keys(
        target,
        {"sha256", "byteLength", "runtimeProfile", "runtimeArtifacts"},
        "coreSuite.target",
    )
    if (
        target.get("sha256") != EXPECTED_TARGET_SHA256
        or target.get("byteLength") != EXPECTED_TARGET_BYTE_LENGTH
        or target.get("runtimeProfile") != "fixed_foobar_2_25_10"
    ):
        raise ComparisonError("coreSuite.target is not the fixed x64 target")
    target_artifacts = _validate_runtime_artifacts(
        target.get("runtimeArtifacts"), "coreSuite.target.runtimeArtifacts"
    )

    summary = require_object(suite.get("summary"), "coreSuite.summary")
    require_exact_keys(
        summary, {"status", "total", "succeeded", "failed"}, "coreSuite.summary"
    )
    if summary != {
        "status": "success",
        "total": EXPECTED_TRACK_COUNT,
        "succeeded": EXPECTED_TRACK_COUNT,
        "failed": 0,
    }:
        raise ComparisonError("coreSuite is not a complete successful run")

    claims = require_object(suite.get("claims"), "coreSuite.claims")
    if (
        claims.get("compatibility") != "none"
        or claims.get("foobarParity") != "not_assessed"
    ):
        raise ComparisonError("coreSuite claims exceed the isolated-core scope")
    limitations = require_array(suite.get("limitations"), "coreSuite.limitations")
    if not limitations or not all(isinstance(item, str) for item in limitations):
        raise ComparisonError("coreSuite.limitations must be non-empty strings")

    items = require_array(suite.get("items"), "coreSuite.items")
    if len(items) != EXPECTED_TRACK_COUNT:
        raise ComparisonError("coreSuite.items does not have exactly 39 entries")
    validated: list[dict[str, Any]] = []
    for index, (raw_item, expected_id) in enumerate(
        zip(items, EXPECTED_CASE_IDS, strict=True), 1
    ):
        context = f"coreSuite.items[{index - 1}]"
        item = require_object(raw_item, context)
        require_exact_keys(
            item,
            {"manifestOrder", "inputId", "requestId", "input", "result", "claims"},
            context,
        )
        if item.get("manifestOrder") != index or item.get("inputId") != expected_id:
            raise ComparisonError(f"{context} is out of fixed manifest order")
        require_sha256(item.get("requestId"), f"{context}.requestId")

        input_value = require_object(item.get("input"), f"{context}.input")
        require_exact_keys(
            input_value,
            {
                "inputId",
                "sourceKind",
                "sourceEncoding",
                "conversion",
                "sourceSha256",
                "sourceByteLength",
                "manifestSha256",
                "pcmSha256",
                "pcmByteLength",
                "sampleRateHz",
                "channels",
                "frames",
            },
            f"{context}.input",
        )
        if (
            input_value.get("inputId") != expected_id
            or input_value.get("sourceKind") != "manifest_wav_fixture"
            or input_value.get("conversion") != "strict_wav_sample_to_binary64"
            or input_value.get("manifestSha256") != EXPECTED_MANIFEST_SHA256
        ):
            raise ComparisonError(f"{context}.input has the wrong fixture identity")
        source_encoding = require_string(
            input_value.get("sourceEncoding"), f"{context}.input.sourceEncoding"
        )
        source_sha256 = require_sha256(
            input_value.get("sourceSha256"), f"{context}.input.sourceSha256"
        )
        source_byte_length = require_int(
            input_value.get("sourceByteLength"),
            f"{context}.input.sourceByteLength",
            minimum=1,
        )
        require_sha256(input_value.get("pcmSha256"), f"{context}.input.pcmSha256")
        sample_rate = require_int(
            input_value.get("sampleRateHz"),
            f"{context}.input.sampleRateHz",
            minimum=1,
        )
        channels = require_int(
            input_value.get("channels"), f"{context}.input.channels", minimum=1
        )
        frames = require_int(
            input_value.get("frames"), f"{context}.input.frames", minimum=1
        )
        expected_fixture = EXPECTED_FIXTURE_IDENTITIES[expected_id]
        observed_fixture = (
            sample_rate,
            channels,
            frames,
            source_encoding,
            source_sha256,
            source_byte_length,
        )
        if observed_fixture != expected_fixture:
            raise ComparisonError(
                f"{context}.input differs from the fixed manifest fixture identity"
            )
        pcm_bytes = require_int(
            input_value.get("pcmByteLength"),
            f"{context}.input.pcmByteLength",
            minimum=1,
        )
        if pcm_bytes != frames * channels * 8:
            raise ComparisonError(f"{context}.input PCM geometry is inconsistent")

        result = require_object(item.get("result"), f"{context}.result")
        require_exact_keys(result, {"kind", "data"}, f"{context}.result")
        if result.get("kind") != "success":
            raise ComparisonError(f"{context}.result is not successful")
        data = require_object(result.get("data"), f"{context}.result.data")
        require_exact_keys(
            data,
            {
                "channelResults",
                "channelStateAfterFinish",
                "channels",
                "fpEnvironment",
                "frames",
                "loaderMode",
                "runtimeArtifacts",
                "sampleRateHz",
                "sessionAfterFinish",
                "sessionBeforeFinish",
                "sharedServiceBoundary",
                "trackDrBits",
            },
            f"{context}.result.data",
        )
        if (
            data.get("sampleRateHz") != sample_rate
            or data.get("channels") != channels
            or data.get("frames") != frames
        ):
            raise ComparisonError(f"{context}.result geometry differs from its input")
        if data.get("loaderMode") != "private_staging_dll_load_dir_system32":
            raise ComparisonError(f"{context}.result loader mode is not fixed")
        if (
            _validate_runtime_artifacts(
                data.get("runtimeArtifacts"), f"{context}.result.data.runtimeArtifacts"
            )
            != target_artifacts
        ):
            raise ComparisonError(f"{context}.result runtime identity changed")
        _validate_session(
            data.get("sessionBeforeFinish"),
            f"{context}.result.data.sessionBeforeFinish",
            frames=frames,
            finalized=False,
        )
        _validate_session(
            data.get("sessionAfterFinish"),
            f"{context}.result.data.sessionAfterFinish",
            frames=frames,
            finalized=True,
        )
        _validate_fp_environment(
            data.get("fpEnvironment"), f"{context}.result.data.fpEnvironment"
        )

        boundary = require_object(
            data.get("sharedServiceBoundary"),
            f"{context}.result.data.sharedServiceBoundary",
        )
        require_exact_keys(
            boundary,
            {"armedImportCount", "coreExecution", "loadLifecycle"},
            f"{context}.result.data.sharedServiceBoundary",
        )
        if boundary != {
            "armedImportCount": 13,
            "coreExecution": "fail_fast_iat_tripwire",
            "loadLifecycle": "real_shared",
        }:
            raise ComparisonError(f"{context}.result shared-service boundary changed")

        track_dr = f32_from_bits(
            data.get("trackDrBits"), f"{context}.result.data.trackDrBits"
        )
        if track_dr < 0.0:
            raise ComparisonError(f"{context}.result track DR is negative")

        raw_channels = require_array(
            data.get("channelResults"), f"{context}.result.data.channelResults"
        )
        raw_states = require_array(
            data.get("channelStateAfterFinish"),
            f"{context}.result.data.channelStateAfterFinish",
        )
        if len(raw_channels) != channels or len(raw_states) != channels:
            raise ComparisonError(f"{context}.result channel geometry is inconsistent")
        channel_values: list[dict[str, float]] = []
        for channel_index, (raw_channel, raw_state) in enumerate(
            zip(raw_channels, raw_states, strict=True)
        ):
            channel_context = (
                f"{context}.result.data.channelResults[{channel_index}]"
            )
            channel = require_object(raw_channel, channel_context)
            require_exact_keys(
                channel, {"index", "drBits", "peakBits", "rmsBits"}, channel_context
            )
            if channel.get("index") != channel_index:
                raise ComparisonError(f"{channel_context}.index is not contiguous")
            dr = f32_from_bits(channel.get("drBits"), f"{channel_context}.drBits")
            peak = f32_from_bits(channel.get("peakBits"), f"{channel_context}.peakBits")
            rms = f32_from_bits(channel.get("rmsBits"), f"{channel_context}.rmsBits")
            if dr < 0.0 or peak < 0.0 or rms < 0.0:
                raise ComparisonError(f"{channel_context} contains a negative metric")
            channel_values.append({"dr": dr, "peak": peak, "rms": rms})

            state_context = (
                f"{context}.result.data.channelStateAfterFinish[{channel_index}]"
            )
            state = require_object(raw_state, state_context)
            require_exact_keys(
                state,
                {
                    "index",
                    "rmsSquareSumBits",
                    "primaryPeakBits",
                    "secondaryPeakBits",
                    "primaryPeakKeyBits",
                    "secondaryPeakKeyBits",
                },
                state_context,
            )
            if state.get("index") != channel_index:
                raise ComparisonError(f"{state_context}.index is not contiguous")
            for key in (
                "rmsSquareSumBits",
                "primaryPeakBits",
                "secondaryPeakBits",
                "primaryPeakKeyBits",
                "secondaryPeakKeyBits",
            ):
                f64_from_bits(state.get(key), f"{state_context}.{key}")

        validated.append(
            {
                "fixtureId": expected_id,
                "manifestOrder": index,
                "sampleRateHz": sample_rate,
                "channels": channels,
                "frames": frames,
                "trackDr": track_dr,
                "channelValues": channel_values,
            }
        )
    if sum(item["channels"] for item in validated) != EXPECTED_CHANNEL_COUNT:
        raise ComparisonError("coreSuite does not have exactly 62 channel results")
    return validated


def validate_report(report: dict[str, Any]) -> list[dict[str, Any]]:
    require_exact_keys(
        report,
        {"schemaVersion", "kind", "source", "header", "cases", "footer", "validation"},
        "report",
    )
    if report.get("schemaVersion") != 1 or report.get("kind") != REPORT_KIND:
        raise ComparisonError("report has the wrong schema or kind")
    assert_path_free(report, "report")

    source = require_object(report.get("source"), "report.source")
    require_exact_keys(
        source,
        {
            "rawReportSha256",
            "rawReportByteLength",
            "encoding",
            "lineEndings",
            "manifestSha256",
            "corpusId",
            "playlist",
        },
        "report.source",
    )
    require_sha256(source.get("rawReportSha256"), "report.source.rawReportSha256")
    require_int(
        source.get("rawReportByteLength"),
        "report.source.rawReportByteLength",
        minimum=1,
    )
    if (
        source.get("encoding") != "US-ASCII"
        or source.get("lineEndings") != "CRLF"
        or source.get("manifestSha256") != EXPECTED_MANIFEST_SHA256
        or source.get("corpusId") != EXPECTED_CORPUS_ID
        or source.get("playlist") != EXPECTED_PLAYLIST
    ):
        raise ComparisonError("report.source is not the fixed safe-master source")

    header = require_object(report.get("header"), "report.header")
    require_exact_keys(
        header,
        {"foobar2000Version", "drMeterVersion", "reportedLogDate"},
        "report.header",
    )
    if (
        header.get("foobar2000Version") != "2.25.10"
        or header.get("drMeterVersion") != "1.0.8"
        or not require_string(header.get("reportedLogDate"), "report.header.reportedLogDate")
    ):
        raise ComparisonError("report.header is not the fixed x64 report header")

    validation = require_object(report.get("validation"), "report.validation")
    require_exact_keys(
        validation,
        {
            "expectedTrackCount",
            "observedTrackCount",
            "expectedChannelValueCount",
            "observedChannelValueCount",
            "manifestStemsExactlyOnce",
            "manifestOrderExact",
        },
        "report.validation",
    )
    if validation != {
        "expectedTrackCount": EXPECTED_TRACK_COUNT,
        "observedTrackCount": EXPECTED_TRACK_COUNT,
        "expectedChannelValueCount": EXPECTED_CHANNEL_COUNT,
        "observedChannelValueCount": EXPECTED_CHANNEL_COUNT,
        "manifestStemsExactlyOnce": True,
        "manifestOrderExact": True,
    }:
        raise ComparisonError("report.validation is not exact")

    footer = require_object(report.get("footer"), "report.footer")
    require_exact_keys(
        footer,
        {
            "numberOfTracksToken",
            "officialDrToken",
            "sampleRateToken",
            "channelsToken",
            "bitsPerSampleToken",
            "bitrateToken",
            "codecToken",
        },
        "report.footer",
    )
    if footer.get("numberOfTracksToken") != str(EXPECTED_TRACK_COUNT):
        raise ComparisonError("report.footer track count is not exact")
    for key in footer:
        require_string(footer.get(key), f"report.footer.{key}")

    cases = require_array(report.get("cases"), "report.cases")
    if len(cases) != EXPECTED_TRACK_COUNT:
        raise ComparisonError("report.cases does not have exactly 39 entries")
    seen_paths: set[str] = set()
    validated: list[dict[str, Any]] = []
    for index, (raw_case, expected_id) in enumerate(
        zip(cases, EXPECTED_CASE_IDS, strict=True), 1
    ):
        context = f"report.cases[{index - 1}]"
        case = require_object(raw_case, context)
        require_exact_keys(
            case,
            {
                "fixtureId",
                "manifestOrder",
                "path",
                "stem",
                "channels",
                "trackDr",
                "peakDbfsToken",
                "rmsDbfsToken",
                "durationToken",
                "channelDrDbTokens",
                "channelRmsDbfsTokens",
            },
            context,
        )
        if case.get("fixtureId") != expected_id or case.get("manifestOrder") != index:
            raise ComparisonError(f"{context} is out of fixed manifest order")
        path_text = require_string(case.get("path"), f"{context}.path")
        relative = PurePosixPath(path_text)
        if (
            relative.is_absolute()
            or ".." in relative.parts
            or "." in relative.parts
            or "\\" in path_text
            or path_text in seen_paths
        ):
            raise ComparisonError(f"{context}.path is not a unique safe relative path")
        seen_paths.add(path_text)
        stem = require_string(case.get("stem"), f"{context}.stem")
        if relative.stem != stem:
            raise ComparisonError(f"{context}.stem differs from the relative path")
        channels = require_int(case.get("channels"), f"{context}.channels", minimum=1)
        track_dr = require_int(case.get("trackDr"), f"{context}.trackDr", minimum=0)
        peak_token = require_db_token(case.get("peakDbfsToken"), f"{context}.peakDbfsToken")
        require_db_token(case.get("rmsDbfsToken"), f"{context}.rmsDbfsToken")
        duration = require_string(case.get("durationToken"), f"{context}.durationToken")
        if DURATION_TOKEN_RE.fullmatch(duration) is None:
            raise ComparisonError(f"{context}.durationToken is not canonical")
        raw_dr = require_array(case.get("channelDrDbTokens"), f"{context}.channelDrDbTokens")
        raw_rms = require_array(
            case.get("channelRmsDbfsTokens"), f"{context}.channelRmsDbfsTokens"
        )
        if len(raw_dr) != channels or len(raw_rms) != channels:
            raise ComparisonError(f"{context} has inconsistent channel token geometry")
        validated.append(
            {
                "fixtureId": expected_id,
                "manifestOrder": index,
                "channels": channels,
                "trackDr": track_dr,
                "peakDbfsToken": peak_token,
                "channelDrDbTokens": [
                    require_db_token(token, f"{context}.channelDrDbTokens[{token_index}]")
                    for token_index, token in enumerate(raw_dr)
                ],
                "channelRmsDbfsTokens": [
                    require_db_token(
                        token, f"{context}.channelRmsDbfsTokens[{token_index}]"
                    )
                    for token_index, token in enumerate(raw_rms)
                ],
            }
        )
    if sum(item["channels"] for item in validated) != EXPECTED_CHANNEL_COUNT:
        raise ComparisonError("report does not have exactly 62 channel tokens")
    return validated


def compare(core_suite_path: Path, normalized_report_path: Path) -> dict[str, Any]:
    suite_raw, suite = load_json_bytes(core_suite_path)
    report_raw, report = load_json_bytes(normalized_report_path)
    core_items = validate_suite(suite)
    report_cases = validate_report(report)

    track_matched = 0
    channel_dr_matched = 0
    channel_rms_matched = 0
    peak_matched = 0
    differences: list[dict[str, Any]] = []
    for core, reference in zip(core_items, report_cases, strict=True):
        fixture_id = core["fixtureId"]
        if (
            core["manifestOrder"] != reference["manifestOrder"]
            or fixture_id != reference["fixtureId"]
            or core["channels"] != reference["channels"]
        ):
            raise ComparisonError("fixture identity or channel geometry differs")

        rendered_track = math.trunc(core["trackDr"] + 0.5)
        if rendered_track == reference["trackDr"]:
            track_matched += 1
        else:
            differences.append(
                {
                    "fixtureId": fixture_id,
                    "field": "trackDr",
                    "reference": reference["trackDr"],
                    "isolatedCoreRendered": rendered_track,
                }
            )

        rendered_dr = [
            two_decimal_token(channel["dr"], f"{fixture_id}.channelDr")
            for channel in core["channelValues"]
        ]
        rendered_rms = [
            linear_f32_db_token(channel["rms"], f"{fixture_id}.channelRms")
            for channel in core["channelValues"]
        ]
        for channel_index, (actual, expected) in enumerate(
            zip(rendered_dr, reference["channelDrDbTokens"], strict=True)
        ):
            if actual == expected:
                channel_dr_matched += 1
            else:
                differences.append(
                    {
                        "fixtureId": fixture_id,
                        "channelIndex": channel_index,
                        "field": "channelDrDbToken",
                        "reference": expected,
                        "isolatedCoreRendered": actual,
                    }
                )
        for channel_index, (actual, expected) in enumerate(
            zip(rendered_rms, reference["channelRmsDbfsTokens"], strict=True)
        ):
            if actual == expected:
                channel_rms_matched += 1
            else:
                differences.append(
                    {
                        "fixtureId": fixture_id,
                        "channelIndex": channel_index,
                        "field": "channelRmsDbfsToken",
                        "reference": expected,
                        "isolatedCoreRendered": actual,
                    }
                )

        overall_peak_linear = max(
            channel["peak"] for channel in core["channelValues"]
        )
        rendered_peak = linear_f32_db_token(
            overall_peak_linear, f"{fixture_id}.overallPeak"
        )
        if rendered_peak == reference["peakDbfsToken"]:
            peak_matched += 1
        else:
            differences.append(
                {
                    "fixtureId": fixture_id,
                    "field": "peakDbfsToken",
                    "reference": reference["peakDbfsToken"],
                    "isolatedCoreRendered": rendered_peak,
                }
            )

    summary = {
        "status": "match" if not differences else "different",
        "trackDrMatched": track_matched,
        "trackDrTotal": EXPECTED_TRACK_COUNT,
        "channelDrMatched": channel_dr_matched,
        "channelDrTotal": EXPECTED_CHANNEL_COUNT,
        "channelRmsMatched": channel_rms_matched,
        "channelRmsTotal": EXPECTED_CHANNEL_COUNT,
        "overallPeakMatched": peak_matched,
        "overallPeakTotal": EXPECTED_TRACK_COUNT,
        "differenceCount": len(differences),
        "fixtureSetExact": True,
        "manifestOrderExact": True,
        "successfulCoreItems": EXPECTED_TRACK_COUNT,
    }
    result = {
        "schemaVersion": SCHEMA_VERSION,
        "kind": RECORD_KIND,
        "inputs": {
            "isolatedCoreSuiteSha256": sha256_bytes(suite_raw),
            "normalizedReportSha256": sha256_bytes(report_raw),
            "corpusId": EXPECTED_CORPUS_ID,
            "manifestSha256": EXPECTED_MANIFEST_SHA256,
            "safeCaseCount": EXPECTED_TRACK_COUNT,
            "channelValueCount": EXPECTED_CHANNEL_COUNT,
            "targetSha256": EXPECTED_TARGET_SHA256,
        },
        "policy": {
            "trackDr": (
                "decode finite non-negative binary32 trackDrBits; "
                "truncate value plus 0.5"
            ),
            "channelDrDb": (
                "decode finite non-negative binary32 drBits; fixed "
                "C-locale two-decimal renderer"
            ),
            "channelRmsDbfs": (
                "decode non-negative linear binary32 rmsBits; zero renders "
                "-inf, otherwise binary64 20*log10, narrow to binary32, "
                "then fixed C-locale two-decimal renderer"
            ),
            "overallPeakDbfs": (
                "maximum non-negative binary32 peakBits across channels; "
                "zero renders -inf, otherwise binary64 20*log10, narrow to "
                "binary32, then fixed C-locale two-decimal renderer"
            ),
            "numericTolerance": 0,
            "fixtureIdentity": (
                "fixed corpus, manifest digest, exact 39 fixture IDs, exact "
                "manifest order, and per-item channel geometry"
            ),
        },
        "summary": summary,
        "differences": differences,
        "claims": {
            "status": (
                "compared_fields_match"
                if not differences
                else "compared_fields_differ"
            ),
            "scope": "four exported field classes on the fixed safe-master corpus",
            "foobarParity": "not_assessed",
            "compatibility": "none",
        },
        "notCompared": [
            {
                "fieldClass": "decoder and source-to-PCM behavior",
                "reason": "the isolated suite uses deterministic harness conversion",
            },
            {
                "fieldClass": "foobar host services and component lifecycle",
                "reason": "the isolated worker calls only analyzer-core entry RVAs",
            },
            {
                "fieldClass": "album aggregation and grouping",
                "reason": "the core suite runs one input per worker and compares no footer",
            },
            {
                "fieldClass": "remaining report renderer fields",
                "reason": (
                    "overall RMS, duration, metadata, footer, labels, layout, "
                    "encoding, and byte-for-byte text are outside this comparison"
                ),
            },
        ],
    }
    assert_path_free(result, "comparison")
    canonical_json_bytes(result)
    return result


def write_record(value: dict[str, Any], output: Path | None) -> None:
    raw = canonical_json_bytes(value)
    if output is None:
        sys.stdout.buffer.write(raw)
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=output.parent, prefix=f".{output.name}.", delete=False
    ) as temporary:
        temporary.write(raw)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)
    try:
        os.replace(temporary_path, output)
    finally:
        try:
            temporary_path.unlink()
        except FileNotFoundError:
            pass


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--core-suite", required=True, type=Path)
    parser.add_argument("--normalized-report", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        result = compare(args.core_suite, args.normalized_report)
        write_record(result, args.output)
    except (ComparisonError, OSError):
        print("core/report comparison error: contract_violation", file=sys.stderr)
        return 1
    return 0 if result["summary"]["differenceCount"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
