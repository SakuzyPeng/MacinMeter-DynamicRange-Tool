//! Identity gate for the committed `malformed-media-v1` corpus.
//!
//! This in-process workspace test deliberately does not pass hostile corpus
//! bytes to a decoder. Cases with forged multi-gigabyte length declarations
//! must run only through the isolated subprocess verifier with an effective
//! memory limit. Route-specific unit tests retain deterministic terminal-error
//! and sticky-state coverage without exposing the test runner to those inputs.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const MINIMUM_CASES: usize = 54;

fn corpus_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/malformed-media-v1")
        .join(name)
}

#[test]
fn committed_corpus_bytes_match_the_manifest() {
    let manifest: Value = serde_json::from_slice(
        &fs::read(corpus_path("manifest.json")).expect("corpus manifest must exist"),
    )
    .expect("corpus manifest must be valid JSON");
    assert_eq!(
        manifest["sourceCorpora"],
        serde_json::json!(["native-pcm-v1", "native-pcm-extensible-v1"])
    );
    let cases = manifest["cases"]
        .as_array()
        .expect("corpus manifest must contain cases");
    assert!(
        cases.len() >= MINIMUM_CASES,
        "corpus shrank below its committed size: {} < {MINIMUM_CASES}",
        cases.len()
    );

    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let file = corpus_path(case["path"].as_str().expect("case path"));
        let bytes = fs::read(&file).unwrap_or_else(|error| panic!("{id}: unreadable: {error}"));
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(digest, case["sha256"], "{id}: corpus bytes drifted");
        assert_eq!(
            bytes.len() as u64,
            case["sizeBytes"].as_u64().expect("case size"),
            "{id}: corpus size drifted"
        );
        assert!(
            case["expected"]["code"].is_string(),
            "{id}: expected error code must be recorded"
        );
        assert!(
            case["expected"]["stage"].is_string(),
            "{id}: expected error stage must be recorded"
        );
    }
}
