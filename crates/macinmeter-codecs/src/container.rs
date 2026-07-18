use crate::error::{analysis_error, io_analysis_error};
use macinmeter_domain::{AnalysisError, AnalysisStage, ContainerFormat, ErrorCode};
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
    length_patch: AiffLengthPatch,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WaveInfo {
    pub(crate) declared_frames: u64,
}

#[derive(Debug, Clone, Copy)]
struct AiffLengthPatch {
    offset: u64,
    bytes: [u8; 4],
}

pub(crate) fn identify_container(
    file: &mut File,
    path: &Path,
) -> Result<ContainerSignature, AnalysisError> {
    let mut header = [0_u8; 12];
    let mut read = 0;
    while read < header.len() {
        match file.read(&mut header[read..]) {
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

pub(crate) fn inspect_wave(file: &mut File, path: &Path) -> Result<WaveInfo, AnalysisError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?;
    let mut riff_header = [0_u8; 12];
    file.read_exact(&mut riff_header)
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
    let file_len = file
        .metadata()
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?
        .len();
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
    let mut block_align = None;
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

        file.seek(SeekFrom::Start(position))
            .map_err(|error| malformed_wave_io(path, error))?;
        let mut chunk_header = [0_u8; 8];
        file.read_exact(&mut chunk_header)
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
            if block_align.is_some() {
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
            file.seek(SeekFrom::Start(header_end))
                .map_err(|error| malformed_wave_io(path, error))?;
            let mut format_prefix = [0_u8; 16];
            file.read_exact(&mut format_prefix)
                .map_err(|error| malformed_wave_io(path, error))?;
            let channels = u16::from_le_bytes([format_prefix[2], format_prefix[3]]);
            let sample_rate = u32::from_le_bytes([
                format_prefix[4],
                format_prefix[5],
                format_prefix[6],
                format_prefix[7],
            ]);
            let value = u16::from_le_bytes([format_prefix[12], format_prefix[13]]);
            if channels == 0 || sample_rate == 0 || value == 0 {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "WAV channel count, sample rate, and block alignment must be nonzero",
                    None,
                ));
            }
            block_align = Some(u64::from(value));
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

    let block_align = block_align.ok_or_else(|| {
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
    })
}

pub(crate) fn inspect_aiff(file: &mut File, path: &Path) -> Result<AiffInfo, AnalysisError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?;
    let mut form_header = [0_u8; 12];
    file.read_exact(&mut form_header)
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
    let file_len = file
        .metadata()
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?
        .len();
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

        file.seek(SeekFrom::Start(position))
            .map_err(|error| malformed_aiff_io(path, error))?;
        let mut chunk_header = [0_u8; 8];
        file.read_exact(&mut chunk_header)
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
            if chunk_len < 18 {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF COMM chunk is too short",
                    None,
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
            file.read_exact(&mut common_prefix)
                .map_err(|error| malformed_aiff_io(path, error))?;
            let channels = u64::from(u16::from_be_bytes([common_prefix[0], common_prefix[1]]));
            let declared_frames = u64::from(u32::from_be_bytes([
                common_prefix[2],
                common_prefix[3],
                common_prefix[4],
                common_prefix[5],
            ]));
            let sample_size = u64::from(u16::from_be_bytes([common_prefix[6], common_prefix[7]]));
            let sample_rate_is_zero = common_prefix[8..18].iter().all(|byte| *byte == 0);
            if channels == 0 || sample_size == 0 || sample_rate_is_zero {
                return Err(analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF channel count, sample size, and sample rate must be nonzero",
                    None,
                ));
            }
            common_format = Some((channels, declared_frames, sample_size));
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
            file.read_exact(&mut sound_header)
                .map_err(|error| malformed_aiff_io(path, error))?;
            let offset = u32::from_be_bytes([
                sound_header[0],
                sound_header[1],
                sound_header[2],
                sound_header[3],
            ]);
            let payload_len = audio_len.checked_sub(offset).ok_or_else(|| {
                analysis_error(
                    path,
                    ErrorCode::MalformedMedia,
                    AnalysisStage::Probe,
                    "AIFF SSND offset exceeds its audio payload",
                    None,
                )
            })?;
            sound_data_len = Some(u64::from(payload_len));
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

    let (channels, declared_frames, sample_size) = common_format.ok_or_else(|| {
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
    let bytes_per_sample = sample_size.checked_add(7).ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::ResourceExhausted,
            AnalysisStage::Probe,
            "AIFF sample size overflowed",
            None,
        )
    })? / 8;
    let frame_bytes = channels.checked_mul(bytes_per_sample).ok_or_else(|| {
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
        length_patch,
    })
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
