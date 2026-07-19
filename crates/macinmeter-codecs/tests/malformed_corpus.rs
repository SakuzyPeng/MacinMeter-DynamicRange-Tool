//! Regression harness for the committed `malformed-media-v1` corpus.
//!
//! Every case must fail with the structured error recorded in the corpus
//! manifest and must never produce EOF or a partial success. The claims here
//! cover exactly the committed corpus files, not all byte inputs.

use macinmeter_codecs::{DecoderFactory, ReadOutcome};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const MINIMUM_CASES: usize = 34;
const MAXIMUM_BLOCKS_PER_CASE: usize = 10_000;

fn corpus_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/malformed-media-v1")
        .join(name)
}

#[test]
fn every_corpus_case_fails_with_its_recorded_structured_error() {
    let manifest: Value = serde_json::from_slice(
        &fs::read(corpus_path("manifest.json")).expect("corpus manifest must exist"),
    )
    .expect("corpus manifest must be valid JSON");
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

        let expected_code = &case["expected"]["code"];
        let expected_stage = &case["expected"]["stage"];
        let error = match DecoderFactory::new().open(&file) {
            Err(error) => error,
            Ok(mut opened) => {
                let mut blocks = 0_usize;
                loop {
                    match opened.reader.read_block() {
                        Ok(ReadOutcome::Data(_)) => {
                            blocks += 1;
                            assert!(
                                blocks <= MAXIMUM_BLOCKS_PER_CASE,
                                "{id}: produced more than {MAXIMUM_BLOCKS_PER_CASE} blocks \
                                 without a terminal outcome"
                            );
                        }
                        Ok(ReadOutcome::Eof) => {
                            panic!("{id}: reached EOF as if the media were valid")
                        }
                        Err(error) => {
                            let replay = opened
                                .reader
                                .read_block()
                                .expect_err("terminal decode errors must be sticky");
                            assert_eq!(replay.code, error.code, "{id}: sticky code");
                            assert_eq!(replay.stage, error.stage, "{id}: sticky stage");
                            break error;
                        }
                    }
                }
            }
        };

        assert_eq!(
            serde_json::to_value(error.code).expect("serializable code"),
            *expected_code,
            "{id}: error code drifted from the recorded regression target"
        );
        assert_eq!(
            serde_json::to_value(error.stage).expect("serializable stage"),
            *expected_stage,
            "{id}: error stage drifted from the recorded regression target"
        );
    }
}
