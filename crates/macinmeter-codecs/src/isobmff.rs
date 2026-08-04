use crate::{
    container::ContainerPcmInfo,
    error::{analysis_error, io_analysis_error},
};
use macinmeter_domain::{AnalysisError, AnalysisStage, ErrorCode, SourceCodec};
use std::{
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

const ALAC_FRAME_LENGTH: u32 = 4096;
const ALAC_MIN_CHANNELS: u8 = 1;
const ALAC_MAX_CHANNELS: u8 = 8;
/// Validate variable `stsz` entries in bounded sequential reads.
///
/// A valid table may describe many packets, so its declared length must never
/// become one unbounded allocation. 16,384 entries keep the scratch buffer at
/// 64 KiB while avoiding one seek/read syscall for every four-byte entry.
const STSZ_VALIDATION_CHUNK_ENTRIES: u32 = 16 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct IsoBmffAlacInfo {
    pub(crate) declared_frames: u64,
    pub(crate) pcm: ContainerPcmInfo,
    pub(crate) magic_cookie: Box<[u8]>,
}

#[derive(Debug, Clone, Copy)]
struct BoxHeader {
    kind: [u8; 4],
    data_start: u64,
    end: u64,
}

impl BoxHeader {
    fn data_len(self) -> u64 {
        self.end - self.data_start
    }
}

#[derive(Debug)]
struct AlacConfig {
    bit_depth: u8,
    channels: u8,
    sample_rate: u32,
    magic_cookie: Box<[u8]>,
}

#[derive(Debug, Clone, Copy)]
struct MediaHeader {
    timescale: u32,
    duration: u64,
}

#[derive(Debug, Clone, Copy)]
struct MovieHeader {
    timescale: u32,
}

#[derive(Debug, Clone, Copy)]
struct EditList {
    segment_duration: u64,
}

#[derive(Debug, Clone, Copy)]
struct TimeToSample {
    packets: u64,
    frames: u64,
}

pub(crate) fn inspect_isobmff_alac<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
) -> Result<IsoBmffAlacInfo, AnalysisError> {
    let file_len = reader
        .seek(SeekFrom::End(0))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?;
    if file_len < 8 {
        return Err(malformed(
            path,
            "ISO BMFF box header is truncated",
            Some(format!("file_length={file_len}")),
        ));
    }

    let first = read_box_header(reader, path, 0, file_len)?;
    if first.kind != *b"ftyp" {
        return Err(unsupported(
            path,
            "ISO BMFF input does not begin with an ftyp box",
            Some(format!("first_box={}", fourcc(first.kind))),
        ));
    }
    inspect_ftyp(reader, path, first)?;

    let mut position = 0_u64;
    let mut ftyp_count = 0_u32;
    let mut moov = None;
    let mut mdat_count = 0_u32;
    let mut mdat_payload = 0_u64;
    while position < file_len {
        let header = read_box_header(reader, path, position, file_len)?;
        match &header.kind {
            b"ftyp" => {
                ftyp_count = ftyp_count
                    .checked_add(1)
                    .ok_or_else(|| resource_limit(path, "ISO BMFF ftyp box count overflowed"))?;
                if ftyp_count > 1 {
                    return Err(malformed(
                        path,
                        "ISO BMFF contains duplicate ftyp boxes",
                        None,
                    ));
                }
            }
            b"moov" => {
                if moov.replace(header).is_some() {
                    return Err(malformed(
                        path,
                        "ISO BMFF contains duplicate moov boxes",
                        None,
                    ));
                }
            }
            b"mdat" => {
                mdat_count = mdat_count
                    .checked_add(1)
                    .ok_or_else(|| resource_limit(path, "ISO BMFF mdat box count overflowed"))?;
                mdat_payload = mdat_payload.checked_add(header.data_len()).ok_or_else(|| {
                    resource_limit(path, "ISO BMFF media payload length overflowed")
                })?;
            }
            b"moof" => {
                return Err(unsupported(
                    path,
                    "fragmented ISO BMFF is outside the stable ALAC route",
                    None,
                ));
            }
            _ => {}
        }
        position = header.end;
    }

    let moov =
        moov.ok_or_else(|| malformed(path, "ISO BMFF input is missing its moov box", None))?;
    if mdat_count == 0 {
        return Err(malformed(
            path,
            "ISO BMFF input is missing its mdat box",
            None,
        ));
    }
    if mdat_payload == 0 {
        return Err(malformed(
            path,
            "ISO BMFF mdat boxes contain no media payload",
            None,
        ));
    }

    inspect_moov(reader, path, moov)
}

fn inspect_ftyp<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    header: BoxHeader,
) -> Result<(), AnalysisError> {
    let data_len = header.data_len();
    if data_len < 8 || !(data_len - 8).is_multiple_of(4) {
        return Err(malformed(
            path,
            "ISO BMFF ftyp payload has an invalid length",
            Some(format!("payload_length={data_len}")),
        ));
    }
    let mut major_brand = [0_u8; 4];
    read_exact_at(reader, path, header.data_start, &mut major_brand)?;
    if major_brand == [0; 4] {
        return Err(malformed(path, "ISO BMFF ftyp major brand is empty", None));
    }
    Ok(())
}

fn inspect_moov<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    moov: BoxHeader,
) -> Result<IsoBmffAlacInfo, AnalysisError> {
    let mut position = moov.data_start;
    let mut track = None;
    let mut mvhd = None;
    while position < moov.end {
        let header = read_box_header(reader, path, position, moov.end)?;
        match &header.kind {
            b"mvex" => {
                return Err(unsupported(
                    path,
                    "fragmented ISO BMFF is outside the stable ALAC route",
                    None,
                ));
            }
            b"trak" => {
                if track.replace(header).is_some() {
                    return Err(unsupported(
                        path,
                        "ISO BMFF files with multiple tracks are outside the stable ALAC route",
                        None,
                    ));
                }
            }
            b"mvhd" => set_unique(path, &mut mvhd, header, "mvhd")?,
            _ => {}
        }
        position = header.end;
    }
    let track = track.ok_or_else(|| {
        unsupported(
            path,
            "ISO BMFF input contains no supported ALAC audio track",
            None,
        )
    })?;
    let movie = inspect_movie_header(
        reader,
        path,
        mvhd.ok_or_else(|| missing_box(path, "moov", "mvhd"))?,
    )?;
    inspect_track(reader, path, track, movie)
}

fn inspect_track<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    trak: BoxHeader,
    movie: MovieHeader,
) -> Result<IsoBmffAlacInfo, AnalysisError> {
    let mut position = trak.data_start;
    let mut mdia = None;
    let mut edit = None;
    while position < trak.end {
        let header = read_box_header(reader, path, position, trak.end)?;
        match &header.kind {
            b"mdia" => {
                if mdia.replace(header).is_some() {
                    return Err(malformed(
                        path,
                        "ISO BMFF track contains duplicate mdia boxes",
                        None,
                    ));
                }
            }
            b"edts" => {
                if edit.is_some() {
                    return Err(malformed(
                        path,
                        "ISO BMFF track contains duplicate edit boxes",
                        None,
                    ));
                }
                edit = Some(inspect_edits(reader, path, header)?);
            }
            _ => {}
        }
        position = header.end;
    }
    let mdia =
        mdia.ok_or_else(|| malformed(path, "ISO BMFF track is missing its mdia box", None))?;
    let info = inspect_media(reader, path, mdia)?;
    if let Some(edit) = edit {
        validate_identity_edit(path, edit, movie, &info)?;
    }
    Ok(info)
}

fn inspect_edits<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    edts: BoxHeader,
) -> Result<EditList, AnalysisError> {
    let mut position = edts.data_start;
    let mut elst = None;
    while position < edts.end {
        let header = read_box_header(reader, path, position, edts.end)?;
        if header.kind == *b"elst" && elst.replace(header).is_some() {
            return Err(malformed(
                path,
                "ISO BMFF edit container contains duplicate elst boxes",
                None,
            ));
        }
        position = header.end;
    }
    let elst = elst.ok_or_else(|| {
        malformed(
            path,
            "ISO BMFF edit container is missing its elst box",
            None,
        )
    })?;
    inspect_edit_list(reader, path, elst)
}

fn inspect_edit_list<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    elst: BoxHeader,
) -> Result<EditList, AnalysisError> {
    let full = read_u32(reader, path, elst.data_start)?;
    let version = (full >> 24) as u8;
    if full & 0x00ff_ffff != 0 {
        return Err(malformed(
            path,
            "ISO BMFF edit list uses nonzero flags",
            None,
        ));
    }
    let entry_count = read_u32(reader, path, elst.data_start + 4)?;
    let entry_len = match version {
        0 => 12_u64,
        1 => 20_u64,
        _ => {
            return Err(unsupported(
                path,
                "ISO BMFF edit-list version is outside the stable ALAC route",
                Some(format!("version={version}")),
            ));
        }
    };
    let expected = 8_u64
        .checked_add(
            u64::from(entry_count)
                .checked_mul(entry_len)
                .ok_or_else(|| resource_limit(path, "ISO BMFF edit-list length overflowed"))?,
        )
        .ok_or_else(|| resource_limit(path, "ISO BMFF edit-list length overflowed"))?;
    require_exact_payload(path, elst, expected, "elst")?;
    if entry_count != 1 {
        return Err(unsupported(
            path,
            "only one identity ISO BMFF edit is supported",
            Some(format!("entry_count={entry_count}")),
        ));
    }

    let entry = elst.data_start + 8;
    let (segment_duration, media_time, rate_offset) = if version == 0 {
        (
            u64::from(read_u32(reader, path, entry)?),
            i64::from(read_u32(reader, path, entry + 4)? as i32),
            entry + 8,
        )
    } else {
        (
            read_u64(reader, path, entry)?,
            read_u64(reader, path, entry + 8)? as i64,
            entry + 16,
        )
    };
    let rate_integer = read_u16(reader, path, rate_offset)? as i16;
    let rate_fraction = read_u16(reader, path, rate_offset + 2)? as i16;
    if segment_duration == 0 || media_time != 0 || rate_integer != 1 || rate_fraction != 0 {
        return Err(unsupported(
            path,
            "trimmed or rate-adjusted ISO BMFF edit lists are unsupported",
            Some(format!(
                "segment_duration={segment_duration}; media_time={media_time}; rate={rate_integer}.{rate_fraction}"
            )),
        ));
    }
    Ok(EditList { segment_duration })
}

fn inspect_movie_header<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    mvhd: BoxHeader,
) -> Result<MovieHeader, AnalysisError> {
    let full = read_u32(reader, path, mvhd.data_start)?;
    let version = (full >> 24) as u8;
    if full & 0x00ff_ffff != 0 {
        return Err(malformed(
            path,
            "ISO BMFF mvhd box uses nonzero flags",
            None,
        ));
    }
    let (expected_len, timescale_offset) = match version {
        0 => (100_u64, 12_u64),
        1 => (112_u64, 20_u64),
        _ => {
            return Err(unsupported(
                path,
                "ISO BMFF movie-header version is outside the stable ALAC route",
                Some(format!("version={version}")),
            ));
        }
    };
    require_exact_payload(path, mvhd, expected_len, "mvhd")?;
    let timescale = read_u32(reader, path, mvhd.data_start + timescale_offset)?;
    if timescale == 0 {
        return Err(malformed(path, "ISO BMFF mvhd timescale is zero", None));
    }
    Ok(MovieHeader { timescale })
}

fn validate_identity_edit(
    path: &Path,
    edit: EditList,
    movie: MovieHeader,
    track: &IsoBmffAlacInfo,
) -> Result<(), AnalysisError> {
    let denominator = u128::from(track.pcm.sample_rate);
    let numerator = u128::from(track.declared_frames) * u128::from(movie.timescale);
    let expected = numerator.div_ceil(denominator);
    let expected = u64::try_from(expected)
        .map_err(|_| resource_limit(path, "ISO BMFF identity-edit duration overflowed"))?;
    if edit.segment_duration != expected {
        return Err(unsupported(
            path,
            "cropped or padded ISO BMFF edit mappings are unsupported",
            Some(format!(
                "segment_duration={}; expected_identity_duration={expected}; movie_timescale={}",
                edit.segment_duration, movie.timescale
            )),
        ));
    }
    Ok(())
}

fn inspect_media<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    mdia: BoxHeader,
) -> Result<IsoBmffAlacInfo, AnalysisError> {
    let mut position = mdia.data_start;
    let mut hdlr = None;
    let mut mdhd = None;
    let mut minf = None;
    while position < mdia.end {
        let header = read_box_header(reader, path, position, mdia.end)?;
        match &header.kind {
            b"hdlr" => set_unique(path, &mut hdlr, header, "hdlr")?,
            b"mdhd" => set_unique(path, &mut mdhd, header, "mdhd")?,
            b"minf" => set_unique(path, &mut minf, header, "minf")?,
            _ => {}
        }
        position = header.end;
    }

    let hdlr = hdlr.ok_or_else(|| missing_box(path, "mdia", "hdlr"))?;
    if inspect_handler(reader, path, hdlr)? != *b"soun" {
        return Err(unsupported(
            path,
            "non-audio ISO BMFF tracks are outside the stable ALAC route",
            None,
        ));
    }
    let media = inspect_media_header(
        reader,
        path,
        mdhd.ok_or_else(|| missing_box(path, "mdia", "mdhd"))?,
    )?;
    let (config, timing, sample_count) = inspect_media_info(
        reader,
        path,
        minf.ok_or_else(|| missing_box(path, "mdia", "minf"))?,
    )?;

    if media.timescale != config.sample_rate {
        return Err(malformed(
            path,
            "ISO BMFF media timescale disagrees with the ALAC sample rate",
            Some(format!(
                "mdhd_timescale={}; alac_sample_rate={}",
                media.timescale, config.sample_rate
            )),
        ));
    }
    if media.duration != timing.frames {
        return Err(malformed(
            path,
            "ISO BMFF media duration disagrees with the sample table",
            Some(format!(
                "mdhd_duration={}; stts_frames={}",
                media.duration, timing.frames
            )),
        ));
    }
    if timing.packets != u64::from(sample_count) {
        return Err(malformed(
            path,
            "ISO BMFF sample tables disagree about packet count",
            Some(format!(
                "stts_packets={}; stsz_packets={sample_count}",
                timing.packets
            )),
        ));
    }
    if timing.frames == 0 {
        return Err(unsupported(
            path,
            "zero-frame ISO BMFF audio is outside the stable ALAC route",
            None,
        ));
    }

    Ok(IsoBmffAlacInfo {
        declared_frames: timing.frames,
        pcm: ContainerPcmInfo {
            sample_rate: config.sample_rate,
            channels: u16::from(config.channels),
            bits_per_sample: u32::from(config.bit_depth),
            source_codec: SourceCodec::Alac,
        },
        magic_cookie: config.magic_cookie,
    })
}

fn inspect_handler<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    hdlr: BoxHeader,
) -> Result<[u8; 4], AnalysisError> {
    if hdlr.data_len() < 12 {
        return Err(malformed(
            path,
            "ISO BMFF hdlr box is truncated",
            Some(format!("payload_length={}", hdlr.data_len())),
        ));
    }
    let full = read_u32(reader, path, hdlr.data_start)?;
    if full != 0 {
        return Err(malformed(
            path,
            "ISO BMFF hdlr version or flags are invalid",
            Some(format!("full_box=0x{full:08x}")),
        ));
    }
    read_fourcc(reader, path, hdlr.data_start + 8)
}

fn inspect_media_header<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    mdhd: BoxHeader,
) -> Result<MediaHeader, AnalysisError> {
    let full = read_u32(reader, path, mdhd.data_start)?;
    let version = (full >> 24) as u8;
    if full & 0x00ff_ffff != 0 {
        return Err(malformed(
            path,
            "ISO BMFF mdhd box uses nonzero flags",
            None,
        ));
    }
    let (expected_len, timescale_offset, duration_offset) = match version {
        0 => (24_u64, 12_u64, 16_u64),
        1 => (36_u64, 20_u64, 24_u64),
        _ => {
            return Err(unsupported(
                path,
                "ISO BMFF mdhd version is outside the stable ALAC route",
                Some(format!("version={version}")),
            ));
        }
    };
    require_exact_payload(path, mdhd, expected_len, "mdhd")?;
    let timescale = read_u32(reader, path, mdhd.data_start + timescale_offset)?;
    let duration = if version == 0 {
        u64::from(read_u32(reader, path, mdhd.data_start + duration_offset)?)
    } else {
        read_u64(reader, path, mdhd.data_start + duration_offset)?
    };
    if timescale == 0 {
        return Err(malformed(path, "ISO BMFF mdhd timescale is zero", None));
    }
    if duration == 0 || duration == u64::MAX || (version == 0 && duration == u64::from(u32::MAX)) {
        return Err(unsupported(
            path,
            "ISO BMFF media does not declare a usable nonzero duration",
            Some(format!("duration={duration}")),
        ));
    }
    Ok(MediaHeader {
        timescale,
        duration,
    })
}

fn inspect_media_info<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    minf: BoxHeader,
) -> Result<(AlacConfig, TimeToSample, u32), AnalysisError> {
    let mut position = minf.data_start;
    let mut stbl = None;
    while position < minf.end {
        let header = read_box_header(reader, path, position, minf.end)?;
        if header.kind == *b"stbl" {
            set_unique(path, &mut stbl, header, "stbl")?;
        }
        position = header.end;
    }
    inspect_sample_table(
        reader,
        path,
        stbl.ok_or_else(|| missing_box(path, "minf", "stbl"))?,
    )
}

fn inspect_sample_table<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    stbl: BoxHeader,
) -> Result<(AlacConfig, TimeToSample, u32), AnalysisError> {
    let mut position = stbl.data_start;
    let mut stsd = None;
    let mut stts = None;
    let mut stsz = None;
    while position < stbl.end {
        let header = read_box_header(reader, path, position, stbl.end)?;
        match &header.kind {
            b"stsd" => set_unique(path, &mut stsd, header, "stsd")?,
            b"stts" => set_unique(path, &mut stts, header, "stts")?,
            b"stsz" => set_unique(path, &mut stsz, header, "stsz")?,
            _ => {}
        }
        position = header.end;
    }
    let config = inspect_sample_description(
        reader,
        path,
        stsd.ok_or_else(|| missing_box(path, "stbl", "stsd"))?,
    )?;
    let timing = inspect_time_to_sample(
        reader,
        path,
        stts.ok_or_else(|| missing_box(path, "stbl", "stts"))?,
    )?;
    let sample_count = inspect_sample_sizes(
        reader,
        path,
        stsz.ok_or_else(|| missing_box(path, "stbl", "stsz"))?,
    )?;
    Ok((config, timing, sample_count))
}

fn inspect_sample_description<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    stsd: BoxHeader,
) -> Result<AlacConfig, AnalysisError> {
    let full = read_u32(reader, path, stsd.data_start)?;
    if full != 0 {
        return Err(malformed(
            path,
            "ISO BMFF stsd version or flags are invalid",
            Some(format!("full_box=0x{full:08x}")),
        ));
    }
    let entry_count = read_u32(reader, path, stsd.data_start + 4)?;
    if entry_count != 1 {
        return Err(unsupported(
            path,
            "ISO BMFF audio tracks must contain exactly one sample entry",
            Some(format!("entry_count={entry_count}")),
        ));
    }
    let entry_start = stsd
        .data_start
        .checked_add(8)
        .ok_or_else(|| resource_limit(path, "ISO BMFF sample-entry position overflowed"))?;
    let entry = read_box_header(reader, path, entry_start, stsd.end)?;
    if entry.end != stsd.end {
        return Err(malformed(
            path,
            "ISO BMFF stsd payload contains trailing sample-entry data",
            None,
        ));
    }
    if entry.kind != *b"alac" {
        return Err(unsupported(
            path,
            "ISO BMFF audio codec is outside the stable ALAC route",
            Some(format!("sample_entry={}", fourcc(entry.kind))),
        ));
    }
    inspect_alac_sample_entry(reader, path, entry)
}

fn inspect_alac_sample_entry<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    entry: BoxHeader,
) -> Result<AlacConfig, AnalysisError> {
    if entry.data_len() < 28 {
        return Err(malformed(
            path,
            "ISO BMFF ALAC sample entry is truncated",
            Some(format!("payload_length={}", entry.data_len())),
        ));
    }
    let version = read_u16(reader, path, entry.data_start + 8)?;
    if version != 0 {
        return Err(unsupported(
            path,
            "ISO BMFF ALAC sample-entry version is outside the stable route",
            Some(format!("version={version}")),
        ));
    }
    let declared_channels = read_u16(reader, path, entry.data_start + 16)?;
    let sample_rate_fixed = read_u32(reader, path, entry.data_start + 24)?;
    if sample_rate_fixed & 0xffff != 0 {
        return Err(unsupported(
            path,
            "fractional ISO BMFF ALAC sample rates are unsupported",
            Some(format!("fixed_16_16=0x{sample_rate_fixed:08x}")),
        ));
    }
    let declared_sample_rate = sample_rate_fixed >> 16;

    let mut position = entry.data_start + 28;
    let mut config = None;
    while position < entry.end {
        let child = read_box_header(reader, path, position, entry.end)?;
        if child.kind == *b"alac" {
            if config.is_some() {
                return Err(malformed(
                    path,
                    "ISO BMFF ALAC sample entry contains duplicate codec configuration",
                    None,
                ));
            }
            config = Some(inspect_alac_config(reader, path, child)?);
        }
        position = child.end;
    }
    let config = config.ok_or_else(|| {
        malformed(
            path,
            "ISO BMFF ALAC sample entry is missing codec configuration",
            None,
        )
    })?;
    if declared_channels != u16::from(config.channels) {
        return Err(malformed(
            path,
            "ISO BMFF ALAC channel declarations disagree",
            Some(format!(
                "sample_entry_channels={declared_channels}; cookie_channels={}",
                config.channels
            )),
        ));
    }
    if declared_sample_rate == 0 && config.sample_rate <= u32::from(u16::MAX) {
        return Err(malformed(
            path,
            "ISO BMFF ALAC sample entry uses an invalid zero sample rate",
            Some(format!("cookie_rate={}", config.sample_rate)),
        ));
    }
    if declared_sample_rate != config.sample_rate
        && !(is_unrepresentable_rate_sentinel(declared_sample_rate)
            && config.sample_rate > u32::from(u16::MAX))
    {
        return Err(malformed(
            path,
            "ISO BMFF ALAC sample-rate declarations disagree",
            Some(format!(
                "sample_entry_rate={declared_sample_rate}; cookie_rate={}",
                config.sample_rate
            )),
        ));
    }
    Ok(config)
}

/// Whether an AudioSampleEntry rate is a sentinel for "see the codec config".
///
/// The field is 16.16 fixed point, so no rate above `u16::MAX` fits. Writers
/// signal that and leave the real rate to the ALAC cookie. Two spellings occur
/// in the wild: a zero field, and the fixed-point value `1.0` (`0x0001_0000`).
/// Both are only accepted when the cookie rate genuinely exceeds the field's
/// range, so neither can mask a real disagreement.
pub(crate) const fn is_unrepresentable_rate_sentinel(declared_sample_rate: u32) -> bool {
    matches!(declared_sample_rate, 0 | 1)
}

fn inspect_alac_config<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    alac: BoxHeader,
) -> Result<AlacConfig, AnalysisError> {
    if alac.data_len() < 4 {
        return Err(malformed(
            path,
            "ISO BMFF ALAC configuration box is truncated",
            None,
        ));
    }
    let full = read_u32(reader, path, alac.data_start)?;
    let version = (full >> 24) as u8;
    if version != 0 {
        return Err(unsupported(
            path,
            "ALAC configuration version is outside the stable route",
            Some(format!("version={version}")),
        ));
    }
    if full & 0x00ff_ffff != 0 {
        return Err(malformed(
            path,
            "ALAC configuration box uses nonzero flags",
            Some(format!("flags=0x{:06x}", full & 0x00ff_ffff)),
        ));
    }
    let cookie_len = alac.data_len() - 4;
    if !matches!(cookie_len, 24 | 48) {
        return Err(malformed(
            path,
            "ALAC magic cookie must contain exactly 24 or 48 bytes",
            Some(format!("cookie_length={cookie_len}")),
        ));
    }
    let cookie_len = usize::try_from(cookie_len)
        .map_err(|_| resource_limit(path, "ALAC magic-cookie length cannot be represented"))?;
    let mut cookie = vec![0_u8; cookie_len];
    read_exact_at(reader, path, alac.data_start + 4, &mut cookie)?;

    let frame_length = be_u32(&cookie[0..4]);
    let compatible_version = cookie[4];
    let bit_depth = cookie[5];
    let channels = cookie[9];
    let sample_rate = be_u32(&cookie[20..24]);
    if compatible_version != 0 {
        return Err(unsupported(
            path,
            "ALAC compatible version is outside the stable version-0 route",
            Some(format!("compatible_version={compatible_version}")),
        ));
    }
    if frame_length != ALAC_FRAME_LENGTH {
        return Err(unsupported(
            path,
            "ALAC frame length is outside the stable route",
            Some(format!(
                "frame_length={frame_length}; required={ALAC_FRAME_LENGTH}"
            )),
        ));
    }
    if !matches!(bit_depth, 16 | 24) {
        return Err(unsupported(
            path,
            "ALAC bit depth is outside the stable 16/24-bit matrix",
            Some(format!("bit_depth={bit_depth}")),
        ));
    }
    if channels < ALAC_MIN_CHANNELS {
        return Err(malformed(
            path,
            "ALAC magic cookie declares zero channels",
            None,
        ));
    }
    if channels > ALAC_MAX_CHANNELS {
        return Err(unsupported(
            path,
            "ALAC channel count exceeds the stable backend limit",
            Some(format!(
                "channels={channels}; max_channels={ALAC_MAX_CHANNELS}"
            )),
        ));
    }
    if sample_rate == 0 {
        return Err(malformed(
            path,
            "ALAC magic cookie declares a zero sample rate",
            None,
        ));
    }
    if cookie.len() == 48 {
        inspect_explicit_channel_layout(path, &cookie[24..48], channels)?;
    }

    Ok(AlacConfig {
        bit_depth,
        channels,
        sample_rate,
        magic_cookie: cookie.into_boxed_slice(),
    })
}

fn inspect_explicit_channel_layout(
    path: &Path,
    layout: &[u8],
    channels: u8,
) -> Result<(), AnalysisError> {
    if be_u32(&layout[0..4]) != 24 || &layout[4..8] != b"chan" || be_u32(&layout[8..12]) != 0 {
        return Err(malformed(
            path,
            "ALAC explicit channel-layout header is invalid",
            None,
        ));
    }
    let tag = be_u32(&layout[12..16]);
    let layout_channels = match tag {
        0x0064_0001 => 1,
        0x0065_0002 => 2,
        0x0071_0003 => 3,
        0x0074_0004 => 4,
        0x0078_0005 => 5,
        0x007c_0006 => 6,
        0x008e_0007 => 7,
        0x007f_0008 => 8,
        _ => {
            return Err(unsupported(
                path,
                "ALAC explicit channel layout is outside the stable standard set",
                Some(format!("layout_tag=0x{tag:08x}")),
            ));
        }
    };
    if layout_channels != channels {
        return Err(malformed(
            path,
            "ALAC explicit channel layout disagrees with the channel count",
            Some(format!(
                "layout_channels={layout_channels}; cookie_channels={channels}"
            )),
        ));
    }
    if be_u32(&layout[16..20]) != 0 || be_u32(&layout[20..24]) != 0 {
        return Err(malformed(
            path,
            "ALAC explicit channel layout uses nonzero reserved fields",
            None,
        ));
    }
    Ok(())
}

fn inspect_time_to_sample<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    stts: BoxHeader,
) -> Result<TimeToSample, AnalysisError> {
    let full = read_u32(reader, path, stts.data_start)?;
    if full != 0 {
        return Err(malformed(
            path,
            "ISO BMFF stts version or flags are invalid",
            Some(format!("full_box=0x{full:08x}")),
        ));
    }
    let entry_count = read_u32(reader, path, stts.data_start + 4)?;
    let expected = 8_u64
        .checked_add(
            u64::from(entry_count)
                .checked_mul(8)
                .ok_or_else(|| resource_limit(path, "ISO BMFF stts length overflowed"))?,
        )
        .ok_or_else(|| resource_limit(path, "ISO BMFF stts length overflowed"))?;
    require_exact_payload(path, stts, expected, "stts")?;
    if entry_count == 0 {
        return Err(unsupported(
            path,
            "zero-entry ISO BMFF timing tables are unsupported",
            None,
        ));
    }

    let mut packets = 0_u64;
    let mut frames = 0_u64;
    for index in 0..entry_count {
        let offset = stts.data_start + 8 + u64::from(index) * 8;
        let count = read_u32(reader, path, offset)?;
        let delta = read_u32(reader, path, offset + 4)?;
        if count == 0 || delta == 0 {
            return Err(malformed(
                path,
                "ISO BMFF stts entries must have nonzero count and duration",
                Some(format!("entry={index}; count={count}; duration={delta}")),
            ));
        }
        packets = packets
            .checked_add(u64::from(count))
            .ok_or_else(|| resource_limit(path, "ISO BMFF packet count overflowed"))?;
        frames = frames
            .checked_add(u64::from(count) * u64::from(delta))
            .ok_or_else(|| resource_limit(path, "ISO BMFF frame count overflowed"))?;
    }
    Ok(TimeToSample { packets, frames })
}

fn inspect_sample_sizes<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    stsz: BoxHeader,
) -> Result<u32, AnalysisError> {
    let full = read_u32(reader, path, stsz.data_start)?;
    if full != 0 {
        return Err(malformed(
            path,
            "ISO BMFF stsz version or flags are invalid",
            Some(format!("full_box=0x{full:08x}")),
        ));
    }
    let fixed_size = read_u32(reader, path, stsz.data_start + 4)?;
    let sample_count = read_u32(reader, path, stsz.data_start + 8)?;
    let expected = if fixed_size == 0 {
        12_u64
            .checked_add(
                u64::from(sample_count)
                    .checked_mul(4)
                    .ok_or_else(|| resource_limit(path, "ISO BMFF stsz length overflowed"))?,
            )
            .ok_or_else(|| resource_limit(path, "ISO BMFF stsz length overflowed"))?
    } else {
        12
    };
    require_exact_payload(path, stsz, expected, "stsz")?;
    if sample_count == 0 {
        return Err(unsupported(
            path,
            "zero-entry ISO BMFF sample-size tables are unsupported",
            None,
        ));
    }
    if fixed_size == 0 {
        let scratch_entries = sample_count.min(STSZ_VALIDATION_CHUNK_ENTRIES);
        let mut scratch = vec![0_u8; scratch_entries as usize * size_of::<u32>()];
        let mut first_index = 0_u32;
        while first_index < sample_count {
            let entries = (sample_count - first_index).min(STSZ_VALIDATION_CHUNK_ENTRIES);
            let byte_len = entries as usize * size_of::<u32>();
            let offset = stsz.data_start + 12 + u64::from(first_index) * 4;
            read_exact_at(reader, path, offset, &mut scratch[..byte_len])?;
            for (entry, bytes) in scratch[..byte_len].chunks_exact(4).enumerate() {
                if be_u32(bytes) == 0 {
                    let index = first_index + entry as u32;
                    return Err(malformed(
                        path,
                        "ISO BMFF compressed sample size must be nonzero",
                        Some(format!("sample_index={index}")),
                    ));
                }
            }
            first_index += entries;
        }
    }
    Ok(sample_count)
}

fn read_box_header<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    start: u64,
    parent_end: u64,
) -> Result<BoxHeader, AnalysisError> {
    let remaining = parent_end.checked_sub(start).ok_or_else(|| {
        malformed(
            path,
            "ISO BMFF child box starts outside its parent",
            Some(format!("start={start}; parent_end={parent_end}")),
        )
    })?;
    if remaining < 8 {
        return Err(malformed(
            path,
            "ISO BMFF box header is truncated",
            Some(format!("start={start}; remaining={remaining}")),
        ));
    }
    let size32 = read_u32(reader, path, start)?;
    let kind = read_fourcc(reader, path, start + 4)?;
    let (size, header_len) = match size32 {
        0 => {
            return Err(unsupported(
                path,
                "zero-length ISO BMFF boxes are outside the stable ALAC route",
                Some(format!("box={}; start={start}", fourcc(kind))),
            ));
        }
        1 => {
            if remaining < 16 {
                return Err(malformed(
                    path,
                    "extended ISO BMFF box header is truncated",
                    Some(format!("box={}; start={start}", fourcc(kind))),
                ));
            }
            (read_u64(reader, path, start + 8)?, 16_u64)
        }
        value => (u64::from(value), 8_u64),
    };
    if size < header_len {
        return Err(malformed(
            path,
            "ISO BMFF box length is smaller than its header",
            Some(format!(
                "box={}; size={size}; header_size={header_len}",
                fourcc(kind)
            )),
        ));
    }
    let end = start.checked_add(size).ok_or_else(|| {
        malformed(
            path,
            "ISO BMFF box length overflows its file position",
            Some(format!("box={}; start={start}; size={size}", fourcc(kind))),
        )
    })?;
    if end > parent_end {
        return Err(malformed(
            path,
            "ISO BMFF box extends beyond its parent",
            Some(format!(
                "box={}; end={end}; parent_end={parent_end}",
                fourcc(kind)
            )),
        ));
    }
    Ok(BoxHeader {
        kind,
        data_start: start + header_len,
        end,
    })
}

fn set_unique(
    path: &Path,
    slot: &mut Option<BoxHeader>,
    value: BoxHeader,
    name: &str,
) -> Result<(), AnalysisError> {
    if slot.replace(value).is_some() {
        return Err(malformed(
            path,
            format!("ISO BMFF container contains duplicate {name} boxes"),
            None,
        ));
    }
    Ok(())
}

fn require_exact_payload(
    path: &Path,
    header: BoxHeader,
    expected: u64,
    name: &str,
) -> Result<(), AnalysisError> {
    if header.data_len() != expected {
        return Err(malformed(
            path,
            format!("ISO BMFF {name} payload length is inconsistent"),
            Some(format!(
                "declared_payload={}; expected_payload={expected}",
                header.data_len()
            )),
        ));
    }
    Ok(())
}

fn missing_box(path: &Path, parent: &str, child: &str) -> AnalysisError {
    malformed(
        path,
        format!("ISO BMFF {parent} box is missing its {child} box"),
        None,
    )
}

fn read_exact_at<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    offset: u64,
    bytes: &mut [u8],
) -> Result<(), AnalysisError> {
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| io_analysis_error(path, AnalysisStage::Probe, error))?;
    match reader.read_exact(bytes) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(malformed(
            path,
            "ISO BMFF field is truncated",
            Some(format!("offset={offset}; length={}", bytes.len())),
        )),
        Err(error) => Err(io_analysis_error(path, AnalysisStage::Probe, error)),
    }
}

fn read_u16<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    offset: u64,
) -> Result<u16, AnalysisError> {
    let mut bytes = [0_u8; 2];
    read_exact_at(reader, path, offset, &mut bytes)?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_u32<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    offset: u64,
) -> Result<u32, AnalysisError> {
    let mut bytes = [0_u8; 4];
    read_exact_at(reader, path, offset, &mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_u64<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    offset: u64,
) -> Result<u64, AnalysisError> {
    let mut bytes = [0_u8; 8];
    read_exact_at(reader, path, offset, &mut bytes)?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_fourcc<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    offset: u64,
) -> Result<[u8; 4], AnalysisError> {
    let mut bytes = [0_u8; 4];
    read_exact_at(reader, path, offset, &mut bytes)?;
    Ok(bytes)
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn fourcc(bytes: [u8; 4]) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn malformed(path: &Path, message: impl Into<String>, details: Option<String>) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::MalformedMedia,
        AnalysisStage::Probe,
        message,
        details,
    )
}

fn unsupported(path: &Path, message: impl Into<String>, details: Option<String>) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::UnsupportedFormat,
        AnalysisStage::Probe,
        message,
        details,
    )
}

fn resource_limit(path: &Path, message: impl Into<String>) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::ResourceExhausted,
        AnalysisStage::Probe,
        message,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct CountingCursor {
        inner: Cursor<Vec<u8>>,
        reads: usize,
    }

    impl Read for CountingCursor {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            self.inner.read(bytes)
        }
    }

    impl Seek for CountingCursor {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    fn variable_stsz(sample_count: u32, zero_index: Option<u32>) -> CountingCursor {
        let mut bytes = Vec::with_capacity(12 + sample_count as usize * 4);
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&sample_count.to_be_bytes());
        for index in 0..sample_count {
            let size = u32::from(Some(index) != zero_index);
            bytes.extend_from_slice(&size.to_be_bytes());
        }
        CountingCursor {
            inner: Cursor::new(bytes),
            reads: 0,
        }
    }

    fn header(reader: &CountingCursor) -> BoxHeader {
        BoxHeader {
            kind: *b"stsz",
            data_start: 0,
            end: reader.inner.get_ref().len() as u64,
        }
    }

    #[test]
    fn variable_sample_sizes_are_validated_in_bounded_sequential_chunks() {
        let sample_count = STSZ_VALIDATION_CHUNK_ENTRIES + 1;
        let mut reader = variable_stsz(sample_count, None);
        let stsz = header(&reader);
        assert_eq!(
            inspect_sample_sizes(&mut reader, Path::new("chunked.m4a"), stsz).unwrap(),
            sample_count
        );
        assert_eq!(reader.reads, 5, "three header reads plus two table chunks");
    }

    #[test]
    fn chunked_sample_size_validation_preserves_the_exact_failing_index() {
        let zero_index = STSZ_VALIDATION_CHUNK_ENTRIES;
        let mut reader = variable_stsz(zero_index + 1, Some(zero_index));
        let stsz = header(&reader);
        let error = inspect_sample_sizes(&mut reader, Path::new("zero.m4a"), stsz).unwrap_err();
        assert_eq!(error.code, ErrorCode::MalformedMedia);
        assert_eq!(error.stage, AnalysisStage::Probe);
        assert_eq!(error.details.as_deref(), Some("sample_index=16384"));
    }
}
