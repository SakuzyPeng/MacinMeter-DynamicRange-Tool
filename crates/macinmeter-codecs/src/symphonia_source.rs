#[cfg(feature = "performance-probes")]
use crate::performance_probe::PacketPipelineProbe;
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
    flac_integrity::{ASYNC_HASH_EXTRA_PACKETS, FlacIntegrityPlan, FlacVerification},
    isobmff::{IsoBmffAlacInfo, inspect_isobmff_alac, is_unrepresentable_rate_sentinel},
    packet::{DecodedPacket, PacketOutcome, PacketReorderBuffer},
};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelLayout, DecodeDiagnostics, DecodeProgress,
    DecodeReservation, ErrorCode, MAX_DECODE_WORKERS, PcmBlock, PcmStreamInfo, SourceInfo,
    StreamSpec,
};
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    num::NonZeroUsize,
    path::{Path, PathBuf},
};
#[cfg(feature = "performance-probes")]
use std::{sync::Arc, time::Instant};
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

#[cfg(feature = "performance-probes")]
pub(crate) fn open_with_performance_probe(
    path: &Path,
    reservation: DecodeReservation,
) -> Result<(OpenedAudio, DecodeExecution, Arc<PacketPipelineProbe>), AnalysisError> {
    let probe = Arc::new(PacketPipelineProbe::default());
    let options = PoolOptions::with_probe(Arc::clone(&probe));
    let (source, reader, execution) = open_source_with_pool_options(path, reservation, options)?;
    let opened = OpenedAudio {
        source,
        reader: Box::new(reader),
    };
    Ok((opened, execution, probe))
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
    #[cfg(feature = "performance-probes")]
    let probe = pool_options.probe().cloned();
    #[cfg(feature = "performance-probes")]
    let phase_started = Instant::now();
    let mut file = File::open(path).map_err(|error| file_open_error(path, error))?;
    let signature = identify_container(&mut file, path)?;
    #[cfg(feature = "performance-probes")]
    if let Some(probe) = probe.as_ref() {
        probe.add_file_identify(phase_started);
    }
    #[cfg(feature = "performance-probes")]
    let phase_started = Instant::now();
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
    #[cfg(feature = "performance-probes")]
    if let Some(probe) = probe.as_ref() {
        probe.add_container_inspection(phase_started);
    }
    #[cfg(feature = "performance-probes")]
    let phase_started = Instant::now();
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
    #[cfg(feature = "performance-probes")]
    if let Some(probe) = probe.as_ref() {
        probe.add_backend_probe(phase_started);
    }
    #[cfg(feature = "performance-probes")]
    let phase_started = Instant::now();

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
    let max_pcm_block_bytes = maximum_pcm_block_bytes(&codec_params, alac_info.as_ref(), channels);

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
    #[cfg(feature = "performance-probes")]
    let context = {
        let mut context = context;
        if let Some(probe) = probe.as_ref() {
            context.attach_probe(Arc::clone(probe));
        }
        context
    };
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
    let parallel = if reservation.workers().get() <= 1 {
        None
    } else if alac_info.is_some() {
        Some(ParallelExecution {
            route: ParallelRoute::Alac,
            decode_reservation: reservation,
            hasher_workers: 0,
        })
    } else if codec_params.codec == CODEC_TYPE_FLAC {
        flac_parallel_execution(&codec_params, channels, integrity_plan, reservation)
    } else {
        None
    };

    // A signed FLAC parallel route spends exactly one of the source's worker
    // permits on the same ordered verifier used by the serial oracle. Start it
    // before decoder workers so an injected or real spawn failure cannot leave
    // a partially constructed packet pool behind.
    let integrity = match integrity_plan {
        Some(plan) if parallel.is_some_and(|execution| execution.hasher_workers == 1) => {
            Some(FlacVerification::spawn(
                path,
                plan,
                pool_options.hasher(),
                #[cfg(feature = "performance-probes")]
                probe.clone(),
            )?)
        }
        Some(plan) => Some(FlacVerification::inline(plan)),
        None => None,
    };

    let (engine, execution, reorder_reservation) = if let Some(parallel) = parallel {
        let engine = PacketWorkerPool::new(
            parallel.route,
            context,
            format,
            &codec_params,
            track_id,
            parallel.decode_reservation,
            pool_options,
        )?;
        (
            PacketEngine::PacketWorkers(engine),
            DecodeExecution::packet_workers(
                parallel.route,
                reservation.workers(),
                parallel.decode_reservation.workers(),
                parallel.hasher_workers,
            ),
            parallel.decode_reservation,
        )
    } else {
        #[cfg(feature = "performance-probes")]
        if let Some(probe) = probe.as_ref() {
            probe.set_decoder_workers(1);
        }
        let decoder_options = DecoderOptions {
            verify: context.backend_verification(),
        };
        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &decoder_options)
            .map_err(|error| decoder_creation_error(path, error))?;
        (
            PacketEngine::Serial(SerialEngine::new(context, format, decoder, track_id)),
            DecodeExecution::serial(),
            DecodeReservation::serial(),
        )
    };
    let execution = execution.with_max_pcm_block_bytes(max_pcm_block_bytes);

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
        reorder: PacketReorderBuffer::new(reorder_reservation),
        integrity,
        terminal: TerminalState::Active,
        #[cfg(feature = "performance-probes")]
        probe: probe.clone(),
        #[cfg(test)]
        injected_read_error: None,
        diagnostics: DecodeDiagnostics {
            backend: BACKEND.to_owned(),
            decoded_frames: 0,
            warnings: Vec::new(),
        },
    };

    #[cfg(feature = "performance-probes")]
    if let Some(probe) = probe.as_ref() {
        probe.add_route_setup(phase_started);
    }

    Ok((source, reader, execution))
}

#[derive(Debug, Clone, Copy)]
struct ParallelExecution {
    route: ParallelRoute,
    decode_reservation: DecodeReservation,
    hasher_workers: usize,
}

/// Subdivide one FLAC allocation without widening any application-owned bound.
///
/// The pending map accepts up to `queue_capacity` later results while an
/// earlier packet is outstanding. Each such result owns interleaved `f64` PCM
/// and, for a signed stream, its ordered-integrity bytes. STREAMINFO's maximum
/// block length also bounds the hasher's one in-progress and one queued
/// signature buffer. Signed streams spend one total worker permit on that
/// hasher and hand the decoder/reorder path the remaining workers and bytes.
/// Missing or overflowing geometry degrades to the stable serial oracle before
/// any thread is created.
fn flac_parallel_execution(
    codec_params: &CodecParameters,
    channels: macinmeter_domain::ChannelCount,
    integrity_plan: Option<FlacIntegrityPlan>,
    reservation: DecodeReservation,
) -> Option<ParallelExecution> {
    let max_frames = flac_max_frames_per_packet(codec_params)?;
    let samples_per_packet = max_frames.checked_mul(u64::from(channels.get()))?;
    // The equal-permit Windows A/B graduated this overlap only at the fixed
    // eight-worker product ceiling. At two and four total permits, giving up a
    // decoder cost 46–51% and 13–31% respectively in the direct decode sweep;
    // those unmeasured application allocations therefore retain every decoder
    // and the inline verifier. Never extrapolate the eight-worker win downward.
    let hasher_workers =
        if integrity_plan.is_some() && reservation.workers().get() == MAX_DECODE_WORKERS {
            1
        } else {
            0
        };
    let decoder_workers =
        NonZeroUsize::new(reservation.workers().get().checked_sub(hasher_workers)?)?;
    let hash_bytes = match (integrity_plan, hasher_workers) {
        (Some(plan), 1) => samples_per_packet
            .checked_mul(plan.retained_bytes_per_sample())
            .and_then(|bytes| bytes.checked_mul(ASYNC_HASH_EXTRA_PACKETS))?,
        _ => 0,
    };
    let reorder_bytes = reservation
        .max_in_flight_pcm_bytes()
        .checked_sub(hash_bytes)?;
    let decode_reservation =
        DecodeReservation::new(decoder_workers, reservation.queue_capacity(), reorder_bytes)
            .ok()?;
    let retained_bytes_per_sample = (size_of::<f64>() as u64)
        .saturating_add(integrity_plan.map_or(0, FlacIntegrityPlan::retained_bytes_per_sample));
    samples_per_packet
        .checked_mul(retained_bytes_per_sample)
        .and_then(|packet_bytes| {
            packet_bytes.checked_mul(decode_reservation.queue_capacity().get() as u64)
        })
        .filter(|window_bytes| *window_bytes <= reorder_bytes)?;
    Some(ParallelExecution {
        route: ParallelRoute::Flac,
        decode_reservation,
        hasher_workers,
    })
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

/// Price the largest decoded block before any upper-layer retention starts.
///
/// RIFF/AIFF and ALAC publish their packet ceiling directly. Symphonia does
/// not publish FLAC's STREAMINFO maximum on every probe path, so FLAC reuses
/// the same validated field extraction as its reorder-window admission.
fn maximum_pcm_block_bytes(
    codec_params: &CodecParameters,
    alac_info: Option<&IsoBmffAlacInfo>,
    channels: macinmeter_domain::ChannelCount,
) -> Option<u64> {
    let max_frames = if let Some(info) = alac_info {
        Some(info.max_frames_per_packet)
    } else if codec_params.codec == CODEC_TYPE_FLAC {
        flac_max_frames_per_packet(codec_params)
    } else {
        codec_params
            .max_frames_per_packet
            .filter(|frames| *frames > 0)
    }?;
    max_frames
        .checked_mul(u64::from(channels.get()))
        .and_then(|samples| samples.checked_mul(size_of::<f64>() as u64))
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
        let execution =
            flac_parallel_execution(&conventional, channels, conventional_integrity, granted)
                .expect("conventional signed FLAC must fit");
        assert_eq!(execution.decode_reservation.workers().get(), 7);
        assert_eq!(execution.hasher_workers, 1);
        assert!(
            execution.decode_reservation.max_in_flight_pcm_bytes()
                < granted.max_in_flight_pcm_bytes(),
            "the hasher window must be removed from the reorder permit"
        );

        // 65,535 frames × 8 channels retain 5,767,080 bytes per signed
        // packet. The reservation cannot cover its full pending window, so
        // starting workers would make success depend on completion order.
        let mut large = signed_flac(None, 24);
        let mut stream_info = vec![0_u8; 34];
        stream_info[2..4].copy_from_slice(&u16::MAX.to_be_bytes());
        large.extra_data = Some(stream_info.into_boxed_slice());
        let large_integrity = FlacIntegrityPlan::for_stream(path, &large).unwrap();
        assert!(flac_parallel_execution(&large, channels, large_integrity, granted,).is_none());

        let unknown = signed_flac(None, 24);
        let unknown_integrity = FlacIntegrityPlan::for_stream(path, &unknown).unwrap();
        assert!(flac_parallel_execution(&unknown, channels, unknown_integrity, granted,).is_none());
    }

    #[test]
    fn async_hash_buffers_are_reserved_beyond_the_reorder_window() {
        let path = Path::new("tight-signed.flac");
        let channels = ChannelCount::new(2).unwrap();
        let params = signed_flac(Some(4096), 24);
        let integrity = FlacIntegrityPlan::for_stream(path, &params).unwrap();
        let packet_samples = 4096_u64 * 2;
        let reorder_window = packet_samples * (8 + 3) * MAX_DECODE_WORKERS as u64;
        let hash_window = packet_samples * 3 * ASYNC_HASH_EXTRA_PACKETS;

        let reorder_only = DecodeReservation::new(
            NonZeroUsize::new(MAX_DECODE_WORKERS).unwrap(),
            NonZeroUsize::new(MAX_DECODE_WORKERS).unwrap(),
            reorder_window,
        )
        .unwrap();
        assert!(
            flac_parallel_execution(&params, channels, integrity, reorder_only).is_none(),
            "a permit that covers only reordered payloads must not hide hash buffers"
        );

        let complete = DecodeReservation::new(
            NonZeroUsize::new(MAX_DECODE_WORKERS).unwrap(),
            NonZeroUsize::new(MAX_DECODE_WORKERS).unwrap(),
            reorder_window + hash_window,
        )
        .unwrap();
        let execution = flac_parallel_execution(&params, channels, integrity, complete)
            .expect("the full reorder plus hash window must fit exactly");
        assert_eq!(execution.decode_reservation.workers().get(), 7);
        assert_eq!(execution.hasher_workers, 1);
        assert_eq!(
            execution.decode_reservation.max_in_flight_pcm_bytes(),
            reorder_window
        );
    }

    #[test]
    fn inline_hashing_does_not_reserve_async_buffers() {
        let path = Path::new("inline-signed.flac");
        let channels = ChannelCount::new(2).unwrap();
        // This valid block geometry fits the complete four-worker reorder
        // window, but only if the two buffers owned exclusively by the async
        // hasher are not charged to the inline verifier.
        let params = signed_flac(Some(47_000), 24);
        let integrity = FlacIntegrityPlan::for_stream(path, &params).unwrap();
        let granted = reservation(4);
        let packet_samples = 47_000_u64 * 2;
        let reorder_window = packet_samples * (8 + 3) * granted.queue_capacity().get() as u64;
        assert!(reorder_window <= granted.max_in_flight_pcm_bytes());

        let execution = flac_parallel_execution(&params, channels, integrity, granted)
            .expect("inline verification must not force a fitting stream to serial");
        assert_eq!(execution.hasher_workers, 0);
        assert_eq!(execution.decode_reservation.workers().get(), 4);
        assert_eq!(
            execution.decode_reservation.max_in_flight_pcm_bytes(),
            granted.max_in_flight_pcm_bytes()
        );
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
    integrity: Option<FlacVerification>,
    terminal: TerminalState,
    #[cfg(feature = "performance-probes")]
    probe: Option<Arc<PacketPipelineProbe>>,
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
            .as_mut()
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

    fn finish_observed(&mut self) -> Result<ReadOutcome, AnalysisError> {
        #[cfg(feature = "performance-probes")]
        let started = Instant::now();
        let result = self.finish();
        #[cfg(feature = "performance-probes")]
        if let Some(probe) = self.probe.as_ref() {
            probe.add_caller_finish(started);
        }
        result
    }

    fn commit_observed(&mut self, packet: DecodedPacket) -> Result<ReadOutcome, AnalysisError> {
        #[cfg(feature = "performance-probes")]
        let started = Instant::now();
        let result = self.commit(packet);
        #[cfg(feature = "performance-probes")]
        if let Some(probe) = self.probe.as_ref() {
            probe.add_caller_commit(started);
        }
        result
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
                    PacketOutcome::Decoded(packet) => return self.commit_observed(packet),
                    PacketOutcome::Empty => continue,
                    PacketOutcome::Failed(error) => return self.fail(error),
                }
            }

            let (index, outcome) = match self.engine.next() {
                Ok(EngineOutcome::Indexed { index, outcome }) => (index, outcome),
                Ok(EngineOutcome::Exhausted) => return self.finish_observed(),
                Err(error) => return self.fail(error),
            };
            #[cfg(feature = "performance-probes")]
            let stalled = index != self.reorder.next_index();
            let accepted = self.reorder.accept(index, outcome);
            #[cfg(feature = "performance-probes")]
            if let Some(probe) = self.probe.as_ref() {
                let (packets, bytes) = self.reorder.pending_geometry();
                probe.observe_reorder(stalled, packets, bytes);
            }
            match accepted {
                Ok(Some(PacketOutcome::Decoded(packet))) => {
                    return self.commit_observed(packet);
                }
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
        if let (Some(verifier), Some(bytes)) = (self.integrity.as_mut(), integrity)
            && let Err(error) = verifier.commit(bytes)
        {
            return self.fail(error);
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
