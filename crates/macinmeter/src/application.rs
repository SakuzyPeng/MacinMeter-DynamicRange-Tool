use crate::{
    AnalysisError, AnalysisEvent, AnalysisReport, AnalysisStage, ErrorCode, ExecutionControl,
};
use macinmeter_analysis::AnalyzerSession;
use macinmeter_codecs::{DecoderFactory, OpenedAudio, ReadOutcome};
use macinmeter_domain::{DecodeReservation, PcmBlock, PcmStreamInfo};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf, sync::mpsc::sync_channel, thread};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeRequest {
    pub path: PathBuf,
}

impl AnalyzeRequest {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[cfg(test)]
thread_local! {
    /// The engine the most recent analysis on this thread actually selected.
    pub(crate) static LAST_DECODE_EXECUTION:
        std::cell::Cell<Option<macinmeter_codecs::DecodeExecution>> =
        const { std::cell::Cell::new(None) };
    /// Whether the most recent analysis on this thread ran decode and analysis
    /// on separate threads. A differential that silently stayed serial would
    /// pass while proving nothing, exactly as for the decode engine above.
    pub(crate) static LAST_ANALYSIS_OVERLAP: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[derive(Debug, Default)]
pub(crate) struct Analyzer {
    decoder_factory: DecoderFactory,
    /// The same grant the factory decodes inside. Overlap is charged against
    /// it, so this is a copy of one plan rather than a second budget.
    #[cfg(feature = "performance-probes")]
    reservation: DecodeReservation,
}

impl Analyzer {
    /// Build an analyzer that decodes inside an already-granted permit.
    pub(crate) const fn new(decode: DecodeReservation) -> Self {
        Self {
            decoder_factory: DecoderFactory::with_application_reservation(decode),
            #[cfg(feature = "performance-probes")]
            reservation: decode,
        }
    }

    pub(crate) fn analyze_file_with_control(
        &self,
        request: AnalyzeRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<AnalysisReport, AnalysisError> {
        self.analyze_file_at(request, 0, control)
    }

    pub(crate) fn analyze_file_at(
        &self,
        request: AnalyzeRequest,
        item_index: usize,
        control: &ExecutionControl<'_>,
    ) -> Result<AnalysisReport, AnalysisError> {
        ensure_not_cancelled(control)?;
        let display_path = request.path.display().to_string();
        control.progress.emit(AnalysisEvent::FileStarted {
            index: item_index,
            display_path: display_path.clone(),
        });

        let result = self.analyze_started(request, item_index, control, &display_path);
        control.progress.emit(AnalysisEvent::FileFinished {
            index: item_index,
            display_path,
            success: result.is_ok(),
        });
        result
    }

    fn analyze_started(
        &self,
        request: AnalyzeRequest,
        item_index: usize,
        control: &ExecutionControl<'_>,
        display_path: &str,
    ) -> Result<AnalysisReport, AnalysisError> {
        // Measurement builds read the selected engine too, because the permits
        // a route did not spend are exactly what decode/analysis overlap may
        // use. A requested reservation alone cannot tell them apart: every
        // route that has not graduated falls back to the serial engine.
        let (opened, execution) = self.decoder_factory.open_with_execution(&request.path)?;
        #[cfg(test)]
        LAST_DECODE_EXECUTION.with(|last| last.set(Some(execution)));
        // The candidate remains a non-default measurement path until its
        // ADR-0007 A/B and the rest of its graduation gates pass. Ordinary
        // library, CLI and GUI builds therefore receive no overlap budget.
        #[cfg(feature = "performance-probes")]
        let overlap = OverlapBudget {
            spare_permits: self
                .reservation
                .workers()
                .get()
                .saturating_sub(execution.workers().get()),
            max_in_flight_pcm_bytes: self.reservation.max_in_flight_pcm_bytes(),
            max_pcm_block_bytes: execution.max_pcm_block_bytes(),
            inject_spawn_failure: false,
        };
        #[cfg(not(feature = "performance-probes"))]
        let overlap = {
            let _ = execution;
            OverlapBudget::default()
        };
        Self::analyze_opened(opened, overlap, item_index, control, display_path)
    }

    pub(crate) fn analyze_opened(
        mut opened: OpenedAudio,
        budget: OverlapBudget,
        item_index: usize,
        control: &ExecutionControl<'_>,
        display_path: &str,
    ) -> Result<AnalysisReport, AnalysisError> {
        #[cfg(test)]
        LAST_ANALYSIS_OVERLAP.with(|last| last.set(false));
        let pcm = opened.reader.stream_info().clone();
        let mut session = AnalyzerSession::new(pcm.spec.clone())?;

        // Read one block on the calling thread first. Overlap is admitted only
        // once a real block has proven its retention fits the granted budget,
        // so a stream that cannot prove that bound stays serial before any
        // thread exists, exactly as an over-wide FLAC reorder window does.
        ensure_not_cancelled(control)?;
        let first = read_checked(&mut opened, &pcm, display_path)?;

        let analysis = match first {
            None => {
                // Preserve the serial boundary: EOF progress is observable and
                // may itself request cancellation, which must win before a
                // zero-frame report is finalized.
                emit_decode_progress(&opened, item_index, control, display_path);
                ensure_not_cancelled(control)?;
                session.finish()?
            }
            Some(block) if budget.admits(&block) => {
                // The first block establishes the same public commit boundary
                // as the serial path. Overlap begins with the next decode, so
                // analysis failures still precede this block's progress event.
                session.push_interleaved(block.samples())?;
                emit_decode_progress(&opened, item_index, control, display_path);
                ensure_not_cancelled(control)?;
                analyze_overlapped(
                    session,
                    &mut opened,
                    item_index,
                    control,
                    display_path,
                    budget,
                )?
            }
            Some(block) => {
                session.push_interleaved(block.samples())?;
                emit_decode_progress(&opened, item_index, control, display_path);
                analyze_serially(
                    session,
                    &mut opened,
                    &pcm,
                    item_index,
                    control,
                    display_path,
                )?
            }
        };

        let diagnostics = opened.reader.diagnostics().clone();
        match AnalysisReport::try_new(opened.source, pcm, analysis, diagnostics) {
            Ok(report) => Ok(report),
            Err(error) => Err(error
                .with_display_path(display_path)
                .with_backend(opened.reader.diagnostics().backend.clone())),
        }
    }
}

/// What an already-granted plan leaves available for decode/analysis overlap.
///
/// Both fields come from the one reservation the route decodes inside. Overlap
/// spends a permit that route did not, so it can never add a thread the plan
/// has not already counted.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OverlapBudget {
    spare_permits: usize,
    max_in_flight_pcm_bytes: u64,
    /// Probe-time upper bound for one decoded block. `None` means the route
    /// cannot prove safe retention and must stay serial.
    max_pcm_block_bytes: Option<u64>,
    /// Instance-owned deterministic fault injection; production always leaves
    /// it false.
    inject_spawn_failure: bool,
}

impl OverlapBudget {
    /// Whether this block's stream may run decode and analysis concurrently.
    ///
    /// Overlap retains two blocks beyond the one a serial run already holds:
    /// one handed off and one being pushed while the caller decodes the next.
    /// The decoder proves a worst-case block bound during probe. Pricing that
    /// bound rather than the first observed block keeps valid variable-block
    /// streams from making retention depend on their first packet geometry.
    fn admits(self, block: &PcmBlock) -> bool {
        let first_bytes = (block.samples().len() as u64).saturating_mul(size_of::<f64>() as u64);
        let Some(max_block_bytes) = self.max_pcm_block_bytes else {
            return false;
        };
        let retained = max_block_bytes.saturating_mul(2);
        self.spare_permits >= 1
            && first_bytes <= max_block_bytes
            && retained <= self.max_in_flight_pcm_bytes
    }
}

enum AnalysisInput {
    Block(PcmBlock),
    Finish,
}

/// Drive decode and analysis on separate threads, committing in read order.
///
/// The hand-off is a single ordered channel consumed by one thread, so the
/// analyzer sees exactly the block sequence a serial run would push and the
/// result cannot depend on the overlap.
fn analyze_overlapped(
    mut session: AnalyzerSession,
    opened: &mut OpenedAudio,
    item_index: usize,
    control: &ExecutionControl<'_>,
    display_path: &str,
    budget: OverlapBudget,
) -> Result<macinmeter_domain::AnalysisResult, AnalysisError> {
    // One queued block. A deeper queue would buy no overlap for two stages and
    // would retain PCM the plan has not priced.
    let (blocks, incoming) = sync_channel::<AnalysisInput>(1);

    thread::scope(|scope| {
        let operation = move || -> Result<Option<_>, AnalysisError> {
            loop {
                match incoming.recv() {
                    Ok(AnalysisInput::Block(block)) => {
                        session.push_interleaved(block.samples())?;
                    }
                    Ok(AnalysisInput::Finish) => {
                        // This is the serial pre-finish cancellation boundary,
                        // after every accepted block has reached the analyzer.
                        ensure_not_cancelled(control)?;
                        return session.finish().map(Some);
                    }
                    // Decode failure and cancellation deliberately disconnect
                    // without sending Finish. Do not finalize a partial prefix.
                    Err(_) => return Ok(None),
                }
            }
        };
        let builder = thread::Builder::new().name("macinmeter-analysis".to_owned());
        let spawned = if budget.inject_spawn_failure {
            Err(io::Error::other(
                "injected overlapped analysis spawn failure",
            ))
        } else {
            builder.spawn_scoped(scope, operation)
        };
        let analyst = spawned.map_err(|error| analyst_spawn_error(display_path, error))?;
        #[cfg(test)]
        LAST_ANALYSIS_OVERLAP.with(|last| last.set(true));
        let pcm = opened.reader.stream_info().clone();

        let decoded = (|| -> Result<(), AnalysisError> {
            loop {
                ensure_not_cancelled(control)?;
                let Some(block) = read_checked(opened, &pcm, display_path)? else {
                    emit_decode_progress(opened, item_index, control, display_path);
                    // A progress observer can cancel at EOF. The analysis
                    // thread performs the authoritative pre-finish check after
                    // all preceding blocks, so send the explicit terminator.
                    if blocks.send(AnalysisInput::Finish).is_err() {
                        return Ok(());
                    }
                    return Ok(());
                };
                emit_decode_progress(opened, item_index, control, display_path);
                if blocks.send(AnalysisInput::Block(block)).is_err() {
                    // The analyzer stopped early, so it holds the failure that
                    // decides this stream. Stop feeding it and let the join
                    // below report that error rather than a disconnect.
                    return Ok(());
                }
            }
        })();
        drop(blocks);

        // Join before deciding anything: the analyzer thread must be finished
        // before its session or its error can be read.
        let analysed = match analyst.join() {
            Ok(analysed) => analysed,
            Err(_) => return Err(analyst_panic_error(display_path)),
        };

        // An analysis failure is always the earlier one. The analyzer only sees
        // blocks decode already produced, so a failure at block J means decode
        // got at least to J, and any decode failure is at a later block.
        match analysed {
            Err(error) => Err(error),
            Ok(Some(analysis)) => {
                decoded?;
                Ok(analysis)
            }
            Ok(None) => match decoded {
                Err(error) => Err(error),
                Ok(()) => Err(analysis_channel_error(display_path)),
            },
        }
    })
}

/// Drive decode and analysis on the calling thread alone.
fn analyze_serially(
    mut session: AnalyzerSession,
    opened: &mut OpenedAudio,
    pcm: &PcmStreamInfo,
    item_index: usize,
    control: &ExecutionControl<'_>,
    display_path: &str,
) -> Result<macinmeter_domain::AnalysisResult, AnalysisError> {
    loop {
        ensure_not_cancelled(control)?;
        let Some(block) = read_checked(opened, pcm, display_path)? else {
            emit_decode_progress(opened, item_index, control, display_path);
            break;
        };
        session.push_interleaved(block.samples())?;
        emit_decode_progress(opened, item_index, control, display_path);
    }

    ensure_not_cancelled(control)?;
    session.finish()
}

/// Read the next block, rejecting one whose geometry contradicts the stream.
fn read_checked(
    opened: &mut OpenedAudio,
    pcm: &PcmStreamInfo,
    display_path: &str,
) -> Result<Option<PcmBlock>, AnalysisError> {
    match opened.reader.read_block()? {
        ReadOutcome::Data(block) => {
            if block.channels() != pcm.spec.channels {
                return Err(AnalysisError::new(
                    ErrorCode::DecodeFailed,
                    AnalysisStage::Decode,
                    format!(
                        "decoder produced a {}-channel PCM block for a {}-channel stream",
                        block.channels().get(),
                        pcm.spec.channels.get()
                    ),
                )
                .with_display_path(display_path)
                .with_backend(opened.reader.diagnostics().backend.clone())
                .with_details(format!(
                    "block_channels={}; stream_channels={}",
                    block.channels().get(),
                    pcm.spec.channels.get()
                )));
            }
            Ok(Some(block))
        }
        ReadOutcome::Eof => Ok(None),
    }
}

fn emit_decode_progress(
    opened: &OpenedAudio,
    item_index: usize,
    control: &ExecutionControl<'_>,
    display_path: &str,
) {
    control.progress.emit(AnalysisEvent::DecodeProgress {
        index: item_index,
        display_path: display_path.to_owned(),
        progress: opened.reader.progress(),
    });
}

fn analyst_panic_error(display_path: &str) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Analysis,
        "the overlapped analysis thread panicked",
    )
    .with_display_path(display_path)
}

fn analyst_spawn_error(display_path: &str, error: io::Error) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::ResourceExhausted,
        AnalysisStage::Analysis,
        "failed to start the overlapped analysis thread",
    )
    .with_display_path(display_path)
    .with_details(error.to_string())
    .recoverable(true)
}

fn analysis_channel_error(display_path: &str) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Analysis,
        "the overlapped analysis channel disconnected without a terminal outcome",
    )
    .with_display_path(display_path)
}

fn ensure_not_cancelled(control: &ExecutionControl<'_>) -> Result<(), AnalysisError> {
    if control.cancellation.is_cancelled() {
        Err(AnalysisError::cancelled())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CancellationToken, ChannelCount, ChannelLayout, ContainerFormat, DecodeDiagnostics,
        DecodeProgress, NoopProgressSink, PcmBlock, PcmStreamInfo, SampleRate, SourceCodec,
        SourceInfo, StreamSpec,
    };
    use macinmeter_codecs::PcmSource;
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    struct FakeSource {
        stream_info: PcmStreamInfo,
        blocks: VecDeque<PcmBlock>,
        decoded_frames: u64,
        eof: bool,
        diagnostics: DecodeDiagnostics,
        terminal_diagnostic_frames: Option<u64>,
        terminal_error: Option<AnalysisError>,
    }

    impl PcmSource for FakeSource {
        fn stream_info(&self) -> &PcmStreamInfo {
            &self.stream_info
        }

        fn read_block(&mut self) -> Result<ReadOutcome, AnalysisError> {
            if let Some(block) = self.blocks.pop_front() {
                self.decoded_frames += u64::try_from(block.frames()).unwrap();
                self.diagnostics.decoded_frames = self.decoded_frames;
                return Ok(ReadOutcome::Data(block));
            }

            if let Some(error) = self.terminal_error.as_ref() {
                return Err(error.clone());
            }

            self.eof = true;
            if let Some(decoded_frames) = self.terminal_diagnostic_frames {
                self.diagnostics.decoded_frames = decoded_frames;
            }
            Ok(ReadOutcome::Eof)
        }

        fn progress(&self) -> DecodeProgress {
            DecodeProgress::new(self.decoded_frames, None, self.eof)
        }

        fn diagnostics(&self) -> &DecodeDiagnostics {
            &self.diagnostics
        }
    }

    fn opened_audio(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
    ) -> OpenedAudio {
        opened_audio_with_terminal_diagnostics(source_channels, stream_channels, blocks, None)
    }

    fn opened_audio_with_terminal_diagnostics(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
        terminal_diagnostic_frames: Option<u64>,
    ) -> OpenedAudio {
        opened_audio_with_terminal(
            source_channels,
            stream_channels,
            blocks,
            terminal_diagnostic_frames,
            None,
        )
    }

    fn opened_audio_with_terminal_error(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
        terminal_error: AnalysisError,
    ) -> OpenedAudio {
        opened_audio_with_terminal(
            source_channels,
            stream_channels,
            blocks,
            None,
            Some(terminal_error),
        )
    }

    fn opened_audio_with_terminal(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
        terminal_diagnostic_frames: Option<u64>,
        terminal_error: Option<AnalysisError>,
    ) -> OpenedAudio {
        let stream_info = PcmStreamInfo {
            spec: StreamSpec::new(48_000, stream_channels.get(), ChannelLayout::Unknown).unwrap(),
            expected_frames: None,
        };
        OpenedAudio {
            source: SourceInfo {
                display_path: "source.fake".to_owned(),
                container: ContainerFormat::Wave,
                codec: SourceCodec::PcmFloat,
                sample_rate: SampleRate::new(48_000).unwrap(),
                channels: source_channels,
                bits_per_sample: Some(64),
                expected_frames: None,
            },
            reader: Box::new(FakeSource {
                stream_info,
                blocks: blocks.into(),
                decoded_frames: 0,
                eof: false,
                diagnostics: DecodeDiagnostics {
                    backend: "fake-source".to_owned(),
                    decoded_frames: 0,
                    warnings: Vec::new(),
                },
                terminal_diagnostic_frames,
                terminal_error,
            }),
        }
    }

    #[test]
    fn rejects_pcm_blocks_built_with_a_different_channel_geometry() {
        let stream_channels = ChannelCount::new(2).unwrap();
        let block_channels = ChannelCount::new(1).unwrap();
        let opened = opened_audio(
            block_channels,
            stream_channels,
            // If this block reaches the two-channel analyzer, squaring f64::MAX fails with an
            // analysis error. The expected decode error therefore proves geometry is checked first.
            vec![PcmBlock::new(vec![f64::MAX, 0.0], block_channels).unwrap()],
        );
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened,
            OverlapBudget::default(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "event-path.fake",
        )
        .expect_err("block geometry must match the immutable stream geometry");

        assert_eq!(error.code, ErrorCode::DecodeFailed);
        assert_eq!(error.stage, AnalysisStage::Decode);
        assert_eq!(error.display_path.as_deref(), Some("event-path.fake"));
        assert_eq!(error.backend.as_deref(), Some("fake-source"));
        let details = error.details.expect("geometry details should be present");
        assert!(details.contains("block_channels=1"));
        assert!(details.contains("stream_channels=2"));
    }

    #[test]
    fn validates_every_block_before_analysis_and_progress_publication() {
        let stream_channels = ChannelCount::new(2).unwrap();
        let opened = opened_audio(
            ChannelCount::new(1).unwrap(),
            stream_channels,
            vec![
                PcmBlock::new(vec![0.25, -0.25], stream_channels).unwrap(),
                // This must be rejected before it can mutate or fail the analyzer.
                PcmBlock::new(vec![f64::MAX, 0.0], ChannelCount::new(1).unwrap()).unwrap(),
            ],
        );
        let cancellation = CancellationToken::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_sink = Arc::clone(&events);
        let progress = move |event| {
            events_for_sink.lock().unwrap().push(event);
        };

        let error = Analyzer::analyze_opened(
            opened,
            OverlapBudget::default(),
            7,
            &ExecutionControl::new(&cancellation, &progress),
            "event-path.fake",
        )
        .expect_err("a later mismatched block must prevent a partial report");

        assert_eq!(error.code, ErrorCode::DecodeFailed);
        assert_eq!(error.stage, AnalysisStage::Decode);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AnalysisEvent::DecodeProgress {
                index: 7,
                display_path,
                progress,
            } if display_path == "event-path.fake"
                && progress.decoded_frames() == 1
                && !progress.is_eof()
        ));
    }

    #[test]
    fn rejects_diagnostics_that_disagree_with_the_finished_analysis() {
        let channels = ChannelCount::new(1).unwrap();
        let opened = opened_audio_with_terminal_diagnostics(
            channels,
            channels,
            vec![PcmBlock::new(vec![0.25], channels).unwrap()],
            Some(2),
        );
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened,
            OverlapBudget::default(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "diagnostics.fake",
        )
        .expect_err("mismatched terminal diagnostics must not escape as a successful report");

        assert_eq!(error.code, ErrorCode::DecodeFailed);
        assert_eq!(error.stage, AnalysisStage::Decode);
        assert_eq!(error.display_path.as_deref(), Some("diagnostics.fake"));
        assert_eq!(error.backend.as_deref(), Some("fake-source"));
        assert!(error.message.contains("diagnostics"));
    }

    /// A budget wide enough to admit every block these tests build.
    fn overlapping() -> OverlapBudget {
        OverlapBudget {
            spare_permits: 1,
            max_in_flight_pcm_bytes: 4 * 1024 * 1024,
            max_pcm_block_bytes: Some(256 * size_of::<f64>() as u64),
            inject_spawn_failure: false,
        }
    }

    fn varied_blocks(channels: ChannelCount) -> Vec<PcmBlock> {
        // Values chosen so every window accumulator carries a different
        // magnitude: an overlap that dropped, reordered or duplicated a block
        // would move peak, RMS or the frame count.
        (1..=64_u32)
            .map(|step| {
                let base = f64::from(step) / 512.0;
                PcmBlock::new(
                    (0..256)
                        .map(|index| {
                            let offset = f64::from(index) / 4096.0;
                            if index % 3 == 0 {
                                base - offset
                            } else {
                                offset - base
                            }
                        })
                        .collect(),
                    channels,
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn overlapped_and_serial_analysis_agree_bit_for_bit() {
        let channels = ChannelCount::new(2).unwrap();
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);

        let serial = Analyzer::analyze_opened(
            opened_audio(channels, channels, varied_blocks(channels)),
            OverlapBudget::default(),
            0,
            &control,
            "overlap.fake",
        )
        .expect("the serial path must analyze the fixture");
        assert!(
            !LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "a budget without a spare permit must not start an analysis thread"
        );

        let overlapped = Analyzer::analyze_opened(
            opened_audio(channels, channels, varied_blocks(channels)),
            overlapping(),
            0,
            &control,
            "overlap.fake",
        )
        .expect("the overlapped path must analyze the same fixture");
        assert!(
            LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "this fixture must actually overlap, or the comparison proves nothing"
        );

        // Raw bits, not an approximate compare: overlap may not perturb a
        // single float, and the analysis is the whole product of this path.
        assert_eq!(
            format!("{:?}", serial.analysis()),
            format!("{:?}", overlapped.analysis())
        );
        assert_eq!(serial.analysis().frames_seen(), 64 * 128);
    }

    #[test]
    fn overlap_stays_serial_when_retention_exceeds_the_granted_budget() {
        let channels = ChannelCount::new(2).unwrap();
        let block = PcmBlock::new(vec![0.5; 64], channels).unwrap();
        let max_block_bytes = 256 * size_of::<f64>() as u64;

        assert!(
            !OverlapBudget {
                spare_permits: 1,
                // One byte short of two worst-case blocks, even though the
                // observed first block is much smaller.
                max_in_flight_pcm_bytes: max_block_bytes * 2 - 1,
                max_pcm_block_bytes: Some(max_block_bytes),
                ..OverlapBudget::default()
            }
            .admits(&block),
            "a stream that cannot prove its retention must stay serial"
        );
        assert!(
            OverlapBudget {
                spare_permits: 1,
                max_in_flight_pcm_bytes: max_block_bytes * 2,
                max_pcm_block_bytes: Some(max_block_bytes),
                ..OverlapBudget::default()
            }
            .admits(&block)
        );
        assert!(
            !OverlapBudget {
                spare_permits: 0,
                max_in_flight_pcm_bytes: u64::MAX,
                max_pcm_block_bytes: Some(max_block_bytes),
                ..OverlapBudget::default()
            }
            .admits(&block),
            "a route that spent every permit leaves nothing for an overlap thread"
        );
        assert!(
            !OverlapBudget {
                spare_permits: 1,
                max_in_flight_pcm_bytes: u64::MAX,
                ..OverlapBudget::default()
            }
            .admits(&block),
            "a route without a probe-time block bound must stay serial"
        );
    }

    #[test]
    fn an_overlapped_analysis_failure_outranks_the_later_decode_failure() {
        let channels = ChannelCount::new(2).unwrap();
        let mut blocks = vec![PcmBlock::new(vec![0.25, 0.25], channels).unwrap()];
        // Block 1 fails in the analyzer; block 2 fails in decode. Serial order
        // reaches the analysis failure first, so overlap must report it even
        // though the decoding thread runs ahead.
        blocks.push(PcmBlock::new(vec![f64::MAX, f64::MAX], channels).unwrap());
        blocks.push(PcmBlock::new(vec![0.25], ChannelCount::new(1).unwrap()).unwrap());
        let opened = opened_audio(channels, channels, blocks);
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened,
            overlapping(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "precedence.fake",
        )
        .expect_err("the earliest failure in input order must decide the stream");

        assert_eq!(
            error.stage,
            AnalysisStage::Analysis,
            "a later decode failure must not outrank the earlier analysis failure"
        );
    }

    #[test]
    fn a_decode_failure_does_not_finalize_or_mask_itself_with_a_partial_prefix() {
        let channels = ChannelCount::new(1).unwrap();
        let terminal_error = AnalysisError::new(
            ErrorCode::DecodeFailed,
            AnalysisStage::Decode,
            "injected later decode failure",
        );
        let block = || PcmBlock::new(vec![1.0e100], channels).unwrap();

        // The accepted prefix fails only at finish when its report values are
        // narrowed. A serial decode failure arrives before finish and must
        // therefore remain the result of the overlapped path too.
        let mut prefix = AnalyzerSession::new(
            StreamSpec::new(48_000, channels.get(), ChannelLayout::Unknown).unwrap(),
        )
        .unwrap();
        prefix.push_interleaved(block().samples()).unwrap();
        assert_eq!(prefix.finish().unwrap_err().stage, AnalysisStage::Analysis);

        let serial_error = Analyzer::analyze_opened(
            opened_audio_with_terminal_error(
                channels,
                channels,
                vec![block()],
                terminal_error.clone(),
            ),
            OverlapBudget::default(),
            0,
            &ExecutionControl::new(&CancellationToken::new(), &NoopProgressSink),
            "decode-error.fake",
        )
        .expect_err("the serial oracle must report the decoder failure");
        let overlapped_error = Analyzer::analyze_opened(
            opened_audio_with_terminal_error(channels, channels, vec![block()], terminal_error),
            overlapping(),
            0,
            &ExecutionControl::new(&CancellationToken::new(), &NoopProgressSink),
            "decode-error.fake",
        )
        .expect_err("disconnect must not finalize a partial analysis prefix");

        assert_eq!(overlapped_error.code, ErrorCode::DecodeFailed);
        assert_eq!(overlapped_error.stage, AnalysisStage::Decode);
        assert_eq!(overlapped_error.message, serial_error.message);
    }

    #[test]
    fn analysis_thread_spawn_failure_is_structured() {
        let channels = ChannelCount::new(1).unwrap();
        let mut budget = overlapping();
        budget.inject_spawn_failure = true;
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened_audio(
                channels,
                channels,
                vec![PcmBlock::new(vec![0.25], channels).unwrap()],
            ),
            budget,
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "spawn.fake",
        )
        .expect_err("thread construction failure must not unwind the application");

        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Analysis);
        assert_eq!(error.display_path.as_deref(), Some("spawn.fake"));
        assert!(error.recoverable);
        assert!(
            !LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "a failed spawn must not claim that overlap actually ran"
        );
        assert!(
            error
                .details
                .as_deref()
                .is_some_and(|details| details.contains("injected"))
        );
    }

    #[test]
    fn a_cancelled_overlap_joins_its_analysis_thread_and_reports_no_report() {
        let channels = ChannelCount::new(2).unwrap();
        let opened = opened_audio(channels, channels, varied_blocks(channels));
        let cancellation = CancellationToken::new();
        let token_for_progress = cancellation.clone();
        let progress_events = Arc::new(AtomicUsize::new(0));
        let events_for_progress = Arc::clone(&progress_events);
        let progress = move |event| {
            if matches!(event, AnalysisEvent::DecodeProgress { .. })
                && events_for_progress.fetch_add(1, Ordering::Relaxed) == 1
            {
                token_for_progress.cancel();
            }
        };

        let error = Analyzer::analyze_opened(
            opened,
            overlapping(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "cancel.fake",
        )
        .expect_err("a cancelled overlap must not produce a partial report");

        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(
            progress_events.load(Ordering::Relaxed) >= 2,
            "cancellation must happen after the overlap thread starts"
        );
        assert!(
            LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "cancellation coverage must actually start the overlap path"
        );
    }

    #[test]
    fn cancellation_at_zero_frame_eof_prevents_a_report() {
        let channels = ChannelCount::new(1).unwrap();
        let cancellation = CancellationToken::new();
        let token_for_progress = cancellation.clone();
        let progress = move |event| {
            if matches!(
                event,
                AnalysisEvent::DecodeProgress { progress, .. } if progress.is_eof()
            ) {
                token_for_progress.cancel();
            }
        };

        let error = Analyzer::analyze_opened(
            opened_audio(channels, channels, Vec::new()),
            overlapping(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "empty.fake",
        )
        .expect_err("EOF cancellation must win before an empty report is finalized");

        assert_eq!(error.code, ErrorCode::Cancelled);
    }
}
