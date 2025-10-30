//! 首尾边缘静音裁切器（P0阶段实现 - 已修复 min_run 语义）
//!
//! 实现样本级/帧级的首尾静音裁切，中段静音（艺术表达）保留不动。
//!
//! ## 核心语义（修复后）
//!
//! **最小持续时长 (min_run) 必须严格满足**：
//! - 只有当连续静音持续时间 ≥ min_run 时，才确认为"有效裁切目标"
//! - 短于 min_run 的首尾静音不会被裁切，而是完整回灌到输出
//! - 这防止了"极短的编码 artifact 或开头静音"被误伤
//!
//! ### 三态状态机（改进版）
//! - **Leading**: 首部裁切态，累积静音帧；仅当连续静音 ≥ min_run_frames 时丢弃（否则回灌）
//! - **Passing**: 通过态，正常输出所有帧
//! - **Trailing**: 尾部缓冲态，缓冲所有帧，跟踪末尾连续静音帧数；EOF 时仅当末尾静音 ≥ min_run_frames 才丢弃
//!
//! ### 迟滞机制（Hysteresis）
//! 防止古典音乐弱音段被误判为静音，要求连续N帧满足条件才转换状态。
//! 迟滞是"确认状态转换"的机制，不替代 min_run 的"确认裁切有效性"。
//!
//! ## 性能特性
//! - 时间复杂度: O(N) 单遍扫描
//! - 空间复杂度: O(min_run_frames) 环形缓冲 + O(min_run_frames) 首部缓冲
//! - 流式处理: 支持任意大小音频文件

use std::collections::VecDeque;

/// 边缘裁切配置（实验性功能）
#[derive(Debug, Clone, Copy)]
pub struct EdgeTrimConfig {
    /// 是否启用边缘裁切
    pub enabled: bool,
    /// 静音阈值（dBFS）- 假设 PCM 已归一化到 ±1.0FS
    pub threshold_db: f64,
    /// 最小连续静音时长（毫秒）- 只有达到此时长的静音才会被裁切
    pub min_run_ms: f64,
    /// 迟滞时长（毫秒） - 防止弱音乐段误判，用于确认状态转换
    pub hysteresis_ms: f64,
}

impl Default for EdgeTrimConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -60.0,
            min_run_ms: 60.0,    // 60ms 最小静音持续时长
            hysteresis_ms: 50.0, // 50ms 迟滞确认
        }
    }
}

impl EdgeTrimConfig {
    /// 创建禁用配置
    pub fn disabled() -> Self {
        Self::default()
    }

    /// 创建启用配置
    pub fn enabled(threshold_db: f64, min_run_ms: f64) -> Self {
        Self {
            enabled: true,
            threshold_db,
            min_run_ms,
            hysteresis_ms: (min_run_ms * 0.33).clamp(50.0, 200.0),
        }
    }

    /// 计算阈值的线性幅度（假设 PCM 归一化到 ±1.0FS）
    #[inline]
    fn threshold_amplitude(&self) -> f64 {
        10_f64.powf(self.threshold_db / 20.0)
    }
}

/// 裁切器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrimState {
    /// 首部裁切态：累积静音帧，等待确认
    Leading,
    /// 通过态：正常输出
    Passing,
    /// 尾部缓冲态：缓冲所有帧，跟踪末尾静音
    Trailing,
}

/// 裁切统计信息
#[derive(Debug, Clone, Copy, Default)]
pub struct TrimStats {
    /// 首部裁切的样本数（仅计当连续静音达到 min_run 后的丢弃）
    pub leading_samples_trimmed: usize,
    /// 尾部裁切的样本数（仅计 EOF 时末尾连续静音达到 min_run 的丢弃）
    pub trailing_samples_trimmed: usize,
    /// 总处理样本数（包括回灌的短静音）
    pub total_samples_processed: usize,
}

impl TrimStats {
    /// 计算首部裁切时长（秒）
    #[inline]
    pub fn leading_duration_sec(&self, sample_rate: u32, channels: usize) -> f64 {
        self.leading_samples_trimmed as f64 / (sample_rate as f64 * channels as f64)
    }

    /// 计算尾部裁切时长（秒）
    #[inline]
    pub fn trailing_duration_sec(&self, sample_rate: u32, channels: usize) -> f64 {
        self.trailing_samples_trimmed as f64 / (sample_rate as f64 * channels as f64)
    }

    /// 总裁切时长（秒）
    #[inline]
    pub fn total_duration_sec(&self, sample_rate: u32, channels: usize) -> f64 {
        self.leading_duration_sec(sample_rate, channels)
            + self.trailing_duration_sec(sample_rate, channels)
    }
}

/// 边缘裁切报告（包含配置与统计）
#[derive(Debug, Clone, Copy)]
pub struct EdgeTrimReport {
    pub config: EdgeTrimConfig,
    pub stats: TrimStats,
}

impl EdgeTrimReport {
    #[inline]
    pub fn leading_duration_sec(&self, sample_rate: u32, channels: usize) -> f64 {
        self.stats.leading_duration_sec(sample_rate, channels)
    }

    #[inline]
    pub fn trailing_duration_sec(&self, sample_rate: u32, channels: usize) -> f64 {
        self.stats.trailing_duration_sec(sample_rate, channels)
    }

    #[inline]
    pub fn total_duration_sec(&self, sample_rate: u32, channels: usize) -> f64 {
        self.stats.total_duration_sec(sample_rate, channels)
    }

    #[inline]
    pub fn total_samples_trimmed(&self) -> usize {
        self.stats.leading_samples_trimmed + self.stats.trailing_samples_trimmed
    }
}

/// 首尾边缘静音裁切器（min_run 语义已修复）
pub struct EdgeTrimmer {
    config: EdgeTrimConfig,
    state: TrimState,
    channels: usize,

    /// 线性幅度阈值（预计算，避免重复转换）
    threshold_amplitude: f64,

    /// 迟滞计数器（连续高于/低于阈值的帧数）
    hysteresis_counter: usize,
    /// 迟滞阈值（帧数）
    hysteresis_threshold_frames: usize,

    /// 最小持续时长（帧数）
    min_run_frames: usize,

    /// ===== Leading 态缓冲 =====
    /// 累积首部静音帧，直到达到 min_run 或检测到音频
    leading_buffer: VecDeque<f32>,
    /// 当前 Leading 态中累积的连续静音帧数
    leading_silent_count: usize,

    /// ===== Trailing 态缓冲 =====
    /// 环形缓冲区（存储 Trailing 中的所有帧）
    ring_buffer: VecDeque<f32>,
    /// Trailing 中最后的连续静音帧数
    trailing_silent_count: usize,

    /// 统计信息
    stats: TrimStats,
}

impl EdgeTrimmer {
    /// 创建新的边缘裁切器
    pub fn new(config: EdgeTrimConfig, channels: usize, sample_rate: u32) -> Self {
        let threshold_amplitude = config.threshold_amplitude();

        // 计算迟滞阈值（帧数）
        let hysteresis_threshold_frames =
            ((config.hysteresis_ms / 1000.0) * sample_rate as f64).ceil() as usize;

        // 计算最小持续时长（帧数）
        let min_run_frames = ((config.min_run_ms / 1000.0) * sample_rate as f64).ceil() as usize;

        // 计算缓冲区容量（样本数）
        let buffer_capacity_samples = min_run_frames * channels;

        Self {
            config,
            state: TrimState::Leading,
            channels,
            threshold_amplitude,
            hysteresis_counter: 0,
            hysteresis_threshold_frames,
            min_run_frames,
            leading_buffer: VecDeque::with_capacity(buffer_capacity_samples),
            leading_silent_count: 0,
            ring_buffer: VecDeque::with_capacity(buffer_capacity_samples),
            trailing_silent_count: 0,
            stats: TrimStats::default(),
        }
    }

    /// 计算单帧的幅度（声道数量大于1时的聚合策略）
    ///
    /// **声道数量大于1时的策略：取最大值（max strategy）**
    /// - 逻辑：只要任意一个声道不是静音，整帧就不视为静音
    /// - 应用：立体声中若某一声道有音频信号，则不应被裁切
    /// - 实现：对所有声道取绝对值的最大值
    ///
    /// **PCM归一化假设**
    /// - 本模块假设输入 PCM 样本已归一化到 ±1.0FS（标准浮点范围）
    /// - dBFS 转幅度：`10^(db/20)`，例 -60 dBFS ≈ 1.00e-3
    /// - 若输入未归一化，应在解码层进行规范化，或在阈值处理前进行转换
    #[inline]
    fn frame_amplitude(&self, samples: &[f32], frame_idx: usize) -> f64 {
        let base_idx = frame_idx * self.channels;
        let mut max_amplitude = 0.0_f64;

        for ch in 0..self.channels {
            let sample_idx = base_idx + ch;
            if sample_idx < samples.len() {
                let amp = samples[sample_idx].abs() as f64;
                if amp > max_amplitude {
                    max_amplitude = amp;
                }
            }
        }

        max_amplitude
    }

    /// 检查帧是否为静音（低于阈值）
    #[inline]
    fn is_silence_frame(&self, samples: &[f32], frame_idx: usize) -> bool {
        self.frame_amplitude(samples, frame_idx) < self.threshold_amplitude
    }

    /// 处理单个音频块（流式）
    pub fn process_chunk(&mut self, samples: &[f32]) -> Vec<f32> {
        if !self.config.enabled || samples.is_empty() {
            return samples.to_vec();
        }

        let frame_count = samples.len() / self.channels;
        let mut output = Vec::with_capacity(samples.len());

        for frame_idx in 0..frame_count {
            let is_silence = self.is_silence_frame(samples, frame_idx);

            match self.state {
                TrimState::Leading => {
                    if is_silence {
                        // 累积静音帧
                        self.leading_silent_count += 1;
                        Self::append_frame_to_buffer(
                            &mut self.leading_buffer,
                            samples,
                            frame_idx,
                            self.channels,
                        );

                        // 检查是否达到 min_run 确认点
                        if self.leading_silent_count >= self.min_run_frames {
                            // 确认为有效首部静音，开始丢弃
                            self.stats.leading_samples_trimmed += self.leading_buffer.len();
                            self.leading_buffer.clear();
                            self.leading_silent_count = 0;
                        }
                    } else {
                        // 遇到非静音帧

                        // 如果累积的静音 < min_run，这不符合"最小持续"要求，需要回灌
                        if self.leading_silent_count < self.min_run_frames {
                            output.extend(self.leading_buffer.drain(..));
                        } else {
                            // 否则这些静音已被确认为有效首部，不回灌
                            self.leading_buffer.clear();
                        }

                        self.leading_silent_count = 0;

                        // 输出当前非静音帧，并尝试进入 Passing 状态
                        self.hysteresis_counter += 1;
                        if self.hysteresis_counter >= self.hysteresis_threshold_frames {
                            // 迟滞确认：进入 Passing 状态
                            self.state = TrimState::Passing;
                            self.hysteresis_counter = 0;
                        }

                        Self::append_frame_to_output(
                            &mut output,
                            samples,
                            frame_idx,
                            self.channels,
                        );
                    }
                }

                TrimState::Passing => {
                    if is_silence {
                        // 检测到静音，进入 Trailing 状态
                        self.state = TrimState::Trailing;
                        self.hysteresis_counter = 0;
                        self.trailing_silent_count = 0;
                        self.ring_buffer.clear();
                        Self::append_frame_to_buffer(
                            &mut self.ring_buffer,
                            samples,
                            frame_idx,
                            self.channels,
                        );
                        self.trailing_silent_count += 1;
                    } else {
                        // 正常音频帧，直接输出
                        Self::append_frame_to_output(
                            &mut output,
                            samples,
                            frame_idx,
                            self.channels,
                        );
                    }
                }

                TrimState::Trailing => {
                    // 先将当前帧加入缓冲区
                    Self::append_frame_to_buffer(
                        &mut self.ring_buffer,
                        samples,
                        frame_idx,
                        self.channels,
                    );

                    if is_silence {
                        // 继续静音，累积计数
                        self.trailing_silent_count += 1;
                        self.hysteresis_counter = 0;
                    } else {
                        // 检测到非静音帧，重置静音计数
                        self.trailing_silent_count = 0;
                        self.hysteresis_counter += 1;

                        // 迟滞确认：缓冲区内检测到足够的音频，可以转回 Passing
                        if self.hysteresis_counter >= self.hysteresis_threshold_frames {
                            // 回灌缓冲区（这些不是有效的尾部静音）
                            output.extend(self.ring_buffer.drain(..));
                            self.state = TrimState::Passing;
                            self.hysteresis_counter = 0;
                            self.trailing_silent_count = 0;
                        }
                    }

                    // 不在此阶段丢弃缓冲，避免误删除需要回灌的静音。
                }
            }
        }

        self.stats.total_samples_processed += samples.len();
        output
    }

    /// 将单帧追加到输出
    #[inline]
    fn append_frame_to_output(
        output: &mut Vec<f32>,
        samples: &[f32],
        frame_idx: usize,
        channels: usize,
    ) {
        let base_idx = frame_idx * channels;
        for ch in 0..channels {
            let sample_idx = base_idx + ch;
            if sample_idx < samples.len() {
                output.push(samples[sample_idx]);
            }
        }
    }

    /// 将单帧追加到通用缓冲区
    #[inline]
    fn append_frame_to_buffer(
        buffer: &mut VecDeque<f32>,
        samples: &[f32],
        frame_idx: usize,
        channels: usize,
    ) {
        let base_idx = frame_idx * channels;
        for ch in 0..channels {
            let sample_idx = base_idx + ch;
            if sample_idx < samples.len() {
                buffer.push_back(samples[sample_idx]);
            }
        }
    }

    /// 结束处理，处理尾部缓冲区
    pub fn finalize(mut self) -> (Vec<f32>, TrimStats) {
        let final_output = match self.state {
            TrimState::Leading => {
                // 文件结尾仍在 Leading，回灌所有累积的首部静音（即使达到 min_run，也是EOF，不裁）
                self.leading_buffer.into_iter().collect()
            }
            TrimState::Trailing => {
                // 文件结尾在 Trailing
                // 仅当末尾的连续静音 >= min_run_frames 时，确认为有效尾部静音，丢弃缓冲区
                if self.trailing_silent_count >= self.min_run_frames {
                    // 末尾是有效尾部静音，丢弃 ring_buffer
                    self.stats.trailing_samples_trimmed += self.ring_buffer.len();
                    Vec::new()
                } else {
                    // 末尾的静音不足 min_run，或者缓冲区中有混合帧，回灌所有内容
                    self.ring_buffer.into_iter().collect()
                }
            }
            TrimState::Passing => {
                // 正常结尾，无需处理
                Vec::new()
            }
        };

        (final_output, self.stats)
    }

    /// 获取当前统计信息（不消耗trimmer）
    #[inline]
    pub fn stats(&self) -> &TrimStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助函数：生成测试音频（立体声交错）
    fn generate_test_audio(frames: usize, amplitude: f32, channels: usize) -> Vec<f32> {
        vec![amplitude; frames * channels]
    }

    #[test]
    fn test_leading_short_silence_回灌() {
        // 短静音 (50帧 ≈ 1.13ms < 10ms min_run) 应该被回灌，不裁
        let mut config = EdgeTrimConfig::enabled(-40.0, 10.0);
        config.hysteresis_ms = 5.0;
        let mut trimmer = EdgeTrimmer::new(config, 2, 44100);

        // 首部短静音（50帧 = ~1.13ms，远小于 10ms min_run）
        let short_silence = generate_test_audio(50, 0.0001, 2);
        let out1 = trimmer.process_chunk(&short_silence);

        // 正常音频（足够让迟滞计数达到 hysteresis_threshold）
        let audio = generate_test_audio(500, 0.5, 2);
        let out2 = trimmer.process_chunk(&audio);

        // 在 Leading 状态中，短静音被累积但不输出（out1 应为空）
        // 当遇到非静音时，由于短静音 < min_run，应在 out2 的开头回灌短静音
        assert!(out1.is_empty(), "Leading 状态中短静音应累积，不输出");
        // out2 应包含：回灌的短静音 + 一部分音频
        assert!(!out2.is_empty(), "非静音时应回灌短静音");
        assert!(out2.len() > short_silence.len(), "out2 应包含短静音+音频");
    }

    #[test]
    fn test_trailing_short_silence_回灌() {
        let mut config = EdgeTrimConfig::enabled(-40.0, 10.0);
        config.hysteresis_ms = 5.0;
        let mut trimmer = EdgeTrimmer::new(config, 2, 44100);

        // 正常音频
        let audio = generate_test_audio(500, 0.5, 2);
        trimmer.process_chunk(&audio);

        // 尾部短静音（300帧 ≈ 6.8ms < 10ms min_run）
        let short_trailing_silence = generate_test_audio(300, 0.0001, 2);
        trimmer.process_chunk(&short_trailing_silence);

        // Finalize：短静音应该回灌
        let (final_chunk, stats) = trimmer.finalize();

        assert_eq!(stats.trailing_samples_trimmed, 0, "短尾部静音不应被丢弃");
        assert!(!final_chunk.is_empty(), "短尾部静音应被回灌");
    }

    #[test]
    fn test_leading_long_silence_丢弃() {
        let mut config = EdgeTrimConfig::enabled(-40.0, 10.0);
        config.hysteresis_ms = 5.0;
        let mut trimmer = EdgeTrimmer::new(config, 2, 44100);

        // 首部长静音（5000帧 ≈ 113ms > 10ms min_run，会被丢弃）
        let long_silence = generate_test_audio(5000, 0.0001, 2);
        let out1 = trimmer.process_chunk(&long_silence);

        // 正常音频
        let audio = generate_test_audio(500, 0.5, 2);
        let _out2 = trimmer.process_chunk(&audio);

        // 长静音应该被丢弃，输出应该只有音频
        assert!(out1.is_empty() || out1.len() < long_silence.len());
    }

    #[test]
    fn test_mixed_frames_in_trailing_回灌() {
        // Trailing 中如果有混合帧（静音+非静音），应该整体回灌
        let mut config = EdgeTrimConfig::enabled(-40.0, 10.0);
        config.hysteresis_ms = 5.0;
        let mut trimmer = EdgeTrimmer::new(config, 2, 44100);

        let audio = generate_test_audio(500, 0.5, 2);
        trimmer.process_chunk(&audio);

        // Trailing 序列：静音 + 少量非静音（未达 hysteresis）
        let mut mixed = Vec::new();
        mixed.extend_from_slice(&generate_test_audio(200, 0.0001, 2)); // 200帧静音
        mixed.extend_from_slice(&generate_test_audio(50, 0.5, 2)); // 50帧非静音
        let _out = trimmer.process_chunk(&mixed);

        // 由于有非静音且未达 hysteresis，Trailing 应保持
        // EOF 时 trailing_silent_count 应该被重置为 0（因为有非静音），所以不满足 min_run 条件，回灌
        let (final_chunk, stats) = trimmer.finalize();

        assert_eq!(stats.trailing_samples_trimmed, 0, "混合帧不应被丢弃");
        assert!(!final_chunk.is_empty(), "混合尾段应被回灌");
    }

    #[test]
    fn test_disabled_passthrough() {
        let config = EdgeTrimConfig::disabled();
        let mut trimmer = EdgeTrimmer::new(config, 2, 44100);

        let input = generate_test_audio(1000, 0.5, 2);
        let output = trimmer.process_chunk(&input);

        assert_eq!(output.len(), input.len());
    }
}
