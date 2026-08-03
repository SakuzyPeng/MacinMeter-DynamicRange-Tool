use crate::{
    CapabilityStatus, DecodeEngineKind, DecoderFactory, NATIVE_CAPABILITY_CATALOG, PcmSource,
    ReadOutcome, stable_discovery_extensions,
};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelCount, ChannelLayout, ContainerFormat, DecodeReservation,
    ErrorCode, MAX_ANALYSIS_CHANNELS, MAX_DECODE_QUEUE_CAPACITY, PcmBlock, SourceCodec,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct TestFile(PathBuf);

impl TestFile {
    fn new(extension: &str, bytes: &[u8]) -> Self {
        let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "macinmeter-codecs-{}-{id}.{extension}",
            std::process::id()
        ));
        fs::write(&path, bytes).expect("write generated test media");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn assert_block_geometry(block: &PcmBlock, expected_channels: ChannelCount) {
    assert_eq!(block.channels(), expected_channels);
    assert_eq!(
        block.samples().len(),
        block.frames() * block.channels().as_usize()
    );
}

fn product_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/native-pcm-v1")
        .join(name)
}

fn product_fixture_pcm_sha256(name: &str) -> String {
    let manifest: Value = serde_json::from_slice(
        &fs::read(product_fixture_path("manifest.json"))
            .expect("native PCM fixture manifest must exist"),
    )
    .expect("native PCM fixture manifest must be valid JSON");
    manifest["fixtures"]
        .as_array()
        .expect("fixture manifest must contain an array")
        .iter()
        .find(|fixture| fixture["path"] == name)
        .and_then(|fixture| fixture["normalizedInterleavedF64LeSha256"].as_str())
        .unwrap_or_else(|| panic!("fixture manifest must record normalized PCM for {name}"))
        .to_owned()
}

fn extensible_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/native-pcm-extensible-v1")
        .join(name)
}

fn extensible_fixture_manifest() -> Value {
    serde_json::from_slice(
        &fs::read(extensible_fixture_path("manifest.json"))
            .expect("Extensible PCM fixture manifest must exist"),
    )
    .expect("Extensible PCM fixture manifest must be valid JSON")
}

fn extensible_fixture_entry(name: &str) -> Value {
    extensible_fixture_manifest()["fixtures"]
        .as_array()
        .expect("Extensible fixture manifest must contain an array")
        .iter()
        .find(|fixture| fixture["path"] == name)
        .unwrap_or_else(|| panic!("Extensible fixture manifest must record {name}"))
        .clone()
}

fn alac_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/native-alac-v1")
        .join(name)
}

fn alac_fixture_manifest() -> Value {
    serde_json::from_slice(
        &fs::read(alac_fixture_path("manifest.json"))
            .expect("native ALAC fixture manifest must exist"),
    )
    .expect("native ALAC fixture manifest must be valid JSON")
}

fn malformed_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/malformed-media-v1")
        .join(name)
}

fn manifest_pcm_samples(fixture: &Value) -> Vec<f64> {
    let oracle = &fixture["pcmOracle"];
    match oracle["kind"].as_str().expect("PCM oracle kind") {
        "integer_normalization" => {
            let divisor = oracle["divisor"].as_f64().expect("integer divisor");
            oracle["interleavedValues"]
                .as_array()
                .expect("integer oracle values")
                .iter()
                .map(|value| value.as_i64().expect("signed PCM value") as f64 / divisor)
                .collect()
        }
        "explicit_f64_bits" => oracle["interleavedValues"]
            .as_array()
            .expect("float oracle values")
            .iter()
            .map(|value| {
                let bits = u64::from_str_radix(
                    value
                        .as_str()
                        .expect("f64 oracle value must be hexadecimal"),
                    16,
                )
                .expect("f64 oracle bits must parse");
                f64::from_bits(bits)
            })
            .collect(),
        kind => panic!("unsupported fixture PCM oracle kind {kind}"),
    }
}

fn decode_all_samples(path: &Path) -> (macinmeter_domain::SourceInfo, Vec<f64>) {
    let mut opened = DecoderFactory::new()
        .open(path)
        .unwrap_or_else(|error| panic!("{} should open: {error}", path.display()));
    let source = opened.source.clone();
    let mut samples = Vec::new();
    while let ReadOutcome::Data(block) = opened.reader.read_block().expect("fixture should decode")
    {
        samples.extend_from_slice(block.samples());
    }
    (source, samples)
}

/// Worker counts every ALAC parallel-equivalence test sweeps.
const ALAC_WORKER_COUNTS: [usize; 3] = [2, 4, 8];

/// Worker counts the FLAC parallel-equivalence tests sweep.
const PACKET_WORKER_COUNTS: [usize; 3] = [2, 4, 8];

/// A reservation shaped exactly like the one the application plan derives.
fn worker_reservation(workers: usize) -> DecodeReservation {
    worker_reservation_with_queue(workers, workers * 4)
}

fn worker_reservation_with_queue(workers: usize, queue_capacity: usize) -> DecodeReservation {
    DecodeReservation::new(
        NonZeroUsize::new(workers).unwrap(),
        NonZeroUsize::new(queue_capacity).unwrap(),
        4 * 1024 * 1024 * workers as u64,
    )
    .expect("the plan's per-worker derivation must stay inside the domain ceilings")
}

fn decode_all_samples_with(
    path: &Path,
    reservation: DecodeReservation,
) -> (macinmeter_domain::SourceInfo, Vec<f64>) {
    let mut opened = DecoderFactory::with_application_reservation(reservation)
        .open(path)
        .unwrap_or_else(|error| panic!("{} should open: {error}", path.display()));
    let source = opened.source.clone();
    let mut samples = Vec::new();
    while let ReadOutcome::Data(block) = opened.reader.read_block().expect("fixture should decode")
    {
        samples.extend_from_slice(block.samples());
    }
    (source, samples)
}

/// Count worker pools started while `body` runs.
///
/// An equivalence test that quietly fell back to the serial route would pass
/// without proving anything, so every parallel test states how many pools it
/// expects to have started.
fn started_worker_pools(body: impl FnOnce()) -> usize {
    let counter = &crate::decode_engine::STARTED_WORKER_POOLS;
    let before = counter.with(std::cell::Cell::get);
    body();
    counter.with(std::cell::Cell::get) - before
}

fn raw_bits(samples: &[f64]) -> Vec<u64> {
    samples.iter().map(|sample| sample.to_bits()).collect()
}

fn alac_fixture_names() -> Vec<String> {
    alac_fixture_manifest()["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fixture| fixture["kind"] == "alac")
        .map(|fixture| fixture["path"].as_str().unwrap().to_owned())
        .collect()
}

struct PcmSourceContractCase {
    name: &'static str,
    wrong_extension: &'static str,
    bytes: Vec<u8>,
    container: ContainerFormat,
    codec: SourceCodec,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u32,
    expected_frames: u64,
    expected_samples: Vec<f64>,
    normalized_pcm_sha256: String,
    minimum_data_blocks: usize,
}

fn assert_pcm_source_contract(case: PcmSourceContractCase) {
    let file = TestFile::new(case.wrong_extension, &case.bytes);
    let mut opened = DecoderFactory::new()
        .open(file.path())
        .unwrap_or_else(|error| panic!("{} failed to open: {error}", case.name));
    let display_path = file.path().display().to_string();

    assert_eq!(
        opened.source.display_path, display_path,
        "{} source path",
        case.name
    );
    assert_eq!(
        opened.source.container, case.container,
        "{} container",
        case.name
    );
    assert_eq!(opened.source.codec, case.codec, "{} codec", case.name);
    assert_eq!(
        opened.source.sample_rate.get(),
        case.sample_rate,
        "{} source sample rate",
        case.name
    );
    assert_eq!(
        opened.source.channels.get(),
        case.channels,
        "{} source channels",
        case.name
    );
    assert_eq!(
        opened.source.bits_per_sample,
        Some(case.bits_per_sample),
        "{} source bit depth",
        case.name
    );
    assert_eq!(
        opened.source.expected_frames,
        Some(case.expected_frames),
        "{} source frame count",
        case.name
    );

    let immutable_info = opened.reader.stream_info().clone();
    assert_eq!(
        immutable_info.spec.sample_rate, opened.source.sample_rate,
        "{} PCM/source sample-rate agreement",
        case.name
    );
    assert_eq!(
        immutable_info.spec.channels, opened.source.channels,
        "{} PCM/source channel agreement",
        case.name
    );
    assert_eq!(
        immutable_info.spec.channel_layout,
        ChannelLayout::Unknown,
        "{} channel layout",
        case.name
    );
    assert_eq!(
        immutable_info.expected_frames, opened.source.expected_frames,
        "{} PCM/source expected-frame agreement",
        case.name
    );

    let initial_progress = opened.reader.progress();
    assert_eq!(
        initial_progress.decoded_frames(),
        0,
        "{} initial decoded frames",
        case.name
    );
    assert_eq!(
        initial_progress.expected_frames(),
        Some(case.expected_frames),
        "{} initial expected frames",
        case.name
    );
    assert_eq!(
        initial_progress.fraction(),
        Some(0.0),
        "{} initial progress fraction",
        case.name
    );
    assert!(!initial_progress.is_eof(), "{} began at EOF", case.name);
    let initial_diagnostics = opened.reader.diagnostics().clone();
    assert_eq!(
        initial_diagnostics.backend, "symphonia",
        "{} diagnostics backend",
        case.name
    );
    assert_eq!(
        initial_diagnostics.decoded_frames, 0,
        "{} initial diagnostic frame count",
        case.name
    );
    assert!(
        initial_diagnostics.warnings.is_empty(),
        "{} began with decoder warnings",
        case.name
    );

    let expected_channels = immutable_info.spec.channels;
    let mut decoded_frames = 0_u64;
    let mut data_blocks = 0_usize;
    let mut samples = Vec::new();
    while let ReadOutcome::Data(block) = opened
        .reader
        .read_block()
        .unwrap_or_else(|error| panic!("{} decode failed: {error}", case.name))
    {
        data_blocks += 1;
        assert_block_geometry(&block, expected_channels);
        assert!(block.frames() > 0, "{} returned an empty block", case.name);
        assert!(
            block.samples().iter().all(|sample| sample.is_finite()),
            "{} returned non-finite PCM",
            case.name
        );
        let block_frames =
            u64::try_from(block.frames()).expect("contract fixture frame count fits u64");
        let previous_frames = decoded_frames;
        decoded_frames = decoded_frames
            .checked_add(block_frames)
            .expect("contract fixture frame count does not overflow");
        assert!(
            decoded_frames > previous_frames,
            "{} progress did not advance after Data",
            case.name
        );
        samples.extend_from_slice(block.samples());

        assert_eq!(
            opened.reader.stream_info(),
            &immutable_info,
            "{} stream info changed after Data",
            case.name
        );
        let progress = opened.reader.progress();
        assert_eq!(
            progress.decoded_frames(),
            decoded_frames,
            "{} progress disagrees with returned Data",
            case.name
        );
        assert_eq!(
            progress.expected_frames(),
            Some(case.expected_frames),
            "{} progress expected frames changed",
            case.name
        );
        let expected_fraction = decoded_frames as f64 / case.expected_frames as f64;
        assert_eq!(
            progress.fraction(),
            Some(expected_fraction),
            "{} progress fraction",
            case.name
        );
        assert!(
            progress.fraction().is_some_and(f64::is_finite),
            "{} progress fraction is non-finite",
            case.name
        );
        assert!(
            !progress.is_eof(),
            "{} marked EOF while returning Data",
            case.name
        );

        let diagnostics = opened.reader.diagnostics();
        assert_eq!(
            diagnostics.backend, initial_diagnostics.backend,
            "{} diagnostics backend changed",
            case.name
        );
        assert_eq!(
            diagnostics.decoded_frames, decoded_frames,
            "{} diagnostics disagree with returned Data",
            case.name
        );
        assert_eq!(
            diagnostics.warnings, initial_diagnostics.warnings,
            "{} produced unexpected warnings",
            case.name
        );
    }

    assert!(
        data_blocks >= case.minimum_data_blocks,
        "{} returned {data_blocks} Data blocks, expected at least {}",
        case.name,
        case.minimum_data_blocks
    );
    assert_eq!(
        decoded_frames, case.expected_frames,
        "{} cumulative frame count",
        case.name
    );
    assert_eq!(
        samples.len(),
        usize::try_from(case.expected_frames).unwrap() * usize::from(case.channels),
        "{} cumulative sample count",
        case.name
    );
    let actual_bits: Vec<u64> = samples.iter().map(|sample| sample.to_bits()).collect();
    let expected_bits: Vec<u64> = case
        .expected_samples
        .iter()
        .map(|sample| sample.to_bits())
        .collect();
    assert_eq!(actual_bits, expected_bits, "{} normalized PCM", case.name);
    let mut normalized_hasher = Sha256::new();
    for sample in &samples {
        normalized_hasher.update(sample.to_le_bytes());
    }
    assert_eq!(
        format!("{:x}", normalized_hasher.finalize()),
        case.normalized_pcm_sha256,
        "{} normalized PCM manifest hash",
        case.name
    );

    assert_eq!(
        opened.reader.stream_info(),
        &immutable_info,
        "{} stream info changed at EOF",
        case.name
    );
    let terminal_progress = opened.reader.progress();
    assert_eq!(terminal_progress.decoded_frames(), case.expected_frames);
    assert_eq!(
        terminal_progress.expected_frames(),
        Some(case.expected_frames)
    );
    assert_eq!(terminal_progress.fraction(), Some(1.0));
    assert!(
        terminal_progress.is_eof(),
        "{} EOF was not sticky state",
        case.name
    );
    let terminal_diagnostics = opened.reader.diagnostics().clone();
    assert_eq!(
        terminal_diagnostics.backend, initial_diagnostics.backend,
        "{} terminal diagnostics backend",
        case.name
    );
    assert_eq!(
        terminal_diagnostics.decoded_frames, case.expected_frames,
        "{} terminal diagnostic frame count",
        case.name
    );
    assert!(
        terminal_diagnostics.warnings.is_empty(),
        "{} successful decode ended with warnings",
        case.name
    );

    for repeated_read in 1..=2 {
        assert_eq!(
            opened.reader.read_block().unwrap(),
            ReadOutcome::Eof,
            "{} repeated EOF read {repeated_read}",
            case.name
        );
        assert_eq!(
            opened.reader.stream_info(),
            &immutable_info,
            "{} stream info changed after repeated EOF",
            case.name
        );
        assert_eq!(
            opened.reader.progress(),
            terminal_progress,
            "{} progress changed after repeated EOF",
            case.name
        );
        assert_eq!(
            opened.reader.diagnostics(),
            &terminal_diagnostics,
            "{} diagnostics changed after repeated EOF",
            case.name
        );
    }
}

fn assert_analysis_channel_limit_error(error: &AnalysisError, declared_channels: u16) {
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    assert_eq!(error.stage, AnalysisStage::Probe);
    assert_eq!(error.backend.as_deref(), Some("symphonia"));
    let details = error
        .details
        .as_deref()
        .expect("channel limit error should include declared and maximum counts");
    assert!(details.contains(&format!("declared_channels={declared_channels}")));
    assert!(details.contains(&format!("max_analysis_channels={MAX_ANALYSIS_CHANNELS}")));
}

#[test]
fn extensions_are_discovery_only_and_exclude_aifc() {
    let stable: Vec<&str> = stable_discovery_extensions().collect();
    assert_eq!(
        stable,
        [
            "wav", "wave", "wav", "wave", "flac", "aif", "aiff", "m4a", "mp4"
        ]
    );
    assert!(!stable.contains(&"aifc"));
}

#[test]
fn capability_catalog_keeps_planned_routes_out_of_discovery() {
    for route in NATIVE_CAPABILITY_CATALOG {
        assert!(!route.container.is_empty() && !route.codec.is_empty());
        assert!(!route.backend.is_empty());
        if route.status != CapabilityStatus::Stable {
            for extension in route.discovery_extensions {
                assert!(
                    !stable_discovery_extensions().any(|stable| stable == *extension),
                    "non-stable route {}/{} must not leak {extension} into discovery",
                    route.container,
                    route.codec
                );
            }
        }
        for extension in route.discovery_extensions {
            assert_eq!(
                extension.to_ascii_lowercase(),
                *extension,
                "discovery extensions must be lowercase"
            );
        }
    }
    let stable_pairs: BTreeSet<(&str, &str)> = NATIVE_CAPABILITY_CATALOG
        .iter()
        .filter(|route| route.status == CapabilityStatus::Stable)
        .map(|route| (route.container, route.codec))
        .collect();
    assert_eq!(
        stable_pairs,
        BTreeSet::from([
            ("wave", "pcm_integer"),
            ("wave", "pcm_float"),
            ("flac", "flac"),
            ("aiff", "pcm_integer"),
            ("mp4", "alac"),
        ])
    );
}

#[test]
fn wave_capabilities_publish_the_extensible_stable_subset() {
    for codec in ["pcm_integer", "pcm_float"] {
        let route = NATIVE_CAPABILITY_CATALOG
            .iter()
            .find(|route| route.container == "wave" && route.codec == codec)
            .expect("stable WAV route must exist");
        let limitations = route.limitations.join("; ");
        assert!(limitations.contains("exact WAVE_FORMAT_EXTENSIBLE"));
        assert!(limitations.contains("valid bits must equal container bits"));
        assert!(limitations.contains("1-26 channels"));
        assert!(limitations.contains("low 18 speaker bits"));
        assert!(!limitations.contains("rejected at probe"));
    }
}

#[test]
fn stable_catalog_identifiers_match_the_domain_enum_serialization() {
    let container_ids: BTreeSet<String> = [
        ContainerFormat::Wave,
        ContainerFormat::Flac,
        ContainerFormat::Aiff,
        ContainerFormat::Mp4,
    ]
    .iter()
    .map(|value| {
        serde_json::to_value(value)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    })
    .collect();
    let codec_ids: BTreeSet<String> = [
        SourceCodec::PcmInteger,
        SourceCodec::PcmFloat,
        SourceCodec::Flac,
        SourceCodec::Alac,
    ]
    .iter()
    .map(|value| {
        serde_json::to_value(value)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    })
    .collect();
    for route in NATIVE_CAPABILITY_CATALOG
        .iter()
        .filter(|route| route.status == CapabilityStatus::Stable)
    {
        assert!(
            container_ids.contains(route.container),
            "stable container id {} must match a domain enum identifier",
            route.container
        );
        assert!(
            codec_ids.contains(route.codec),
            "stable codec id {} must match a domain enum identifier",
            route.codec
        );
    }
}

#[test]
fn alac_capability_publishes_the_bounded_stable_route() {
    let route = NATIVE_CAPABILITY_CATALOG
        .iter()
        .find(|route| route.container == "mp4" && route.codec == "alac")
        .expect("stable ALAC route must exist");
    assert_eq!(route.status, CapabilityStatus::Stable);
    assert_eq!(route.discovery_extensions, ["m4a", "mp4"]);
    let limitations = route.limitations.join("; ");
    assert!(limitations.contains("unfragmented"));
    assert!(limitations.contains("16-bit or 24-bit"));
    assert!(limitations.contains("1-8 channels"));
    assert!(limitations.contains("one audio-only ALAC track"));
}

#[test]
fn product_fixture_manifest_matches_the_committed_native_corpus() {
    let corpus = product_fixture_path("");
    let manifest_path = corpus.join("manifest.json");
    let manifest: Value = serde_json::from_slice(
        &fs::read(&manifest_path).expect("native PCM fixture manifest must exist"),
    )
    .expect("native PCM fixture manifest must be valid JSON");

    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["corpusId"], "native-pcm-v1");
    assert_eq!(
        manifest["generator"]["path"],
        "scripts/generate-native-pcm-v1.py"
    );
    let fixtures = manifest["fixtures"]
        .as_array()
        .expect("fixture manifest must contain an array");
    assert_eq!(fixtures.len(), 11);

    let mut referenced_files = BTreeSet::new();
    for fixture in fixtures {
        let relative = fixture["path"]
            .as_str()
            .expect("fixture path must be a string");
        assert_eq!(
            Path::new(relative).components().count(),
            1,
            "fixture paths must remain inside the corpus directory"
        );
        assert!(
            referenced_files.insert(relative.to_owned()),
            "fixture path {relative} is duplicated"
        );
        let bytes =
            fs::read(corpus.join(relative)).expect("every manifest fixture must be committed");
        assert_eq!(
            u64::try_from(bytes.len()).unwrap(),
            fixture["sizeBytes"]
                .as_u64()
                .expect("fixture size must be an unsigned integer"),
            "fixture size drifted for {relative}"
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            fixture["sha256"]
                .as_str()
                .expect("fixture SHA-256 must be a string"),
            "fixture bytes drifted for {relative}"
        );
        assert_eq!(fixture["provenance"]["kind"], "deterministically_generated");
        assert_eq!(fixture["provenance"]["copyrightedAudio"], false);
        assert_eq!(fixture["provenance"]["license"], "MIT");
        assert!(
            fixture["minimumDataBlocks"]
                .as_u64()
                .is_some_and(|blocks| blocks >= 1)
        );
        let pcm_hash = fixture["normalizedInterleavedF64LeSha256"]
            .as_str()
            .expect("normalized PCM SHA-256 must be recorded");
        assert_eq!(pcm_hash.len(), 64);
        assert!(pcm_hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    let committed_audio_files: BTreeSet<String> = fs::read_dir(&corpus)
        .expect("native PCM corpus directory must exist")
        .map(|entry| entry.expect("fixture directory entry must be readable"))
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?;
            matches!(extension, "wav" | "aiff" | "flac")
                .then(|| path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    assert_eq!(referenced_files, committed_audio_files);
    assert_eq!(
        manifest["derivedMutations"][0]["source"],
        "flac-pcm-s16-stereo-multiblock.flac"
    );
    assert_eq!(
        manifest["derivedMutations"][0]["operation"],
        "xor the final byte with 0xff"
    );
    assert_eq!(
        manifest["derivedMutations"][0]["expected"],
        "sticky decode_failed; never EOF or a successful report"
    );
}

#[test]
fn extensible_fixture_manifest_matches_the_committed_twin_corpus() {
    let corpus = extensible_fixture_path("");
    let manifest = extensible_fixture_manifest();
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["corpusId"], "native-pcm-extensible-v1");
    assert_eq!(
        manifest["generator"]["path"],
        "scripts/generate-native-pcm-extensible-v1.py"
    );
    assert_eq!(manifest["generator"]["externalToolsRequired"], false);
    assert_eq!(manifest["generator"]["networkRequired"], false);

    let fixtures = manifest["fixtures"]
        .as_array()
        .expect("Extensible fixture manifest must contain an array");
    assert_eq!(fixtures.len(), 20);
    let mut referenced_files = BTreeSet::new();
    let mut twin_counts = std::collections::BTreeMap::new();
    for fixture in fixtures {
        let relative = fixture["path"].as_str().expect("fixture path");
        assert!(referenced_files.insert(relative.to_owned()));
        let bytes = fs::read(corpus.join(relative)).expect("fixture must be committed");
        assert_eq!(fixture["sizeBytes"].as_u64(), Some(bytes.len() as u64));
        assert_eq!(
            fixture["sha256"].as_str(),
            Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
        );
        assert_eq!(fixture["container"], "wave");
        assert_eq!(fixture["validBits"], fixture["containerBits"]);
        assert_eq!(fixture["provenance"]["kind"], "deterministically_generated");
        assert_eq!(fixture["provenance"]["copyrightedAudio"], false);
        assert_eq!(fixture["provenance"]["license"], "MIT");
        let hash = fixture["normalizedInterleavedF64LeSha256"]
            .as_str()
            .expect("normalized PCM hash");
        assert_eq!(hash.len(), 64);
        *twin_counts
            .entry(fixture["twinId"].as_str().expect("twin id"))
            .or_insert(0_u8) += 1;
    }
    assert!(twin_counts.values().all(|count| *count == 2));
    let committed_audio_files: BTreeSet<String> = fs::read_dir(corpus)
        .expect("Extensible corpus directory")
        .map(|entry| entry.expect("fixture directory entry"))
        .filter_map(|entry| {
            (entry.path().extension()?.to_str()? == "wav")
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    assert_eq!(referenced_files, committed_audio_files);
}

#[test]
fn alac_fixture_manifest_matches_the_committed_twin_corpus() {
    let corpus = alac_fixture_path("");
    let manifest = alac_fixture_manifest();
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["corpus"], "native-alac-v1");
    assert_eq!(
        manifest["generator"]["path"],
        "scripts/generate-native-alac-v1.py"
    );
    assert_eq!(
        manifest["generator"]["commands"],
        serde_json::json!([
            "python3 scripts/generate-native-alac-v1.py",
            "python3 scripts/generate-native-alac-v1.py --check"
        ])
    );
    assert_eq!(
        manifest["generator"]["externalToolsRequiredForRegeneration"],
        true
    );
    assert_eq!(
        manifest["generator"]["normalTestsRequireExternalTools"],
        false
    );
    assert_eq!(manifest["encoder"]["tool"], "ffmpeg");
    assert_eq!(manifest["encoder"]["version"], "8.0.1");
    assert_eq!(
        manifest["provenance"]["kind"],
        "deterministically_generated"
    );
    assert_eq!(manifest["provenance"]["copyrightedAudio"], false);
    assert_eq!(manifest["provenance"]["license"], "MIT");

    let fixtures = manifest["fixtures"]
        .as_array()
        .expect("native ALAC fixture manifest must contain an array");
    assert_eq!(fixtures.len(), 20);
    let mut referenced_files = BTreeSet::new();
    for fixture in fixtures {
        let relative = fixture["path"].as_str().expect("fixture path");
        assert!(referenced_files.insert(relative.to_owned()));
        let bytes = fs::read(corpus.join(relative)).expect("fixture must be committed");
        assert_eq!(
            fixture["sha256"].as_str(),
            Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
        );
        assert!(matches!(fixture["bitsPerSample"].as_u64(), Some(16 | 24)));
        assert!(matches!(fixture["channels"].as_u64(), Some(1..=8)));
        assert_eq!(
            fixture["normalizedInterleavedF64LeSha256"]
                .as_str()
                .expect("normalized PCM hash")
                .len(),
            64
        );
        let twin = fixture["twin"].as_str().expect("twin path");
        assert!(fixtures.iter().any(|candidate| candidate["path"] == twin));
        if fixture["kind"] == "alac" {
            let structure = &fixture["isoBmff"];
            assert_eq!(structure["topLevelBoxes"][0]["type"], "ftyp");
            let top_level_types: Vec<&str> = structure["topLevelBoxes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["type"].as_str().unwrap())
                .collect();
            assert!(top_level_types.contains(&"moov"));
            assert!(top_level_types.contains(&"mdat"));
            assert_eq!(structure["cookie"]["lengthBytes"], 24);
            assert_eq!(structure["cookie"]["frameLength"], 4096);
            assert_eq!(structure["cookie"]["compatibleVersion"], 0);
            assert_eq!(structure["cookie"]["bitDepth"], fixture["bitsPerSample"]);
            assert_eq!(structure["cookie"]["channels"], fixture["channels"]);
            assert_eq!(structure["cookie"]["sampleRate"], fixture["sampleRate"]);
            assert_eq!(
                structure["mediaHeader"]["durationFrames"],
                fixture["frames"]
            );
            assert_eq!(
                structure["sampleTables"]["sttsFrameCount"],
                fixture["frames"]
            );
            assert_eq!(
                structure["sampleTables"]["sttsPacketCount"],
                structure["sampleTables"]["stszSampleCount"]
            );
            let command = serde_json::to_string(&fixture["encoderCommand"]).unwrap();
            assert!(command.contains("<corpus>/"));
            assert!(!command.contains("/var/folders/"));
        }
    }
    for source in manifest["routeNegativeSources"]
        .as_array()
        .expect("route-negative source manifest must contain an array")
    {
        let relative = source["path"].as_str().expect("source path");
        assert!(referenced_files.insert(relative.to_owned()));
        let bytes = fs::read(corpus.join(relative)).expect("source must be committed");
        assert_eq!(
            source["sha256"].as_str(),
            Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
        );
        assert_eq!(source["expected"]["code"], "unsupported_format");
        assert_eq!(source["expected"]["stage"], "probe");
    }
    let committed_audio_files: BTreeSet<String> = fs::read_dir(corpus)
        .expect("native ALAC corpus directory")
        .map(|entry| entry.expect("fixture directory entry"))
        .filter_map(|entry| {
            let extension = entry.path().extension()?.to_str()?.to_owned();
            matches!(extension.as_str(), "wav" | "m4a" | "mp4")
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    assert_eq!(referenced_files, committed_audio_files);
}

#[test]
fn alac_route_negative_sources_are_rejected_during_probe() {
    let manifest = alac_fixture_manifest();
    for source in manifest["routeNegativeSources"].as_array().unwrap() {
        let path = alac_fixture_path(source["path"].as_str().unwrap());
        let error = expect_open_error(&path);
        assert_eq!(
            error.code,
            ErrorCode::UnsupportedFormat,
            "{}",
            path.display()
        );
        assert_eq!(error.stage, AnalysisStage::Probe, "{}", path.display());
    }
}

#[test]
fn alac_malformed_corpus_uses_the_fixed_probe_error_classification() {
    let manifest: Value = serde_json::from_slice(
        &fs::read(malformed_fixture_path("manifest.json")).expect("manifest must exist"),
    )
    .unwrap();
    for case in manifest["cases"].as_array().unwrap().iter().filter(|case| {
        case["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("alac-"))
            && case["expected"]["stage"] == "probe"
    }) {
        let id = case["id"].as_str().unwrap();
        let path = malformed_fixture_path(case["path"].as_str().unwrap());
        let error = expect_open_error(&path);
        let expected_code = match case["expected"]["code"].as_str().unwrap() {
            "malformed_media" => ErrorCode::MalformedMedia,
            "unsupported_format" => ErrorCode::UnsupportedFormat,
            other => panic!("{id}: unexpected error code {other}"),
        };
        assert_eq!(error.code, expected_code, "{id}: {error}");
        assert_eq!(error.stage, AnalysisStage::Probe, "{id}: {error}");
    }
}

#[test]
fn alac_route_rejects_a_non_alac_sample_entry_by_codec_identity() {
    let path = malformed_fixture_path("alac-non-alac-sample-entry.m4a");
    let error = expect_open_error(&path);
    assert_eq!(error.code, ErrorCode::UnsupportedFormat, "{error}");
    assert_eq!(error.stage, AnalysisStage::Probe, "{error}");
    assert!(
        error
            .message
            .contains("audio codec is outside the stable ALAC route"),
        "{error}"
    );
    assert_eq!(
        error.details.as_deref(),
        Some("sample_entry=mp4a"),
        "{error}"
    );
}

#[test]
fn alac_backend_metadata_must_match_the_validated_container() {
    use symphonia::core::codecs::{CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CodecParameters};

    let path = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    let mut file = fs::File::open(&path).unwrap();
    let validated = crate::isobmff::inspect_isobmff_alac(&mut file, &path).unwrap();
    let valid_parameters = || {
        let mut parameters = CodecParameters::new();
        parameters
            .for_codec(CODEC_TYPE_ALAC)
            .with_sample_rate(validated.pcm.sample_rate)
            .with_n_frames(validated.declared_frames)
            .with_extra_data(validated.magic_cookie.clone());
        parameters
    };

    crate::symphonia_source::validate_backend_alac_metadata(&path, &validated, &valid_parameters())
        .unwrap();

    let mut mismatches = Vec::new();
    let mut parameters = valid_parameters();
    parameters.codec = CODEC_TYPE_FLAC;
    mismatches.push(("codec", parameters));
    let mut parameters = valid_parameters();
    parameters.sample_rate = Some(44_100);
    mismatches.push(("sample rate", parameters));
    let mut parameters = valid_parameters();
    parameters.n_frames = Some(validated.declared_frames + 1);
    mismatches.push(("frame count", parameters));
    let mut parameters = valid_parameters();
    parameters.extra_data.as_mut().unwrap()[0] ^= 0xff;
    mismatches.push(("cookie", parameters));

    for (name, parameters) in mismatches {
        let error =
            crate::symphonia_source::validate_backend_alac_metadata(&path, &validated, &parameters)
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::MalformedMedia, "{name}");
        assert_eq!(error.stage, AnalysisStage::Probe, "{name}");
    }
}

#[test]
fn shared_analysis_channel_limit_accepts_64_and_rejects_larger_geometries() {
    let path = Path::new("channel-limit.test");
    crate::error::validate_analysis_channel_count(path, MAX_ANALYSIS_CHANNELS).unwrap();

    for channels in [MAX_ANALYSIS_CHANNELS + 1, u16::MAX] {
        let error = crate::error::validate_analysis_channel_count(path, channels).unwrap_err();
        assert_analysis_channel_limit_error(&error, channels);
        assert_eq!(error.display_path.as_deref(), Some("channel-limit.test"));
    }
}

#[test]
fn rejects_over_limit_wave_and_aiff_before_symphonia_probe() {
    for channels in [MAX_ANALYSIS_CHANNELS + 1, u16::MAX] {
        let file = TestFile::new("wav", &empty_pcm8_wave(channels));
        let error = expect_open_error(file.path());
        assert_analysis_channel_limit_error(&error, channels);
        let display_path = file.path().display().to_string();
        assert_eq!(error.display_path.as_deref(), Some(display_path.as_str()));
    }

    let channels = MAX_ANALYSIS_CHANNELS + 1;
    let file = TestFile::new("aiff", &empty_pcm8_aiff(channels));
    let error = expect_open_error(file.path());
    assert_analysis_channel_limit_error(&error, channels);
    let display_path = file.path().display().to_string();
    assert_eq!(error.display_path.as_deref(), Some(display_path.as_str()));
}

#[test]
fn rejects_negative_aiff_channel_count_as_malformed() {
    let file = TestFile::new("aiff", &empty_pcm8_aiff(u16::MAX));
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert_eq!(error.stage, AnalysisStage::Probe);
    assert!(error.message.contains("channel count must be positive"));
}

#[test]
fn declared_native_matrix_passes_the_shared_pcm_source_contract() {
    let mut cases = Vec::new();
    for (bits, wave_name, aiff_name) in [
        (8, "wav-pcm-u8-stereo.wav", "aiff-pcm-s8-stereo.aiff"),
        (16, "wav-pcm-s16-stereo.wav", "aiff-pcm-s16-stereo.aiff"),
        (24, "wav-pcm-s24-stereo.wav", "aiff-pcm-s24-stereo.aiff"),
        (32, "wav-pcm-s32-stereo.wav", "aiff-pcm-s32-stereo.aiff"),
    ] {
        let expected_samples = normalize_integer_contract_samples(bits);
        cases.push(PcmSourceContractCase {
            name: wave_name,
            wrong_extension: "aiff",
            bytes: fs::read(product_fixture_path(wave_name))
                .expect("declared WAV fixture must be committed"),
            container: ContainerFormat::Wave,
            codec: SourceCodec::PcmInteger,
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: bits,
            expected_frames: 4,
            expected_samples: expected_samples.clone(),
            normalized_pcm_sha256: product_fixture_pcm_sha256(wave_name),
            minimum_data_blocks: 1,
        });
        cases.push(PcmSourceContractCase {
            name: aiff_name,
            wrong_extension: "wav",
            bytes: fs::read(product_fixture_path(aiff_name))
                .expect("declared AIFF fixture must be committed"),
            container: ContainerFormat::Aiff,
            codec: SourceCodec::PcmInteger,
            sample_rate: 44_100,
            channels: 2,
            bits_per_sample: bits,
            expected_frames: 4,
            expected_samples,
            normalized_pcm_sha256: product_fixture_pcm_sha256(aiff_name),
            minimum_data_blocks: 1,
        });
    }

    for (bits, name, expected_samples) in [
        (32, "wav-float32-stereo.wav", float32_contract_samples()),
        (64, "wav-float64-stereo.wav", float64_contract_samples()),
    ] {
        cases.push(PcmSourceContractCase {
            name,
            wrong_extension: "flac",
            bytes: fs::read(product_fixture_path(name))
                .expect("declared WAV float fixture must be committed"),
            container: ContainerFormat::Wave,
            codec: SourceCodec::PcmFloat,
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: bits,
            expected_frames: 4,
            expected_samples,
            normalized_pcm_sha256: product_fixture_pcm_sha256(name),
            minimum_data_blocks: 1,
        });
    }

    let flac_name = "flac-pcm-s16-stereo-multiblock.flac";
    cases.push(PcmSourceContractCase {
        name: flac_name,
        wrong_extension: "wav",
        bytes: fs::read(product_fixture_path(flac_name))
            .expect("representative FLAC fixture must be committed"),
        container: ContainerFormat::Flac,
        codec: SourceCodec::Flac,
        sample_rate: 8_000,
        channels: 2,
        bits_per_sample: 16,
        expected_frames: 400,
        expected_samples: normalize_pcm16(&flac_contract_samples()),
        normalized_pcm_sha256: product_fixture_pcm_sha256(flac_name),
        minimum_data_blocks: 2,
    });

    for case in cases {
        assert_pcm_source_contract(case);
    }
}

#[test]
fn extensible_matrix_passes_the_shared_pcm_source_contract() {
    for (name, codec, bits_per_sample, channels) in [
        (
            "pcm-u8-stereo-mask-extensible.wav",
            SourceCodec::PcmInteger,
            8,
            2,
        ),
        (
            "pcm-s16-stereo-mask-extensible.wav",
            SourceCodec::PcmInteger,
            16,
            2,
        ),
        (
            "pcm-s24-stereo-mask-extensible.wav",
            SourceCodec::PcmInteger,
            24,
            2,
        ),
        (
            "pcm-s32-stereo-mask-extensible.wav",
            SourceCodec::PcmInteger,
            32,
            2,
        ),
        (
            "float32-stereo-mask-extensible.wav",
            SourceCodec::PcmFloat,
            32,
            2,
        ),
        (
            "float64-stereo-mask-extensible.wav",
            SourceCodec::PcmFloat,
            64,
            2,
        ),
        (
            "pcm-s16-mono-center-mask-extensible.wav",
            SourceCodec::PcmInteger,
            16,
            1,
        ),
        (
            "pcm-s24-6ch-mask-extensible.wav",
            SourceCodec::PcmInteger,
            24,
            6,
        ),
        (
            "pcm-s16-stereo-zero-mask-extensible.wav",
            SourceCodec::PcmInteger,
            16,
            2,
        ),
        (
            "pcm-s16-26ch-zero-mask-extensible.wav",
            SourceCodec::PcmInteger,
            16,
            26,
        ),
    ] {
        let fixture = extensible_fixture_entry(name);
        assert_pcm_source_contract(PcmSourceContractCase {
            name,
            wrong_extension: "flac",
            bytes: fs::read(extensible_fixture_path(name))
                .expect("Extensible fixture must be committed"),
            container: ContainerFormat::Wave,
            codec,
            sample_rate: 48_000,
            channels,
            bits_per_sample,
            expected_frames: 8,
            expected_samples: manifest_pcm_samples(&fixture),
            normalized_pcm_sha256: fixture["normalizedInterleavedF64LeSha256"]
                .as_str()
                .expect("normalized PCM hash")
                .to_owned(),
            minimum_data_blocks: 1,
        });
    }
}

#[test]
fn alac_matrix_passes_the_shared_pcm_source_contract() {
    let manifest = alac_fixture_manifest();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    for fixture in fixtures.iter().filter(|fixture| fixture["kind"] == "alac") {
        let name = fixture["path"].as_str().unwrap();
        let twin = fixture["twin"].as_str().unwrap();
        let (_, expected_samples) = decode_all_samples(&alac_fixture_path(twin));
        let leaked_name: &'static str = Box::leak(name.to_owned().into_boxed_str());
        let expected_frames = fixture["frames"].as_u64().unwrap();
        assert_pcm_source_contract(PcmSourceContractCase {
            name: leaked_name,
            wrong_extension: "wav",
            bytes: fs::read(alac_fixture_path(name)).expect("ALAC fixture must be committed"),
            container: ContainerFormat::Mp4,
            codec: SourceCodec::Alac,
            sample_rate: u32::try_from(fixture["sampleRate"].as_u64().unwrap()).unwrap(),
            channels: u16::try_from(fixture["channels"].as_u64().unwrap()).unwrap(),
            bits_per_sample: u32::try_from(fixture["bitsPerSample"].as_u64().unwrap()).unwrap(),
            expected_frames,
            expected_samples,
            normalized_pcm_sha256: fixture["normalizedInterleavedF64LeSha256"]
                .as_str()
                .unwrap()
                .to_owned(),
            minimum_data_blocks: if expected_frames > 4096 { 2 } else { 1 },
        });
    }
}

#[test]
fn alac_and_wave_twins_decode_to_bit_identical_pcm() {
    let manifest = alac_fixture_manifest();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    for alac in fixtures.iter().filter(|fixture| fixture["kind"] == "alac") {
        let alac_name = alac["path"].as_str().unwrap();
        let wave_name = alac["twin"].as_str().unwrap();
        let (alac_source, alac_samples) = decode_all_samples(&alac_fixture_path(alac_name));
        let (wave_source, wave_samples) = decode_all_samples(&alac_fixture_path(wave_name));

        assert_eq!(alac_source.container, ContainerFormat::Mp4, "{alac_name}");
        assert_eq!(alac_source.codec, SourceCodec::Alac, "{alac_name}");
        assert_eq!(wave_source.container, ContainerFormat::Wave, "{alac_name}");
        assert_eq!(wave_source.codec, SourceCodec::PcmInteger, "{alac_name}");
        assert_eq!(
            alac_source.sample_rate, wave_source.sample_rate,
            "{alac_name}"
        );
        assert_eq!(alac_source.channels, wave_source.channels, "{alac_name}");
        assert_eq!(
            alac_source.bits_per_sample, wave_source.bits_per_sample,
            "{alac_name}"
        );
        assert_eq!(
            alac_source.expected_frames, wave_source.expected_frames,
            "{alac_name}"
        );
        assert_eq!(
            alac_samples
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            wave_samples
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            "{alac_name}"
        );
    }
}

#[test]
fn alac_route_accepts_an_explicit_64_bit_ftyp_box_length() {
    let original_path = alac_fixture_path("alac16-mono-44100.m4a");
    let original = fs::read(&original_path).unwrap();
    assert_eq!(u32::from_be_bytes(original[0..4].try_into().unwrap()), 28);
    assert_eq!(&original[4..8], b"ftyp");
    assert_eq!(&original[28..36], b"\0\0\0\x08free");

    let mut extended = Vec::with_capacity(original.len());
    extended.extend_from_slice(&1_u32.to_be_bytes());
    extended.extend_from_slice(b"ftyp");
    extended.extend_from_slice(&36_u64.to_be_bytes());
    extended.extend_from_slice(&original[8..28]);
    extended.extend_from_slice(&original[36..]);
    assert_eq!(extended.len(), original.len());

    let file = TestFile::new("mp4", &extended);
    let (extended_source, extended_samples) = decode_all_samples(file.path());
    let (original_source, original_samples) = decode_all_samples(&original_path);
    assert_eq!(extended_source.container, ContainerFormat::Mp4);
    assert_eq!(extended_source.codec, SourceCodec::Alac);
    assert_eq!(extended_source.sample_rate, original_source.sample_rate);
    assert_eq!(extended_source.channels, original_source.channels);
    assert_eq!(
        extended_source.expected_frames,
        original_source.expected_frames
    );
    assert_eq!(
        extended_samples
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        original_samples
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn alac_route_accepts_a_48_byte_cookie_with_the_standard_stereo_layout() {
    fn find(bytes: &[u8], needle: &[u8]) -> usize {
        bytes
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap_or_else(|| panic!("missing {needle:?}"))
    }

    let original_path = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    let mut bytes = fs::read(&original_path).unwrap();
    let config_type = find(&bytes, b"alac\0\0\0\0\0\0\x10\0");
    let outer_type = bytes[..config_type]
        .windows(4)
        .rposition(|window| window == b"alac")
        .unwrap();
    let config_start = config_type - 4;
    let config_size = u32::from_be_bytes(bytes[config_start..config_type].try_into().unwrap());
    assert_eq!(config_size, 36);
    let insertion = config_start + usize::try_from(config_size).unwrap();
    let mut layout = Vec::new();
    layout.extend_from_slice(&24_u32.to_be_bytes());
    layout.extend_from_slice(b"chan");
    layout.extend_from_slice(&0_u32.to_be_bytes());
    layout.extend_from_slice(&0x0065_0002_u32.to_be_bytes());
    layout.extend_from_slice(&0_u32.to_be_bytes());
    layout.extend_from_slice(&0_u32.to_be_bytes());
    assert_eq!(layout.len(), 24);

    for kind in [
        b"moov".as_slice(),
        b"trak",
        b"mdia",
        b"minf",
        b"stbl",
        b"stsd",
    ] {
        let box_type = find(&bytes, kind);
        let size_offset = box_type - 4;
        let size = u32::from_be_bytes(bytes[size_offset..box_type].try_into().unwrap());
        bytes[size_offset..box_type].copy_from_slice(&(size + 24).to_be_bytes());
    }
    let outer_size = u32::from_be_bytes(bytes[outer_type - 4..outer_type].try_into().unwrap());
    bytes[outer_type - 4..outer_type].copy_from_slice(&(outer_size + 24).to_be_bytes());
    bytes[config_start..config_type].copy_from_slice(&(config_size + 24).to_be_bytes());
    bytes.splice(insertion..insertion, layout);

    let file = TestFile::new("m4a", &bytes);
    let (explicit_source, explicit_samples) = decode_all_samples(file.path());
    let (original_source, original_samples) = decode_all_samples(&original_path);
    assert_eq!(explicit_source.container, ContainerFormat::Mp4);
    assert_eq!(explicit_source.codec, SourceCodec::Alac);
    assert_eq!(explicit_source.sample_rate, original_source.sample_rate);
    assert_eq!(explicit_source.channels, original_source.channels);
    assert_eq!(
        explicit_source.expected_frames,
        original_source.expected_frames
    );
    assert_eq!(
        explicit_samples
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        original_samples
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn alac_route_accepts_an_absent_edit_list() {
    fn find(bytes: &[u8], needle: &[u8]) -> usize {
        bytes
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap_or_else(|| panic!("missing {needle:?}"))
    }

    let original_path = alac_fixture_path("alac16-mono-44100.m4a");
    let mut bytes = fs::read(&original_path).unwrap();
    let edit_type = find(&bytes, b"edts");
    let edit_start = edit_type - 4;
    let edit_size = u32::from_be_bytes(bytes[edit_start..edit_type].try_into().unwrap());
    for kind in [b"moov".as_slice(), b"trak"] {
        let box_type = find(&bytes, kind);
        let size_offset = box_type - 4;
        let size = u32::from_be_bytes(bytes[size_offset..box_type].try_into().unwrap());
        bytes[size_offset..box_type].copy_from_slice(&(size - edit_size).to_be_bytes());
    }
    bytes.drain(edit_start..edit_start + usize::try_from(edit_size).unwrap());

    let file = TestFile::new("m4a", &bytes);
    let (without_edit_source, without_edit_samples) = decode_all_samples(file.path());
    let (original_source, original_samples) = decode_all_samples(&original_path);
    assert_eq!(without_edit_source.container, ContainerFormat::Mp4);
    assert_eq!(without_edit_source.codec, SourceCodec::Alac);
    assert_eq!(
        without_edit_source.expected_frames,
        original_source.expected_frames
    );
    assert_eq!(
        without_edit_samples
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        original_samples
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn extensible_and_classic_twins_decode_to_bit_identical_pcm() {
    let manifest = extensible_fixture_manifest();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    for extensible in fixtures
        .iter()
        .filter(|fixture| fixture["encapsulation"] == "wave_format_extensible")
    {
        let twin_id = extensible["twinId"].as_str().unwrap();
        let classic = fixtures
            .iter()
            .find(|fixture| {
                fixture["twinId"] == twin_id && fixture["encapsulation"] == "classic_wave_format"
            })
            .unwrap_or_else(|| panic!("classic twin missing for {twin_id}"));
        let extensible_path = extensible_fixture_path(extensible["path"].as_str().unwrap());
        let classic_path = extensible_fixture_path(classic["path"].as_str().unwrap());
        let (extensible_source, extensible_samples) = decode_all_samples(&extensible_path);
        let (classic_source, classic_samples) = decode_all_samples(&classic_path);

        assert_eq!(
            extensible_source.container, classic_source.container,
            "{twin_id}"
        );
        assert_eq!(extensible_source.codec, classic_source.codec, "{twin_id}");
        assert_eq!(
            extensible_source.sample_rate, classic_source.sample_rate,
            "{twin_id}"
        );
        assert_eq!(
            extensible_source.channels, classic_source.channels,
            "{twin_id}"
        );
        assert_eq!(
            extensible_source.bits_per_sample, classic_source.bits_per_sample,
            "{twin_id}"
        );
        assert_eq!(
            extensible_source.expected_frames, classic_source.expected_frames,
            "{twin_id}"
        );
        assert_eq!(
            extensible_samples
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            classic_samples
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            "{twin_id}"
        );
    }
}

#[test]
fn rejects_non_finite_float_pcm_as_a_sticky_decode_error() {
    for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let file = TestFile::new("wav", &float32_wave(44_100, &[sample, 0.0]));
        let mut opened = DecoderFactory::new().open(file.path()).unwrap();

        let first = opened.reader.read_block().unwrap_err();
        assert_eq!(first.code, ErrorCode::DecodeFailed);
        assert!(first.message.contains("non-finite"));
        assert_eq!(opened.reader.read_block().unwrap_err(), first);
        assert!(!opened.reader.progress().is_eof());
    }
}

#[test]
fn rejects_aiff_payload_that_disagrees_with_declared_complete_frames() {
    let mut bytes = pcm16_aiff(44_100, &[1, 2]);
    bytes[4..8].copy_from_slice(&52_u32.to_be_bytes());
    bytes[42..46].copy_from_slice(&13_u32.to_be_bytes());
    bytes.extend_from_slice(&[3, 0]);
    let file = TestFile::new("aiff", &bytes);

    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert!(error.message.contains("declared complete frames"));
}

#[test]
fn rejects_overlong_aiff_comm_that_embeds_a_competing_ssnd_chunk() {
    let file = TestFile::new("aiff", &aiff_with_ssnd_embedded_in_overlong_comm());
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert_eq!(error.stage, AnalysisStage::Probe);
    assert!(error.message.contains("exactly 18 bytes"));
}

#[test]
fn rejects_invalid_aiff_sample_rates_as_malformed() {
    let valid_significand = 44_100_u64 << 48;
    let invalid_rates = [
        ("zero", [0_u8; 10]),
        ("negative", extended80_bytes(0xc00e, valid_significand)),
        ("infinity", extended80_bytes(0x7fff, 1_u64 << 63)),
        ("nan", extended80_bytes(0x7fff, (1_u64 << 63) | 1)),
        ("unnormal", extended80_bytes(0x400e, 1)),
        ("pseudo-denormal", extended80_bytes(0, 1_u64 << 63)),
    ];

    for (name, rate) in invalid_rates {
        let mut bytes = pcm16_aiff(44_100, &[1, -1]);
        bytes[28..38].copy_from_slice(&rate);
        let file = TestFile::new("aiff", &bytes);
        let error = expect_open_error(file.path());
        assert_eq!(error.code, ErrorCode::MalformedMedia, "{name}");
        assert_eq!(error.stage, AnalysisStage::Probe, "{name}");
        assert!(
            error.message.contains("valid finite positive"),
            "{name}: {}",
            error.message
        );
    }
}

#[test]
fn rejects_valid_but_unavailable_aiff_sample_rates_as_unsupported() {
    let valid_significand = 44_100_u64 << 48;
    let unavailable_rates = [
        ("positive subnormal", extended80_bytes(0, 1)),
        ("u32 overflow", extended80_bytes(0x401f, 1_u64 << 63)),
        (
            "large overflow with low residue",
            extended80_bytes(0x403f, (1_u64 << 63) | 1),
        ),
        (
            "fractional",
            extended80_bytes(0x400e, valid_significand | (1_u64 << 47)),
        ),
    ];

    for (name, rate) in unavailable_rates {
        let mut bytes = pcm16_aiff(44_100, &[1, -1]);
        bytes[28..38].copy_from_slice(&rate);
        let file = TestFile::new("aiff", &bytes);
        let error = expect_open_error(file.path());
        assert_eq!(error.code, ErrorCode::UnsupportedFormat, "{name}");
        assert_eq!(error.stage, AnalysisStage::Probe, "{name}");
        assert!(
            error.message.contains("positive integral u32"),
            "{name}: {}",
            error.message
        );
    }
}

#[test]
fn rejects_nonzero_aiff_ssnd_offset_and_block_size_as_unsupported() {
    for field in [46..50, 50..54] {
        let mut bytes = pcm16_aiff(44_100, &[1, -1]);
        bytes[field].copy_from_slice(&1_u32.to_be_bytes());
        let file = TestFile::new("aiff", &bytes);
        let error = expect_open_error(file.path());
        assert_eq!(error.code, ErrorCode::UnsupportedFormat);
        assert_eq!(error.stage, AnalysisStage::Probe);
        assert!(error.message.contains("SSND offset or block size"));
    }
}

#[test]
fn symphonia_terminal_error_is_sticky_and_freezes_observable_state() {
    let frames = multiblock_pcm16_wave_frames();
    let file = TestFile::new("wav", &pcm16_wave(48_000, &frames));
    let mut reader = crate::symphonia_source::open_test_source(file.path()).unwrap();

    let first_block = match reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("multi-block fault fixture unexpectedly returned EOF"),
    };
    assert!(
        first_block.frames() < frames.len(),
        "fault fixture must leave unread backend data"
    );
    let immutable_info = reader.stream_info().clone();
    let frozen_progress = reader.progress();
    let frozen_diagnostics = reader.diagnostics().clone();
    assert!(!frozen_progress.is_eof());

    let injected = AnalysisError::new(
        ErrorCode::DecodeFailed,
        AnalysisStage::Decode,
        "deterministic injected decoder failure",
    )
    .with_display_path(file.path().display().to_string())
    .with_backend("symphonia-test")
    .with_details("fault=after_first_data")
    .recoverable(true);
    reader.inject_error_on_next_read(injected.clone());

    assert_eq!(reader.read_block().unwrap_err(), injected);
    for repeated_read in 1..=3 {
        assert_eq!(
            reader.read_block().unwrap_err(),
            injected,
            "terminal error changed on repeated read {repeated_read}"
        );
        assert_eq!(reader.stream_info(), &immutable_info);
        assert_eq!(reader.progress(), frozen_progress);
        assert_eq!(reader.diagnostics(), &frozen_diagnostics);
    }
    assert!(!reader.progress().is_eof());
}

#[test]
fn rejected_overrun_block_is_not_committed_to_progress_or_diagnostics() {
    let frames = [[1_i16, -1_i16], [2, -2], [3, -3]];
    let file = TestFile::new("wav", &pcm16_wave(48_000, &frames));
    let mut reader = crate::symphonia_source::open_test_source(file.path()).unwrap();
    reader.override_expected_frames(Some(2));

    let error = reader.read_block().unwrap_err();
    assert_eq!(error.code, ErrorCode::DecodeFailed);
    assert_eq!(error.stage, AnalysisStage::Decode);
    assert!(error.message.contains("exceeds the expected frame count"));
    let progress = reader.progress();
    assert_eq!(progress.decoded_frames(), 0);
    assert_eq!(progress.expected_frames(), Some(2));
    assert_eq!(progress.fraction(), Some(0.0));
    assert!(!progress.is_eof());
    let diagnostics = reader.diagnostics().clone();
    assert_eq!(diagnostics.decoded_frames, 0);
    assert_eq!(diagnostics.warnings.len(), 1);

    assert_eq!(reader.read_block().unwrap_err(), error);
    assert_eq!(reader.progress(), progress);
    assert_eq!(reader.diagnostics(), &diagnostics);
}

#[test]
fn corrupt_flac_is_a_sticky_error_not_eof() {
    let mut corrupt = fs::read(product_fixture_path("flac-pcm-s16-stereo-multiblock.flac"))
        .expect("canonical FLAC fixture must be committed");
    let last = corrupt.last_mut().unwrap();
    *last ^= 0xff;
    let file = TestFile::new("flac", &corrupt);
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    let mut returned_frames = 0_u64;
    let first_error = loop {
        match opened.reader.read_block() {
            Ok(ReadOutcome::Data(block)) => {
                returned_frames += u64::try_from(block.frames()).unwrap();
            }
            Err(error) => break error,
            Ok(ReadOutcome::Eof) => panic!("corrupt FLAC became a successful EOF"),
        }
    };
    assert_eq!(first_error.code, ErrorCode::DecodeFailed);
    assert!(
        returned_frames > 0,
        "terminal-frame corruption must exercise Data followed by an error"
    );
    let terminal_progress = opened.reader.progress();
    let terminal_diagnostics = opened.reader.diagnostics().clone();
    assert_eq!(terminal_progress.decoded_frames(), returned_frames);
    assert_eq!(terminal_diagnostics.decoded_frames, returned_frames);
    assert!(!terminal_progress.is_eof());

    for repeated_read in 1..=2 {
        assert_eq!(
            opened.reader.read_block().unwrap_err(),
            first_error,
            "terminal decoder error changed on read {repeated_read}"
        );
        assert_eq!(opened.reader.progress(), terminal_progress);
        assert_eq!(opened.reader.diagnostics(), &terminal_diagnostics);
    }
}

#[test]
fn corrupt_alac_packet_is_a_sticky_error_not_eof() {
    let path = malformed_fixture_path("alac-corrupt-first-packet.m4a");
    let mut opened = DecoderFactory::new().open(&path).unwrap();

    let first_error = opened.reader.read_block().unwrap_err();
    assert_eq!(first_error.code, ErrorCode::DecodeFailed);
    assert_eq!(first_error.stage, AnalysisStage::Decode);
    let terminal_progress = opened.reader.progress();
    let terminal_diagnostics = opened.reader.diagnostics().clone();
    assert_eq!(terminal_progress.decoded_frames(), 0);
    assert!(!terminal_progress.is_eof());

    for repeated_read in 1..=2 {
        assert_eq!(
            opened.reader.read_block().unwrap_err(),
            first_error,
            "terminal decoder error changed on read {repeated_read}"
        );
        assert_eq!(opened.reader.progress(), terminal_progress);
        assert_eq!(opened.reader.diagnostics(), &terminal_diagnostics);
    }
}

#[test]
fn truncated_wave_is_rejected_before_partial_decode() {
    let mut truncated = pcm16_wave(48_000, &[[1, -1], [2, -2], [3, -3]]);
    truncated.truncate(truncated.len() - 4);
    let file = TestFile::new("wav", &truncated);
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert_eq!(error.stage, macinmeter_domain::AnalysisStage::Probe);
}

#[test]
fn rejects_incoherent_classic_wave_geometry_during_probe() {
    let base = pcm16_wave(48_000, &[[1, -1], [2, -2], [3, -3]]);
    for (name, field, replacement) in [
        ("byte rate", 28..32, 1_u32.to_le_bytes().to_vec()),
        ("block align", 32..34, 2_u16.to_le_bytes().to_vec()),
    ] {
        let mut bytes = base.clone();
        bytes[field].copy_from_slice(&replacement);
        let file = TestFile::new("wav", &bytes);
        let error = expect_open_error(file.path());
        assert_eq!(error.code, ErrorCode::MalformedMedia, "{name}");
        assert_eq!(error.stage, AnalysisStage::Probe, "{name}");
        assert!(error.message.contains("PCM geometry"), "{name}");
    }
}

#[test]
fn accepts_wave_format_extensible_with_exact_supported_fields() {
    let file = TestFile::new("wav", &extensible_pcm16_wave());
    let opened = DecoderFactory::new()
        .open(file.path())
        .expect("supported WAVE_FORMAT_EXTENSIBLE should open");
    assert_eq!(opened.source.codec, SourceCodec::PcmInteger);
    assert_eq!(opened.source.channels.get(), 2);
    assert_eq!(opened.source.bits_per_sample, Some(16));
}

#[test]
fn extensible_probe_uses_the_adr_0012_error_classification() {
    let pcm_guid = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b,
        0x71,
    ];
    let mut unknown_guid = pcm_guid;
    unknown_guid[0] = 0x02;
    let mut wrong_guid_tail = pcm_guid;
    wrong_guid_tail[15] ^= 0x01;

    for (name, bytes, expected_code, message) in [
        (
            "unsupported sub-format GUID",
            extensible_wave_with_fields(2, 16, 40, 22, 16, 3, unknown_guid),
            ErrorCode::UnsupportedFormat,
            "sub-format GUID",
        ),
        (
            "GUID template tail mismatch",
            extensible_wave_with_fields(2, 16, 40, 22, 16, 3, wrong_guid_tail),
            ErrorCode::UnsupportedFormat,
            "sub-format GUID",
        ),
        (
            "truncated fmt",
            extensible_wave_with_fields(2, 16, 39, 22, 16, 3, pcm_guid),
            ErrorCode::MalformedMedia,
            "truncated or internally inconsistent",
        ),
        (
            "coherent extra extension",
            extensible_wave_with_fields(2, 16, 42, 24, 16, 3, pcm_guid),
            ErrorCode::UnsupportedFormat,
            "unsupported extra extensions",
        ),
        (
            "incoherent extension size",
            extensible_wave_with_fields(2, 16, 40, 23, 16, 3, pcm_guid),
            ErrorCode::MalformedMedia,
            "truncated or internally inconsistent",
        ),
        (
            "zero valid bits",
            extensible_wave_with_fields(2, 16, 40, 22, 0, 3, pcm_guid),
            ErrorCode::UnsupportedFormat,
            "valid bits",
        ),
        (
            "padded valid bits",
            extensible_wave_with_fields(2, 16, 40, 22, 15, 3, pcm_guid),
            ErrorCode::UnsupportedFormat,
            "valid bits",
        ),
        (
            "valid bits exceed container",
            extensible_wave_with_fields(2, 16, 40, 22, 17, 3, pcm_guid),
            ErrorCode::MalformedMedia,
            "valid bits exceed",
        ),
        (
            "reserved channel-mask bit",
            extensible_wave_with_fields(2, 16, 40, 22, 16, 0x0004_0001, pcm_guid),
            ErrorCode::UnsupportedFormat,
            "reserved speaker bits",
        ),
        (
            "channel-mask popcount mismatch",
            extensible_wave_with_fields(2, 16, 40, 22, 16, 1, pcm_guid),
            ErrorCode::MalformedMedia,
            "disagrees with the channel count",
        ),
    ] {
        let file = TestFile::new("wav", &bytes);
        let error = expect_open_error(file.path());
        assert_eq!(error.code, expected_code, "{name}");
        assert_eq!(error.stage, AnalysisStage::Probe, "{name}");
        assert!(error.message.contains(message), "{name}: {error}");
    }

    for channels in [27, 32, 64] {
        for channel_mask in [0, 1, 0x8000_0000] {
            let file = TestFile::new(
                "wav",
                &extensible_wave_with_fields(channels, 16, 40, 22, 16, channel_mask, pcm_guid),
            );
            let error = expect_open_error(file.path());
            assert_eq!(
                error.code,
                ErrorCode::UnsupportedFormat,
                "{channels}ch mask 0x{channel_mask:08x}"
            );
            assert_eq!(error.stage, AnalysisStage::Probe, "{channels}ch");
            assert!(error.message.contains("backend channel limit"), "{error}");
        }
    }
}

#[test]
fn rejects_backend_codec_identity_that_disagrees_with_first_party_probe() {
    let path = Path::new("codec-identity.wav");
    let validated = crate::container::ContainerPcmInfo {
        sample_rate: 48_000,
        channels: 2,
        bits_per_sample: 16,
        source_codec: SourceCodec::PcmInteger,
    };
    let stream_spec = macinmeter_domain::StreamSpec::new(48_000, 2, ChannelLayout::Unknown)
        .expect("test stream geometry is valid");
    let error = crate::symphonia_source::validate_backend_pcm_metadata(
        path,
        validated,
        SourceCodec::PcmFloat,
        &stream_spec,
        Some(16),
    )
    .expect_err("backend codec identity mismatch must be rejected");
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert_eq!(error.stage, AnalysisStage::Probe);
    assert!(error.message.contains("decoder metadata disagrees"));
    assert!(
        error.details.as_deref().is_some_and(|details| {
            details.contains("PcmInteger") && details.contains("PcmFloat")
        })
    );
}

#[test]
fn rejects_a_complete_wave_chunk_with_a_partial_pcm_frame() {
    let mut bytes = wave_header(1, 2, 48_000, 16, 5);
    bytes[4..8].copy_from_slice(&42_u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 2, 3, 4, 0]);
    let file = TestFile::new("wav", &bytes);

    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert!(error.message.contains("complete PCM frames"));
}

#[test]
fn rejects_aifc_before_symphonia_probe() {
    let file = TestFile::new("aiff", b"FORM\0\0\0\x04AIFC");
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    assert!(error.message.contains("AIFC"));
}

#[test]
fn rejects_unknown_content_even_with_supported_extension() {
    let file = TestFile::new("wav", b"this is not an audio file");
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
}

#[test]
fn classifies_partial_supported_signatures_as_malformed() {
    for prefix in [
        b"f".as_slice(),
        b"fL",
        b"fLa",
        b"R",
        b"RIFF",
        b"RIFF\0\0\0",
        b"FORM\0\0",
        b"\0\0\0\x18f",
        b"\0\0\0\x18ft",
        b"\0\0\0\x18fty",
    ] {
        let file = TestFile::new("bin", prefix);
        let error = expect_open_error(file.path());
        assert_eq!(error.code, ErrorCode::MalformedMedia, "prefix {prefix:?}");
    }

    let empty = TestFile::new("wav", b"");
    assert_eq!(
        expect_open_error(empty.path()).code,
        ErrorCode::UnsupportedFormat
    );
}

#[test]
fn rejects_non_linear_pcm_in_wave() {
    let file = TestFile::new("wav", &alaw_wave());
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
}

#[test]
fn classifies_zero_rate_media_as_malformed_probe_input() {
    let file = TestFile::new("wav", &pcm16_wave(0, &[[0, 0], [1, -1]]));
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert_eq!(error.stage, macinmeter_domain::AnalysisStage::Probe);
}

#[test]
fn recognized_but_invalid_flac_is_not_routed_by_extension() {
    let file = TestFile::new("wav", b"fLaCnot-a-valid-stream");
    let error = expect_open_error(file.path());
    assert_ne!(error.code, ErrorCode::InputNotFound);
    assert!(matches!(
        error.code,
        ErrorCode::MalformedMedia | ErrorCode::UnsupportedFormat
    ));
}

fn expect_open_error(path: &Path) -> AnalysisError {
    match DecoderFactory::new().open(path) {
        Ok(_) => panic!("generated invalid media unexpectedly opened"),
        Err(error) => error,
    }
}

fn normalize_pcm16(samples: &[i16]) -> Vec<f64> {
    samples
        .iter()
        .map(|sample| f64::from(*sample) / 32_768.0)
        .collect()
}

fn integer_contract_values(bits: u32) -> [i64; 8] {
    let minimum = -(1_i64 << (bits - 1));
    let maximum = (1_i64 << (bits - 1)) - 1;
    let half = 1_i64 << (bits - 2);
    let marker = (1_i64 << (bits - 3)) + 3;
    [minimum, maximum, -half, half, -1, 1, 0, marker]
}

fn normalize_integer_contract_samples(bits: u32) -> Vec<f64> {
    let divisor = (1_u64 << (bits - 1)) as f64;
    integer_contract_values(bits)
        .into_iter()
        .map(|sample| sample as f64 / divisor)
        .collect()
}

fn float32_contract_samples() -> Vec<f64> {
    [
        0.0_f32,
        -0.0,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        0.25,
        -0.5,
        1.5,
        -2.0,
    ]
    .into_iter()
    .map(f64::from)
    .collect()
}

fn float64_contract_samples() -> Vec<f64> {
    vec![
        0.0,
        -0.0,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        0.125 + f64::EPSILON,
        -0.75 + f64::EPSILON,
        1.0 - f64::EPSILON,
        -1.0 + f64::EPSILON,
    ]
}

fn flac_contract_samples() -> Vec<i16> {
    let mut samples = Vec::with_capacity(800);
    samples.extend_from_slice(&[i16::MIN, i16::MAX, 0, 0, -1, 1, -16_384, 16_384]);
    for index in 4_i32..400 {
        let left = ((index * 257 + 17) % 40_001) - 20_000;
        let right = ((index * 509 + 1_234) % 30_001) - 15_000;
        samples.push(i16::try_from(left).unwrap());
        samples.push(i16::try_from(right).unwrap());
    }
    samples
}

fn multiblock_pcm16_wave_frames() -> Vec<[i16; 2]> {
    let mut frames = Vec::with_capacity(2_305);
    for index in 0..2_305 {
        let sample = i16::try_from(index % 2_001).unwrap() - 1_000;
        frames.push([sample, -sample]);
    }
    frames[0] = [i16::MIN, i16::MAX];
    frames[1] = [0, 16_384];
    frames[2] = [-16_384, 1_000];
    frames
}

fn pcm16_wave(sample_rate: u32, frames: &[[i16; 2]]) -> Vec<u8> {
    let channels = 2_u16;
    let data_size = u32::try_from(frames.len() * channels as usize * 2).unwrap();
    let mut bytes = wave_header(1, channels, sample_rate, 16, data_size);
    for frame in frames {
        for sample in frame {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    bytes
}

fn float32_wave(sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let data_size = u32::try_from(samples.len() * 4).unwrap();
    let mut bytes = wave_header(3, 1, sample_rate, 32, data_size);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn extensible_pcm16_wave() -> Vec<u8> {
    let guid = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b,
        0x71,
    ];
    let mut bytes = extensible_wave_with_fields(2, 16, 40, 22, 16, 3, guid);
    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) + 8;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    bytes[64..68].copy_from_slice(&8_u32.to_le_bytes());
    for sample in [1_i16, -1, 2, -2] {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn extensible_wave_with_fields(
    channels: u16,
    bits_per_sample: u16,
    fmt_size: usize,
    extension_size: u16,
    valid_bits_per_sample: u16,
    channel_mask: u32,
    sub_format_guid: [u8; 16],
) -> Vec<u8> {
    assert!(fmt_size >= 16);
    let bytes_per_sample = bits_per_sample / 8;
    let block_align = channels.checked_mul(bytes_per_sample).unwrap();
    let byte_rate = 48_000_u32.checked_mul(u32::from(block_align)).unwrap();
    let mut format = Vec::with_capacity(fmt_size);
    format.extend_from_slice(&0xfffe_u16.to_le_bytes());
    format.extend_from_slice(&channels.to_le_bytes());
    format.extend_from_slice(&48_000_u32.to_le_bytes());
    format.extend_from_slice(&byte_rate.to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&bits_per_sample.to_le_bytes());
    format.extend_from_slice(&extension_size.to_le_bytes());
    format.extend_from_slice(&valid_bits_per_sample.to_le_bytes());
    format.extend_from_slice(&channel_mask.to_le_bytes());
    format.extend_from_slice(&sub_format_guid);
    format.resize(fmt_size, 0);
    format.truncate(fmt_size);

    let padded_fmt_size = fmt_size + (fmt_size & 1);
    let riff_size = u32::try_from(4 + 8 + padded_fmt_size + 8).unwrap();
    let mut bytes = Vec::with_capacity(riff_size as usize + 8);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&riff_size.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&u32::try_from(fmt_size).unwrap().to_le_bytes());
    bytes.extend_from_slice(&format);
    if fmt_size & 1 == 1 {
        bytes.push(0);
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

fn alaw_wave() -> Vec<u8> {
    let mut bytes = wave_header(6, 1, 8_000, 8, 4);
    bytes[4..8].copy_from_slice(&42_u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&18_u32.to_le_bytes());
    bytes.splice(36..36, [0, 0]);
    bytes.extend_from_slice(&[0, 1, 2, 3]);
    bytes
}

fn empty_pcm8_wave(channels: u16) -> Vec<u8> {
    wave_header(1, channels, 8_000, 8, 0)
}

fn wave_header(
    format_tag: u16,
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    data_size: u32,
) -> Vec<u8> {
    let bytes_per_sample = u32::from(bits_per_sample) / 8;
    let block_align = channels * u16::try_from(bytes_per_sample).unwrap();
    let byte_rate = sample_rate * u32::from(block_align);
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&format_tag.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes
}

fn empty_pcm8_aiff(channels: u16) -> Vec<u8> {
    let form_size = 4_u32 + (8 + 18) + (8 + 8);
    let mut bytes = Vec::with_capacity((form_size + 8) as usize);
    bytes.extend_from_slice(b"FORM");
    bytes.extend_from_slice(&form_size.to_be_bytes());
    bytes.extend_from_slice(b"AIFF");
    bytes.extend_from_slice(b"COMM");
    bytes.extend_from_slice(&18_u32.to_be_bytes());
    bytes.extend_from_slice(&channels.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&8_u16.to_be_bytes());
    bytes.extend_from_slice(&[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(b"SSND");
    bytes.extend_from_slice(&8_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes
}

fn extended80_bytes(sign_exponent: u16, significand: u64) -> [u8; 10] {
    let mut bytes = [0_u8; 10];
    bytes[..2].copy_from_slice(&sign_exponent.to_be_bytes());
    bytes[2..].copy_from_slice(&significand.to_be_bytes());
    bytes
}

fn pcm16_aiff(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    assert_eq!(sample_rate, 44_100, "test helper only encodes 44.1 kHz");
    let data_size = u32::try_from(samples.len() * 2).unwrap();
    let form_size = 4 + (8 + 18) + (8 + 8 + data_size);
    let mut bytes = Vec::with_capacity((form_size + 8) as usize);
    bytes.extend_from_slice(b"FORM");
    bytes.extend_from_slice(&form_size.to_be_bytes());
    bytes.extend_from_slice(b"AIFF");
    bytes.extend_from_slice(b"COMM");
    bytes.extend_from_slice(&18_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&u32::try_from(samples.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&16_u16.to_be_bytes());
    bytes.extend_from_slice(&[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(b"SSND");
    bytes.extend_from_slice(&(8 + data_size).to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_be_bytes());
    }
    bytes
}

fn aiff_with_ssnd_embedded_in_overlong_comm() -> Vec<u8> {
    let mut bytes = pcm16_aiff(44_100, &[i16::MIN]);
    let mut competing_ssnd = Vec::new();
    competing_ssnd.extend_from_slice(b"SSND");
    competing_ssnd.extend_from_slice(&10_u32.to_be_bytes());
    competing_ssnd.extend_from_slice(&0_u32.to_be_bytes());
    competing_ssnd.extend_from_slice(&0_u32.to_be_bytes());
    competing_ssnd.extend_from_slice(&i16::MAX.to_be_bytes());

    let original_form_size = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    let extra_size = u32::try_from(competing_ssnd.len()).unwrap();
    bytes[4..8].copy_from_slice(&(original_form_size + extra_size).to_be_bytes());
    bytes[16..20].copy_from_slice(&(18 + extra_size).to_be_bytes());
    bytes.splice(38..38, competing_ssnd);
    bytes
}

#[test]
fn container_parsers_consume_in_memory_bytes_through_the_byte_seam() {
    use crate::container::{ContainerSignature, identify_container, inspect_aiff, inspect_wave};
    use std::io::Cursor;

    let path = Path::new("in-memory.bin");

    let wave = pcm16_wave(48_000, &multiblock_pcm16_wave_frames());
    let mut cursor = Cursor::new(wave.as_slice());
    assert_eq!(
        identify_container(&mut cursor, path).unwrap(),
        ContainerSignature::Wave
    );
    let info = inspect_wave(&mut cursor, path).unwrap();
    assert_eq!(info.pcm.sample_rate, 48_000);
    assert_eq!(info.pcm.channels, 2);
    assert_eq!(
        info.declared_frames,
        multiblock_pcm16_wave_frames().len() as u64
    );

    let aiff = pcm16_aiff(44_100, &[i16::MIN, i16::MAX]);
    let mut cursor = Cursor::new(aiff.as_slice());
    assert_eq!(
        identify_container(&mut cursor, path).unwrap(),
        ContainerSignature::Aiff
    );
    let info = inspect_aiff(&mut cursor, path).unwrap();
    assert_eq!(info.pcm.sample_rate, 44_100);
    assert_eq!(info.pcm.bits_per_sample, 16);

    // Structural failures surface directly from bytes, without any file.
    let mut truncated = Cursor::new(&wave[..30]);
    assert_eq!(
        identify_container(&mut truncated, path).unwrap(),
        ContainerSignature::Wave
    );
    let error = inspect_wave(&mut truncated, path).unwrap_err();
    assert_eq!(error.code, ErrorCode::MalformedMedia);
    assert_eq!(error.stage, AnalysisStage::Probe);
}

#[test]
fn flac_without_a_declared_total_sample_count_is_rejected_at_probe() {
    let source = fs::read(product_fixture_path("flac-pcm-s16-stereo-multiblock.flac"))
        .expect("committed FLAC fixture must exist");

    // Zero the 36-bit STREAMINFO total-sample count and the MD5 signature so
    // neither the end-of-stream frame check nor decoder verification could
    // observe a lost tail frame.
    let mut unverifiable = source.clone();
    unverifiable[21] &= 0xF0;
    unverifiable[22..26].fill(0);
    unverifiable[26..42].fill(0);

    // Both the full stream and a stream truncated exactly on a frame boundary
    // must be rejected before any decode: the truncated variant previously
    // produced a silent partial success.
    let frame_boundary = 916;
    for (name, bytes) in [
        ("full", unverifiable.clone()),
        (
            "boundary-truncated",
            unverifiable[..frame_boundary].to_vec(),
        ),
    ] {
        let file = TestFile::new("flac", &bytes);
        let error = match DecoderFactory::new().open(file.path()) {
            Err(error) => error,
            Ok(_) => panic!("unknown-total FLAC must not open ({name})"),
        };
        assert_eq!(error.code, ErrorCode::UnsupportedFormat, "{name} code");
        assert_eq!(error.stage, AnalysisStage::Probe, "{name} stage");
    }

    // The same boundary truncation with the declared count intact keeps
    // failing through the end-of-stream frame check.
    let file = TestFile::new("flac", &source[..frame_boundary]);
    let mut opened = DecoderFactory::new()
        .open(file.path())
        .expect("declared-count FLAC opens");
    let error = loop {
        match opened.reader.read_block() {
            Ok(ReadOutcome::Data(_)) => continue,
            Ok(ReadOutcome::Eof) => panic!("boundary truncation must not reach EOF"),
            Err(error) => break error,
        }
    };
    assert_eq!(error.code, ErrorCode::DecodeFailed);
    assert_eq!(error.stage, AnalysisStage::Decode);
}

// ADR-0014 step 2: the ALAC packet workers. Production still runs on the serial
// plan, so these tests drive the parallel engine through an explicit
// reservation and hold it to the serial oracle's exact results.

#[test]
fn alac_packet_workers_decode_bit_identically_at_every_worker_count() {
    let names = alac_fixture_names();
    let started = started_worker_pools(|| {
        for name in &names {
            let path = alac_fixture_path(name);
            let (oracle_source, oracle_samples) = decode_all_samples(&path);
            let oracle_bits = raw_bits(&oracle_samples);

            for workers in ALAC_WORKER_COUNTS {
                let (source, samples) = decode_all_samples_with(&path, worker_reservation(workers));
                assert_eq!(
                    raw_bits(&samples),
                    oracle_bits,
                    "{name} decoded different raw f64 bits on {workers} workers"
                );
                assert_eq!(
                    source, oracle_source,
                    "{name} reported different source metadata on {workers} workers"
                );
            }
        }
    });
    assert_eq!(
        started,
        names.len() * ALAC_WORKER_COUNTS.len(),
        "every parallel case must actually have run on packet workers"
    );
}

#[test]
fn a_single_worker_reservation_stays_on_the_serial_route() {
    // A worker count of one must degrade before decoding starts, even when the
    // reservation would otherwise permit a deeper queue.
    let reservation = DecodeReservation::new(
        NonZeroUsize::new(1).unwrap(),
        NonZeroUsize::new(4).unwrap(),
        4 * 1024 * 1024,
    )
    .unwrap();

    let started = started_worker_pools(|| {
        for name in alac_fixture_names() {
            let path = alac_fixture_path(&name);
            let (oracle_source, oracle_samples) = decode_all_samples(&path);
            let (source, samples) = decode_all_samples_with(&path, reservation);
            assert_eq!(raw_bits(&samples), raw_bits(&oracle_samples), "{name}");
            assert_eq!(source, oracle_source, "{name}");
        }
    });
    assert_eq!(started, 0, "one worker must not start a pool at all");
}

#[test]
fn only_graduated_routes_create_packet_workers() {
    // ADR-0014 forbids inferring packet independence from an extension or a
    // generic codec descriptor. A route that has not graduated must ignore a
    // multi-worker reservation entirely and still produce identical PCM. FLAC
    // is deliberately absent here: it graduated alongside the product's own
    // in-order stream verifier and has its own equivalence coverage.
    let reservation = worker_reservation(4);
    let started = started_worker_pools(|| {
        for name in [
            "native-pcm-v1/wav-pcm-s16-stereo.wav",
            "native-pcm-v1/wav-float64-stereo.wav",
            "native-pcm-v1/aiff-pcm-s24-stereo.aiff",
        ] {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures")
                .join(name);
            let (oracle_source, oracle_samples) = decode_all_samples(&path);
            let (source, samples) = decode_all_samples_with(&path, reservation);
            assert_eq!(raw_bits(&samples), raw_bits(&oracle_samples), "{name}");
            assert_eq!(source, oracle_source, "{name}");
        }
    });
    assert_eq!(
        started, 0,
        "a route that has not graduated must never start packet workers"
    );
}

#[test]
fn decoder_factory_reports_the_engine_selected_after_content_probe() {
    let alac = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    let wav = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/native-pcm-v1/wav-pcm-s16-stereo.wav");

    let (_, execution) = DecoderFactory::new().open_with_execution(&alac).unwrap();
    assert_eq!(execution.engine(), DecodeEngineKind::Serial);
    assert_eq!(execution.workers().get(), 1);

    let reservation = worker_reservation(4);
    let (_, execution) = DecoderFactory::with_application_reservation(reservation)
        .open_with_execution(&wav)
        .unwrap();
    assert_eq!(execution.engine(), DecodeEngineKind::Serial);
    assert_eq!(execution.workers().get(), 1);

    let (_, execution) = DecoderFactory::with_application_reservation(reservation)
        .open_with_execution(&alac)
        .unwrap();
    assert_eq!(execution.engine(), DecodeEngineKind::AlacPacketWorkers);
    assert_eq!(execution.workers(), reservation.workers());
}

#[test]
fn a_corrupt_alac_packet_fails_identically_at_every_worker_count() {
    let path = malformed_fixture_path("alac-corrupt-first-packet.m4a");
    let oracle_error = {
        let mut opened = DecoderFactory::new().open(&path).unwrap();
        opened.reader.read_block().unwrap_err()
    };
    assert_eq!(oracle_error.code, ErrorCode::DecodeFailed);
    assert_eq!(oracle_error.stage, AnalysisStage::Decode);

    // The corrupt packet is index 0. The instance-owned injection holds its
    // outcome until a later packet has been published, so successful work may
    // not overtake the earlier failure or turn it into data or EOF.
    for force_reordering in [false, true] {
        for workers in ALAC_WORKER_COUNTS {
            let options = if force_reordering {
                crate::decode_engine::PoolOptions::force_first_result_after_later()
            } else {
                crate::decode_engine::PoolOptions::default()
            };
            let mut reader = crate::symphonia_source::open_test_source_with_pool_options(
                &path,
                worker_reservation(workers),
                options,
            )
            .unwrap();
            let error = reader
                .read_block()
                .expect_err("a corrupt packet must never be skipped");
            let terminal_progress = reader.progress();
            let repeated: Vec<_> = (0..2)
                .map(|_| (reader.read_block(), reader.progress()))
                .collect();

            let case = format!("{workers} workers, force_reordering={force_reordering}");
            assert_eq!(
                error, oracle_error,
                "{case} changed the failing packet's identity"
            );
            assert_eq!(terminal_progress.decoded_frames(), 0, "{case}");
            assert!(!terminal_progress.is_eof(), "{case}");
            for (repeated_read, progress) in repeated {
                assert_eq!(
                    repeated_read.unwrap_err(),
                    error,
                    "{case} lost sticky terminal state"
                );
                assert_eq!(progress, terminal_progress, "{case}");
            }
        }
    }
}

#[test]
fn worker_progress_stays_monotonic_and_reaches_eof_only_after_every_commit() {
    let path = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    let expected_frames = alac_fixture_manifest()["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fixture| fixture["path"] == "alac16-stereo-48000-multipacket.m4a")
        .unwrap()["frames"]
        .as_u64()
        .unwrap();

    for workers in ALAC_WORKER_COUNTS {
        let mut opened = DecoderFactory::with_application_reservation(worker_reservation(workers))
            .open(&path)
            .unwrap();
        let mut previous = 0;
        let mut blocks = 0;
        while let ReadOutcome::Data(block) =
            opened.reader.read_block().expect("fixture should decode")
        {
            blocks += 1;
            let progress = opened.reader.progress();
            assert!(
                progress.decoded_frames() > previous,
                "{workers} workers published non-monotonic progress"
            );
            assert!(
                !progress.is_eof(),
                "{workers} workers reported EOF while data remained"
            );
            previous = progress.decoded_frames();
            assert!(block.frames() > 0);
        }
        assert!(blocks > 1, "this fixture must exercise multiple packets");
        let progress = opened.reader.progress();
        assert!(progress.is_eof());
        assert_eq!(
            progress.decoded_frames(),
            expected_frames,
            "{workers} workers committed a different frame count"
        );
    }
}

#[test]
fn dropping_a_worker_source_early_joins_every_thread() {
    // Abandoning a source mid-stream must not leave decoding running behind it.
    // A leaked thread would keep the channel alive and hang this test.
    let path = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    for workers in ALAC_WORKER_COUNTS {
        let mut opened = DecoderFactory::with_application_reservation(worker_reservation(workers))
            .open(&path)
            .unwrap();
        assert!(matches!(
            opened.reader.read_block().unwrap(),
            ReadOutcome::Data(_)
        ));
        drop(opened);
    }
}

#[test]
fn forced_out_of_order_completion_still_commits_identical_pcm() {
    // Hold packet zero at the engine boundary until a later result has been
    // published. This deterministically exercises the production reorder and
    // commit path without relying on thread timing.
    let path = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    let (_, oracle_samples) = decode_all_samples(&path);
    let oracle_bits = raw_bits(&oracle_samples);

    for workers in ALAC_WORKER_COUNTS {
        let mut reader = crate::symphonia_source::open_test_source_with_pool_options(
            &path,
            worker_reservation(workers),
            crate::decode_engine::PoolOptions::force_first_result_after_later(),
        )
        .unwrap();
        let mut samples = Vec::new();
        let outcome = loop {
            match reader.read_block() {
                Ok(ReadOutcome::Data(block)) => samples.extend_from_slice(block.samples()),
                Ok(ReadOutcome::Eof) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        let stalled = reader.stalled_accepts();

        outcome.unwrap_or_else(|error| panic!("{workers} workers failed to decode: {error}"));
        assert_eq!(
            raw_bits(&samples),
            oracle_bits,
            "{workers} workers changed the PCM under forced reordering"
        );
        assert!(
            stalled > 0,
            "{workers} workers never reordered, so this proved nothing"
        );
    }
}

#[test]
fn alac_workers_honor_minimum_and_maximum_queue_reservations() {
    let path = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    let (oracle_source, oracle_samples) = decode_all_samples(&path);
    let oracle_bits = raw_bits(&oracle_samples);

    let started = started_worker_pools(|| {
        for workers in ALAC_WORKER_COUNTS {
            for queue_capacity in [workers, MAX_DECODE_QUEUE_CAPACITY] {
                let reservation = worker_reservation_with_queue(workers, queue_capacity);
                let expected_depth = if queue_capacity == workers { 0 } else { 2 };
                assert_eq!(
                    crate::decode_engine::dispatch_depth(reservation),
                    expected_depth,
                    "inbox depth widened the queue_capacity={queue_capacity} permit"
                );
                let (source, samples) = decode_all_samples_with(&path, reservation);
                assert_eq!(
                    raw_bits(&samples),
                    oracle_bits,
                    "{workers} workers changed PCM with queue_capacity={queue_capacity}"
                );
                assert_eq!(
                    source, oracle_source,
                    "{workers} workers changed metadata with queue_capacity={queue_capacity}"
                );
            }
        }
    });
    assert_eq!(
        started,
        ALAC_WORKER_COUNTS.len() * 2,
        "every queue-bound case must run on packet workers"
    );
}

#[test]
fn pool_spawn_failures_join_every_thread_started_during_construction() {
    let path = alac_fixture_path("alac16-stereo-48000-multipacket.m4a");
    let reservation = worker_reservation(4);
    let joined = &crate::decode_engine::FAILED_START_JOINED_THREADS;

    for (options, expected_joins, expected_message) in [
        (
            crate::decode_engine::PoolOptions::fail_worker_spawn(2),
            1,
            "failed to start a packet decode worker on the ALAC route",
        ),
        (
            crate::decode_engine::PoolOptions::fail_demux_spawn(),
            3,
            "failed to start the demux thread on the ALAC route",
        ),
    ] {
        let before = joined.with(std::cell::Cell::get);
        let error = match crate::symphonia_source::open_test_source_with_pool_options(
            &path,
            reservation,
            options,
        ) {
            Ok(_) => panic!("the injected thread creation failure unexpectedly opened"),
            Err(error) => error,
        };
        let joined_during_open = joined.with(std::cell::Cell::get) - before;

        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Decode);
        assert_eq!(error.message, expected_message);
        assert_eq!(
            joined_during_open, expected_joins,
            "open returned before every previously started thread was joined"
        );
    }
}

#[test]
fn both_unrepresentable_rate_sentinels_are_accepted_for_high_sample_rates() {
    // The AudioSampleEntry rate is 16.16 fixed point, so 96 kHz cannot be
    // written there. ffmpeg leaves the field zero; other writers store the
    // fixed-point value 1.0. Both spellings must reach the same PCM.
    let manifest = alac_fixture_manifest();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    let mut seen = BTreeSet::new();
    for fixture in fixtures.iter().filter(|fixture| fixture["kind"] == "alac") {
        let name = fixture["path"].as_str().unwrap();
        let entry_rate = fixture["isoBmff"]["sampleEntry"]["sampleRateFixed16_16"]
            .as_str()
            .unwrap();
        let cookie_rate = fixture["cookie"]["sampleRate"].as_u64();
        let cookie_rate = cookie_rate
            .or_else(|| fixture["isoBmff"]["cookie"]["sampleRate"].as_u64())
            .unwrap();
        if cookie_rate <= u64::from(u16::MAX) {
            assert_ne!(
                entry_rate, "0x00000000",
                "{name} may not use a sentinel it does not need"
            );
            assert_ne!(entry_rate, "0x00010000", "{name}");
            continue;
        }
        seen.insert(entry_rate.to_owned());
        let (source, samples) = decode_all_samples(&alac_fixture_path(name));
        assert_eq!(source.sample_rate.get() as u64, cookie_rate, "{name}");
        let twin = fixture["twin"].as_str().unwrap();
        let (_, expected) = decode_all_samples(&alac_fixture_path(twin));
        assert_eq!(raw_bits(&samples), raw_bits(&expected), "{name}");
    }
    assert_eq!(
        seen,
        BTreeSet::from(["0x00000000".to_owned(), "0x00010000".to_owned()]),
        "the corpus must cover both sentinel spellings"
    );
}

#[test]
fn a_rate_sentinel_is_rejected_when_the_cookie_rate_fits_the_field() {
    let path = malformed_fixture_path("alac-rate-sentinel-one-within-range.m4a");
    let error = expect_open_error(&path);
    assert_eq!(error.code, ErrorCode::MalformedMedia, "{error}");
    assert_eq!(error.stage, AnalysisStage::Probe, "{error}");
    assert!(
        error.message.contains("sample-rate declarations disagree"),
        "{error}"
    );
    assert_eq!(
        error.details.as_deref(),
        Some("sample_entry_rate=1; cookie_rate=48000"),
        "{error}"
    );
}

/// STREAMINFO's MD5 field, for a FLAC file whose first metadata block is
/// STREAMINFO: 4 signature bytes, a 4-byte block header, then 26 bytes of
/// STREAMINFO body before the digest.
const FLAC_STREAMINFO_MD5: std::ops::Range<usize> = 26..42;

/// Overwrite the 36-bit STREAMINFO total sample count.
fn patch_flac_total_samples(bytes: &mut [u8], frames: u64) {
    bytes[21] = (bytes[21] & 0xF0) | ((frames >> 32) & 0x0F) as u8;
    bytes[22] = (frames >> 24) as u8;
    bytes[23] = (frames >> 16) as u8;
    bytes[24] = (frames >> 8) as u8;
    bytes[25] = frames as u8;
}

/// Symphonia's own FLAC verdict and decoded frame count for the same bytes.
///
/// The product owns FLAC stream verification, so this drives the backend's
/// built-in validator separately and reports what it concluded. It is the
/// oracle the product verdict is compared against, not a second product path.
fn backend_flac_verdict(bytes: &[u8]) -> (Option<bool>, u64) {
    use symphonia::core::{
        codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
        meta::MetadataOptions, probe::Hint,
    };

    let media = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            media,
            &FormatOptions {
                enable_gapless: true,
                ..FormatOptions::default()
            },
            &MetadataOptions::default(),
        )
        .expect("the FLAC bytes must probe");
    let mut format = probed.format;
    let codec_params = format
        .tracks()
        .first()
        .expect("the FLAC bytes must carry a track")
        .codec_params
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions { verify: true })
        .expect("the FLAC bytes must have a decoder");

    let mut frames = 0;
    // Any demux or decode failure ends the run: this oracle reports what the
    // backend verified over the packets it actually reached.
    while let Ok(packet) = format.next_packet() {
        let Ok(decoded) = decoder.decode(&packet) else {
            break;
        };
        frames += decoded.frames() as u64;
    }
    (decoder.finalize().verify_ok, frames)
}

/// Read one FLAC stream through the product to end of stream.
fn product_flac_outcome(bytes: &[u8]) -> Result<u64, AnalysisError> {
    let file = TestFile::new("flac", bytes);
    let mut opened = DecoderFactory::new().open(file.path())?;
    let mut frames = 0;
    loop {
        match opened.reader.read_block()? {
            ReadOutcome::Data(block) => frames += block.frames() as u64,
            ReadOutcome::Eof => return Ok(frames),
        }
    }
}

#[test]
fn the_product_flac_verdict_matches_the_backend_on_an_intact_and_a_tampered_signature() {
    let intact = fs::read(product_fixture_path("flac-pcm-s16-stereo-multiblock.flac"))
        .expect("committed FLAC fixture must exist");

    let mut tampered = intact.clone();
    tampered[FLAC_STREAMINFO_MD5.start] ^= 0x01;

    for (name, bytes, backend_ok) in [
        ("intact", intact.clone(), true),
        ("tampered-signature", tampered, false),
    ] {
        let (verdict, backend_frames) = backend_flac_verdict(&bytes);
        assert_eq!(verdict, Some(backend_ok), "{name}: backend verdict");

        match (backend_ok, product_flac_outcome(&bytes)) {
            (true, Ok(frames)) => assert_eq!(frames, backend_frames, "{name}: decoded frames"),
            (false, Err(error)) => {
                assert_eq!(error.code, ErrorCode::DecodeFailed, "{name}: code");
                assert_eq!(error.stage, AnalysisStage::Decode, "{name}: stage");
            }
            (expected_ok, outcome) => {
                panic!("{name}: backend said {expected_ok}, product said {outcome:?}")
            }
        }
    }
}

#[test]
fn a_flac_stream_missing_its_tail_is_caught_by_the_signature_alone() {
    let source = fs::read(product_fixture_path("flac-pcm-s16-stereo-multiblock.flac"))
        .expect("committed FLAC fixture must exist");

    // Cut whole frames off the end, then rewrite the declared total sample
    // count so it agrees with what survives. The end-of-stream frame check is
    // now satisfied and only the stream signature can still notice the loss.
    let frame_boundary = 916;
    let mut truncated = source[..frame_boundary].to_vec();
    let (_, surviving_frames) = backend_flac_verdict(&truncated);
    assert!(
        surviving_frames > 0,
        "the truncation must leave decodable audio"
    );
    patch_flac_total_samples(&mut truncated, surviving_frames);

    let (verdict, rechecked_frames) = backend_flac_verdict(&truncated);
    assert_eq!(rechecked_frames, surviving_frames);
    assert_eq!(
        verdict,
        Some(false),
        "the backend oracle must also reject the shortened stream"
    );

    // Control: with the signature removed the same shortened stream reaches a
    // clean EOF, so nothing else in the product notices the missing audio.
    let mut unsigned = truncated.clone();
    unsigned[FLAC_STREAMINFO_MD5].fill(0);
    assert_eq!(
        product_flac_outcome(&unsigned).expect("no other check catches the loss"),
        surviving_frames
    );

    let error = product_flac_outcome(&truncated)
        .expect_err("a stream missing audio must not reach a clean EOF");
    assert_eq!(error.code, ErrorCode::DecodeFailed);
    assert_eq!(error.stage, AnalysisStage::Decode);
    assert!(
        error
            .details
            .as_deref()
            .is_some_and(|details| details.contains("decoded_md5=")),
        "the failure must name the digests it compared, got {:?}",
        error.details
    );
}

#[test]
fn a_flac_stream_without_a_signature_still_decodes() {
    let mut unsigned = fs::read(product_fixture_path("flac-pcm-s16-stereo-multiblock.flac"))
        .expect("committed FLAC fixture must exist");
    unsigned[FLAC_STREAMINFO_MD5].fill(0);

    // A zeroed STREAMINFO digest declares that no signature was computed. The
    // product verifies nothing, exactly as the backend does, rather than
    // failing a stream it simply cannot check.
    assert_eq!(backend_flac_verdict(&unsigned).0, None);
    let frames = product_flac_outcome(&unsigned).expect("an unsigned FLAC stream still decodes");
    assert_eq!(frames, 400);
}

// ADR-0014 step 3: the FLAC packet workers. FLAC differs from ALAC in one way
// that these tests are built around: its stream signature is order-dependent,
// so a digest that still matches under forced reordering is direct evidence
// that the verifier was fed in commit order rather than completion order.

fn flac_fixture_bytes() -> Vec<u8> {
    fs::read(product_fixture_path("flac-pcm-s16-stereo-multiblock.flac"))
        .expect("committed FLAC fixture must exist")
}

#[test]
fn flac_packet_workers_decode_bit_identically_at_every_worker_count() {
    let path = product_fixture_path("flac-pcm-s16-stereo-multiblock.flac");
    let (oracle_source, oracle_samples) = decode_all_samples(&path);
    let oracle_bits = raw_bits(&oracle_samples);

    let started = started_worker_pools(|| {
        for workers in PACKET_WORKER_COUNTS {
            let (source, samples) = decode_all_samples_with(&path, worker_reservation(workers));
            assert_eq!(
                raw_bits(&samples),
                oracle_bits,
                "FLAC decoded different raw f64 bits on {workers} workers"
            );
            assert_eq!(
                source, oracle_source,
                "FLAC reported different source metadata on {workers} workers"
            );
        }
    });
    assert_eq!(
        started,
        PACKET_WORKER_COUNTS.len(),
        "every parallel case must actually have run on packet workers"
    );
}

#[test]
fn the_flac_route_reports_its_own_engine_and_falls_back_on_one_worker() {
    let path = product_fixture_path("flac-pcm-s16-stereo-multiblock.flac");

    let (_, execution) = DecoderFactory::with_application_reservation(worker_reservation(4))
        .open_with_execution(&path)
        .expect("the FLAC fixture opens on a multi-worker reservation");
    assert_eq!(execution.engine(), DecodeEngineKind::FlacPacketWorkers);
    assert_eq!(execution.workers().get(), 4);

    // A single-worker allocation degrades before decoding starts, exactly as
    // every other route does.
    let serial = DecodeReservation::new(
        NonZeroUsize::new(1).unwrap(),
        NonZeroUsize::new(4).unwrap(),
        4 * 1024 * 1024,
    )
    .unwrap();
    let (_, execution) = DecoderFactory::with_application_reservation(serial)
        .open_with_execution(&path)
        .expect("the FLAC fixture opens on a single-worker reservation");
    assert_eq!(execution.engine(), DecodeEngineKind::Serial);
}

#[test]
fn the_flac_signature_survives_forced_out_of_order_completion() {
    // The digest is a function of commit order, so publishing a later packet
    // before packet zero would change it if the verifier were fed as results
    // arrive. Reaching EOF is therefore the assertion: verification runs at
    // `finish` and a mismatch is a decode error, not a silent difference.
    let path = product_fixture_path("flac-pcm-s16-stereo-multiblock.flac");
    let (_, oracle_samples) = decode_all_samples(&path);
    let oracle_bits = raw_bits(&oracle_samples);

    for workers in [2, 4, 8] {
        let mut reader = crate::symphonia_source::open_test_source_with_pool_options(
            &path,
            worker_reservation(workers),
            crate::decode_engine::PoolOptions::force_first_result_after_later(),
        )
        .unwrap();
        let mut samples = Vec::new();
        let outcome = loop {
            match reader.read_block() {
                Ok(ReadOutcome::Data(block)) => samples.extend_from_slice(block.samples()),
                Ok(ReadOutcome::Eof) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        let stalled = reader.stalled_accepts();

        outcome.unwrap_or_else(|error| {
            panic!("{workers} workers failed to verify under forced reordering: {error}")
        });
        assert_eq!(
            raw_bits(&samples),
            oracle_bits,
            "{workers} workers changed the PCM under forced reordering"
        );
        assert!(
            stalled > 0,
            "{workers} workers never reordered, so this proved nothing"
        );
    }
}

#[test]
fn packet_workers_reject_a_tampered_flac_signature_exactly_as_the_serial_route_does() {
    let mut tampered = flac_fixture_bytes();
    tampered[FLAC_STREAMINFO_MD5.start] ^= 0x01;
    let file = TestFile::new("flac", &tampered);

    let serial = DecoderFactory::new()
        .open(file.path())
        .expect("a tampered signature must not change probing");
    let serial_error = drain_to_error(serial);

    for workers in PACKET_WORKER_COUNTS.into_iter().filter(|count| *count > 1) {
        let opened = DecoderFactory::with_application_reservation(worker_reservation(workers))
            .open(file.path())
            .expect("a tampered signature must not change probing");
        let parallel_error = drain_to_error(opened);
        assert_eq!(
            (
                parallel_error.code,
                parallel_error.stage,
                parallel_error.message
            ),
            (
                serial_error.code,
                serial_error.stage,
                serial_error.message.clone()
            ),
            "{workers} workers reported a different verdict than the serial oracle"
        );
        assert_eq!(
            parallel_error.details, serial_error.details,
            "{workers} workers computed a different digest than the serial oracle"
        );
    }
}

/// Read one opened source to its first error.
fn drain_to_error(mut opened: crate::OpenedAudio) -> AnalysisError {
    loop {
        match opened.reader.read_block() {
            Ok(ReadOutcome::Data(_)) => continue,
            Ok(ReadOutcome::Eof) => panic!("the stream was expected to fail before EOF"),
            Err(error) => return error,
        }
    }
}
