from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "verify-malformed-corpus.py"
SPEC = importlib.util.spec_from_file_location("verify_malformed_corpus", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
verify_malformed_corpus = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify_malformed_corpus)


class VerifyMalformedCorpusTests(unittest.TestCase):
    def test_default_cli_path_tracks_the_renamed_binary_on_every_host(self) -> None:
        root = Path("/repository")

        self.assertEqual(
            verify_malformed_corpus.default_cli_path(root, "linux"),
            root / "target/debug/mdrmeter",
        )
        self.assertEqual(
            verify_malformed_corpus.default_cli_path(root, "darwin"),
            root / "target/debug/mdrmeter",
        )
        self.assertEqual(
            verify_malformed_corpus.default_cli_path(root, "win32"),
            root / "target/debug/mdrmeter.exe",
        )
