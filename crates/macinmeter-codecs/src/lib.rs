#![forbid(unsafe_code)]

mod codec;
mod container;
mod error;
mod symphonia_source;

#[cfg(test)]
mod tests;

/// Hidden byte-oriented dev entry for local fuzz runners.
///
/// This module is not part of the product decode surface: the default API stays
/// the `Path`-based [`DecoderFactory`]. It only exists behind the non-default
/// `malformed-dev` feature so an external fuzz harness can drive the
/// first-party WAV/AIFF chunk parsers directly from in-memory bytes.
#[cfg(feature = "malformed-dev")]
#[doc(hidden)]
pub mod dev {
    use crate::container::{ContainerSignature, identify_container, inspect_aiff, inspect_wave};
    use macinmeter_domain::AnalysisError;
    use std::{io::Cursor, path::Path};

    /// Run container signature identification and, for WAV/AIFF, the full
    /// structural chunk inspection over `bytes`.
    ///
    /// FLAC bytes stop after signature identification: FLAC structure is owned
    /// by the Symphonia probe, not by the first-party chunk parsers this entry
    /// is meant to fuzz.
    pub fn probe_container_bytes(bytes: &[u8]) -> Result<(), AnalysisError> {
        let path = Path::new("<memory>");
        let mut cursor = Cursor::new(bytes);
        match identify_container(&mut cursor, path)? {
            ContainerSignature::Wave => inspect_wave(&mut cursor, path).map(|_| ()),
            ContainerSignature::Aiff => inspect_aiff(&mut cursor, path).map(|_| ()),
            ContainerSignature::Flac => Ok(()),
        }
    }
}

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

/// A sequential source of finite, interleaved `f64` PCM blocks.
pub trait PcmSource {
    /// Return the immutable format of PCM blocks produced by this source.
    fn stream_info(&self) -> &PcmStreamInfo;

    /// Decode the next non-empty, frame-aligned PCM block.
    ///
    /// Every returned block's channel geometry must equal
    /// `self.stream_info().spec.channels`.
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
