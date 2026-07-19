use crate::{DecoderFactory, PcmSource, ReadOutcome, SUPPORTED_EXTENSIONS};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelCount, ChannelLayout, ContainerFormat, ErrorCode,
    MAX_ANALYSIS_CHANNELS, PcmBlock, SourceCodec,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
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
    assert_eq!(
        SUPPORTED_EXTENSIONS,
        &["wav", "wave", "flac", "aif", "aiff"]
    );
    assert!(!SUPPORTED_EXTENSIONS.contains(&"aifc"));
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
fn rejects_wave_format_extensible_until_it_has_its_own_capability_evidence() {
    let file = TestFile::new("wav", &extensible_pcm16_wave());
    let error = expect_open_error(file.path());
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    assert_eq!(error.stage, AnalysisStage::Probe);
    assert!(error.message.contains("WAVE_FORMAT_EXTENSIBLE"));
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
    let mut bytes = pcm16_wave(48_000, &[[1, -1], [2, -2]]);
    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    bytes[4..8].copy_from_slice(&(riff_size + 24).to_le_bytes());
    bytes[16..20].copy_from_slice(&40_u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&0xfffe_u16.to_le_bytes());

    let mut extension = Vec::with_capacity(24);
    extension.extend_from_slice(&22_u16.to_le_bytes());
    extension.extend_from_slice(&16_u16.to_le_bytes());
    extension.extend_from_slice(&3_u32.to_le_bytes());
    extension.extend_from_slice(&[
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b,
        0x71,
    ]);
    bytes.splice(36..36, extension);
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
