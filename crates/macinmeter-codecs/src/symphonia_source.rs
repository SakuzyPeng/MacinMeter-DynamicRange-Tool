use crate::{
    DecodeExecution, OpenedAudio, PcmSource, ReadOutcome,
    codec::{source_bits_per_sample, stream_spec, validate_codec},
    container::{
        ContainerPcmInfo, ContainerSignature, container_format, identify_container, inspect_aiff,
        inspect_wave, media_source,
    },
    decode_engine::{
        EngineOutcome, PacketDecodeContext, PacketEngine, PacketWorkerPool, ParallelRoute,
        PoolOptions, SerialEngine,
    },
    error::{
        BACKEND, analysis_error, decoder_creation_error, file_open_error, io_analysis_error,
        probe_error,
    },
    flac_integrity::{FlacIntegrityPlan, FlacStreamVerifier},
    isobmff::{IsoBmffAlacInfo, inspect_isobmff_alac, is_unrepresentable_rate_sentinel},
    packet::{DecodedPacket, PacketOutcome, PacketReorderBuffer},
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
    codecs::{CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_NULL, CodecParameters, DecoderOptions},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

pub(crate) fn open(
    path: &Path,
    reservation: DecodeReservation,
) -> Result<OpenedAudio, AnalysisError> {
    open_with_execution(path, reservation).map(|(opened, _)| opened)
}

pub(crate) fn open_with_execution(
    path: &Path,
    reservation: DecodeReservation,
) -> Result<(OpenedAudio, DecodeExecution), AnalysisError> {
    let (source, reader, execution) = open_source(path, reservation)?;
    let opened = OpenedAudio {
        source,
        reader: Box::new(reader),
    };
    Ok((opened, execution))
}

fn open_source(
    path: &Path,
    reservation: DecodeReservation,
) -> Result<(SourceInfo, SymphoniaPcmSource, DecodeExecution), AnalysisError> {
    open_source_with_pool_options(path, reservation, PoolOptions::default())
}

fn open_source_with_pool_options(
    path: &Path,
    reservation: DecodeReservation,
    pool_options: PoolOptions,
) -> Result<(SourceInfo, SymphoniaPcmSource, DecodeExecution), AnalysisError> {
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

    // ADR-0014 §4: FLAC stream verification is the product's, on every route.
    // Keeping one implementation for serial and parallel is what makes the
    // serial oracle a valid reference for the parallel verdict.
    let integrity_plan = FlacIntegrityPlan::for_stream(path, &codec_params)?;
    let context = PacketDecodeContext::new(
        path,
        stream_spec.sample_rate.get(),
        channels,
        integrity_plan,
    );
    // ADR-0014 §2/§3: packet workers are created only for a route that has
    // graduated, never from an extension or a generic codec descriptor. Every
    // other route, and any single-worker allocation, stays on the serial oracle.
    //
    // FLAC qualifies because its frames are independently coded and its stream
    // signature is verified by the product in commit order rather than inside a
    // decoder; see `flac_integrity`. Its variable block geometry must also fit
    // the granted reorder memory in the worst case. A stream that cannot prove
    // that bound degrades before decoding starts instead of failing depending
    // on which packet happens to complete first.
    let parallel_route = if alac_info.is_some() {
        Some(ParallelRoute::Alac)
    } else if codec_params.codec == CODEC_TYPE_FLAC
        && flac_reorder_window_fits(&codec_params, channels, integrity_plan, reservation)
    {
        Some(ParallelRoute::Flac)
    } else {
        None
    };
    let (engine, execution) = if let Some(route) = parallel_route
        && reservation.workers().get() > 1
    {
        let engine = PacketWorkerPool::new(
            route,
            context,
            format,
            &codec_params,
            track_id,
            reservation,
            pool_options,
        )?;
        (
            PacketEngine::PacketWorkers(engine),
            DecodeExecution::packet_workers(route, reservation.workers()),
        )
    } else {
        let decoder_options = DecoderOptions {
            verify: context.backend_verification(),
        };
        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &decoder_options)
            .map_err(|error| decoder_creation_error(path, error))?;
        (
            PacketEngine::Serial(SerialEngine::new(context, format, decoder, track_id)),
            DecodeExecution::serial(),
        )
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
        integrity: integrity_plan.map(FlacStreamVerifier::new),
        terminal: TerminalState::Active,
        #[cfg(test)]
        injected_read_error: None,
        diagnostics: DecodeDiagnostics {
            backend: BACKEND.to_owned(),
            decoded_frames: 0,
            warnings: Vec::new(),
        },
    };

    Ok((source, reader, execution))
}

/// Whether every FLAC packet the probed stream permits can be retained inside
/// the application-owned reorder reservation.
///
/// The pending map accepts up to `queue_capacity` later results while an
/// earlier packet is outstanding. Each such result owns interleaved `f64` PCM
/// and, for a signed stream, its ordered-integrity bytes. STREAMINFO's maximum
/// block length is immutable after probing and enforced by the decoder, so it
/// is the only media geometry used here; it may shrink concurrency but can
/// never enlarge the reservation. Missing or overflowing geometry is not a
/// decode failure: the stable serial oracle remains valid and bounded.
fn flac_reorder_window_fits(
    codec_params: &CodecParameters,
    channels: macinmeter_domain::ChannelCount,
    integrity_plan: Option<FlacIntegrityPlan>,
    reservation: DecodeReservation,
) -> bool {
    let Some(max_frames) = flac_max_frames_per_packet(codec_params) else {
        return false;
    };
    let retained_bytes_per_sample = (size_of::<f64>() as u64)
        .saturating_add(integrity_plan.map_or(0, FlacIntegrityPlan::retained_bytes_per_sample));
    max_frames
        .checked_mul(u64::from(channels.get()))
        .and_then(|samples| samples.checked_mul(retained_bytes_per_sample))
        .and_then(|packet_bytes| {
            packet_bytes.checked_mul(reservation.queue_capacity().get() as u64)
        })
        .is_some_and(|window_bytes| window_bytes <= reservation.max_in_flight_pcm_bytes())
}

/// Read the maximum FLAC block length used by the decoder.
///
/// Symphonia 0.5.5's native FLAC demuxer attaches the parsed 34-byte
/// STREAMINFO body to `extra_data` but only its decoder-local parameter clone
/// publishes `max_frames_per_packet`. Reading the same big-endian field here
/// makes the pre-decode resource decision from the exact bytes that decoder
/// later enforces. The explicit parameter remains first for synthetic tests
/// and any backend version that publishes it at probe time.
fn flac_max_frames_per_packet(codec_params: &CodecParameters) -> Option<u64> {
    codec_params
        .max_frames_per_packet
        .filter(|frames| *frames > 0)
        .or_else(|| {
            let stream_info = codec_params.extra_data.as_deref()?;
            let bytes: [u8; 2] = stream_info.get(2..4)?.try_into().ok()?;
            Some(u64::from(u16::from_be_bytes(bytes))).filter(|frames| *frames > 0)
        })
}

#[cfg(test)]
mod flac_reorder_window_tests {
    use super::*;
    use macinmeter_domain::ChannelCount;
    use std::num::NonZeroUsize;
    use symphonia::core::codecs::VerificationCheck;

    fn reservation(workers: usize) -> DecodeReservation {
        DecodeReservation::new(
            NonZeroUsize::new(workers).unwrap(),
            NonZeroUsize::new(workers * 4).unwrap(),
            workers as u64 * 4 * 1024 * 1024,
        )
        .unwrap()
    }

    fn signed_flac(max_frames: Option<u64>, bits_per_sample: u32) -> CodecParameters {
        let mut params = CodecParameters::new();
        params.codec = CODEC_TYPE_FLAC;
        params.max_frames_per_packet = max_frames;
        params.bits_per_sample = Some(bits_per_sample);
        params.verification_check = Some(VerificationCheck::Md5([0x5a; 16]));
        params
    }

    #[test]
    fn large_multichannel_blocks_degrade_before_exceeding_reorder_memory() {
        let path = Path::new("large-multichannel.flac");
        let channels = ChannelCount::new(8).unwrap();
        let granted = reservation(8);

        let conventional = signed_flac(Some(4096), 24);
        let conventional_integrity = FlacIntegrityPlan::for_stream(path, &conventional).unwrap();
        assert!(flac_reorder_window_fits(
            &conventional,
            channels,
            conventional_integrity,
            granted,
        ));

        // 65,535 frames × 8 channels retain 5,767,080 bytes per signed
        // packet. The reservation cannot cover its full pending window, so
        // starting workers would make success depend on completion order.
        let mut large = signed_flac(None, 24);
        let mut stream_info = vec![0_u8; 34];
        stream_info[2..4].copy_from_slice(&u16::MAX.to_be_bytes());
        large.extra_data = Some(stream_info.into_boxed_slice());
        let large_integrity = FlacIntegrityPlan::for_stream(path, &large).unwrap();
        assert!(!flac_reorder_window_fits(
            &large,
            channels,
            large_integrity,
            granted,
        ));

        let unknown = signed_flac(None, 24);
        let unknown_integrity = FlacIntegrityPlan::for_stream(path, &unknown).unwrap();
        assert!(!flac_reorder_window_fits(
            &unknown,
            channels,
            unknown_integrity,
            granted,
        ));
    }
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
    // The backend reads the same 16.16 AudioSampleEntry field, so when the real
    // rate cannot be represented there it reports the same sentinel the
    // first-party parser accepted rather than the cookie rate.
    let sample_rate_matches = codec_params.sample_rate == Some(validated.pcm.sample_rate)
        || (validated.pcm.sample_rate > u32::from(u16::MAX)
            && codec_params
                .sample_rate
                .is_some_and(is_unrepresentable_rate_sentinel));
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
    /// The one hasher for this stream's FLAC signature, fed only by `commit`.
    integrity: Option<FlacStreamVerifier>,
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

        // The product's own verdict lands where the backend's used to, so
        // integrity still precedes the declared frame count in error order.
        let verified = self
            .integrity
            .as_ref()
            .map_or(Ok(()), |verifier| verifier.finish(&self.path));
        if let Err(error) = verified {
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
                    PacketOutcome::Decoded(packet) => return self.commit(packet),
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
                Ok(Some(PacketOutcome::Decoded(packet))) => return self.commit(packet),
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
    fn commit(&mut self, packet: DecodedPacket) -> Result<ReadOutcome, AnalysisError> {
        let DecodedPacket { block, integrity } = packet;
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
        // The signature only ever advances on a packet that has already been
        // accepted here, so the digest follows input packet order exactly and a
        // packet can never be counted twice or skipped.
        if self.integrity.is_some() && integrity.is_none() {
            return self.fail(analysis_error(
                &self.path,
                ErrorCode::Internal,
                AnalysisStage::Internal,
                "a committed packet carried no stream signature bytes",
                None,
            ));
        }
        if let (Some(verifier), Some(bytes)) = (self.integrity.as_mut(), integrity) {
            verifier.commit(&bytes);
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
    open_source(path, DecodeReservation::serial()).map(|(_, reader, _)| reader)
}

#[cfg(test)]
pub(crate) fn open_test_source_with_pool_options(
    path: &Path,
    reservation: DecodeReservation,
    options: PoolOptions,
) -> Result<SymphoniaPcmSource, AnalysisError> {
    open_source_with_pool_options(path, reservation, options).map(|(_, reader, _)| reader)
}

#[cfg(test)]
impl SymphoniaPcmSource {
    /// Results so far that had to wait for an earlier packet index.
    pub(crate) const fn stalled_accepts(&self) -> usize {
        self.reorder.stalled_accepts()
    }
}
