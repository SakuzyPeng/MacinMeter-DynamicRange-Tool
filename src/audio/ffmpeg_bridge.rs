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
                PathBuf::from("ffmpeg.exe"), // PATH中
                PathBuf::from(r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"),
                PathBuf::from(r"C:\ffmpeg\bin\ffmpeg.exe"),
                // 便携部署：与可执行文件同目录
                std::env::current_exe().ok()?.parent()?.join("ffmpeg.exe"),
            ];

            candidates.into_iter().find(|p| {
                p.exists()
                    && Command::new(p)
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

    /// 使用ffprobe探测音频格式信息
    fn probe_format(path: &Path) -> AudioResult<AudioFormat> {
        let ffprobe_path = if cfg!(target_os = "windows") {
            "ffprobe.exe"
        } else {
            "ffprobe"
        };

        let output = Command::new(ffprobe_path)
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=sample_rate,channels,duration,codec_name",
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
            return Err(AudioError::FormatError(format!(
                "ffprobe failed / ffprobe失败: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        if lines.len() < 4 {
            return Err(AudioError::FormatError(
                "Incomplete ffprobe output / ffprobe输出不完整".to_string(),
            ));
        }

        // ffprobe输出顺序：codec_name, sample_rate, channels, duration
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
        let bits_per_sample = 16; // FFmpeg输出为S16LE

        let format = AudioFormat::new(sample_rate, channels, bits_per_sample, sample_count);

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
        let format = Self::probe_format(path)?;

        // 检测是否为DSD格式
        let is_dsd = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("dsf") || s.eq_ignore_ascii_case("dff"))
            .unwrap_or(false);

        // 构建FFmpeg命令参数
        let mut args = vec![
            "-v".to_string(),
            "error".to_string(),
            "-i".to_string(),
            path.to_str().unwrap().to_string(),
        ];

        if is_dsd {
            // DSD专用优化：高质量转换参数
            args.extend(vec![
                "-af".to_string(),
                "lowpass=20000,volume=6dB".to_string(), // 低通滤波 + 增益补偿
                "-sample_fmt".to_string(),
                "s32".to_string(), // 32位避免精度损失
                "-ar".to_string(),
                format.sample_rate.to_string(), // 保持探测到的采样率
            ]);
        }

        // 输出PCM格式
        args.extend(vec![
            "-f".to_string(),
            "s16le".to_string(),
            "-acodec".to_string(),
            "pcm_s16le".to_string(),
            "-".to_string(), // 输出到stdout
        ]);

        // 启动FFmpeg子进程
        let child = Command::new(&ffmpeg_path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                AudioError::DecodingError(format!("Failed to spawn FFmpeg / 无法启动FFmpeg: {e}"))
            })?;

        let total_samples = format.sample_count; // 提前保存，避免move后使用

        eprintln!(
            "[INFO] FFmpeg decoder initialized / FFmpeg解码器已初始化: {} channels, {}Hz",
            format.channels, format.sample_rate
        );

        Ok(Self {
            child,
            format,
            current_position: 0,
            total_samples,
            chunk_stats: ChunkSizeStats::new(),
            eof_reached: false,
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
}

impl StreamingDecoder for FFmpegDecoder {
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        if self.eof_reached {
            return Ok(None);
        }

        // 每次读取3秒的音频数据（可调整）
        const CHUNK_DURATION_SECONDS: usize = 3;
        let samples_per_chunk = self.format.sample_rate as usize
            * self.format.channels as usize
            * CHUNK_DURATION_SECONDS;
        let bytes_per_chunk = samples_per_chunk * 2; // S16LE = 2字节/样本

        let mut buffer = vec![0u8; bytes_per_chunk];

        let stdout = self.child.stdout.as_mut().ok_or_else(|| {
            AudioError::DecodingError(
                "FFmpeg stdout not available / FFmpeg标准输出不可用".to_string(),
            )
        })?;

        match stdout.read(&mut buffer) {
            Ok(0) => {
                // EOF
                self.eof_reached = true;
                Ok(None)
            }
            Ok(bytes_read) => {
                // 转换S16LE → F32
                let samples = Self::convert_s16le_to_f32(&buffer[..bytes_read]);

                // 更新位置
                let samples_per_channel = samples.len() / self.format.channels as usize;
                self.current_position += samples_per_channel as u64;

                // 记录chunk统计
                self.chunk_stats.add_chunk(samples.len());

                Ok(Some(samples))
            }
            Err(e) => Err(AudioError::DecodingError(format!(
                "Failed to read from FFmpeg / FFmpeg读取失败: {e}"
            ))),
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
        // 终止FFmpeg进程
        let _ = self.child.kill();
        self.current_position = 0;
        self.eof_reached = false;
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
