//! FFmpeg桥接解码器
//!
//! 为Symphonia不支持的格式提供FFmpeg回退方案，通过管道实现流式解码。
//! 支持格式：AC-3, E-AC-3, DTS, DSD (DSF/DFF)等。

use crate::error::{AudioError, AudioResult};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

/// 在 Windows 上隐藏子进程控制台窗口（用于 GUI 场景避免 FFmpeg 弹窗）
#[cfg(target_os = "windows")]
fn configure_creation_flags(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// 非 Windows 平台：不做任何额外配置
#[cfg(not(target_os = "windows"))]
fn configure_creation_flags(_cmd: &mut Command) {}

use super::channel_layout;
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
/// Windows 管道缓冲区较小（4KB），使用 BufReader 减少系统调用
const PIPE_BUFFER_SIZE: usize = 128 * 1024; // 128KB

pub struct FFmpegDecoder {
    /// FFmpeg子进程
    child: Child,
    /// 带缓冲的 stdout 读取器（减少 Windows 管道系统调用）
    stdout_reader: Option<BufReader<ChildStdout>>,
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
    /// 是否使用32位整型样本（S32LE）
    use_s32: bool,
    /// 是否使用32位浮点样本（F32LE）
    use_f32: bool,
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
        if let Some(override_path) = std::env::var("MACINMETER_FFMPEG_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
        {
            let candidate = PathBuf::from(override_path.trim());
            let mut cmd = Command::new(&candidate);
            cmd.arg("-version");
            configure_creation_flags(&mut cmd);
            if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
                return Some(candidate);
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Windows: 额外从常见环境变量中解析 ffmpeg 路径
            // 例如用户设置的 FFMPEG_PATH / FFMPEG_DIR 等
            let mut env_candidates: Vec<PathBuf> = Vec::new();

            if let Ok(p) = std::env::var("FFMPEG_PATH") {
                let p = p.trim();
                if !p.is_empty() {
                    env_candidates.push(PathBuf::from(p));
                }
            }

            if let Ok(p) = std::env::var("FFMPEG_DIR") {
                let p = p.trim();
                if !p.is_empty() {
                    let base = PathBuf::from(p);
                    env_candidates.push(base.join("bin").join("ffmpeg.exe"));
                    env_candidates.push(base.join("ffmpeg.exe"));
                }
            }

            // Windows: 检查多个常见位置
            let mut candidates = vec![
                PathBuf::from(r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"),
                PathBuf::from(r"C:\ffmpeg\bin\ffmpeg.exe"),
                // 便携部署：与可执行文件同目录
                std::env::current_exe().ok()?.parent()?.join("ffmpeg.exe"),
            ];

            // 环境变量优先于内置候选路径
            if !env_candidates.is_empty() {
                env_candidates.extend(candidates);
                candidates = env_candidates;
            }

            // 先尝试 PATH 中的 ffmpeg（不依赖文件存在，直接调用）
            {
                let mut cmd = Command::new("ffmpeg");
                cmd.arg("-version");
                configure_creation_flags(&mut cmd);
                if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
                    return Some(PathBuf::from("ffmpeg"));
                }
            }

            candidates.into_iter().find(|p| {
                if !p.exists() {
                    return false;
                }
                let mut cmd = Command::new(p);
                cmd.arg("-version");
                configure_creation_flags(&mut cmd);
                cmd.output().map(|o| o.status.success()).unwrap_or(false)
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            // macOS/Linux: 直接使用PATH中的ffmpeg
            let candidate = PathBuf::from("ffmpeg");
            let mut cmd = Command::new(&candidate);
            cmd.arg("-version");
            configure_creation_flags(&mut cmd);
            if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
                Some(candidate)
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

        let mut cmd = Command::new(ffprobe_cmd);
        cmd.arg("-version");
        configure_creation_flags(&mut cmd);
        if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
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

        let mut cmd = Command::new(&ffprobe_path);
        cmd.args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            // 追加布局字段和容器格式，按声明顺序输出，便于解析
            "stream=codec_name,sample_rate,channels,duration,channel_layout,ch_layout",
            "-show_entries",
            "format=format_name",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str().ok_or_else(|| {
                AudioError::InvalidInput(
                    "File path contains invalid UTF-8 / 文件路径包含无效UTF-8".to_string(),
                )
            })?,
        ]);
        configure_creation_flags(&mut cmd);
        let output = cmd.output().map_err(|e| {
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

        // ffprobe输出顺序实际为：codec_name, sample_rate, channels, channel_layout, duration, ch_layout
        // 注意：channel_layout在duration之前！
        let codec_name = Some(lines[0].to_string());

        let sample_rate = lines[1].parse::<u32>().map_err(|e| {
            AudioError::FormatError(format!("Invalid sample rate / 无效的采样率: {e}"))
        })?;

        let channels = lines[2].parse::<u16>().map_err(|e| {
            AudioError::FormatError(format!("Invalid channel count / 无效的声道数: {e}"))
        })?;

        // lines[3]可能是channel_layout或duration，需要鲁棒解析
        // 注意："7.1"也能解析为f64，所以不能简单用parse判断
        // 判断规则：channel_layout通常包含字母、括号或小于20的小数（如5.1, 7.1）
        //          duration通常是较大数字（如187.968000）
        let is_likely_channel_layout = {
            let s = lines[3];
            // 包含字母或括号，肯定是channel_layout
            if s.chars().any(|c| c.is_alphabetic() || c == '(' || c == ')') {
                true
            } else if let Ok(num) = s.parse::<f64>() {
                // 纯数字：小于20认为是channel_layout（5.1, 7.1等），否则是duration
                num < 20.0
            } else {
                true // 无法解析，默认认为是channel_layout
            }
        };

        let (channel_layout_line, duration_line) = if is_likely_channel_layout {
            // 标准输出：channel_layout在duration之前
            (lines[3], lines.get(4).map(|s| s.trim()).unwrap_or(""))
        } else {
            // 旧版ffprobe或特殊格式：duration在channel_layout之前
            (lines.get(4).map(|s| s.trim()).unwrap_or(""), lines[3])
        };

        let duration = duration_line.parse::<f64>().ok().unwrap_or(0.0);
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

        // 解析布局字符串和容器格式：用于标记布局元数据与推导LFE索引
        // format_name总是最后一行输出
        let format_name_raw = lines.last().map(|s| s.trim()).unwrap_or("");

        // ch_layout可能为空（不输出），所以需要鲁棒解析
        let ch_layout_str_raw = if lines.len() > 6 {
            lines.get(5).map(|s| s.trim()).unwrap_or("")
        } else {
            ""
        };

        let layout_str_raw = channel_layout_line;
        let layout_joined = if !layout_str_raw.is_empty() {
            layout_str_raw.to_string()
        } else if !ch_layout_str_raw.is_empty() {
            ch_layout_str_raw.to_string()
        } else {
            String::new()
        };

        if !layout_joined.is_empty() {
            format.mark_has_channel_layout();

            // 容器格式决定声道顺序：
            // - EAC3裸流（.ec3）：format_name=eac3，声道顺序 L,C,R,Ls,Rs,LFE（索引5）
            // - M4A/MP4容器：format_name包含mov/mp4/m4a，声道顺序 L,R,C,LFE,Ls,Rs（索引3）
            let format_lower = format_name_raw.to_lowercase();
            let is_eac3_raw = format_lower == "eac3" || format_lower == "ac3";

            if is_eac3_raw {
                // EAC3裸流：LFE固定在索引5
                let lfe_idx = match (layout_joined.to_lowercase().as_str(), format.channels) {
                    ("5.1" | "5.1(side)", 6) => Some(vec![5]), // EAC3 5.1: L,C,R,Ls,Rs,LFE
                    ("5.1.2", 8) => Some(vec![5]), // EAC3 5.1.2: L,C,R,Ls,Rs,LFE,Ltm,Rtm
                    ("7.1" | "7.1(wide)", 8) => Some(vec![5]), // EAC3 7.1: L,C,R,Ls,Rs,LFE,Rls,Rrs
                    _ => None,
                };

                if let Some(indices) = lfe_idx {
                    format.set_lfe_indices(indices);
                }
            } else {
                // 容器格式或其他编码：使用精确的声道布局检测（基于Apple CoreAudio规范）
                if let Some(lfe_idxs) =
                    channel_layout::detect_lfe_from_layout(&layout_joined, format.channels)
                    && !lfe_idxs.is_empty()
                {
                    format.set_lfe_indices(lfe_idxs);
                }
            }
        }

        // 进一步：尝试用 JSON 精确解析通道标签顺序（如 FL+FR+FC+LFE+...），直接定位 LFE/LFE2 的下标
        // 仅在尚未获得精确 lfe_indices 时尝试
        #[allow(clippy::collapsible_if)]
        {
            if format.lfe_indices.is_empty() {
                if let Some(path_str) = path.to_str() {
                    let mut cmd = Command::new(&ffprobe_path);
                    cmd.args([
                        "-v",
                        "error",
                        "-select_streams",
                        "a:0",
                        "-show_entries",
                        "stream=channel_layout,side_data_list",
                        "-of",
                        "json",
                        path_str,
                    ]);
                    configure_creation_flags(&mut cmd);
                    let json_output = cmd.output().ok();

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

        // 若为 DSD（通过扩展名或 codec_name 判断），尝试解析原生一位采样率（Hz）与 DSD 档位
        let is_dsd_ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("dsf") || s.eq_ignore_ascii_case("dff"))
            .unwrap_or(false);
        let is_dsd_codec = codec_name
            .as_ref()
            .map(|s| s.to_ascii_lowercase().contains("dsd"))
            .unwrap_or(false);
        if is_dsd_ext || is_dsd_codec {
            // 明确将位深标记为 1（来源为 DSD 1-bit），避免报告显示默认 16bit
            format.bits_per_sample = 1;
        }

        if (is_dsd_ext || is_dsd_codec)
            && let Some(path_str) = path.to_str()
        {
            let mut cmd = Command::new(&ffprobe_path);
            cmd.args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=sample_rate,bits_per_raw_sample,codec_name,profile",
                "-of",
                "json",
                path_str,
            ]);
            configure_creation_flags(&mut cmd);
            let json_out = cmd.output().ok();
            if let Some(out) = json_out
                && out.status.success()
                && let Ok(text) = String::from_utf8(out.stdout)
            {
                #[derive(serde::Deserialize)]
                struct StreamDSD {
                    #[serde(default)]
                    sample_rate: Option<String>,
                    #[serde(default)]
                    bits_per_raw_sample: Option<u32>,
                    #[allow(dead_code)]
                    #[serde(default)]
                    codec_name: Option<String>,
                    #[allow(dead_code)]
                    #[serde(default)]
                    profile: Option<String>,
                }
                #[derive(serde::Deserialize)]
                struct RootDSD {
                    #[serde(default)]
                    streams: Vec<StreamDSD>,
                }
                if let Ok(parsed) = serde_json::from_str::<RootDSD>(&text)
                    && let Some(st) = parsed.streams.first()
                {
                    let sr = st
                        .sample_rate
                        .as_deref()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    let bits1 = st.bits_per_raw_sample.unwrap_or_default() == 1;
                    // 解析 profile 优先：dsd64/dsd128/...
                    let mut native: Option<u32> = None;
                    let mut level: Option<u32> = None;
                    if let Some(p) = &st.profile {
                        let pl = p.to_ascii_lowercase();
                        for &(tag, mul) in [
                            ("dsd64", 64u32),
                            ("dsd128", 128u32),
                            ("dsd256", 256u32),
                            ("dsd512", 512u32),
                            ("dsd1024", 1024u32),
                        ]
                        .iter()
                        {
                            if pl.contains(tag) {
                                level = Some(mul);
                                native = Some(44_100u32.saturating_mul(mul));
                                break;
                            }
                        }
                    }

                    // 回退1：1bit 且 sr ≥ 2MHz → 直接采用 sr 作为原生率
                    if native.is_none() && (bits1 && sr >= 2_000_000) {
                        native = Some(sr);
                    }

                    // 回退2：1bit 且 sr 为 44.1k*N（如 352800/705600/1411200），推断原生率 = sr*8
                    if native.is_none() && bits1 && sr % 44_100 == 0 {
                        let pre_mul = sr / 44_100; // 8/16/32...
                        let mul = pre_mul.saturating_mul(8); // → 64/128/256...
                        if [64u32, 128, 256, 512, 1024].contains(&mul) {
                            level = Some(mul);
                            native = Some(44_100u32.saturating_mul(mul));
                        }
                    }

                    // 3) 最后回退：仅凭扩展名与 sr 推断（无 bits 信息）
                    if native.is_none() && is_dsd_ext && sr % 44_100 == 0 {
                        let pre_mul = sr / 44_100;
                        if pre_mul >= 8 {
                            let mul = pre_mul.saturating_mul(8);
                            if [64u32, 128, 256, 512, 1024].contains(&mul) {
                                level = Some(mul);
                                native = Some(44_100u32.saturating_mul(mul));
                            }
                        }
                    }

                    if let Some(n) = native {
                        format.dsd_native_rate_hz = Some(n);
                        if level.is_none() {
                            let m = (n as f64 / 44_100.0).round() as u32;
                            let candidates = [64u32, 128, 256, 512, 1024];
                            let mut best = None;
                            let mut best_diff = u32::MAX;
                            for &c in &candidates {
                                let diff = m.abs_diff(c);
                                if diff < best_diff {
                                    best_diff = diff;
                                    best = Some(c);
                                }
                            }
                            level = best;
                        }
                        format.dsd_multiple_of_44k = level;
                    }
                }
            }
        }

        // 标记为FFmpeg解码（用于诊断）
        // 注：codec_name信息已通过ffprobe获取（{codec_name:?}），保留供调试使用
        let _codec_name = codec_name; // 避免未使用变量警告

        Ok(format)
    }

    /// 创建FFmpeg解码器（默认策略）
    pub fn new(path: &Path) -> AudioResult<Self> {
        Self::new_with_options(path, None, None, None)
    }

    /// 创建FFmpeg解码器（可配置 DSD → PCM 采样率与增益）
    pub fn new_with_options(
        path: &Path,
        dsd_pcm_rate: Option<u32>,
        dsd_gain_db: Option<f32>,
        dsd_filter: Option<String>,
    ) -> AudioResult<Self> {
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
            path.to_str()
                .ok_or_else(|| {
                    AudioError::InvalidInput(
                        "File path contains invalid UTF-8 / 文件路径包含无效UTF-8".to_string(),
                    )
                })?
                .to_string(),
        ];

        let use_s32 = false;
        let mut use_f32 = false;

        if is_dsd {
            // DSD 专用：统一转换为可配置的 PCM 采样率（默认 352.8 kHz，整数比率）
            // 注：foobar2000 常见显示为 384 kHz（非 44.1k 整数比），此处默认选用 352.8 kHz 以避免分数重采样。
            let target_rate: u32 = dsd_pcm_rate.unwrap_or(352_800);

            // 计算低通截止频率（TEAC 或 Studio 模式）
            let mode = dsd_filter.as_deref().unwrap_or("teac");
            let mut fc_hz: f32 = 20_000.0; // studio 默认 20kHz
            if mode.eq_ignore_ascii_case("teac") {
                if let Some(mul) = format.dsd_multiple_of_44k {
                    let scale = (mul.max(64) as f32) / 64.0;
                    fc_hz = 39_000.0 * scale;
                } else if let Some(native) = format.dsd_native_rate_hz {
                    let m = (native as f32 / 44_100.0).round().max(64.0);
                    let scale = m / 64.0;
                    fc_hz = 39_000.0 * scale;
                } else {
                    fc_hz = 20_000.0; // 未识别档位时，保守回退
                }
                // 安全上限：0.45 × 目标采样率
                let fc_limit = (target_rate as f32) * 0.45;
                if fc_hz > fc_limit {
                    fc_hz = fc_limit;
                }
            }

            // 组装滤镜与采样率
            let gain_db = dsd_gain_db.unwrap_or(6.0);
            let mut filter_chain = String::new();
            if !mode.eq_ignore_ascii_case("off") {
                filter_chain.push_str(&format!("lowpass=f={fc_hz:.0}"));
            }
            if gain_db.abs() > 0.0001 {
                if !filter_chain.is_empty() {
                    filter_chain.push(',');
                }
                filter_chain.push_str(&format!("volume={gain_db}dB"));
            }
            if !filter_chain.is_empty() {
                args.push("-af".to_string());
                args.push(filter_chain);
            }
            args.push("-ar".to_string());
            args.push(target_rate.to_string());

            // DSD 输出使用 32-bit float（F32LE）
            use_f32 = true;

            // 标记处理用采样率，便于报告输出“源 → 处理（DSD降采样）”
            // 注意：AudioFormat.sample_rate 保留为“源采样率”
            //       processed_sample_rate 记录真实处理用采样率
            // SAFETY: 仅在内部使用，可安全更新
            // 这里 format 仍在本作用域，未跨线程共享
            format.processed_sample_rate = Some(target_rate);
        }
        // 非 DSD 路径：统一使用 32-bit float（F32LE）输出，便于后续处理一致性
        if !is_dsd {
            use_f32 = true;
        }

        // 输出PCM格式：DSD→F32，其余默认 S16
        if use_f32 {
            args.extend(vec![
                "-f".to_string(),
                "f32le".to_string(),
                "-acodec".to_string(),
                "pcm_f32le".to_string(),
            ]);
        } else if use_s32 {
            args.extend(vec![
                "-f".to_string(),
                "s32le".to_string(),
                "-acodec".to_string(),
                "pcm_s32le".to_string(),
            ]);
        } else {
            args.extend(vec![
                "-f".to_string(),
                "s16le".to_string(),
                "-acodec".to_string(),
                "pcm_s16le".to_string(),
            ]);
        }
        args.push("-".to_string()); // 输出到stdout

        // 启动FFmpeg子进程
        let mut cmd = Command::new(&ffmpeg_path);
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_creation_flags(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| {
            AudioError::DecodingError(format!("Failed to spawn FFmpeg / 无法启动FFmpeg: {e}"))
        })?;

        // 用 BufReader 包装 stdout，减少 Windows 管道系统调用
        let stdout = child.stdout.take().ok_or_else(|| {
            AudioError::DecodingError(
                "FFmpeg stdout not available / FFmpeg标准输出不可用".to_string(),
            )
        })?;
        let stdout_reader = Some(BufReader::with_capacity(PIPE_BUFFER_SIZE, stdout));

        let total_samples = format.sample_count; // 提前保存，避免move后使用

        eprintln!(
            "[INFO] FFmpeg decoder initialized / FFmpeg解码器已初始化: {} channels, {}Hz, format={}",
            format.channels,
            format.sample_rate,
            if use_f32 {
                "F32LE"
            } else if use_s32 {
                "S32LE"
            } else {
                "S16LE"
            }
        );

        Ok(Self {
            child,
            stdout_reader,
            ffmpeg_path,
            spawn_args: args.clone(),
            input_path: path.to_path_buf(),
            format,
            current_position: 0,
            total_samples,
            chunk_stats: ChunkSizeStats::new(),
            eof_reached: false,
            use_s32,
            use_f32,
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

    /// F32LE 字节转 f32 样本（小端序，零拷贝重组）
    #[allow(dead_code)]
    fn convert_f32le_to_f32(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
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
        let bytes_per_sample = if self.use_f32 || self.use_s32 { 4 } else { 2 };
        let default_chunk_bytes =
            self.format.sample_rate as usize * channels * bytes_per_sample * 3; // 目标≈3秒
        let bytes_per_chunk = default_chunk_bytes.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);
        // 向下对齐到整帧（channels样本边界）
        let bytes_per_chunk =
            (bytes_per_chunk / (channels * bytes_per_sample)) * (channels * bytes_per_sample);

        let mut buffer = vec![0u8; bytes_per_chunk];
        let mut accumulated = Vec::new(); // 积累不足一帧的数据

        let reader = self.stdout_reader.as_mut().ok_or_else(|| {
            AudioError::DecodingError(
                "FFmpeg stdout not available / FFmpeg标准输出不可用".to_string(),
            )
        })?;

        // 填满式读取循环，直到获得完整帧数或EOF
        loop {
            match reader.read(&mut buffer) {
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
                    // 计算字节大小（S16LE=2字节，S32LE/F32LE=4字节）
                    let bytes_per_complete_sample =
                        if self.use_f32 || self.use_s32 { 4 } else { 2 };
                    // 检查是否有完整帧
                    let complete_frames = (accumulated.len()
                        / (channels * bytes_per_complete_sample))
                        * (channels * bytes_per_complete_sample);
                    if complete_frames > 0 {
                        // 有完整帧，返回
                        let data = accumulated.drain(..complete_frames).collect::<Vec<_>>();
                        let samples = if self.use_f32 {
                            Self::convert_f32le_to_f32(&data)
                        } else if self.use_s32 {
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
            let samples = if self.use_f32 {
                Self::convert_f32le_to_f32(&accumulated)
            } else if self.use_s32 {
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
        let mut cmd = Command::new(&self.ffmpeg_path);
        cmd.args(&self.spawn_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_creation_flags(&mut cmd);
        self.child = cmd.spawn().map_err(|e| {
            AudioError::DecodingError(format!("Failed to respawn FFmpeg / 无法重启FFmpeg: {e}"))
        })?;

        // 重新创建 BufReader
        let stdout = self.child.stdout.take().ok_or_else(|| {
            AudioError::DecodingError(
                "FFmpeg stdout not available after reset / 重启后FFmpeg标准输出不可用".to_string(),
            )
        })?;
        self.stdout_reader = Some(BufReader::with_capacity(PIPE_BUFFER_SIZE, stdout));

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
