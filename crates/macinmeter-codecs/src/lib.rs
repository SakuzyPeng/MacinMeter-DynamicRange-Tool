#![forbid(unsafe_code)]

mod capability;
mod codec;
mod container;
mod decode_engine;
mod error;
mod flac_integrity;
mod isobmff;
mod packet;
mod symphonia_source;

pub use capability::{
    CapabilityStatus, NATIVE_CAPABILITY_CATALOG, NativeRouteCapability, stable_discovery_extensions,
};

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
    use crate::{
        container::{ContainerSignature, identify_container, inspect_aiff, inspect_wave},
        isobmff::inspect_isobmff_alac,
    };
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
            ContainerSignature::Mp4 => inspect_isobmff_alac(&mut cursor, path).map(|_| ()),
        }
    }
}

use macinmeter_domain::{
    AnalysisError, DecodeDiagnostics, DecodeProgress, DecodeReservation, PcmBlock, PcmStreamInfo,
    SourceInfo,
};
use std::{num::NonZeroUsize, path::Path};

/// Structured decoder failure used by the PCM source contract.
pub type DecodeError = AnalysisError;

/// A successfully opened source together with immutable source and output PCM metadata.
pub struct OpenedAudio {
    pub source: SourceInfo,
    pub reader: Box<dyn PcmSource>,
}

/// The decoder engine that an opened source actually selected.
///
/// This is a hidden first-party correctness surface, not a public tuning API.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeEngineKind {
    Serial,
    AlacPacketWorkers,
}

/// The actual execution selected after content probing and decoder creation.
///
/// A requested reservation alone cannot prove that a route used packet
/// workers, because every route except graduated ALAC intentionally falls back
/// to the serial engine. Correctness harnesses use this value to reject that
/// otherwise-silent fallback.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeExecution {
    engine: DecodeEngineKind,
    workers: NonZeroUsize,
}

impl DecodeExecution {
    pub(crate) const fn serial() -> Self {
        Self {
            engine: DecodeEngineKind::Serial,
            workers: NonZeroUsize::MIN,
        }
    }

    pub(crate) const fn alac_packet_workers(workers: NonZeroUsize) -> Self {
        Self {
            engine: DecodeEngineKind::AlacPacketWorkers,
            workers,
        }
    }

    pub const fn engine(self) -> DecodeEngineKind {
        self.engine
    }

    pub const fn workers(self) -> NonZeroUsize {
        self.workers
    }
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

/// Opens the small, correctness-first stable native codec set.
///
/// [`DecoderFactory::new`] always uses the serial route. First-party application
/// wiring may hand the factory a validated [`DecodeReservation`]; the factory
/// never widens that allocation or creates workers beyond it.
#[derive(Debug, Default, Clone, Copy)]
pub struct DecoderFactory {
    reservation: DecodeReservation,
}

impl DecoderFactory {
    /// A factory on the serial reservation.
    pub const fn new() -> Self {
        Self {
            reservation: DecodeReservation::serial(),
        }
    }

    /// First-party wiring for a factory bound to an application allocation.
    ///
    /// This cross-crate entry exists for `macinmeter::Application`; it is not a
    /// supported public worker or queue tuning surface. Direct decoder callers
    /// should use [`DecoderFactory::new`], which is permanently serial.
    #[doc(hidden)]
    pub const fn with_application_reservation(reservation: DecodeReservation) -> Self {
        Self { reservation }
    }

    pub fn open(&self, path: &Path) -> Result<OpenedAudio, AnalysisError> {
        symphonia_source::open(path, self.reservation)
    }

    /// Open a source and report the engine that content probing actually chose.
    ///
    /// This exists for first-party differential harnesses. Product callers use
    /// [`DecoderFactory::open`] and do not observe or select an engine.
    #[doc(hidden)]
    pub fn open_with_execution(
        &self,
        path: &Path,
    ) -> Result<(OpenedAudio, DecodeExecution), AnalysisError> {
        symphonia_source::open_with_execution(path, self.reservation)
    }
}
