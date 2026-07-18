use crate::{container::ContainerSignature, error::analysis_error};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelCount, ChannelLayout, ErrorCode, SourceCodec, StreamSpec,
};
use std::path::Path;
use symphonia::core::codecs::{
    CODEC_TYPE_FLAC, CODEC_TYPE_PCM_F32BE, CODEC_TYPE_PCM_F32BE_PLANAR, CODEC_TYPE_PCM_F32LE,
    CODEC_TYPE_PCM_F32LE_PLANAR, CODEC_TYPE_PCM_F64BE, CODEC_TYPE_PCM_F64BE_PLANAR,
    CODEC_TYPE_PCM_F64LE, CODEC_TYPE_PCM_F64LE_PLANAR, CODEC_TYPE_PCM_S8, CODEC_TYPE_PCM_S8_PLANAR,
    CODEC_TYPE_PCM_S16BE, CODEC_TYPE_PCM_S16BE_PLANAR, CODEC_TYPE_PCM_S16LE,
    CODEC_TYPE_PCM_S16LE_PLANAR, CODEC_TYPE_PCM_S24BE, CODEC_TYPE_PCM_S24BE_PLANAR,
    CODEC_TYPE_PCM_S24LE, CODEC_TYPE_PCM_S24LE_PLANAR, CODEC_TYPE_PCM_S32BE,
    CODEC_TYPE_PCM_S32BE_PLANAR, CODEC_TYPE_PCM_S32LE, CODEC_TYPE_PCM_S32LE_PLANAR,
    CODEC_TYPE_PCM_U8, CODEC_TYPE_PCM_U8_PLANAR, CODEC_TYPE_PCM_U16BE, CODEC_TYPE_PCM_U16BE_PLANAR,
    CODEC_TYPE_PCM_U16LE, CODEC_TYPE_PCM_U16LE_PLANAR, CODEC_TYPE_PCM_U24BE,
    CODEC_TYPE_PCM_U24BE_PLANAR, CODEC_TYPE_PCM_U24LE, CODEC_TYPE_PCM_U24LE_PLANAR,
    CODEC_TYPE_PCM_U32BE, CODEC_TYPE_PCM_U32BE_PLANAR, CODEC_TYPE_PCM_U32LE,
    CODEC_TYPE_PCM_U32LE_PLANAR, CodecParameters, CodecType,
};

pub(crate) fn validate_codec(
    path: &Path,
    signature: ContainerSignature,
    codec: CodecType,
) -> Result<SourceCodec, AnalysisError> {
    let source_codec = match signature {
        ContainerSignature::Flac if codec == CODEC_TYPE_FLAC => SourceCodec::Flac,
        ContainerSignature::Wave if is_float_pcm(codec) => SourceCodec::PcmFloat,
        ContainerSignature::Wave | ContainerSignature::Aiff if is_integer_pcm(codec) => {
            SourceCodec::PcmInteger
        }
        _ => {
            return Err(analysis_error(
                path,
                ErrorCode::UnsupportedFormat,
                AnalysisStage::Probe,
                "container codec is outside the M0 PCM/FLAC support set",
                Some(format!("Symphonia codec id: {codec}")),
            ));
        }
    };
    Ok(source_codec)
}

pub(crate) fn stream_spec(
    path: &Path,
    codec_params: &CodecParameters,
) -> Result<(StreamSpec, ChannelCount), AnalysisError> {
    let sample_rate = codec_params.sample_rate.ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "audio stream does not declare a sample rate",
            None,
        )
    })?;
    let channels = codec_params.channels.ok_or_else(|| {
        analysis_error(
            path,
            ErrorCode::MalformedMedia,
            AnalysisStage::Probe,
            "audio stream does not declare its channels",
            None,
        )
    })?;
    let channel_count = u16::try_from(channels.count()).map_err(|_| {
        analysis_error(
            path,
            ErrorCode::UnsupportedFormat,
            AnalysisStage::Probe,
            "audio stream has too many channels",
            None,
        )
    })?;
    let spec =
        StreamSpec::new(sample_rate, channel_count, ChannelLayout::Unknown).map_err(|error| {
            analysis_error(
                path,
                ErrorCode::MalformedMedia,
                AnalysisStage::Probe,
                "audio stream declares invalid PCM parameters",
                Some(error.message),
            )
        })?;
    Ok((spec.clone(), spec.channels))
}

pub(crate) fn source_bits_per_sample(codec_params: &CodecParameters) -> Option<u32> {
    codec_params
        .bits_per_sample
        .or(codec_params.bits_per_coded_sample)
        .or_else(|| pcm_codec_bits(codec_params.codec))
}

fn pcm_codec_bits(codec: CodecType) -> Option<u32> {
    if matches!(
        codec,
        CODEC_TYPE_PCM_S8 | CODEC_TYPE_PCM_S8_PLANAR | CODEC_TYPE_PCM_U8 | CODEC_TYPE_PCM_U8_PLANAR
    ) {
        Some(8)
    } else if matches!(
        codec,
        CODEC_TYPE_PCM_S16LE
            | CODEC_TYPE_PCM_S16LE_PLANAR
            | CODEC_TYPE_PCM_S16BE
            | CODEC_TYPE_PCM_S16BE_PLANAR
            | CODEC_TYPE_PCM_U16LE
            | CODEC_TYPE_PCM_U16LE_PLANAR
            | CODEC_TYPE_PCM_U16BE
            | CODEC_TYPE_PCM_U16BE_PLANAR
    ) {
        Some(16)
    } else if matches!(
        codec,
        CODEC_TYPE_PCM_S24LE
            | CODEC_TYPE_PCM_S24LE_PLANAR
            | CODEC_TYPE_PCM_S24BE
            | CODEC_TYPE_PCM_S24BE_PLANAR
            | CODEC_TYPE_PCM_U24LE
            | CODEC_TYPE_PCM_U24LE_PLANAR
            | CODEC_TYPE_PCM_U24BE
            | CODEC_TYPE_PCM_U24BE_PLANAR
    ) {
        Some(24)
    } else if matches!(
        codec,
        CODEC_TYPE_PCM_S32LE
            | CODEC_TYPE_PCM_S32LE_PLANAR
            | CODEC_TYPE_PCM_S32BE
            | CODEC_TYPE_PCM_S32BE_PLANAR
            | CODEC_TYPE_PCM_U32LE
            | CODEC_TYPE_PCM_U32LE_PLANAR
            | CODEC_TYPE_PCM_U32BE
            | CODEC_TYPE_PCM_U32BE_PLANAR
            | CODEC_TYPE_PCM_F32LE
            | CODEC_TYPE_PCM_F32LE_PLANAR
            | CODEC_TYPE_PCM_F32BE
            | CODEC_TYPE_PCM_F32BE_PLANAR
    ) {
        Some(32)
    } else if matches!(
        codec,
        CODEC_TYPE_PCM_F64LE
            | CODEC_TYPE_PCM_F64LE_PLANAR
            | CODEC_TYPE_PCM_F64BE
            | CODEC_TYPE_PCM_F64BE_PLANAR
    ) {
        Some(64)
    } else {
        None
    }
}

fn is_float_pcm(codec: CodecType) -> bool {
    matches!(
        codec,
        CODEC_TYPE_PCM_F32LE
            | CODEC_TYPE_PCM_F32LE_PLANAR
            | CODEC_TYPE_PCM_F32BE
            | CODEC_TYPE_PCM_F32BE_PLANAR
            | CODEC_TYPE_PCM_F64LE
            | CODEC_TYPE_PCM_F64LE_PLANAR
            | CODEC_TYPE_PCM_F64BE
            | CODEC_TYPE_PCM_F64BE_PLANAR
    )
}

fn is_integer_pcm(codec: CodecType) -> bool {
    matches!(
        codec,
        CODEC_TYPE_PCM_S32LE
            | CODEC_TYPE_PCM_S32LE_PLANAR
            | CODEC_TYPE_PCM_S32BE
            | CODEC_TYPE_PCM_S32BE_PLANAR
            | CODEC_TYPE_PCM_S24LE
            | CODEC_TYPE_PCM_S24LE_PLANAR
            | CODEC_TYPE_PCM_S24BE
            | CODEC_TYPE_PCM_S24BE_PLANAR
            | CODEC_TYPE_PCM_S16LE
            | CODEC_TYPE_PCM_S16LE_PLANAR
            | CODEC_TYPE_PCM_S16BE
            | CODEC_TYPE_PCM_S16BE_PLANAR
            | CODEC_TYPE_PCM_S8
            | CODEC_TYPE_PCM_S8_PLANAR
            | CODEC_TYPE_PCM_U32LE
            | CODEC_TYPE_PCM_U32LE_PLANAR
            | CODEC_TYPE_PCM_U32BE
            | CODEC_TYPE_PCM_U32BE_PLANAR
            | CODEC_TYPE_PCM_U24LE
            | CODEC_TYPE_PCM_U24LE_PLANAR
            | CODEC_TYPE_PCM_U24BE
            | CODEC_TYPE_PCM_U24BE_PLANAR
            | CODEC_TYPE_PCM_U16LE
            | CODEC_TYPE_PCM_U16LE_PLANAR
            | CODEC_TYPE_PCM_U16BE
            | CODEC_TYPE_PCM_U16BE_PLANAR
            | CODEC_TYPE_PCM_U8
            | CODEC_TYPE_PCM_U8_PLANAR
    )
}
