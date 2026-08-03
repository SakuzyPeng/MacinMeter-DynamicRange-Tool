//! The product's ordered full-stream FLAC signature check.
//!
//! ADR-0014 §4 blocks FLAC packet workers until stream-level verification runs
//! in input packet order. Symphonia keeps its MD5 validator inside the decoder,
//! so N worker-local decoders would each hash their own subsequence and none
//! would reproduce the STREAMINFO signature.
//!
//! This module moves the check to the product and splits it along the only line
//! that parallelizes: turning a decoded packet into the bytes the signature
//! covers is per-packet work, while hashing them is inherently sequential. The
//! product therefore owns FLAC verification on *every* route, serial included,
//! so the serial oracle and any packet workers cannot reach different verdicts.
//!
//! The hash function itself is Symphonia's own [`Md5`], so the algorithm is
//! shared rather than reimplemented; only the ordering and the byte layout are
//! ours.

use crate::error::analysis_error;
use macinmeter_domain::{AnalysisError, AnalysisStage, ErrorCode};
use std::path::Path;
use symphonia::core::{
    audio::{AudioBufferRef, Signal},
    checksum::Md5,
    codecs::{CODEC_TYPE_FLAC, CodecParameters, VerificationCheck},
    io::Monitor,
};

/// The immutable per-stream parameters of the FLAC signature.
///
/// Workers only read this, so the width a packet is hashed at cannot depend on
/// how far decoding has progressed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FlacIntegrityPlan {
    expected: [u8; 16],
    bits_per_sample: u32,
}

impl FlacIntegrityPlan {
    /// Build the plan when the opened stream is FLAC and declares a signature.
    ///
    /// A STREAMINFO whose MD5 field is all zero declares that no signature was
    /// computed. Symphonia reports that as an absent verification check and
    /// verifies nothing; so does this plan, which keeps the set of accepted
    /// streams unchanged.
    pub(crate) fn for_stream(
        path: &Path,
        codec_params: &CodecParameters,
    ) -> Result<Option<Self>, AnalysisError> {
        if codec_params.codec != CODEC_TYPE_FLAC {
            return Ok(None);
        }
        let Some(VerificationCheck::Md5(expected)) = codec_params.verification_check else {
            return Ok(None);
        };
        // The STREAMINFO bit depth is what the signature is defined over, so a
        // stream that declares a signature without a usable depth is rejected
        // rather than verified at a guessed width.
        let bits_per_sample = codec_params
            .bits_per_sample
            .filter(|bits| (1..=32).contains(bits));
        let Some(bits_per_sample) = bits_per_sample else {
            return Err(analysis_error(
                path,
                ErrorCode::UnsupportedFormat,
                AnalysisStage::Probe,
                "FLAC stream declares a signature without a bit depth the signature is defined over",
                Some(format!(
                    "declared_bits_per_sample={:?}",
                    codec_params.bits_per_sample
                )),
            ));
        };
        Ok(Some(Self {
            expected,
            bits_per_sample,
        }))
    }

    /// Bytes each sample occupies in the hashed buffer.
    ///
    /// The FLAC signature is defined over samples widened to whole bytes, so a
    /// 20-bit stream hashes three bytes per sample.
    const fn bytes_per_sample(self) -> usize {
        self.bits_per_sample.div_ceil(8) as usize
    }

    /// How far Symphonia left-shifted the decoded samples.
    const fn normalization_shift(self) -> u32 {
        32 - self.bits_per_sample
    }

    /// Turn one decoded packet into the bytes the FLAC signature covers.
    ///
    /// Symphonia's FLAC decoder updates its own validator *before* normalizing
    /// samples to the full `i32` range, so the buffer handed to callers is
    /// already shifted left by `32 - bits_per_sample`. Undoing that shift is
    /// exact for every sample that fits the declared depth. A sample that does
    /// not fit means the frame header declared a different bit depth than
    /// STREAMINFO, which the format forbids; such a stream is rejected instead
    /// of being hashed at a width nothing in the container agrees on.
    pub(crate) fn packet_bytes(
        self,
        path: &Path,
        decoded: &AudioBufferRef<'_>,
    ) -> Result<Vec<u8>, AnalysisError> {
        let AudioBufferRef::S32(buffer) = decoded else {
            return Err(analysis_error(
                path,
                ErrorCode::Internal,
                AnalysisStage::Internal,
                "the FLAC decoder produced a sample format the stream signature is not defined over",
                None,
            ));
        };
        let channels = buffer.spec().channels.count();
        let frames = buffer.frames();
        let width = self.bytes_per_sample();
        let shift = self.normalization_shift();

        // Written plane by plane and strided into place, so each sample is read
        // sequentially and neither loop needs a bounds check. The buffer is
        // owned rather than a reused scratch: it has to survive until the
        // commit point, which under packet workers is another thread entirely.
        let mut bytes = vec![0u8; frames * channels * width];
        // Every bit the shift is about to discard, accumulated once instead of
        // branched on per sample.
        let mut discarded = 0u32;
        for channel in 0..channels {
            for (slot, sample) in bytes
                .chunks_exact_mut(width)
                .skip(channel)
                .step_by(channels)
                .zip(buffer.chan(channel))
            {
                discarded |= *sample as u32;
                slot.copy_from_slice(&(sample >> shift).to_le_bytes()[..width]);
            }
        }

        // `shift` is at most 31 because the depth is at least one bit, so this
        // never shifts a `u32` by its own width.
        let mask = (1u32 << shift) - 1;
        if discarded & mask != 0 {
            return Err(analysis_error(
                path,
                ErrorCode::MalformedMedia,
                AnalysisStage::Decode,
                "a FLAC frame decoded at a different bit depth than the stream declares",
                Some(format!(
                    "stream_bits_per_sample={}; unexpected_low_bits=0x{:08x}",
                    self.bits_per_sample,
                    discarded & mask
                )),
            ));
        }
        Ok(bytes)
    }
}

/// The single in-order hasher for one FLAC stream.
///
/// There is exactly one of these per source and only the commit point may feed
/// it, which is what makes the digest a property of the input packet order
/// rather than of decode completion order.
pub(crate) struct FlacStreamVerifier {
    plan: FlacIntegrityPlan,
    state: Md5,
}

impl FlacStreamVerifier {
    pub(crate) fn new(plan: FlacIntegrityPlan) -> Self {
        Self {
            plan,
            state: Md5::default(),
        }
    }

    /// Absorb one committed packet.
    pub(crate) fn commit(&mut self, bytes: &[u8]) {
        self.state.process_buf_bytes(bytes);
    }

    /// Compare the accumulated digest against the declared signature.
    ///
    /// This runs only after the commit buffer has confirmed the index space is
    /// complete, so a passing digest can never mask a dropped packet.
    pub(crate) fn finish(&self, path: &Path) -> Result<(), AnalysisError> {
        let computed = self.state.md5();
        if computed == self.plan.expected {
            return Ok(());
        }
        Err(analysis_error(
            path,
            ErrorCode::DecodeFailed,
            AnalysisStage::Decode,
            "decoder integrity verification failed",
            Some(format!(
                "expected_md5={}; decoded_md5={}",
                hex(&self.plan.expected),
                hex(&computed)
            )),
        ))
    }
}

fn hex(digest: &[u8; 16]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::audio::{AsAudioBufferRef, AudioBuffer, Channels, SignalSpec};

    const DEPTHS: [u32; 6] = [8, 12, 16, 20, 24, 32];

    fn plan(bits_per_sample: u32) -> FlacIntegrityPlan {
        FlacIntegrityPlan {
            expected: [0; 16],
            bits_per_sample,
        }
    }

    /// Build the buffer Symphonia hands to its caller: samples already
    /// left-shifted to the full `i32` range.
    fn normalized_buffer(channels: &[Vec<i32>], bits_per_sample: u32) -> AudioBuffer<i32> {
        let spec = SignalSpec::new(
            44_100,
            Channels::from_bits_truncate((1u32 << channels.len()) - 1),
        );
        let frames = channels[0].len();
        let mut buffer = AudioBuffer::<i32>::new(frames as u64, spec);
        buffer.render_reserved(Some(frames));
        let shift = 32 - bits_per_sample;
        for (index, samples) in channels.iter().enumerate() {
            for (slot, sample) in buffer.chan_mut(index).iter_mut().zip(samples) {
                *slot = sample << shift;
            }
        }
        buffer
    }

    /// The FLAC signature layout, written out independently of the production
    /// path: frame-major interleaved, each sample widened to whole bytes and
    /// stored little-endian.
    fn reference_bytes(channels: &[Vec<i32>], bits_per_sample: u32) -> Vec<u8> {
        let width = bits_per_sample.div_ceil(8) as usize;
        let frames = channels[0].len();
        let mut bytes = Vec::new();
        for frame in 0..frames {
            for samples in channels {
                bytes.extend_from_slice(&samples[frame].to_le_bytes()[..width]);
            }
        }
        bytes
    }

    /// Extremes and interior values of one declared depth.
    fn samples_for(bits_per_sample: u32, offset: i32) -> Vec<i32> {
        let max = (1i64 << (bits_per_sample - 1)) - 1;
        let min = -(1i64 << (bits_per_sample - 1));
        [min, min + 1, -1, 0, 1, max - 1, max]
            .into_iter()
            .map(|value| {
                let shifted = value + i64::from(offset);
                shifted.clamp(min, max) as i32
            })
            .collect()
    }

    #[test]
    fn packet_bytes_reproduce_the_signature_layout_at_every_declared_depth() {
        for bits in DEPTHS {
            for channel_count in 1..=3 {
                let channels: Vec<Vec<i32>> = (0..channel_count)
                    .map(|channel| samples_for(bits, channel))
                    .collect();
                let buffer = normalized_buffer(&channels, bits);
                let bytes = plan(bits)
                    .packet_bytes(Path::new("<test>"), &buffer.as_audio_buffer_ref())
                    .unwrap_or_else(|error| {
                        panic!("{bits}-bit/{channel_count}ch must hash: {}", error.message)
                    });
                assert_eq!(
                    bytes,
                    reference_bytes(&channels, bits),
                    "{bits}-bit/{channel_count}ch layout"
                );
            }
        }
    }

    #[test]
    fn a_sample_wider_than_the_declared_depth_is_rejected_rather_than_hashed() {
        // At 32 bits nothing is shifted away, so there is no wider sample to
        // detect; every narrower depth must catch one.
        for bits in DEPTHS.into_iter().filter(|bits| *bits < 32) {
            let channels = vec![samples_for(bits, 0)];
            let mut buffer = normalized_buffer(&channels, bits);
            // Set a bit the shift is supposed to have vacated. Only a frame
            // decoded at a different depth can produce this.
            buffer.chan_mut(0)[2] |= 1;
            let error = plan(bits)
                .packet_bytes(Path::new("<test>"), &buffer.as_audio_buffer_ref())
                .expect_err("a sample outside the declared depth must not be hashed");
            assert_eq!(error.code, ErrorCode::MalformedMedia, "{bits}-bit code");
            assert_eq!(error.stage, AnalysisStage::Decode, "{bits}-bit stage");
        }
    }

    #[test]
    fn the_digest_depends_on_the_order_packets_are_committed_in() {
        let first = plan(16)
            .packet_bytes(
                Path::new("<test>"),
                &normalized_buffer(&[vec![1, 2, 3, 4]], 16).as_audio_buffer_ref(),
            )
            .unwrap();
        let second = plan(16)
            .packet_bytes(
                Path::new("<test>"),
                &normalized_buffer(&[vec![5, 6, 7, 8]], 16).as_audio_buffer_ref(),
            )
            .unwrap();

        let mut forward = FlacStreamVerifier::new(plan(16));
        forward.commit(&first);
        forward.commit(&second);

        let mut reversed = FlacStreamVerifier::new(plan(16));
        reversed.commit(&second);
        reversed.commit(&first);

        // This is the whole reason verification cannot live inside a worker:
        // the same packets in a different order are a different stream.
        assert_ne!(forward.state.md5(), reversed.state.md5());
    }
}
