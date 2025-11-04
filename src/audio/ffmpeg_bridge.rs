//! FFmpeg桥接解码器
//!
//! 为Symphonia不支持的格式提供FFmpeg回退方案，通过管道实现流式解码。
//! 支持格式：AC-3, E-AC-3, DTS, DSD (DSF/DFF)等。

use crate::error::{AudioError, AudioResult};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use super::format::AudioFormat;
use super::stats::ChunkSizeStats;
use super::streaming::StreamingDecoder;

/// 基于常见布局推断 LFE 在交错顺序中的索引（0-based）
/// 说明：
/// - 2.1 (3ch) -> index 2
/// - 3.1/5.1/7.1/7.1.4/… (>=4ch) -> index 3（FL,FR,FC,LFE,...）
/// - 其他未知布局返回空向量
fn default_lfe_indices_for_channel_count(channel_count: u16) -> Vec<usize> {
    match channel_count {
        0..=2 => Vec::new(),
        3 => vec![2],
        _ => vec![3],
    }
}

/// FFmpeg安装指南（跨平台）
const FFMPEG_INSTALL_GUIDE: &str = r#"
FFmpeg is required for AC-3/E-AC-3/DTS/DSD support / 需要安装FFmpeg以支持AC-3/E-AC-3/DTS/DSD格式

Installation / 安装方法:
  macOS:   brew install ffmpeg
  Windows: https://www.gyan.dev/ffmpeg/builds/ (推荐Full版本)
           或使用: winget install Gyan.FFmpeg
  Linux:
    - Ubuntu/Debian: sudo apt install ffmpeg
    - Fedora/RHEL:   sudo dnf install ffmpeg
    - Arch:          sudo pacman -S ffmpeg

Official site / 官方网站: https://ffmpeg.org/download.html
"#;

/// FFmpeg流式解码器
///
/// 通过子进程管道实现流式PCM解码，支持恒定内存处理大文件。
/// 注意：此方案为串行解码（无packet级并行），但支持文件级并行。
pub struct FFmpegDecoder {
    /// FFmpeg子进程
    child: Child,
    /// FFmpeg可执行路径（用于reset重启）
    ffmpeg_path: PathBuf,
    /// 启动参数（用于reset重启）
    spawn_args: Vec<String>,
    /// 输入文件路径（诊断/重启）
    #[allow(dead_code)]
    input_path: PathBuf,
    /// 音频格式信息
    format: AudioFormat,
    /// 当前读取位置（样本数）
    current_position: u64,
    /// 总样本数（从ffprobe获取）
    total_samples: u64,
    /// chunk统计信息
    chunk_stats: ChunkSizeStats,
    /// 是否已到达流末尾
    eof_reached: bool,
    /// 是否使用32位样本（DSD格式）
    use_s32: bool,
    /// FFmpeg错误输出摘要（最近的错误）
    /// 注：当前未直接使用，保留用于后续错误诊断和I/O鲁棒性增强
    #[allow(dead_code)]
    last_stderr: String,
}

impl FFmpegDecoder {
    /// 检测FFmpeg是否可用
    pub fn is_available() -> bool {
        Self::find_ffmpeg_path().is_some()
    }

    /// 查找FFmpeg可执行文件路径（跨平台）
    fn find_ffmpeg_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            // Windows: 检查多个常见位置
            let candidates = vec![
                PathBuf::from(r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"),
                PathBuf::from(r"C:\ffmpeg\bin\ffmpeg.exe"),
                // 便携部署：与可执行文件同目录
                std::env::current_exe().ok()?.parent()?.join("ffmpeg.exe"),
            ];

            // 先尝试 PATH 中的 ffmpeg（不依赖文件存在，直接调用）
            if Command::new("ffmpeg")
                .arg("-version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return Some(PathBuf::from("ffmpeg"));
            }

            candidates.into_iter().find(|p| {
                if !p.exists() {
                    return false;
                }
                Command::new(p)
                    .arg("-version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            // macOS/Linux: 直接使用PATH中的ffmpeg
            let path = PathBuf::from("ffmpeg");
            if Command::new(&path)
                .arg("-version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                Some(path)
            } else {
                None
            }
        }
    }

    /// 查找ffprobe可执行文件路径，优先使用ffmpeg同目录的ffprobe
    fn find_ffprobe_path() -> Option<PathBuf> {
        // 首先尝试找到ffmpeg路径，然后从同一目录查找ffprobe
        if let Some(ffmpeg_path) = Self::find_ffmpeg_path()
            && let Some(bin_dir) = ffmpeg_path.parent()
        {
            #[cfg(target_os = "windows")]
            let ffprobe_name = "ffprobe.exe";
            #[cfg(not(target_os = "windows"))]
            let ffprobe_name = "ffprobe";

            let ffprobe_in_bin = bin_dir.join(ffprobe_name);
            if ffprobe_in_bin.exists() {
                return Some(ffprobe_in_bin);
            }
        }

        // 回退：直接在PATH中查找ffprobe
        #[cfg(target_os = "windows")]
        let ffprobe_cmd = "ffprobe.exe";
        #[cfg(not(target_os = "windows"))]
        let ffprobe_cmd = "ffprobe";

        if Command::new(ffprobe_cmd)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            Some(PathBuf::from(ffprobe_cmd))
        } else {
            None
        }
    }

    /// 使用ffprobe探测音频格式信息
    fn probe_format(path: &Path) -> AudioResult<AudioFormat> {
        let ffprobe_path = Self::find_ffprobe_path().ok_or_else(|| {
            AudioError::FormatError(format!(
                "ffprobe not found / ffprobe未找到，无法探测音频格式\n{FFMPEG_INSTALL_GUIDE}"
            ))
        })?;

        let output = Command::new(&ffprobe_path)
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                // 追加布局字段，按声明顺序输出，便于解析
                "stream=codec_name,sample_rate,channels,duration,channel_layout,ch_layout",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                path.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| {
                AudioError::FormatError(format!(
                    "Failed to run ffprobe / 无法运行ffprobe: {e}\n{FFMPEG_INSTALL_GUIDE}"
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // 针对常见场景进行错误类型细化：不存在文件 → IoError(NotFound)
            let lower = stderr.to_ascii_lowercase();
            if lower.contains("no such file or directory")
                || lower.contains("not found")
                || lower.contains("the system cannot find the file specified")
            {
                use std::io::{Error, ErrorKind};
                return Err(AudioError::IoError(Error::new(
                    ErrorKind::NotFound,
                    stderr.to_string(),
                )));
            }
            return Err(AudioError::FormatError(format!(
                "ffprobe failed / ffprobe失败: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        if lines.len() < 4 {
            return Err(AudioError::FormatError(
                "Incomplete ffprobe output / ffprobe输出不完整".to_string(),
            ));
        }

        // ffprobe输出顺序：codec_name, sample_rate, channels, duration, channel_layout, ch_layout
        let codec_name = Some(lines[0].to_string());

        let sample_rate = lines[1].parse::<u32>().map_err(|e| {
            AudioError::FormatError(format!("Invalid sample rate / 无效的采样率: {e}"))
        })?;

        let channels = lines[2].parse::<u16>().map_err(|e| {
            AudioError::FormatError(format!("Invalid channel count / 无效的声道数: {e}"))
        })?;

        let duration = lines[3].parse::<f64>().ok().unwrap_or(0.0);
        let sample_count = if duration > 0.0 {
            (duration * sample_rate as f64) as u64
        } else {
            0
        };
        // 进一步校验元数据的合理性，防止非音频文件被错误识别
        if channels == 0 || sample_rate < 8_000 {
            return Err(AudioError::FormatError(
                "Invalid stream parameters from ffprobe / ffprobe返回的流参数无效".to_string(),
            ));
        }

        let bits_per_sample = 16; // 默认按S16LE，若为DSD将在new()中切换为S32路径

        let mut format = AudioFormat::new(sample_rate, channels, bits_per_sample, sample_count);

        // 解析布局字符串：用于标记布局元数据与推导LFE索引
        let layout_str_raw = lines.get(4).map(|s| s.trim()).unwrap_or("");
        let ch_layout_str_raw = lines.get(5).map(|s| s.trim()).unwrap_or("");
        let layout_joined = if !layout_str_raw.is_empty() {
            layout_str_raw.to_string()
        } else if !ch_layout_str_raw.is_empty() {
            ch_layout_str_raw.to_string()
        } else {
            String::new()
        };

        if !layout_joined.is_empty() {
            format.mark_has_channel_layout();
            // 基于布局字符串粗略判断是否存在LFE（*.1 / 5.1 / 7.1 / 7.1.4 / 9.1.6等）
            let lower = layout_joined.to_ascii_lowercase();
            let looks_like_has_lfe = lower.contains(".1")
                || lower.contains("5.1")
                || lower.contains("6.1")
                || lower.contains("7.1")
                || lower.contains("9.1");

            if looks_like_has_lfe {
                let idxs = default_lfe_indices_for_channel_count(format.channels);
                if !idxs.is_empty() {
                    format.set_lfe_indices(idxs);
                }
            }
        }

        // 进一步：尝试用 JSON 精确解析通道标签顺序（如 FL+FR+FC+LFE+...），直接定位 LFE/LFE2 的下标
        // 仅在尚未获得精确 lfe_indices 时尝试
        #[allow(clippy::collapsible_if)]
        {
            if format.lfe_indices.is_empty() {
                if let Some(path_str) = path.to_str() {
                    let json_output = Command::new(&ffprobe_path)
                        .args([
                            "-v",
                            "error",
                            "-select_streams",
                            "a:0",
                            "-show_entries",
                            "stream=channel_layout,side_data_list",
                            "-of",
                            "json",
                            path_str,
                        ])
                        .output()
                        .ok();

                    if let Some(json_out) = json_output {
                        if json_out.status.success() {
                            if let Ok(text) = String::from_utf8(json_out.stdout) {
                                // 解析最常见的两种形态：
                                // 1) side_data_list 内存在 { side_data_type: "ch_layout", ch_layout: "FL+FR+FC+LFE+..." }
                                // 2) 直接的 channel_layout 为 "FL+FR+FC+LFE+..."（较少见）
                                #[allow(clippy::collapsible_if, clippy::get_first)]
                                {
                                    #[derive(serde::Deserialize)]
                                    struct SideDataItem {
                                        #[serde(default)]
                                        side_data_type: String,
                                        #[serde(default)]
                                        ch_layout: Option<String>,
                                    }
                                    #[derive(serde::Deserialize)]
                                    struct StreamObj {
                                        #[serde(default)]
                                        channel_layout: Option<String>,
                                        #[serde(default)]
                                        side_data_list: Option<Vec<SideDataItem>>,
                                    }
                                    #[derive(serde::Deserialize)]
                                    struct StreamsRoot {
                                        #[serde(default)]
                                        streams: Vec<StreamObj>,
                                    }

                                    if let Ok(parsed) = serde_json::from_str::<StreamsRoot>(&text) {
                                        if let Some(stream) = parsed.streams.first() {
                                            // 优先从 side_data_list 中解析 ch_layout 标签序列
                                            let mut labels: Option<String> = None;
                                            if let Some(list) = &stream.side_data_list {
                                                for item in list {
                                                    if item
                                                        .side_data_type
                                                        .eq_ignore_ascii_case("ch_layout")
                                                    {
                                                        if let Some(s) = &item.ch_layout {
                                                            labels = Some(s.clone());
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                            // 其次尝试直接从 channel_layout 获取标签序列（若包含 '+')
                                            if labels.is_none() {
                                                if let Some(cl) = &stream.channel_layout {
                                                    if cl.contains('+') {
                                                        labels = Some(cl.clone());
                                                    }
                                                }
                                            }

                                            if let Some(label_seq) = labels {
                                                let tokens: Vec<&str> =
                                                    label_seq.split('+').collect();
                                                if !tokens.is_empty() {
                                                    let mut lfe_idxs = Vec::new();
                                                    for (i, t) in tokens.iter().enumerate() {
                                                        let tt = t.trim().to_ascii_uppercase();
                                                        if (tt == "LFE" || tt == "LFE2")
                                                            && i < format.channels as usize
                                                        {
                                                            lfe_idxs.push(i);
                                                        }
                                                    }
                                                    if !lfe_idxs.is_empty() {
                                                        format.set_lfe_indices(lfe_idxs);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 标记为FFmpeg解码（用于诊断）
        // 注：codec_name信息已通过ffprobe获取（{codec_name:?}），保留供调试使用
        let _codec_name = codec_name; // 避免未使用变量警告

        Ok(format)
    }

    /// 创建FFmpeg解码器
    pub fn new(path: &Path) -> AudioResult<Self> {
        // 检查FFmpeg可用性
        let ffmpeg_path = Self::find_ffmpeg_path()
            .ok_or_else(|| AudioError::FormatError(FFMPEG_INSTALL_GUIDE.to_string()))?;

        // 探测格式信息
        let mut format = Self::probe_format(path)?;

        // 检测是否为DSD格式
        let is_dsd = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("dsf") || s.eq_ignore_ascii_case("dff"))
            .unwrap_or(false);

        // 构建FFmpeg命令参数（基础参数）
        let mut args = vec![
            "-hide_banner".to_string(),
            "-nostdin".to_string(),
            "-v".to_string(),
            "error".to_string(),
            // 禁止非音频流
            "-vn".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
            "-i".to_string(),
            path.to_str().unwrap().to_string(),
        ];

        let use_s32 = is_dsd; // DSD使用32位输出以避免精度损失

        if is_dsd {
            // DSD 专用：固定策略下采样，避免 2.8224MHz 带来的巨大数据量。
            // 规则：DSD128 及以上 → 176400，否则 → 88200
            let target_rate: u32 = if format.sample_rate >= 5_644_800 {
                176_400
            } else {
                88_200
            };

            // DSD → PCM：低通 + 轻微增益补偿（与常见播放器一致），并设置输出采样率
            args.extend(vec![
                "-af".to_string(),
                "lowpass=20000,volume=6dB".to_string(),
                "-ar".to_string(),
                target_rate.to_string(),
            ]);

            // 标记处理用采样率，便于报告输出“源 → 处理（DSD降采样）”
            // 注意：AudioFormat.sample_rate 保留为“源采样率”
            //       processed_sample_rate 记录真实处理用采样率
            // SAFETY: 仅在内部使用，可安全更新
            // 这里 format 仍在本作用域，未跨线程共享
            format.processed_sample_rate = Some(target_rate);
        }

        // 输出PCM格式（DSD使用32位，其他格式使用16位）
        args.extend(vec![
            "-f".to_string(),
            if use_s32 {
                "s32le".to_string()
            } else {
                "s16le".to_string()
            },
            "-acodec".to_string(),
            if use_s32 {
                "pcm_s32le".to_string()
            } else {
                "pcm_s16le".to_string()
            },
            "-".to_string(), // 输出到stdout
        ]);

        // 启动FFmpeg子进程
        let child = Command::new(&ffmpeg_path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AudioError::DecodingError(format!("Failed to spawn FFmpeg / 无法启动FFmpeg: {e}"))
            })?;

        let total_samples = format.sample_count; // 提前保存，避免move后使用

        eprintln!(
            "[INFO] FFmpeg decoder initialized / FFmpeg解码器已初始化: {} channels, {}Hz, format={}",
            format.channels,
            format.sample_rate,
            if use_s32 { "S32LE" } else { "S16LE" }
        );

        Ok(Self {
            child,
            ffmpeg_path,
            spawn_args: args.clone(),
            input_path: path.to_path_buf(),
            format,
            current_position: 0,
            total_samples,
            chunk_stats: ChunkSizeStats::new(),
            eof_reached: false,
            use_s32,
            last_stderr: String::new(),
        })
    }

    /// S16LE字节转f32样本（小端序）
    fn convert_s16le_to_f32(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(2)
            .map(|chunk| {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                sample as f32 / 32768.0 // 归一化到[-1.0, 1.0]
            })
            .collect()
    }

    /// S32LE字节转f32样本（小端序，DSD专用）
    fn convert_s32le_to_f32(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|chunk| {
                let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                sample as f32 / 2147483648.0 // 归一化到[-1.0, 1.0]
            })
            .collect()
    }
}

impl StreamingDecoder for FFmpegDecoder {
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        if self.eof_reached {
            return Ok(None);
        }

        // 计算自适应分块大小（按字节上限而非固定时长）
        // 约束：8-16MB每块，适应不同声道数和采样率
        const MAX_CHUNK_BYTES: usize = 16 * 1024 * 1024; // 16MB上限
        const MIN_CHUNK_BYTES: usize = 64 * 1024; // 64KB下限
        let channels = self.format.channels as usize;
        // 按实际输出位宽选择样本字节数
        let bytes_per_sample = if self.use_s32 { 4 } else { 2 };
        let default_chunk_bytes =
            self.format.sample_rate as usize * channels * bytes_per_sample * 3; // 目标≈3秒
        let bytes_per_chunk = default_chunk_bytes.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);
        // 向下对齐到整帧（channels样本边界）
        let bytes_per_chunk =
            (bytes_per_chunk / (channels * bytes_per_sample)) * (channels * bytes_per_sample);

        let mut buffer = vec![0u8; bytes_per_chunk];
        let mut accumulated = Vec::new(); // 积累不足一帧的数据

        let stdout = self.child.stdout.as_mut().ok_or_else(|| {
            AudioError::DecodingError(
                "FFmpeg stdout not available / FFmpeg标准输出不可用".to_string(),
            )
        })?;

        // 填满式读取循环，直到获得完整帧数或EOF
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => {
                    // EOF
                    self.eof_reached = true;
                    if accumulated.is_empty() {
                        return Ok(None);
                    }
                    break; // 处理积累的不足一帧的数据
                }
                Ok(bytes_read) => {
                    accumulated.extend_from_slice(&buffer[..bytes_read]);
                    // 计算字节大小（S16LE=2字节，S32LE=4字节）
                    let bytes_per_s16_sample = 2;
                    let bytes_per_s32_sample = 4;
                    let bytes_per_complete_sample = if self.use_s32 {
                        bytes_per_s32_sample
                    } else {
                        bytes_per_s16_sample
                    };
                    // 检查是否有完整帧
                    let complete_frames = (accumulated.len()
                        / (channels * bytes_per_complete_sample))
                        * (channels * bytes_per_complete_sample);
                    if complete_frames > 0 {
                        // 有完整帧，返回
                        let data = accumulated.drain(..complete_frames).collect::<Vec<_>>();
                        let samples = if self.use_s32 {
                            Self::convert_s32le_to_f32(&data)
                        } else {
                            Self::convert_s16le_to_f32(&data)
                        };
                        let samples_per_channel = samples.len() / channels;
                        self.current_position += samples_per_channel as u64;
                        self.chunk_stats.add_chunk(samples.len());
                        return Ok(Some(samples));
                    }
                    // 否则继续读取以积累完整帧
                }
                Err(e) => {
                    return Err(AudioError::DecodingError(format!(
                        "Failed to read from FFmpeg / FFmpeg读取失败: {e}"
                    )));
                }
            }
        }

        // 处理EOF时积累的不足一帧的数据（符合P0实现：尾块直接参与计算）
        if !accumulated.is_empty() {
            let samples = if self.use_s32 {
                Self::convert_s32le_to_f32(&accumulated)
            } else {
                Self::convert_s16le_to_f32(&accumulated)
            };
            let samples_per_channel = samples.len() / channels;
            self.current_position += samples_per_channel as u64;
            self.chunk_stats.add_chunk(samples.len());
            Ok(Some(samples))
        } else {
            Ok(None)
        }
    }

    fn format(&self) -> AudioFormat {
        let mut current_format = self.format.clone();
        current_format.update_sample_count(self.current_position);
        current_format
    }

    fn progress(&self) -> f32 {
        if self.total_samples == 0 {
            0.0
        } else {
            (self.current_position as f32) / (self.total_samples as f32)
        }
    }

    fn reset(&mut self) -> AudioResult<()> {
        // 终止现有FFmpeg进程
        let _ = self.child.kill();
        let _ = self.child.wait();

        // 按相同参数重启FFmpeg子进程
        self.child = Command::new(&self.ffmpeg_path)
            .args(&self.spawn_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AudioError::DecodingError(format!("Failed to respawn FFmpeg / 无法重启FFmpeg: {e}"))
            })?;

        // 重置内部状态
        self.current_position = 0;
        self.eof_reached = false;
        self.chunk_stats = ChunkSizeStats::new();
        Ok(())
    }

    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        self.chunk_stats.finalize();
        Some(self.chunk_stats.clone())
    }
}

impl Drop for FFmpegDecoder {
    fn drop(&mut self) {
        // 确保FFmpeg进程被清理
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffmpeg_availability() {
        // 仅在系统安装FFmpeg时通过
        let available = FFmpegDecoder::is_available();
        println!("FFmpeg available / FFmpeg可用: {available}");
    }

    #[test]
    fn test_s16le_conversion() {
        // 测试S16LE → F32转换
        let bytes = vec![
            0x00, 0x00, // 0
            0x00, 0x40, // 16384
            0x00, 0x80, // -32768
            0xFF, 0x7F, // 32767
        ];

        let samples = FFmpegDecoder::convert_s16le_to_f32(&bytes);
        assert_eq!(samples.len(), 4);
        assert!((samples[0] - 0.0).abs() < 0.001);
        assert!((samples[1] - 0.5).abs() < 0.001);
        assert!((samples[2] - (-1.0)).abs() < 0.001);
        assert!((samples[3] - 0.999969).abs() < 0.001);
    }

    #[test]
    fn test_install_guide_contains_all_platforms() {
        assert!(FFMPEG_INSTALL_GUIDE.contains("macOS"));
        assert!(FFMPEG_INSTALL_GUIDE.contains("Windows"));
        assert!(FFMPEG_INSTALL_GUIDE.contains("Linux"));
        assert!(FFMPEG_INSTALL_GUIDE.contains("ffmpeg.org"));
    }
}
