use crate::error::{analysis_error, io_analysis_error, validate_analysis_channel_count};
use macinmeter_domain::{AnalysisError, AnalysisStage, ContainerFormat, ErrorCode, SourceCodec};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};
use symphonia::core::io::MediaSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerSignature {
    Wave,
    Flac,
    Aiff,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AiffInfo {
    pub(crate) declared_frames: u64,
    pub(crate) pcm: ContainerPcmInfo,
    length_patch: AiffLengthPatch,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WaveInfo {
    pub(crate) declared_frames: u64,
    pub(crate) pcm: ContainerPcmInfo,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ContainerPcmInfo {
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
    pub(crate) bits_per_sample: u32,
    pub(crate) source_codec: SourceCodec,
}

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;
const WAVE_EXTENSIBLE_MAX_CHANNELS: u16 = 26;
const WAVE_EXTENSIBLE_STANDARD_CHANNEL_MASK: u32 = 0x0003_ffff;
const KSDATAFORMAT_SUBTYPE_PCM: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];

#[derive(Debug, Clone, Copy)]
struct AiffLengthPatch {
    offset: u64,
    bytes: [u8; 4],
}

pub(crate) fn identify_container<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
) -> Result<ContainerSignature, AnalysisError> {
    let mut header = [0_u8; 12];
    let mut read = 0;
    while read < header.len() {
        match reader.read(&mut header[read..]) {
            Ok(0) => break,
            Ok(count) => read += count,
            Err(error) => {
                return Err(io_analysis_error(path, AnalysisStage::Probe, error));
            }
        }
    }

    if read >= 4 && &header[..4] == b"fLaC" {
        return Ok(ContainerSignature::Flac);
    }
    if read >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        return Ok(ContainerSignature::Wave);
    }
    if read >= 12 && &header[..4] == b"FORM" && &header[8..12] == b"AIFF" {
        return Ok(ContainerSignature::Aiff);
    }
    if read >= 12 && &header[..4] == b"FORM" && &header[8..12] == b"AIFC" {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "AIFC is not supported by the M0 decoder",
            None,
        ));
    }
    let observed = &header[..read];
    if is_truncated_signature(observed, b"RIFF", 12)
        || is_truncated_signature(observed, b"FORM", 12)
        || is_truncated_signature(observed, b"fLaC", 4)
    {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "audio container signature is truncated",
            None,
        ));
    }

    Err(analysis_error(
        path,
        ErrorCode::UnsupportedFormat,
        AnalysisStage::Probe,
        "content is not WAV, FLAC, or uncompressed AIFF",
        None,
    ))
}

fn is_truncated_signature(observed: &[u8], signature: &[u8], minimum_header: usize) -> bool {
    if observed.is_empty() || observed.len() >= minimum_header {
        return false;
    }
    if observed.len() <= signature.len() {
        signature.starts_with(observed)
    } else {
        observed.starts_with(signature)
    }
}

pub(crate) fn inspect_wave<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
) -> Result<WaveInfo, AnalysisError> {
    let file_len = stream_len(reader, path)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?;
    let mut riff_header = [0_u8; 12];
    reader
        .read_exact(&mut riff_header)
        .map_err(|error| malformed_wave_io(path, error))?;

    let riff_size = u64::from(u32::from_le_bytes([
        riff_header[4],
        riff_header[5],
        riff_header[6],
        riff_header[7],
    ]));
    let riff_end = 8_u64.checked_add(riff_size).ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::ResourceExhausted,
            AnalysisStage::Probe,
            "WAV RIFF length overflowed",
            None,
        )
    })?;
    if riff_end > file_len || riff_end < 12 {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "WAV RIFF length exceeds the input file",
            None,
        ));
    }

    let mut position = 12_u64;
    let mut pcm_format = None;
    let mut data_len = None;
    while position < riff_end {
        let header_end = position.checked_add(8).ok_or_else(|| {
            analysis_error(
                path,
                ErrorCode::ResourceExhausted,
                AnalysisStage::Probe,
                "WAV chunk position overflowed",
                None,
            )
        })?;
        if header_end > riff_end {
            return Err(analysis_error(
                path,
                ErrorCode::MalformedMedia,
                AnalysisStage::Probe,
                "WAV chunk header is truncated",
                None,
            ));
        }

        reader
            .seek(SeekFrom::Start(position))
            .map_err(|error| malformed_wave_io(path, error))?;
        let mut chunk_header = [0_u8; 8];
        reader
            .read_exact(&mut chunk_header)
            .map_err(|error| malformed_wave_io(path, error))?;
        let chunk_len = u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]);
        let padded_len = u64::from(chunk_len)
            .checked_add(u64::from(chunk_len & 1))
            .ok_or_else(|| {
                analysis_error(
                    path,
                    ErrorCode::ResourceExhausted,
                    AnalysisStage::Probe,
                    "WAV chunk length overflowed",
                    None,
                )
            })?;
        let next_position = header_end.checked_add(padded_len).ok_or_else(|| {
            analysis_error(
                path,
                ErrorCode::ResourceExhausted,
                AnalysisStage::Probe,
                "WAV chunk position overflowed",
                None,
            )
        })?;
        if next_position > riff_end {
            return Err(analysis_error(
                path,
                ErrorCode::MalformedMedia,
                AnalysisStage::Probe,
                "WAV chunk length exceeds the RIFF boundary",
                None,
            ));
        }

        if &chunk_header[..4] == b"fmt " {
            if pcm_format.is_some() {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "WAV contains multiple fmt chunks",
                    None,
                ));
            }
            if chunk_len < 16 {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "WAV fmt chunk is too short",
                    None,
                ));
            }
            reader
                .seek(SeekFrom::Start(header_end))
                .map_err(|error| malformed_wave_io(path, error))?;
            let mut format_prefix = [0_u8; 16];
            reader
                .read_exact(&mut format_prefix)
                .map_err(|error| malformed_wave_io(path, error))?;
            let format_tag = u16::from_le_bytes([format_prefix[0], format_prefix[1]]);
            let channels = u16::from_le_bytes([format_prefix[2], format_prefix[3]]);
            validate_analysis_channel_count(path, channels)?;
            let sample_rate = u32::from_le_bytes([
                format_prefix[4],
                format_prefix[5],
                format_prefix[6],
                format_prefix[7],
            ]);
            let byte_rate = u32::from_le_bytes([
                format_prefix[8],
                format_prefix[9],
                format_prefix[10],
                format_prefix[11],
            ]);
            let block_align = u16::from_le_bytes([format_prefix[12], format_prefix[13]]);
            let bits_per_sample = u16::from_le_bytes([format_prefix[14], format_prefix[15]]);
            if channels == 0 || sample_rate == 0 {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "WAV channel count and sample rate must be nonzero",
                    None,
                ));
            }
            let source_codec = validate_wave_format(
                reader,
                path,
                chunk_len,
                format_tag,
                channels,
                bits_per_sample,
            )?;

            let bytes_per_sample = bits_per_sample / 8;
            let expected_block_align = channels.checked_mul(bytes_per_sample).ok_or_else(|| {
                analysis_error(
                    path,
                    ErrorCode::ResourceExhausted,
                    AnalysisStage::Probe,
                    "WAV frame size overflowed",
                    None,
                )
            })?;
            let expected_byte_rate = sample_rate
                .checked_mul(u32::from(expected_block_align))
                .ok_or_else(|| {
                    analysis_error(
                        path,
                        ErrorCode::ResourceExhausted,
                        AnalysisStage::Probe,
                        "WAV byte rate overflowed",
                        None,
                    )
                })?;
            if block_align != expected_block_align || byte_rate != expected_byte_rate {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "WAV byte rate or block alignment disagrees with its PCM geometry",
                    Some(format!(
                        "byte_rate={byte_rate}; expected_byte_rate={expected_byte_rate}; \
                         block_align={block_align}; expected_block_align={expected_block_align}"
                    )),
                ));
            }
            pcm_format = Some((
                ContainerPcmInfo {
                    sample_rate,
                    channels,
                    bits_per_sample: u32::from(bits_per_sample),
                    source_codec,
                },
                u64::from(block_align),
            ));
        } else if &chunk_header[..4] == b"data" && data_len.replace(u64::from(chunk_len)).is_some()
        {
            return Err(analysis_error(
                path,
                ErrorCode::UnsupportedFormat,
                AnalysisStage::Probe,
                "multiple WAV data chunks are outside the M0 decoder contract",
                None,
            ));
        }
        position = next_position;
    }

    let (pcm, block_align) = pcm_format.ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "WAV contains no fmt chunk",
            None,
        )
    })?;
    let data_len = data_len.ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "WAV contains no data chunk",
            None,
        )
    })?;
    if data_len % block_align != 0 {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "WAV data chunk is not aligned to complete PCM frames",
            None,
        ));
    }

    Ok(WaveInfo {
        declared_frames: data_len / block_align,
        pcm,
    })
}

fn validate_wave_format<R: Read>(
    reader: &mut R,
    path: &Path,
    chunk_len: u32,
    format_tag: u16,
    channels: u16,
    bits_per_sample: u16,
) -> Result<SourceCodec, AnalysisError> {
    match format_tag {
        WAVE_FORMAT_PCM => {
            validate_classic_wave_format(
                reader,
                path,
                chunk_len,
                format_tag,
                bits_per_sample,
                &[8, 16, 24, 32],
            )?;
            Ok(SourceCodec::PcmInteger)
        }
        WAVE_FORMAT_IEEE_FLOAT => {
            validate_classic_wave_format(
                reader,
                path,
                chunk_len,
                format_tag,
                bits_per_sample,
                &[32, 64],
            )?;
            Ok(SourceCodec::PcmFloat)
        }
        WAVE_FORMAT_EXTENSIBLE => {
            validate_wave_extensible_format(reader, path, chunk_len, channels, bits_per_sample)
        }
        _ => Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAV codec is outside the stable linear PCM/IEEE float matrix",
            Some(format!("format_tag=0x{format_tag:04x}")),
        )),
    }
}

fn validate_classic_wave_format<R: Read>(
    reader: &mut R,
    path: &Path,
    chunk_len: u32,
    format_tag: u16,
    bits_per_sample: u16,
    allowed_bits: &[u16],
) -> Result<(), AnalysisError> {
    if !allowed_bits.contains(&bits_per_sample) {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAV bit depth is outside the stable native matrix",
            Some(format!(
                "format_tag=0x{format_tag:04x}; bits_per_sample={bits_per_sample}"
            )),
        ));
    }
    if !matches!(chunk_len, 16 | 18) {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "extended WAV fmt data is outside the stable native matrix",
            Some(format!(
                "format_tag=0x{format_tag:04x}; fmt_size={chunk_len}"
            )),
        ));
    }
    if chunk_len == 18 {
        let mut extension_size = [0_u8; 2];
        reader
            .read_exact(&mut extension_size)
            .map_err(|error| malformed_wave_io(path, error))?;
        let extension_size = u16::from_le_bytes(extension_size);
        if extension_size != 0 {
            return Err(analysis_error(
                path,
                ErrorCode::UnsupportedFormat,
                AnalysisStage::Probe,
                "WAV fmt extension data is outside the stable native matrix",
                Some(format!("extension_size={extension_size}")),
            ));
        }
    }
    Ok(())
}

fn validate_wave_extensible_format<R: Read>(
    reader: &mut R,
    path: &Path,
    chunk_len: u32,
    channels: u16,
    bits_per_sample: u16,
) -> Result<SourceCodec, AnalysisError> {
    if chunk_len < 18 {
        return Err(malformed_wave_extensible_size(path, chunk_len, None));
    }

    let mut extension_size = [0_u8; 2];
    reader
        .read_exact(&mut extension_size)
        .map_err(|error| malformed_wave_io(path, error))?;
    let extension_size = u16::from_le_bytes(extension_size);
    let declared_fmt_size = 18_u32 + u32::from(extension_size);
    if chunk_len < 40 || extension_size < 22 || declared_fmt_size > chunk_len {
        return Err(malformed_wave_extensible_size(
            path,
            chunk_len,
            Some(extension_size),
        ));
    }
    if chunk_len > 40 || extension_size > 22 {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE fmt data contains unsupported extra extensions",
            Some(format!(
                "fmt_size={chunk_len}; extension_size={extension_size}"
            )),
        ));
    }

    let mut extension = [0_u8; 22];
    reader
        .read_exact(&mut extension)
        .map_err(|error| malformed_wave_io(path, error))?;
    let valid_bits_per_sample = u16::from_le_bytes([extension[0], extension[1]]);
    let channel_mask = u32::from_le_bytes([extension[2], extension[3], extension[4], extension[5]]);
    let mut sub_format_guid = [0_u8; 16];
    sub_format_guid.copy_from_slice(&extension[6..]);
    let source_codec = if sub_format_guid == KSDATAFORMAT_SUBTYPE_PCM {
        SourceCodec::PcmInteger
    } else if sub_format_guid == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
        SourceCodec::PcmFloat
    } else {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE sub-format GUID is unsupported",
            Some(format!("sub_format_guid={sub_format_guid:02x?}")),
        ));
    };

    let allowed_bits = match source_codec {
        SourceCodec::PcmInteger => [8, 16, 24, 32].contains(&bits_per_sample),
        SourceCodec::PcmFloat => [32, 64].contains(&bits_per_sample),
        SourceCodec::Flac => false,
    };
    if !allowed_bits {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE bit depth is outside the stable native matrix",
            Some(format!(
                "source_codec={source_codec:?}; bits_per_sample={bits_per_sample}"
            )),
        ));
    }
    if valid_bits_per_sample > bits_per_sample {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE valid bits exceed the container width",
            Some(format!(
                "valid_bits_per_sample={valid_bits_per_sample}; bits_per_sample={bits_per_sample}"
            )),
        ));
    }
    if valid_bits_per_sample < bits_per_sample {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE padded or unspecified valid bits are unsupported",
            Some(format!(
                "valid_bits_per_sample={valid_bits_per_sample}; bits_per_sample={bits_per_sample}"
            )),
        ));
    }
    if channels > WAVE_EXTENSIBLE_MAX_CHANNELS {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE exceeds the stable backend channel limit",
            Some(format!(
                "declared_channels={channels}; max_extensible_channels={WAVE_EXTENSIBLE_MAX_CHANNELS}"
            )),
        ));
    }
    if channel_mask & !WAVE_EXTENSIBLE_STANDARD_CHANNEL_MASK != 0 {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE channel mask uses reserved speaker bits",
            Some(format!("channel_mask=0x{channel_mask:08x}")),
        ));
    }
    if channel_mask != 0 && channel_mask.count_ones() != u32::from(channels) {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "WAVE_FORMAT_EXTENSIBLE channel mask disagrees with the channel count",
            Some(format!(
                "channel_mask=0x{channel_mask:08x}; channels={channels}"
            )),
        ));
    }
    Ok(source_codec)
}

fn malformed_wave_extensible_size(
    path: &Path,
    chunk_len: u32,
    extension_size: Option<u16>,
) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::MalformedMedia,
        AnalysisStage::Probe,
        "WAVE_FORMAT_EXTENSIBLE fmt data is truncated or internally inconsistent",
        Some(format!(
            "fmt_size={chunk_len}; extension_size={extension_size:?}"
        )),
    )
}

pub(crate) fn inspect_aiff<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
) -> Result<AiffInfo, AnalysisError> {
    let file_len = stream_len(reader, path)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?;
    let mut form_header = [0_u8; 12];
    reader
        .read_exact(&mut form_header)
        .map_err(|error| malformed_aiff_io(path, error))?;

    let form_size = u64::from(u32::from_be_bytes([
        form_header[4],
        form_header[5],
        form_header[6],
        form_header[7],
    ]));
    let form_end = 8_u64.checked_add(form_size).ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::ResourceExhausted,
            AnalysisStage::Probe,
            "AIFF FORM length overflowed",
            None,
        )
    })?;
    if form_end > file_len || form_end < 12 {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "AIFF FORM length exceeds the input file",
            None,
        ));
    }

    let mut position = 12_u64;
    let mut common_format = None;
    let mut length_patch = None;
    let mut sound_data_len = None;
    while position < form_end {
        let header_end = position.checked_add(8).ok_or_else(|| {
            analysis_error(
                path,
                ErrorCode::ResourceExhausted,
                AnalysisStage::Probe,
                "AIFF chunk position overflowed",
                None,
            )
        })?;
        if header_end > form_end {
            return Err(analysis_error(
                path,
                ErrorCode::MalformedMedia,
                AnalysisStage::Probe,
                "AIFF chunk header is truncated",
                None,
            ));
        }

        reader
            .seek(SeekFrom::Start(position))
            .map_err(|error| malformed_aiff_io(path, error))?;
        let mut chunk_header = [0_u8; 8];
        reader
            .read_exact(&mut chunk_header)
            .map_err(|error| malformed_aiff_io(path, error))?;
        let chunk_len = u32::from_be_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]);
        let padded_len = u64::from(chunk_len)
            .checked_add(u64::from(chunk_len & 1))
            .ok_or_else(|| {
                analysis_error(
                    path,
                    ErrorCode::ResourceExhausted,
                    AnalysisStage::Probe,
                    "AIFF chunk length overflowed",
                    None,
                )
            })?;
        let next_position = header_end.checked_add(padded_len).ok_or_else(|| {
            analysis_error(
                path,
                ErrorCode::ResourceExhausted,
                AnalysisStage::Probe,
                "AIFF chunk position overflowed",
                None,
            )
        })?;
        if next_position > form_end {
            return Err(analysis_error(
                path,
                ErrorCode::MalformedMedia,
                AnalysisStage::Probe,
                "AIFF chunk length exceeds the FORM boundary",
                None,
            ));
        }

        if &chunk_header[..4] == b"COMM" {
            if chunk_len != 18 {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF COMM chunk must contain exactly 18 bytes",
                    Some(format!("comm_size={chunk_len}")),
                ));
            }
            if common_format.is_some() {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF contains multiple COMM chunks",
                    None,
                ));
            }
            let mut common_prefix = [0_u8; 18];
            reader
                .read_exact(&mut common_prefix)
                .map_err(|error| malformed_aiff_io(path, error))?;
            let declared_channels = i16::from_be_bytes([common_prefix[0], common_prefix[1]]);
            if declared_channels <= 0 {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF channel count must be positive",
                    None,
                ));
            }
            let declared_channels = u16::try_from(declared_channels).map_err(|_| {
                analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF channel count cannot be represented",
                    None,
                )
            })?;
            validate_analysis_channel_count(path, declared_channels)?;
            let declared_frames = u64::from(u32::from_be_bytes([
                common_prefix[2],
                common_prefix[3],
                common_prefix[4],
                common_prefix[5],
            ]));
            let sample_size = u16::from_be_bytes([common_prefix[6], common_prefix[7]]);
            if ![8, 16, 24, 32].contains(&sample_size) {
                return Err(analysis_error(
                    path,
                    ErrorCode::UnsupportedFormat,
                    AnalysisStage::Probe,
                    "AIFF bit depth is outside the stable native matrix",
                    Some(format!("bits_per_sample={sample_size}")),
                ));
            }
            let mut sample_rate_bytes = [0_u8; 10];
            sample_rate_bytes.copy_from_slice(&common_prefix[8..18]);
            let sample_rate = parse_aiff_integer_sample_rate(path, sample_rate_bytes)?;
            common_format = Some((
                ContainerPcmInfo {
                    sample_rate,
                    channels: declared_channels,
                    bits_per_sample: u32::from(sample_size),
                    source_codec: SourceCodec::PcmInteger,
                },
                declared_frames,
            ));
        } else if &chunk_header[..4] == b"SSND" {
            if length_patch.is_some() {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF contains multiple SSND chunks",
                    None,
                ));
            }
            let audio_len = chunk_len.checked_sub(8).ok_or_else(|| {
                analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF SSND chunk is too short",
                    None,
                )
            })?;
            let mut sound_header = [0_u8; 8];
            reader
                .read_exact(&mut sound_header)
                .map_err(|error| malformed_aiff_io(path, error))?;
            let offset = u32::from_be_bytes([
                sound_header[0],
                sound_header[1],
                sound_header[2],
                sound_header[3],
            ]);
            let block_size = u32::from_be_bytes([
                sound_header[4],
                sound_header[5],
                sound_header[6],
                sound_header[7],
            ]);
            if offset != 0 || block_size != 0 {
                return Err(analysis_error(
                    path,
                    ErrorCode::UnsupportedFormat,
                    AnalysisStage::Probe,
                    "nonzero AIFF SSND offset or block size is outside the stable native matrix",
                    Some(format!("offset={offset}; block_size={block_size}")),
                ));
            }
            sound_data_len = Some(u64::from(audio_len));
            let patch_offset = position.checked_add(4).ok_or_else(|| {
                analysis_error(
                    path,
                    ErrorCode::ResourceExhausted,
                    AnalysisStage::Probe,
                    "AIFF patch position overflowed",
                    None,
                )
            })?;
            length_patch = Some(AiffLengthPatch {
                offset: patch_offset,
                bytes: audio_len.to_be_bytes(),
            });
        }
        position = next_position;
    }

    let (pcm, declared_frames) = common_format.ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "AIFF contains no COMM chunk",
            None,
        )
    })?;
    let length_patch = length_patch.ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "AIFF contains no SSND chunk",
            None,
        )
    })?;
    let sound_data_len = sound_data_len.ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "AIFF contains no usable SSND payload",
            None,
        )
    })?;
    let bytes_per_sample = u64::from(pcm.bits_per_sample / 8);
    let frame_bytes = u64::from(pcm.channels)
        .checked_mul(bytes_per_sample)
        .ok_or_else(|| {
            analysis_error(
                path,
                ErrorCode::ResourceExhausted,
                AnalysisStage::Probe,
                "AIFF frame size overflowed",
                None,
            )
        })?;
    let expected_bytes = declared_frames.checked_mul(frame_bytes).ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::ResourceExhausted,
            AnalysisStage::Probe,
            "AIFF declared sample payload overflowed",
            None,
        )
    })?;
    if sound_data_len != expected_bytes {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "AIFF SSND payload does not contain exactly the declared complete frames",
            Some(format!(
                "declared {declared_frames} frames require {expected_bytes} bytes, found {sound_data_len}"
            )),
        ));
    }
    Ok(AiffInfo {
        declared_frames,
        pcm,
        length_patch,
    })
}

fn stream_len<R: Read + Seek>(reader: &mut R, path: &Path) -> Result<u64, AnalysisError> {
    reader
        .seek(SeekFrom::End(0))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))
}

fn parse_aiff_integer_sample_rate(path: &Path, bytes: [u8; 10]) -> Result<u32, AnalysisError> {
    let sign_exponent = u16::from_be_bytes([bytes[0], bytes[1]]);
    let exponent = sign_exponent & 0x7fff;
    let significand = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    let details =
        || format!("sign_exponent=0x{sign_exponent:04x}; significand=0x{significand:016x}");
    let malformed = || {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "AIFF sample rate must use a valid finite positive 80-bit encoding",
            Some(details()),
        )
    };
    let unsupported = || {
        analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "AIFF sample rate is outside the stable positive integral u32 matrix",
            Some(details()),
        )
    };

    if sign_exponent & 0x8000 != 0 || exponent == 0x7fff {
        return Err(malformed());
    }
    if exponent == 0 {
        if significand == 0 || significand & (1_u64 << 63) != 0 {
            return Err(malformed());
        }
        return Err(unsupported());
    }
    if significand & (1_u64 << 63) == 0 {
        return Err(malformed());
    }

    let binary_shift = i32::from(exponent) - 16_383 - 63;
    if binary_shift >= 0 {
        return Err(unsupported());
    }
    let fractional_bits = binary_shift.unsigned_abs();
    if fractional_bits >= 64 {
        return Err(unsupported());
    }
    let fractional_mask = (1_u64 << fractional_bits) - 1;
    if significand & fractional_mask != 0 {
        return Err(unsupported());
    }
    let value = significand >> fractional_bits;
    let value = u32::try_from(value).map_err(|_| unsupported())?;
    if value == 0 {
        return Err(malformed());
    }
    Ok(value)
}

pub(crate) fn media_source(file: File, aiff_info: Option<AiffInfo>) -> Box<dyn MediaSource> {
    match aiff_info {
        Some(info) => Box::new(PatchedAiffFile::new(file, info.length_patch)),
        None => Box::new(file),
    }
}

pub(crate) fn container_format(signature: ContainerSignature) -> ContainerFormat {
    match signature {
        ContainerSignature::Wave => ContainerFormat::Wave,
        ContainerSignature::Flac => ContainerFormat::Flac,
        ContainerSignature::Aiff => ContainerFormat::Aiff,
    }
}

/// Symphonia 0.5.5 treats the AIFF SSND chunk length as audio payload length even though the AIFF
/// specification includes the eight-byte offset/block-size header. Patch only the length bytes in
/// the decoder's read view so that valid AIFF data is bounded correctly without modifying the
/// source file or buffering it in memory.
struct PatchedAiffFile {
    inner: File,
    byte_len: Option<u64>,
    patch: AiffLengthPatch,
}

impl PatchedAiffFile {
    fn new(inner: File, patch: AiffLengthPatch) -> Self {
        let byte_len = inner.metadata().ok().map(|metadata| metadata.len());
        Self {
            inner,
            byte_len,
            patch,
        }
    }
}

impl Read for PatchedAiffFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let start = self.inner.stream_position()?;
        let count = self.inner.read(buffer)?;
        let count_u64 = u64::try_from(count)
            .map_err(|_| io::Error::other("read length cannot be represented"))?;
        let end = start
            .checked_add(count_u64)
            .ok_or_else(|| io::Error::other("read position overflow"))?;
        let patch_end = self
            .patch
            .offset
            .checked_add(self.patch.bytes.len() as u64)
            .ok_or_else(|| io::Error::other("patch position overflow"))?;
        let overlap_start = start.max(self.patch.offset);
        let overlap_end = end.min(patch_end);

        if overlap_start < overlap_end {
            let destination_start = usize::try_from(overlap_start - start)
                .map_err(|_| io::Error::other("patch destination cannot be represented"))?;
            let source_start = usize::try_from(overlap_start - self.patch.offset)
                .map_err(|_| io::Error::other("patch source cannot be represented"))?;
            let overlap_len = usize::try_from(overlap_end - overlap_start)
                .map_err(|_| io::Error::other("patch length cannot be represented"))?;
            buffer[destination_start..destination_start + overlap_len]
                .copy_from_slice(&self.patch.bytes[source_start..source_start + overlap_len]);
        }
        Ok(count)
    }
}

impl Seek for PatchedAiffFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

impl MediaSource for PatchedAiffFile {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.byte_len
    }
}

fn malformed_aiff_io(path: &Path, error: io::Error) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::MalformedMedia,
        AnalysisStage::Probe,
        "failed to parse AIFF chunk structure",
        Some(error.to_string()),
    )
}

fn malformed_wave_io(path: &Path, error: io::Error) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::MalformedMedia,
        AnalysisStage::Probe,
        "failed to parse WAV chunk structure",
        Some(error.to_string()),
    )
}
