#!/usr/bin/env python3
"""Run the malformed-media-v1 corpus through the CLI in isolated subprocesses.

This is the isolated verifier described by ADR-0003 and ADR-0006: every corpus
case runs in its own subprocess with a wall-clock timeout and an address-space
limit. The default invocation refuses to decode the hostile corpus when the
limit cannot be enforced. A caller may explicitly opt into timeout-only
execution, but that is not a normal repository gate.

Expectations:
- the process must exit nonzero within the timeout;
- stdout must be a single JSON error document whose code and stage match the
  committed corpus manifest;
- no case may time out.

This verifies exactly the committed corpus files. It is not evidence that all
byte inputs terminate or allocate within bounds.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

try:
    import resource
except ImportError:  # pragma: no cover - non-POSIX platforms
    resource = None


def supports_memory_limit() -> bool:
    return (
        sys.platform.startswith("linux")
        and resource is not None
        and hasattr(resource, "RLIMIT_AS")
    )


def default_cli_path(repo_root: Path, platform: str = sys.platform) -> Path:
    executable = "mdrmeter.exe" if platform == "win32" else "mdrmeter"
    return repo_root / "target" / "debug" / executable


def preexec_with_memory_limit(limit_bytes: int):
    def apply() -> None:
        resource.setrlimit(resource.RLIMIT_AS, (limit_bytes, limit_bytes))

    return apply


def run_case(
    cli: Path, case: dict, corpus: Path, timeout: float, memory_limit_bytes: int | None
) -> list[str]:
    problems: list[str] = []
    file = corpus / case["path"]
    payload = file.read_bytes()
    if hashlib.sha256(payload).hexdigest() != case["sha256"]:
        return [f"{case['id']}: corpus bytes drifted from the manifest"]

    keywords: dict = {}
    if memory_limit_bytes is not None:
        keywords["preexec_fn"] = preexec_with_memory_limit(memory_limit_bytes)
    try:
        completed = subprocess.run(
            [str(cli), "analyze", str(file), "--format", "json"],
            capture_output=True,
            text=True,
            timeout=timeout,
            **keywords,
        )
    except subprocess.TimeoutExpired:
        return [f"{case['id']}: timed out after {timeout}s"]

    if completed.returncode == 0:
        problems.append(f"{case['id']}: exited 0 as if the media were valid")
    try:
        document = json.loads(completed.stdout)
    except json.JSONDecodeError:
        problems.append(f"{case['id']}: stdout is not a JSON document")
        return problems
    if document.get("kind") != "error":
        problems.append(f"{case['id']}: stdout is not an error document")
        return problems
    observed = (document["data"].get("code"), document["data"].get("stage"))
    expected = (case["expected"]["code"], case["expected"]["stage"])
    if observed != expected:
        problems.append(f"{case['id']}: expected {expected}, observed {observed}")
    return problems


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--cli",
        type=Path,
        default=default_cli_path(repo_root),
        help="path to a built mdrmeter CLI binary",
    )
    parser.add_argument(
        "--corpus",
        type=Path,
        default=repo_root / "tests" / "fixtures" / "malformed-media-v1",
        help="directory containing the committed corpus",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=float,
        default=30.0,
        help="per-case wall clock timeout",
    )
    parser.add_argument(
        "--memory-limit-mib",
        type=int,
        default=2048,
        help="per-case Linux RLIMIT_AS in MiB; 0 explicitly disables",
    )
    parser.add_argument(
        "--allow-timeout-only",
        action="store_true",
        help=(
            "allow hostile cases without an address-space limit; unsafe if a "
            "decoder allocation guard regresses"
        ),
    )
    arguments = parser.parse_args()

    if not arguments.cli.exists():
        print(f"CLI binary not found: {arguments.cli} (build with `cargo build`)")
        return 2
    manifest = json.loads((arguments.corpus / "manifest.json").read_text())

    memory_limit_bytes: int | None = None
    if arguments.memory_limit_mib > 0:
        if not supports_memory_limit():
            if not arguments.allow_timeout_only:
                print(
                    "refusing timeout-only malformed-corpus execution: an "
                    "enforceable RLIMIT_AS is available only on Linux; pass "
                    "--allow-timeout-only to acknowledge the allocation risk",
                    file=sys.stderr,
                )
                return 2
            print("WARNING: memory limit is not enforced on this platform")
        else:
            memory_limit_bytes = arguments.memory_limit_mib * 1024 * 1024
    elif not arguments.allow_timeout_only:
        print(
            "refusing malformed-corpus execution without a memory limit; pass "
            "--allow-timeout-only to acknowledge the allocation risk",
            file=sys.stderr,
        )
        return 2

    failures: list[str] = []
    for case in manifest["cases"]:
        failures.extend(
            run_case(
                arguments.cli,
                case,
                arguments.corpus,
                arguments.timeout_seconds,
                memory_limit_bytes,
            )
        )

    total = len(manifest["cases"])
    if failures:
        for line in failures:
            print(f"FAIL {line}")
        print(f"{total - len(failures)}/{total} cases passed")
        return 1
    limit_note = (
        f"RLIMIT_AS={arguments.memory_limit_mib}MiB"
        if memory_limit_bytes is not None
        else "memory limit not enforced on this platform"
    )
    print(
        f"{total}/{total} corpus cases failed cleanly "
        f"(timeout={arguments.timeout_seconds}s per case, {limit_note})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
