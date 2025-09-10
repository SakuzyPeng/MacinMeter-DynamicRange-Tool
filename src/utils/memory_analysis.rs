//! 音频内存需求分析工具
//!
//! 精确计算不同音频格式的最小内存需求，确保即使在极端配置下也能正常工作。

use crate::audio::universal_decoder::AudioFormat;
use std::collections::HashMap;

/// 音频复杂度等级
#[derive(Debug, Clone, PartialEq)]
pub enum AudioComplexity {
    /// 简单：单声道/立体声，<=48kHz，<=24位
    Simple,
    /// 标准：2-8声道，<=96kHz，<=32位
    Standard,
    /// 复杂：8-16声道，<=192kHz，32位
    Complex,
    /// 极端：16+声道，高采样率，32位
    Extreme,
}

/// 内存需求分析结果
#[derive(Debug, Clone)]
pub struct MemoryRequirement {
    /// 音频复杂度等级
    pub complexity: AudioComplexity,

    /// 原始音频数据大小（3秒块）
    pub raw_audio_bytes: u64,

    /// 解码所需内存
    pub decoding_memory: u64,

    /// DR计算所需内存
    pub dr_calculation_memory: u64,

    /// 系统开销内存
    pub system_overhead: u64,

    /// 总最小需求
    pub total_minimum: u64,

    /// 推荐内存（包含安全边际）
    pub recommended_memory: u64,

    /// 是否可以在极限模式下处理
    pub survivable_in_emergency: bool,
}

/// 音频内存分析器
pub struct AudioMemoryAnalyzer;

impl AudioMemoryAnalyzer {
    /// 分析音频格式的内存需求
    pub fn analyze_requirements(format: &AudioFormat) -> MemoryRequirement {
        let complexity = Self::classify_complexity(format);

        // 基础计算：3秒音频块的原始大小
        let samples_per_3_seconds = format.sample_rate as u64 * 3;
        let bytes_per_sample = match format.bits_per_sample {
            16 => 2,
            24 => 3,
            32 => 4,
            _ => 4, // 默认按最大计算
        };

        let raw_audio_bytes = samples_per_3_seconds * format.channels as u64 * bytes_per_sample;

        // 各部分内存需求计算
        let decoding_memory = Self::calculate_decoding_memory(format, raw_audio_bytes);
        let dr_calculation_memory = Self::calculate_dr_memory(format, raw_audio_bytes);
        let system_overhead = Self::calculate_system_overhead(format, raw_audio_bytes);

        let total_minimum =
            raw_audio_bytes + decoding_memory + dr_calculation_memory + system_overhead;
        let recommended_memory = (total_minimum as f64 * 1.5) as u64; // 50%安全边际

        // 判断极限模式存活性
        let survivable_in_emergency = total_minimum <= 512 * 1024 * 1024; // 512MB内能否处理

        MemoryRequirement {
            complexity,
            raw_audio_bytes,
            decoding_memory,
            dr_calculation_memory,
            system_overhead,
            total_minimum,
            recommended_memory,
            survivable_in_emergency,
        }
    }

    /// 分类音频复杂度
    fn classify_complexity(format: &AudioFormat) -> AudioComplexity {
        let channels = format.channels as u32;
        let sample_rate = format.sample_rate;
        let bits = format.bits_per_sample as u32;

        // 超高采样率处理
        if sample_rate >= 384000 {
            return AudioComplexity::Extreme; // 384kHz+直接归为极端
        }

        // 192kHz需要特殊考虑
        if sample_rate >= 192000 && (channels > 2 || bits > 24) {
            return AudioComplexity::Extreme;
        }

        if channels <= 2 && sample_rate <= 48000 && bits <= 24 {
            AudioComplexity::Simple
        } else if channels <= 8 && sample_rate <= 96000 && bits <= 32 {
            AudioComplexity::Standard
        } else if channels <= 16 && sample_rate <= 192000 && bits <= 32 {
            AudioComplexity::Complex
        } else {
            AudioComplexity::Extreme
        }
    }

    /// 计算解码内存需求
    fn calculate_decoding_memory(format: &AudioFormat, raw_bytes: u64) -> u64 {
        // 基础解码缓冲区倍数
        let mut multiplier = match format.bits_per_sample {
            16 => 1.5, // 16位解码开销较小
            24 => 2.0, // 24位需要更多转换
            32 => 2.5, // 32位解码开销最大
            _ => 2.0,
        };

        // 超高采样率需要更大的缓冲区
        let sample_rate_factor = match format.sample_rate {
            ..=48000 => 1.0,        // 标准采样率
            48001..=96000 => 1.2,   // 高采样率
            96001..=192000 => 1.5,  // 超高采样率
            192001..=384000 => 2.0, // 极端采样率
            384001.. => 3.0,        // DSD等极端格式
        };

        multiplier *= sample_rate_factor;

        // 多声道需要额外缓冲
        let channel_factor = match format.channels {
            0 => 1.0,       // 异常情况，按最小处理
            1..=2 => 1.0,   // 单声道/立体声
            3..=8 => 1.2,   // 环绕声
            9..=16 => 1.5,  // 多声道
            17..=32 => 2.0, // 超多声道
            33.. => 2.5,    // 极端多声道
        };

        (raw_bytes as f64 * multiplier * channel_factor) as u64
    }

    /// 计算DR计算内存需求  
    fn calculate_dr_memory(format: &AudioFormat, raw_bytes: u64) -> u64 {
        // DR计算需要：
        // 1. f32样本数组（4字节/样本）
        // 2. 块统计数据结构
        // 3. 累加器和缓冲区

        let f32_array_size = raw_bytes; // 通常与原始大小相当
        let block_metadata = format.channels as u64 * 1024; // 每声道约1KB元数据
        let accumulators = format.channels as u64 * 256; // 累加器内存

        f32_array_size + block_metadata + accumulators
    }

    /// 计算系统开销
    fn calculate_system_overhead(format: &AudioFormat, raw_bytes: u64) -> u64 {
        let base_overhead = 16 * 1024 * 1024; // 16MB基础开销

        // 复杂音频格式需要更多开销
        let complexity_overhead = match Self::classify_complexity(format) {
            AudioComplexity::Simple => 0,
            AudioComplexity::Standard => 8 * 1024 * 1024, // 8MB
            AudioComplexity::Complex => 32 * 1024 * 1024, // 32MB
            AudioComplexity::Extreme => {
                // 极端格式需要根据声道数动态调整开销
                let base_extreme = 64 * 1024 * 1024; // 基础64MB
                if format.channels > 16 {
                    // 超多声道需要额外大量开销：每增加16声道，增加256MB开销
                    base_extreme + ((format.channels as u64 - 16) / 16 + 1) * 256 * 1024 * 1024
                } else {
                    base_extreme
                }
            }
        };

        // 高采样率需要额外开销
        let sample_rate_overhead = match format.sample_rate {
            ..=48000 => 0,
            48001..=96000 => (raw_bytes as f64 * 0.1) as u64, // 10%额外开销
            96001..=192000 => (raw_bytes as f64 * 0.25) as u64, // 25%额外开销
            192001..=384000 => (raw_bytes as f64 * 0.5) as u64, // 50%额外开销
            384001.. => (raw_bytes as f64 * 1.0) as u64,      // 100%额外开销
        };

        base_overhead + complexity_overhead + sample_rate_overhead
    }

    /// 生成内存需求报告
    pub fn generate_report(requirement: &MemoryRequirement, format: &AudioFormat) -> String {
        format!(
            "🧮 音频内存需求分析:\n\
             格式: {}声道 {}位 {}Hz\n\
             复杂度: {:?}\n\
             \n\
             📊 内存分解:\n\
             原始数据: {:.1}MB\n\
             解码缓冲: {:.1}MB\n\
             DR计算: {:.1}MB\n\
             系统开销: {:.1}MB\n\
             \n\
             💾 内存需求:\n\
             最小需求: {:.1}MB\n\
             推荐配置: {:.1}MB\n\
             极限存活: {}\n\
             \n\
             💡 建议:\n\
             {}",
            format.channels,
            format.bits_per_sample,
            format.sample_rate,
            requirement.complexity,
            requirement.raw_audio_bytes as f64 / (1024.0 * 1024.0),
            requirement.decoding_memory as f64 / (1024.0 * 1024.0),
            requirement.dr_calculation_memory as f64 / (1024.0 * 1024.0),
            requirement.system_overhead as f64 / (1024.0 * 1024.0),
            requirement.total_minimum as f64 / (1024.0 * 1024.0),
            requirement.recommended_memory as f64 / (1024.0 * 1024.0),
            if requirement.survivable_in_emergency {
                "是"
            } else {
                "否"
            },
            Self::generate_recommendations(requirement, format)
        )
    }

    /// 生成优化建议
    fn generate_recommendations(requirement: &MemoryRequirement, format: &AudioFormat) -> String {
        let mut recommendations = Vec::new();

        match requirement.complexity {
            AudioComplexity::Simple => {
                recommendations.push("✅ 简单格式，所有模式均可正常处理");
            }
            AudioComplexity::Standard => {
                recommendations.push("✅ 标准格式，推荐128MB+内存");
            }
            AudioComplexity::Complex => {
                recommendations.push("⚠️ 复杂格式，需要256MB+内存");
                recommendations.push("💡 建议使用Standard或更高内存模式");
            }
            AudioComplexity::Extreme => {
                recommendations.push("🔥 极端格式，需要512MB+内存");
                recommendations.push("💡 强烈建议使用Abundant或Ultra内存模式");
                if !requirement.survivable_in_emergency {
                    recommendations.push("⛔ 在Emergency模式下无法处理，需要更多内存");
                }
            }
        }

        // 格式特定建议
        if format.channels > 32 {
            recommendations.push("🎵 超多声道音频(32+)，考虑分批处理或使用专业音频工作站");
        } else if format.channels > 16 {
            recommendations.push("🎵 超多声道音频(16+)，建议使用高内存配置");
        }

        match format.sample_rate {
            192001..=384000 => {
                recommendations.push("🎼 192-384kHz超高采样率，需要大量内存和处理能力");
                recommendations.push("💡 建议确保系统有充足的内存（4GB+）");
            }
            384001.. => {
                recommendations.push("🔥 384kHz+极端采样率，需要专业级硬件配置");
                recommendations.push("💡 强烈建议使用Ultra内存模式（8GB+）");
                recommendations.push("⚠️ 可能需要调整系统虚拟内存设置");
            }
            96001..=192000 => {
                recommendations.push("🎼 96-192kHz高采样率，建议预留额外内存缓冲");
            }
            _ => {}
        }

        // 极端组合警告
        if format.sample_rate >= 192000 && format.channels > 8 {
            recommendations.push("⚡ 高采样率+多声道组合，内存需求极大");
            recommendations.push("💡 考虑使用专业音频处理设备或云端处理");
        }

        recommendations.join("\n             ")
    }

    /// 计算动态最小内存需求
    pub fn calculate_dynamic_minimum(format: &AudioFormat) -> u64 {
        let requirement = Self::analyze_requirements(format);

        // 确保最小值能够处理该格式
        let format_minimum = requirement.total_minimum;
        let absolute_minimum = 32 * 1024 * 1024; // 32MB绝对底线

        std::cmp::max(format_minimum, absolute_minimum)
    }

    /// 批量分析多种格式
    pub fn batch_analysis() -> HashMap<String, MemoryRequirement> {
        let mut results = HashMap::new();

        // 定义测试格式
        let test_formats = vec![
            ("单声道16位44kHz", AudioFormat::new(44100, 1, 16, 0)),
            ("立体声24位96kHz", AudioFormat::new(96000, 2, 24, 0)),
            ("5.1环绕32位48kHz", AudioFormat::new(48000, 6, 32, 0)),
            ("7.1环绕32位96kHz", AudioFormat::new(96000, 8, 32, 0)),
            ("立体声32位192kHz", AudioFormat::new(192000, 2, 32, 0)),
            ("16声道32位192kHz", AudioFormat::new(192000, 16, 32, 0)),
            ("立体声32位384kHz", AudioFormat::new(384000, 2, 32, 0)), // 极端采样率
            ("20声道32位96kHz", AudioFormat::new(96000, 20, 32, 0)),  // 超多声道
            ("32声道32位192kHz", AudioFormat::new(192000, 32, 32, 0)), // 终极极端
            ("64声道32位384kHz", AudioFormat::new(384000, 64, 32, 0)), // 理论极限
        ];

        for (name, format) in test_formats {
            let requirement = Self::analyze_requirements(&format);
            results.insert(name.to_string(), requirement);
        }

        results
    }
}

/// 快速内存需求检查
pub fn quick_memory_check(format: &AudioFormat) -> (u64, bool) {
    let requirement = AudioMemoryAnalyzer::analyze_requirements(format);
    (
        requirement.total_minimum,
        requirement.survivable_in_emergency,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_audio_requirements() {
        let format = AudioFormat::new(44100, 2, 16, 0);
        let req = AudioMemoryAnalyzer::analyze_requirements(&format);

        assert_eq!(req.complexity, AudioComplexity::Simple);
        assert!(req.survivable_in_emergency);
        assert!(req.total_minimum < 64 * 1024 * 1024); // 应小于64MB
    }

    #[test]
    fn test_extreme_audio_requirements() {
        let format = AudioFormat::new(96000, 32, 32, 0);
        let req = AudioMemoryAnalyzer::analyze_requirements(&format);

        assert_eq!(req.complexity, AudioComplexity::Extreme);
        assert!(!req.survivable_in_emergency); // 极端格式不能在紧急模式下生存
        assert!(req.total_minimum > 512 * 1024 * 1024); // 应大于512MB
    }

    #[test]
    fn test_dynamic_minimum_calculation() {
        let simple_format = AudioFormat::new(44100, 2, 16, 0);
        let extreme_format = AudioFormat::new(96000, 20, 32, 0);

        let simple_min = AudioMemoryAnalyzer::calculate_dynamic_minimum(&simple_format);
        let extreme_min = AudioMemoryAnalyzer::calculate_dynamic_minimum(&extreme_format);

        assert!(extreme_min > simple_min);
        assert!(extreme_min > 200 * 1024 * 1024); // 20声道32位应需要200MB+
    }
}
