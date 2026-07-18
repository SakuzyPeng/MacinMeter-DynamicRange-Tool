use crate::{DecoderFactory, ReadOutcome, SUPPORTED_EXTENSIONS};
use macinmeter_domain::{AnalysisError, ContainerFormat, ErrorCode, SourceCodec};
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

#[test]
fn extensions_are_discovery_only_and_exclude_aifc() {
    assert_eq!(
        SUPPORTED_EXTENSIONS,
        &["wav", "wave", "flac", "aif", "aiff"]
    );
    assert!(!SUPPORTED_EXTENSIONS.contains(&"aifc"));
}

#[test]
fn decodes_pcm_wave_by_content_with_a_wrong_extension() {
    let frames = [[i16::MIN, i16::MAX], [0, 16_384], [-16_384, 1_000]];
    let file = TestFile::new("flac", &pcm16_wave(48_000, &frames));
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    assert_eq!(opened.source.container, ContainerFormat::Wave);
    assert_eq!(opened.source.codec, SourceCodec::PcmInteger);
    assert_eq!(opened.source.sample_rate.get(), 48_000);
    assert_eq!(opened.source.channels.get(), 2);
    assert_eq!(opened.source.bits_per_sample, Some(16));
    assert_eq!(opened.source.expected_frames, Some(3));
    assert_eq!(
        opened.reader.stream_info().spec.sample_rate.get(),
        opened.source.sample_rate.get()
    );
    assert_eq!(opened.reader.stream_info().expected_frames, Some(3));
    assert_eq!(opened.reader.progress().decoded_frames, 0);

    let block = match opened.reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("generated WAV unexpectedly contained no PCM"),
    };
    assert_eq!(block.frames(), 3);
    assert_eq!(block.samples().len(), 6);
    assert!((block.samples()[0] + 1.0).abs() < 0.0001);
    assert!(block.samples()[1] > 0.999);
    assert_eq!(opened.reader.progress().decoded_frames, 3);
    assert_eq!(opened.reader.diagnostics().decoded_frames, 3);

    assert_eq!(opened.reader.read_block().unwrap(), ReadOutcome::Eof);
    assert_eq!(opened.reader.read_block().unwrap(), ReadOutcome::Eof);
    let progress = opened.reader.progress();
    assert!(progress.eof);
    assert_eq!(progress.fraction, Some(1.0));
}

#[test]
fn decodes_float32_wave_to_f64_pcm() {
    let samples = [0.25_f32, -0.5, 1.0, -1.0];
    let file = TestFile::new("wav", &float32_wave(44_100, &samples));
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    assert_eq!(opened.source.codec, SourceCodec::PcmFloat);
    assert_eq!(opened.source.bits_per_sample, Some(32));
    let block = match opened.reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("generated WAV unexpectedly contained no PCM"),
    };
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
    let block = match opened.reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("generated WAV unexpectedly contained no PCM"),
    };
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
fn decodes_uncompressed_aiff() {
    let samples = [i16::MIN, -16_384, 0, 16_384, i16::MAX];
    let file = TestFile::new("aiff", &pcm16_aiff(44_100, &samples));
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    assert_eq!(opened.source.container, ContainerFormat::Aiff);
    assert_eq!(opened.source.codec, SourceCodec::PcmInteger);
    assert_eq!(opened.source.sample_rate.get(), 44_100);
    assert_eq!(opened.source.channels.get(), 1);
    assert_eq!(opened.source.expected_frames, Some(5));

    let block = match opened.reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("generated AIFF unexpectedly contained no PCM"),
    };
    assert_eq!(block.frames(), 5);
    assert!((block.samples()[0] + 1.0).abs() < 0.0001);
    assert!(block.samples()[4] > 0.999);
    assert_eq!(opened.reader.read_block().unwrap(), ReadOutcome::Eof);
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
fn decodes_embedded_flac_and_verifies_its_frame_count() {
    let file = TestFile::new("aiff", TINY_FLAC);
    let mut opened = DecoderFactory::new().open(file.path()).unwrap();

    assert_eq!(opened.source.container, ContainerFormat::Flac);
    assert_eq!(opened.source.codec, SourceCodec::Flac);
    assert_eq!(opened.source.sample_rate.get(), 8_000);
    assert_eq!(opened.source.channels.get(), 1);
    assert_eq!(opened.source.bits_per_sample, Some(16));
    assert_eq!(opened.source.expected_frames, Some(8));

    let block = match opened.reader.read_block().unwrap() {
        ReadOutcome::Data(block) => block,
        ReadOutcome::Eof => panic!("embedded FLAC unexpectedly contained no PCM"),
    };
    assert_eq!(block.frames(), 8);
    assert_eq!(opened.reader.read_block().unwrap(), ReadOutcome::Eof);
    assert_eq!(opened.reader.diagnostics().decoded_frames, 8);
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
