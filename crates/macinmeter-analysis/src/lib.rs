#![forbid(unsafe_code)]

//! Streaming, format-independent dynamic-range analysis.
//!
//! The crate accepts finite, frame-aligned, interleaved `f32` PCM. It keeps one
//! fixed-size RMS histogram and constant-size peak state per channel, so memory
//! use does not grow with track duration.

mod profile;
mod session;

pub use session::AnalyzerSession;
