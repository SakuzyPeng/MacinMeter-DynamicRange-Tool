use crate::{
    OpenedAudio, PcmSource, ReadOutcome,
    codec::{source_bits_per_sample, stream_spec, validate_codec},
    container::{
        ContainerPcmInfo, ContainerSignature, container_format, identify_container, inspect_aiff,
        inspect_wave, media_source,
    },
    error::{
        BACKEND, analysis_error, decoder_creation_error, file_open_error, io_analysis_error,
        probe_error, runtime_error,
    },
};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelCount, DecodeDiagnostics, DecodeProgress, ErrorCode,
    PcmBlock, PcmStreamInfo, SourceInfo,
};
use std::{
    fs::File,
    io::{self, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

pub(crate) fn open(path: &Path) -> Result<OpenedAudio, AnalysisError> {
    let (source, reader) = open_source(path)?;
    Ok(OpenedAudio {
        source,
        reader: Box::new(reader),
    })
}

fn open_source(path: &Path) -> Result<(SourceInfo, SymphoniaPcmSource), AnalysisError> {
    let mut file = File::open(path).map_err(|error| file_open_error(path, error))?;
    let signature = identify_container(&mut file, path)?;
    let (aiff_info, container_pcm) = match signature {
        ContainerSignature::Aiff => {
            let info = inspect_aiff(&mut file, path)?;
            (Some(info), Some((info.pcm, info.declared_frames)))
        }
        ContainerSignature::Wave => {
            let info = inspect_wave(&mut file, path)?;
            (None, Some((info.pcm, info.declared_frames)))
        }
        ContainerSignature::Flac => (None, None),
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
            "multiple audio tracks are not supported by the M0 decoder",
            None,
        ));
    }

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let source_codec = validate_codec(path, signature, codec_params.codec)?;
    let (stream_spec, channels) = stream_spec(path, &codec_params)?;
    let bits_per_sample = source_bits_per_sample(&codec_params);
    if let Some((validated_pcm, _)) = container_pcm {
        validate_backend_pcm_metadata(path, validated_pcm, &stream_spec, bits_per_sample)?;
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

    let decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions { verify: true })
        .map_err(|error| decoder_creation_error(path, error))?;

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
        format,
        decoder,
        track_id,
        pcm: pcm.clone(),
        channels,
        decoded_frames: 0,
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

fn validate_backend_pcm_metadata(
    path: &Path,
    validated: ContainerPcmInfo,
    stream_spec: &macinmeter_domain::StreamSpec,
    bits_per_sample: Option<u32>,
) -> Result<(), AnalysisError> {
    if stream_spec.sample_rate.get() != validated.sample_rate
        || stream_spec.channels.get() != validated.channels
        || bits_per_sample != Some(validated.bits_per_sample)
    {
        return Err(analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "decoder metadata disagrees with the validated container PCM format",
            Some(format!(
                "container={}Hz/{}ch/{}bit; decoder={}Hz/{}ch/{bits_per_sample:?}bit",
                validated.sample_rate,
                validated.channels,
                validated.bits_per_sample,
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

pub(crate) struct SymphoniaPcmSource {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    pcm: PcmStreamInfo,
    channels: ChannelCount,
    decoded_frames: u64,
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
        let verification = self.decoder.finalize();
        if verification.verify_ok == Some(false) {
            return self.fail(analysis_error(
                &self.path,
                ErrorCode::DecodeFailed,
                AnalysisStage::Decode,
                "decoder integrity verification failed",
                None,
            ));
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
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    return self.finish();
                }
                Err(error) => {
                    let error = runtime_error(&self.path, "failed to read an audio packet", error);
                    return self.fail(error);
                }
            };
            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(error) => {
                    let error =
                        runtime_error(&self.path, "failed to decode an audio packet", error);
                    return self.fail(error);
                }
            };
            if decoded.frames() == 0 {
                continue;
            }

            let decoded_rate = decoded.spec().rate;
            let decoded_channels = decoded.spec().channels.count();
            if decoded_rate != self.pcm.spec.sample_rate.get()
                || decoded_channels != self.channels.as_usize()
            {
                let details = format!(
                    "opened as {} Hz/{} channels, decoder produced {} Hz/{} channels",
                    self.pcm.spec.sample_rate.get(),
                    self.channels.get(),
                    decoded_rate,
                    decoded_channels
                );
                let error = analysis_error(
                    &self.path,
                    ErrorCode::DecodeFailed,
                    AnalysisStage::Decode,
                    "PCM stream parameters changed after opening",
                    Some(details),
                );
                return self.fail(error);
            }

            let duration = match u64::try_from(decoded.capacity()) {
                Ok(duration) => duration,
                Err(_) => {
                    let error = analysis_error(
                        &self.path,
                        ErrorCode::ResourceExhausted,
                        AnalysisStage::Decode,
                        "decoded audio buffer is too large",
                        None,
                    );
                    return self.fail(error);
                }
            };
            let mut sample_buffer = SampleBuffer::<f64>::new(duration, *decoded.spec());
            sample_buffer.copy_interleaved_ref(decoded);
            let block = match PcmBlock::new(sample_buffer.samples().to_vec(), self.channels) {
                Ok(block) => block,
                Err(error) => {
                    let error = error
                        .with_display_path(self.path.display().to_string())
                        .with_backend(BACKEND);
                    return self.fail(error);
                }
            };

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
            return Ok(ReadOutcome::Data(block));
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

#[cfg(test)]
pub(crate) fn open_test_source(path: &Path) -> Result<SymphoniaPcmSource, AnalysisError> {
    open_source(path).map(|(_, reader)| reader)
}
