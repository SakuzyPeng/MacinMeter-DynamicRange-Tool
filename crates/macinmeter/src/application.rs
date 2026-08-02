use crate::{
    AnalysisError, AnalysisEvent, AnalysisReport, AnalysisStage, ErrorCode, ExecutionControl,
};
use macinmeter_analysis::AnalyzerSession;
use macinmeter_codecs::{DecoderFactory, OpenedAudio, ReadOutcome};
use macinmeter_domain::DecodeReservation;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

#[derive(Debug, Default)]
pub(crate) struct Analyzer {
    decoder_factory: DecoderFactory,
}

impl Analyzer {
    /// Build an analyzer that decodes inside an already-granted permit.
    pub(crate) const fn new(decode: DecodeReservation) -> Self {
        Self {
            decoder_factory: DecoderFactory::with_application_reservation(decode),
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
        let opened = self.decoder_factory.open(&request.path)?;
        Self::analyze_opened(opened, item_index, control, display_path)
    }

    pub(crate) fn analyze_opened(
        mut opened: OpenedAudio,
        item_index: usize,
        control: &ExecutionControl<'_>,
        display_path: &str,
    ) -> Result<AnalysisReport, AnalysisError> {
        let pcm = opened.reader.stream_info().clone();
        let mut session = AnalyzerSession::new(pcm.spec.clone())?;

        loop {
            ensure_not_cancelled(control)?;
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
                    session.push_interleaved(block.samples())?;
                    control.progress.emit(AnalysisEvent::DecodeProgress {
                        index: item_index,
                        display_path: display_path.to_owned(),
                        progress: opened.reader.progress(),
                    });
                }
                ReadOutcome::Eof => {
                    control.progress.emit(AnalysisEvent::DecodeProgress {
                        index: item_index,
                        display_path: display_path.to_owned(),
                        progress: opened.reader.progress(),
                    });
                    break;
                }
            }
        }

        ensure_not_cancelled(control)?;
        let analysis = session.finish()?;
        let diagnostics = opened.reader.diagnostics().clone();
        match AnalysisReport::try_new(opened.source, pcm, analysis, diagnostics) {
            Ok(report) => Ok(report),
            Err(error) => Err(error
                .with_display_path(display_path)
                .with_backend(opened.reader.diagnostics().backend.clone())),
        }
    }
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
        sync::{Arc, Mutex},
    };

    struct FakeSource {
        stream_info: PcmStreamInfo,
        blocks: VecDeque<PcmBlock>,
        decoded_frames: u64,
        eof: bool,
        diagnostics: DecodeDiagnostics,
        terminal_diagnostic_frames: Option<u64>,
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
}
