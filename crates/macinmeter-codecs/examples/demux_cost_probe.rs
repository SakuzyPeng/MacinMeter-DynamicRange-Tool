#![forbid(unsafe_code)]

//! How much of a compressed stream's cost is sequential demux.
//!
//! ADR-0014's packet pools keep demux sequential and parallelize only decoding,
//! so demux is a hard floor on any speedup. A worker sweep can show that a floor
//! exists but cannot say what it is made of: the recorded Windows sweep left
//! "the serial component tracks compressed size" as an observation with two
//! candidate causes, the sequential demux and the sequential stream-signature
//! hash.
//!
//! This probe separates them by measuring demux with no decoding at all. It
//! reports an absolute millisecond figure that can be compared directly against
//! the serial component a sweep implies, rather than a proportion that has to be
//! argued about.
//!
//! It is a measurement harness, not a product path: it drives the backend
//! directly so that "demux" means exactly the packet reads the pool's demux
//! thread performs, with nothing else attributed to it.

use serde_json::json;
use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    process,
    time::Instant,
};
use symphonia::core::{
    codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

/// One interleaved pass measures every phase once, so drift over the run hits
/// each phase equally instead of accumulating in whichever ran last.
const PASSES: usize = 7;

struct Track {
    format: Box<dyn FormatReader>,
    track_id: u32,
}

fn open(path: &Path) -> Track {
    let file =
        File::open(path).unwrap_or_else(|error| fail(&format!("cannot open input: {error}")));
    let media = MediaSourceStream::new(Box::new(file), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            media,
            // The same options the product opens with, so demux does the same
            // work here as it does behind the pool.
            &FormatOptions {
                enable_gapless: true,
                ..FormatOptions::default()
            },
            &MetadataOptions::default(),
        )
        .unwrap_or_else(|error| fail(&format!("cannot probe input: {error}")));
    let format = probed.format;
    let track_id = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .unwrap_or_else(|| fail("input carries no audio track"))
        .id;
    Track { format, track_id }
}

fn decoder(track: &Track) -> Box<dyn Decoder> {
    let codec_params = track
        .format
        .tracks()
        .iter()
        .find(|candidate| candidate.id == track.track_id)
        .map(|candidate| candidate.codec_params.clone())
        .unwrap_or_else(|| fail("the selected track disappeared"));
    symphonia::default::get_codecs()
        // The product owns FLAC verification, so its decoders do not verify.
        // Leaving it on here would charge decoding for a hash the pool does
        // exactly once, elsewhere.
        .make(&codec_params, &DecoderOptions { verify: false })
        .unwrap_or_else(|error| fail(&format!("cannot create decoder: {error}")))
}

/// Pull every packet of the selected track, optionally decoding each one.
///
/// Returns elapsed nanoseconds, packet count and total compressed bytes.
fn drain(path: &Path, decode: bool) -> (u128, u64, u64) {
    let mut track = open(path);
    let mut decoder = decode.then(|| decoder(&track));

    let started = Instant::now();
    let mut packets = 0_u64;
    let mut compressed_bytes = 0_u64;
    loop {
        let packet = match track.format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => fail(&format!("demux failed: {error}")),
        };
        if packet.track_id() != track.track_id {
            continue;
        }
        packets += 1;
        compressed_bytes += packet.data.len() as u64;
        if let Some(decoder) = decoder.as_mut() {
            decoder
                .decode(&packet)
                .unwrap_or_else(|error| fail(&format!("decode failed: {error}")));
        }
    }
    (started.elapsed().as_nanos(), packets, compressed_bytes)
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn fail(message: &str) -> ! {
    eprintln!("demux cost probe: {message}");
    process::exit(2);
}

fn main() {
    let path = match env::args_os().nth(1) {
        Some(path) => PathBuf::from(path),
        None => fail("usage: demux_cost_probe <media-path>"),
    };

    let (_, packets, compressed_bytes) = drain(&path, false);
    let mut demux_only = Vec::with_capacity(PASSES);
    let mut demux_and_decode = Vec::with_capacity(PASSES);
    for _ in 0..PASSES {
        demux_only.push(drain(&path, false).0);
        demux_and_decode.push(drain(&path, true).0);
    }

    let demux_ns = median(demux_only);
    let total_ns = median(demux_and_decode);
    println!(
        "{}",
        json!({
            "path": path.file_name().map(|name| name.to_string_lossy().into_owned()),
            "packets": packets,
            "compressedBytes": compressed_bytes,
            "demuxOnlyNs": demux_ns,
            "demuxAndDecodeNs": total_ns,
            // Decoding alone is the difference: both phases perform identical
            // demux work, so subtracting removes it exactly.
            "decodeOnlyNs": total_ns.saturating_sub(demux_ns),
            "demuxShareOfSerialDecode": (demux_ns as f64) / (total_ns as f64),
            "passes": PASSES,
        })
    );
}
