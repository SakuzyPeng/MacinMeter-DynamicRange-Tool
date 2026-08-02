use crate::{
    OpenedAudio, PcmSource, ReadOutcome,
    codec::{source_bits_per_sample, stream_spec, validate_codec},
    container::{
        ContainerPcmInfo, ContainerSignature, container_format, identify_container, inspect_aiff,
        inspect_wave, media_source,
    },
    decode_engine::{
        AlacWorkerPool, EngineOutcome, PacketDecodeContext, PacketEngine, SerialEngine,
    },
    error::{
        BACKEND, analysis_error, decoder_creation_error, file_open_error, io_analysis_error,
        probe_error,
    },
    isobmff::{IsoBmffAlacInfo, inspect_isobmff_alac},
    packet::{PacketOutcome, PacketReorderBuffer},
};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelLayout, DecodeDiagnostics, DecodeProgress,
    DecodeReservation, ErrorCode, PcmBlock, PcmStreamInfo, SourceInfo, StreamSpec,
};
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
};
use symphonia::core::{
    codecs::{CODEC_TYPE_ALAC, CODEC_TYPE_NULL, CodecParameters, DecoderOptions},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

pub(crate) fn open(
    path: &Path,
    reservation: DecodeReservation,
) -> Result<OpenedAudio, AnalysisError> {
    let (source, reader) = open_source(path, reservation)?;
    Ok(OpenedAudio {
        source,
        reader: Box::new(reader),
    })
}

fn open_source(
    path: &Path,
    reservation: DecodeReservation,
) -> Result<(SourceInfo, SymphoniaPcmSource), AnalysisError> {
    let mut file = File::open(path).map_err(|error| file_open_error(path, error))?;
    let signature = identify_container(&mut file, path)?;
    let (aiff_info, container_pcm, alac_info) = match signature {
        ContainerSignature::Aiff => {
            let info = inspect_aiff(&mut file, path)?;
            (Some(info), Some((info.pcm, info.declared_frames)), None)
        }
        ContainerSignature::Wave => {
            let info = inspect_wave(&mut file, path)?;
            (None, Some((info.pcm, info.declared_frames)), None)
        }
        ContainerSignature::Flac => (None, None, None),
        ContainerSignature::Mp4 => {
            let info = inspect_isobmff_alac(&mut file, path)?;
            (None, Some((info.pcm, info.declared_frames)), Some(info))
        }
    };
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?;

    let source = media_source(file, aiff_info);
    let media_source = MediaSourceStream::new(source, Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            media_source,
            &FormatOptions {
                enable_gapless: true,
                ..FormatOptions::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|error| probe_error(path, error))?;

    let format = probed.format;
    let mut audio_tracks = format
        .tracks()
        .iter()
        .filter(|track| track.codec_params.codec != CODEC_TYPE_NULL);
    let track = audio_tracks.next().ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "supported container contains no audio track",
            None,
        )
    })?;
    if audio_tracks.next().is_some() {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "multiple audio tracks are outside the stable native decoder matrix",
            None,
        ));
    }

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let source_codec = validate_codec(path, signature, codec_params.codec)?;
    let (stream_spec, channels, bits_per_sample) = if let Some(info) = alac_info.as_ref() {
        validate_backend_alac_metadata(path, info, &codec_params)?;
        let stream_spec = StreamSpec::new(
            info.pcm.sample_rate,
            info.pcm.channels,
            ChannelLayout::Unknown,
        )
        .map_err(|error| {
            analysis_error(
                path,
                ErrorCode::MalformedMedia,
                AnalysisStage::Probe,
                "validated ALAC metadata cannot form a PCM stream",
                Some(error.message),
            )
        })?;
        let channels = stream_spec.channels;
        (stream_spec, channels, Some(info.pcm.bits_per_sample))
    } else {
        let (stream_spec, channels) = stream_spec(path, &codec_params)?;
        let bits_per_sample = source_bits_per_sample(&codec_params);
        (stream_spec, channels, bits_per_sample)
    };
    if let Some((validated_pcm, _)) = container_pcm
        && alac_info.is_none()
    {
        validate_backend_pcm_metadata(
            path,
            validated_pcm,
            source_codec,
            &stream_spec,
            bits_per_sample,
        )?;
    }
    let expected_frames = container_pcm
        .map(|(_, expected_frames)| expected_frames)
        .or_else(|| codec_params.n_frames.filter(|frames| *frames > 0));
    // Without a declared total sample count the end-of-stream frame check is
    // inert, and a stream whose STREAMINFO MD5 is also absent can lose whole
    // tail frames undetectably. The stable FLAC route therefore requires a
    // declared count instead of accepting silently unverifiable streams.
    if expected_frames.is_none() {
        return Err(analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "media without a declared total frame count is outside the stable native matrix",
            None,
        ));
    }

    let context = PacketDecodeContext::new(path, stream_spec.sample_rate.get(), channels);
    // ADR-0014 §2/§3: packet workers are created only for a route that has
    // graduated, never from an extension or a generic codec descriptor. Every
    // other route, and any single-worker allocation, stays on the serial oracle.
    let engine = if alac_info.is_some() && reservation.workers().get() > 1 {
        PacketEngine::AlacWorkers(AlacWorkerPool::new(
            context,
            format,
            &codec_params,
            track_id,
            reservation,
        )?)
    } else {
        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions { verify: true })
            .map_err(|error| decoder_creation_error(path, error))?;
        PacketEngine::Serial(SerialEngine::new(context, format, decoder, track_id))
    };

    let source = SourceInfo {
        display_path: path.display().to_string(),
        container: container_format(signature),
        codec: source_codec,
        sample_rate: stream_spec.sample_rate,
        channels: stream_spec.channels,
        bits_per_sample,
        expected_frames,
    };
    let pcm = PcmStreamInfo {
        spec: stream_spec.clone(),
        expected_frames,
    };
    let reader = SymphoniaPcmSource {
        path: path.to_path_buf(),
        engine,
        pcm: pcm.clone(),
        decoded_frames: 0,
        reorder: PacketReorderBuffer::new(reservation),
        terminal: TerminalState::Active,
        #[cfg(test)]
        injected_read_error: None,
        diagnostics: DecodeDiagnostics {
            backend: BACKEND.to_owned(),
            decoded_frames: 0,
            warnings: Vec::new(),
        },
    };

    Ok((source, reader))
}

pub(crate) fn validate_backend_alac_metadata(
    path: &Path,
    validated: &IsoBmffAlacInfo,
    codec_params: &CodecParameters,
) -> Result<(), AnalysisError> {
    let cookie_matches = codec_params
        .extra_data
        .as_deref()
        .is_some_and(|cookie| cookie == validated.magic_cookie.as_ref());
    let sample_rate_matches = codec_params.sample_rate == Some(validated.pcm.sample_rate)
        || (validated.pcm.sample_rate > u32::from(u16::MAX) && codec_params.sample_rate == Some(0));
    if codec_params.codec != CODEC_TYPE_ALAC
        || !sample_rate_matches
        || codec_params.n_frames != Some(validated.declared_frames)
        || !cookie_matches
    {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "decoder metadata disagrees with the validated ISO BMFF ALAC track",
            Some(format!(
                "container={}Hz/{}ch/{}bit/{}frames/{}cookie-bytes; decoder_codec={:?}; decoder_rate={:?}; decoder_frames={:?}; decoder_cookie_match={cookie_matches}",
                validated.pcm.sample_rate,
                validated.pcm.channels,
                validated.pcm.bits_per_sample,
                validated.declared_frames,
                validated.magic_cookie.len(),
                codec_params.codec,
                codec_params.sample_rate,
                codec_params.n_frames,
            )),
        ));
    }
    Ok(())
}

pub(crate) fn validate_backend_pcm_metadata(
    path: &Path,
    validated: ContainerPcmInfo,
    source_codec: macinmeter_domain::SourceCodec,
    stream_spec: &macinmeter_domain::StreamSpec,
    bits_per_sample: Option<u32>,
) -> Result<(), AnalysisError> {
    if source_codec != validated.source_codec
        || stream_spec.sample_rate.get() != validated.sample_rate
        || stream_spec.channels.get() != validated.channels
        || bits_per_sample != Some(validated.bits_per_sample)
    {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "decoder metadata disagrees with the validated container PCM format",
            Some(format!(
                "container={:?}/{}Hz/{}ch/{}bit; decoder={:?}/{}Hz/{}ch/{bits_per_sample:?}bit",
                validated.source_codec,
                validated.sample_rate,
                validated.channels,
                validated.bits_per_sample,
                source_codec,
                stream_spec.sample_rate.get(),
                stream_spec.channels.get(),
            )),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum TerminalState {
    Active,
    Eof,
    Failed(AnalysisError),
}

impl TerminalState {
    fn replay(&self) -> Option<Result<ReadOutcome, AnalysisError>> {
        match self {
            Self::Active => None,
            Self::Eof => Some(Ok(ReadOutcome::Eof)),
            Self::Failed(error) => Some(Err(error.clone())),
        }
    }

    const fn is_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }
}

/// The stable-route PCM source.
///
/// Whichever engine produced a packet, commit stays in input order and the
/// public `Data / Eof / Error` contract is identical: frame counting, progress,
/// integrity and terminal state all live here, above the engine, so they cannot
/// vary with worker count.
pub(crate) struct SymphoniaPcmSource {
    path: PathBuf,
    engine: PacketEngine,
    pcm: PcmStreamInfo,
    decoded_frames: u64,
    reorder: PacketReorderBuffer,
    terminal: TerminalState,
    #[cfg(test)]
    injected_read_error: Option<AnalysisError>,
    diagnostics: DecodeDiagnostics,
}

impl SymphoniaPcmSource {
    fn fail<T>(&mut self, error: AnalysisError) -> Result<T, AnalysisError> {
        self.terminal = TerminalState::Failed(error.clone());
        Err(error)
    }

    fn finish(&mut self) -> Result<ReadOutcome, AnalysisError> {
        // A packet that was decoded but never committed would silently drop
        // audio, so the index space must be closed before integrity and frame
        // count are allowed to pass.
        if let Err(error) = self.reorder.finish() {
            return self.fail(error);
        }

        if let Err(error) = self.engine.finish() {
            return self.fail(error);
        }

        if let Some(expected) = self.pcm.expected_frames
            && expected != self.decoded_frames
        {
            let message = format!(
                "decoded frame count {} does not match the expected frame count {}",
                self.decoded_frames, expected
            );
            self.diagnostics.warnings.push(message.clone());
            return self.fail(analysis_error(
                &self.path,
                ErrorCode::DecodeFailed,
                AnalysisStage::Decode,
                message,
                None,
            ));
        }

        self.terminal = TerminalState::Eof;
        Ok(ReadOutcome::Eof)
    }

    fn checked_frame_total(&mut self, block: &PcmBlock) -> Result<u64, AnalysisError> {
        let frames = match u64::try_from(block.frames()) {
            Ok(frames) => frames,
            Err(_) => {
                return self.fail(analysis_error(
                    &self.path,
                    ErrorCode::ResourceExhausted,
                    AnalysisStage::Decode,
                    "decoded frame count cannot be represented",
                    None,
                ));
            }
        };
        match self.decoded_frames.checked_add(frames) {
            Some(total) => Ok(total),
            None => self.fail(analysis_error(
                &self.path,
                ErrorCode::ResourceExhausted,
                AnalysisStage::Decode,
                "decoded frame counter overflowed",
                None,
            )),
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_error_on_next_read(&mut self, error: AnalysisError) {
        assert!(
            matches!(self.terminal, TerminalState::Active),
            "fault injection requires an active source"
        );
        assert!(
            self.injected_read_error.replace(error).is_none(),
            "only one pending read error may be injected"
        );
    }

    #[cfg(test)]
    pub(crate) fn override_expected_frames(&mut self, expected_frames: Option<u64>) {
        assert!(
            matches!(self.terminal, TerminalState::Active),
            "expected-frame fault injection requires an active source"
        );
        self.pcm.expected_frames = expected_frames;
    }
}

impl PcmSource for SymphoniaPcmSource {
    fn stream_info(&self) -> &PcmStreamInfo {
        &self.pcm
    }

    fn read_block(&mut self) -> Result<ReadOutcome, AnalysisError> {
        if let Some(outcome) = self.terminal.replay() {
            return outcome;
        }

        #[cfg(test)]
        if let Some(error) = self.injected_read_error.take() {
            return self.fail(error);
        }

        loop {
            // Commit whatever the input packet order allows before taking more
            // work. A head result is returned directly from `accept`, while this
            // drain serves outcomes that were previously stalled behind it.
            if let Some(outcome) = self.reorder.take_ready() {
                match outcome {
                    PacketOutcome::Decoded(block) => return self.commit(block),
                    PacketOutcome::Empty => continue,
                    PacketOutcome::Failed(error) => return self.fail(error),
                }
            }

            let (index, outcome) = match self.engine.next() {
                Ok(EngineOutcome::Indexed { index, outcome }) => (index, outcome),
                Ok(EngineOutcome::Exhausted) => return self.finish(),
                Err(error) => return self.fail(error),
            };
            match self.reorder.accept(index, outcome) {
                Ok(Some(PacketOutcome::Decoded(block))) => return self.commit(block),
                Ok(Some(PacketOutcome::Empty)) | Ok(None) => {}
                Ok(Some(PacketOutcome::Failed(error))) | Err(error) => return self.fail(error),
            }
        }
    }

    fn progress(&self) -> DecodeProgress {
        DecodeProgress::new(
            self.decoded_frames,
            self.pcm.expected_frames,
            self.terminal.is_eof(),
        )
    }

    fn diagnostics(&self) -> &DecodeDiagnostics {
        &self.diagnostics
    }
}

impl SymphoniaPcmSource {
    /// Publish one committed block's frames.
    ///
    /// Progress only advances on frames that have been committed in input
    /// order, so it stays monotonic no matter which packet finished first.
    fn commit(&mut self, block: PcmBlock) -> Result<ReadOutcome, AnalysisError> {
        let new_total = self.checked_frame_total(&block)?;
        if let Some(expected) = self.pcm.expected_frames
            && new_total > expected
        {
            let message = format!(
                "decoded frame count {new_total} exceeds the expected frame count {expected}"
            );
            self.diagnostics.warnings.push(message.clone());
            return self.fail(analysis_error(
                &self.path,
                ErrorCode::DecodeFailed,
                AnalysisStage::Decode,
                message,
                None,
            ));
        }
        self.decoded_frames = new_total;
        self.diagnostics.decoded_frames = new_total;
        Ok(ReadOutcome::Data(block))
    }
}

/// Open the serial differential oracle for `path`.
///
/// Route-specific packet workers are graduated by comparing against this path,
/// so it is pinned to [`DecodeReservation::serial`] rather than to whatever the
/// caller happens to hold.
#[cfg(test)]
pub(crate) fn open_test_source(path: &Path) -> Result<SymphoniaPcmSource, AnalysisError> {
    open_source(path, DecodeReservation::serial()).map(|(_, reader)| reader)
}

#[cfg(test)]
pub(crate) fn open_test_source_with(
    path: &Path,
    reservation: DecodeReservation,
) -> Result<SymphoniaPcmSource, AnalysisError> {
    open_source(path, reservation).map(|(_, reader)| reader)
}

#[cfg(test)]
impl SymphoniaPcmSource {
    /// Results so far that had to wait for an earlier packet index.
    pub(crate) const fn stalled_accepts(&self) -> usize {
        self.reorder.stalled_accepts()
    }
}
