#![forbid(unsafe_code)]

mod codec;
mod container;
mod error;
mod symphonia_source;

#[cfg(test)]
mod tests;

use macinmeter_domain::{
    AnalysisError, DecodeDiagnostics, DecodeProgress, PcmBlock, PcmStreamInfo, SourceInfo,
};
use std::path::Path;

/// Structured decoder failure used by the PCM source contract.
pub type DecodeError = AnalysisError;

/// Extensions that the discovery layer may use to find M0 inputs.
///
/// Extensions are not trusted during probing. [`DecoderFactory::open`] only accepts files whose
/// content has a supported container signature.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["wav", "wave", "flac", "aif", "aiff"];

/// A successfully opened source together with immutable source and output PCM metadata.
pub struct OpenedAudio {
    pub source: SourceInfo,
    pub reader: Box<dyn PcmSource>,
}

/// The only non-error outcomes of a synchronous PCM read.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadOutcome {
    Data(PcmBlock),
    Eof,
}

/// A sequential source of finite, interleaved `f32` PCM blocks.
pub trait PcmSource {
    /// Return the immutable format of PCM blocks produced by this source.
    fn stream_info(&self) -> &PcmStreamInfo;

    /// Decode the next non-empty, frame-aligned PCM block.
    ///
    /// Once EOF is returned, every later call must also return EOF. Once an
    /// error is returned, later calls must return the same structured error
    /// and may never resume with data.
    fn read_block(&mut self) -> Result<ReadOutcome, DecodeError>;

    /// Return progress in decoded PCM frames.
    fn progress(&self) -> DecodeProgress;

    /// Return diagnostics accumulated by this decoder instance.
    fn diagnostics(&self) -> &DecodeDiagnostics;
}

/// Opens the small, correctness-first M0 codec set.
#[derive(Debug, Default, Clone, Copy)]
pub struct DecoderFactory;

impl DecoderFactory {
    pub const fn new() -> Self {
        Self
    }

    pub fn open(&self, path: &Path) -> Result<OpenedAudio, AnalysisError> {
        symphonia_source::open(path)
    }
}
