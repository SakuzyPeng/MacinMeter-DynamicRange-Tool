use crate::{DecoderFactory, PcmSource, ReadOutcome, SUPPORTED_EXTENSIONS};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelCount, ChannelLayout, ContainerFormat, ErrorCode,
    MAX_ANALYSIS_CHANNELS, PcmBlock, SourceCodec,
};
use std::{
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
        initial_progress.decoded_frames, 0,
        "{} initial decoded frames",
        case.name
    );
    assert_eq!(
        initial_progress.expected_frames,
        Some(case.expected_frames),
        "{} initial expected frames",
        case.name
    );
    assert_eq!(
        initial_progress.fraction,
        Some(0.0),
        "{} initial progress fraction",
        case.name
    );
    assert!(!initial_progress.eof, "{} began at EOF", case.name);
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
            progress.decoded_frames, decoded_frames,
            "{} progress disagrees with returned Data",
            case.name
        );
        assert_eq!(
            progress.expected_frames,
            Some(case.expected_frames),
            "{} progress expected frames changed",
            case.name
        );
        let expected_fraction = decoded_frames as f64 / case.expected_frames as f64;
        assert_eq!(
            progress.fraction,
            Some(expected_fraction),
            "{} progress fraction",
            case.name
        );
        assert!(
            progress.fraction.is_some_and(f64::is_finite),
            "{} progress fraction is non-finite",
            case.name
        );
        assert!(
            !progress.eof,
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

    assert_eq!(
        opened.reader.stream_info(),
        &immutable_info,
        "{} stream info changed at EOF",
        case.name
    );
    let terminal_progress = opened.reader.progress();
    assert_eq!(terminal_progress.decoded_frames, case.expected_frames);
    assert_eq!(
        terminal_progress.expected_frames,
        Some(case.expected_frames)
    );
    assert_eq!(terminal_progress.fraction, Some(1.0));
    assert!(
        terminal_progress.eof,
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
fn stable_routes_pass_the_shared_pcm_source_contract() {
    let wave_frames = multiblock_pcm16_wave_frames();
    let wave_samples: Vec<i16> = wave_frames
        .iter()
        .flat_map(|frame| frame.iter().copied())
        .collect();

    let aiff_samples = [i16::MIN, -16_384, 0, 16_384, i16::MAX];
    let flac_samples = [0_i16, 5_793, 8_192, 5_793, 0, -5_793, -8_192, -5_793];
    let cases = [
        PcmSourceContractCase {
            name: "WAV integer PCM",
            wrong_extension: "flac",
            bytes: pcm16_wave(48_000, &wave_frames),
            container: ContainerFormat::Wave,
            codec: SourceCodec::PcmInteger,
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 16,
            expected_frames: 2_305,
            expected_samples: normalize_pcm16(&wave_samples),
            minimum_data_blocks: 2,
        },
        PcmSourceContractCase {
            name: "AIFF integer PCM",
            wrong_extension: "wav",
            bytes: pcm16_aiff(44_100, &aiff_samples),
            container: ContainerFormat::Aiff,
            codec: SourceCodec::PcmInteger,
            sample_rate: 44_100,
            channels: 1,
            bits_per_sample: 16,
            expected_frames: 5,
            expected_samples: normalize_pcm16(&aiff_samples),
            minimum_data_blocks: 1,
        },
        PcmSourceContractCase {
            name: "FLAC",
            wrong_extension: "aiff",
            bytes: TINY_FLAC.to_vec(),
            container: ContainerFormat::Flac,
            codec: SourceCodec::Flac,
            sample_rate: 8_000,
            channels: 1,
            bits_per_sample: 16,
            expected_frames: 8,
            expected_samples: normalize_pcm16(&flac_samples),
            minimum_data_blocks: 1,
        },
    ];

    for case in cases {
        assert_pcm_source_contract(case);
    }
}

#[test]
fn decodes_float32_wave_to_f64_pcm() {
    let samples = [0.25_f32, -0.5, 1.0, -1.0];
    let file = TestFile::new("wav", &float32_wave(44_100, &samples));
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    assert_eq!(opened.source.codec, SourceCodec::PcmFloat);
    assert_eq!(opened.source.bits_per_sample, Some(32));
    let expected_channels = opened.reader.stream_info().spec.channels;
    let block = match opened.reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("generated WAV unexpectedly contained no PCM"),
    };
    assert_block_geometry(&block, expected_channels);
    let expected: Vec<f64> = samples.iter().map(|sample| f64::from(*sample)).collect();
    assert_eq!(block.samples(), expected);
    assert_eq!(opened.reader.read_block().unwrap(), ReadOutcome::Eof);
}

#[test]
fn preserves_float64_wave_samples_without_f32_narrowing() {
    let samples = [
        0.125_f64 + f64::EPSILON,
        -0.75 + f64::EPSILON,
        1.0 - f64::EPSILON,
        -1.0 + f64::EPSILON,
    ];
    assert_ne!(samples[0], f64::from(samples[0] as f32));
    let file = TestFile::new("wav", &float64_wave(96_000, &samples));
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    assert_eq!(opened.source.codec, SourceCodec::PcmFloat);
    assert_eq!(opened.source.bits_per_sample, Some(64));
    let expected_channels = opened.reader.stream_info().spec.channels;
    let block = match opened.reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("generated WAV unexpectedly contained no PCM"),
    };
    assert_block_geometry(&block, expected_channels);
    assert_eq!(block.samples(), samples);
    assert_eq!(opened.reader.read_block().unwrap(), ReadOutcome::Eof);
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
        assert!(!opened.reader.progress().eof);
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
    assert!(!frozen_progress.eof);

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
    assert!(!reader.progress().eof);
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
    assert_eq!(progress.decoded_frames, 0);
    assert_eq!(progress.expected_frames, Some(2));
    assert_eq!(progress.fraction, Some(0.0));
    assert!(!progress.eof);
    let diagnostics = reader.diagnostics().clone();
    assert_eq!(diagnostics.decoded_frames, 0);
    assert_eq!(diagnostics.warnings.len(), 1);

    assert_eq!(reader.read_block().unwrap_err(), error);
    assert_eq!(reader.progress(), progress);
    assert_eq!(reader.diagnostics(), &diagnostics);
}

#[test]
fn corrupt_flac_is_a_sticky_error_not_eof() {
    let mut corrupt = TINY_FLAC.to_vec();
    let last = corrupt.last_mut().unwrap();
    *last ^= 0xff;
    let file = TestFile::new("flac", &corrupt);
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    let first_error = match opened.reader.read_block() {
        Err(error) => error,
        Ok(outcome) => panic!("corrupt FLAC returned {outcome:?}"),
    };
    assert_eq!(first_error.code, ErrorCode::DecodeFailed);

    let repeated_error = match opened.reader.read_block() {
        Err(error) => error,
        Ok(outcome) => panic!("terminal decoder error became {outcome:?}"),
    };
    assert_eq!(repeated_error, first_error);
    assert!(!opened.reader.progress().eof);
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

fn float64_wave(sample_rate: u32, samples: &[f64]) -> Vec<u8> {
    let data_size = u32::try_from(samples.len() * 8).unwrap();
    let mut bytes = wave_header(3, 1, sample_rate, 64, data_size);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
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

/// Eight mono 16-bit frames at 8 kHz, encoded without padding or a seek table.
const TINY_FLAC: &[u8] = &[
    0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x1a, 0x00,
    0x00, 0x1a, 0x01, 0xf4, 0x00, 0xf0, 0x00, 0x00, 0x00, 0x08, 0x62, 0x56, 0x10, 0xb6, 0xdc, 0xa8,
    0xcf, 0x62, 0xae, 0x31, 0x08, 0x6e, 0xfa, 0x42, 0xce, 0xc6, 0x84, 0x00, 0x00, 0x28, 0x20, 0x00,
    0x00, 0x00, 0x72, 0x65, 0x66, 0x65, 0x72, 0x65, 0x6e, 0x63, 0x65, 0x20, 0x6c, 0x69, 0x62, 0x46,
    0x4c, 0x41, 0x43, 0x20, 0x31, 0x2e, 0x35, 0x2e, 0x30, 0x20, 0x32, 0x30, 0x32, 0x35, 0x30, 0x32,
    0x31, 0x31, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf8, 0x64, 0x08, 0x00, 0x07, 0xf6, 0x18, 0x00, 0x00,
    0x16, 0xa1, 0x20, 0x00, 0x16, 0xa1, 0x02, 0xcd, 0xf0, 0x7c, 0x64, 0x00, 0x3e, 0x2c, 0x12, 0xbc,
];
