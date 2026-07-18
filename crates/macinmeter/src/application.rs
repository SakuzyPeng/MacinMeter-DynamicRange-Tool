use crate::{
    AnalysisError, AnalysisEvent, AnalysisProfile, AnalysisReport, CancellationToken,
    ExecutionControl, NoopProgressSink,
};
use macinmeter_analysis::AnalyzerSession;
use macinmeter_codecs::{DecoderFactory, ReadOutcome};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeRequest {
    pub path: PathBuf,
    pub profile: AnalysisProfile,
}

impl AnalyzeRequest {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            profile: AnalysisProfile::FooDrMeter108CandidateV1,
        }
    }
}

#[derive(Debug, Default)]
pub struct Analyzer {
    decoder_factory: DecoderFactory,
}

impl Analyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn analyze_file(&self, request: AnalyzeRequest) -> Result<AnalysisReport, AnalysisError> {
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        self.analyze_file_with_control(request, &ExecutionControl::new(&cancellation, &progress))
    }

    pub fn analyze_file_with_control(
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
        let mut opened = self.decoder_factory.open(&request.path)?;
        let pcm = opened.reader.stream_info().clone();
        let mut session = AnalyzerSession::new(pcm.spec.clone(), request.profile)?;

        loop {
            ensure_not_cancelled(control)?;
            match opened.reader.read_block()? {
                ReadOutcome::Data(block) => {
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
        let analysis = session.finish();
        let diagnostics = opened.reader.diagnostics().clone();
        let report = AnalysisReport {
            source: opened.source,
            pcm,
            analysis,
            diagnostics,
        };
        Ok(report)
    }
}

fn ensure_not_cancelled(control: &ExecutionControl<'_>) -> Result<(), AnalysisError> {
    if control.cancellation.is_cancelled() {
        Err(AnalysisError::cancelled())
    } else {
        Ok(())
    }
}
