//! 动态内存管理系统
//!
//! 提供实时内存监控、自适应配置和极端工况处理的智能内存管理。
//! 考虑不同平台的差异和动态变化的内存环境。

use crate::audio::universal_decoder::AudioFormat;
use crate::error::{AudioError, AudioResult};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

/// 动态内存配置
#[derive(Debug, Clone)]
pub struct DynamicMemoryConfig {
    /// 最小可用内存（紧急情况下的底线）
    pub min_memory_bytes: u64,

    /// 最大可用内存（理想情况下的上限）
    pub max_memory_bytes: u64,

    /// 当前推荐内存
    pub current_memory_bytes: u64,

    /// 内存压力等级 (0.0-1.0, 0为充足，1为严重不足)
    pub memory_pressure: f64,

    /// 是否处于内存紧急状态
    pub emergency_mode: bool,

    /// 上次检查时间
    pub last_check: Instant,
}

/// 内存等级分类
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryTier {
    /// 紧急模式：<512MB可用，极度保守
    Emergency,
    /// 受限模式：512MB-2GB可用，较保守
    Limited,
    /// 标准模式：2GB-8GB可用，平衡性能
    Standard,
    /// 充足模式：8GB-32GB可用，较激进
    Abundant,
    /// 超级充足：>32GB可用，最大性能
    Ultra,
}

/// 平台特定的内存特性
#[derive(Debug, Clone)]
pub struct PlatformMemoryProfile {
    /// 平台名称
    pub platform: String,

    /// 系统保留内存估算（字节）
    pub system_reserved: u64,

    /// 安全内存使用比例 (0.0-1.0)
    pub safe_usage_ratio: f64,

    /// 内存片段化系数
    pub fragmentation_factor: f64,
}

/// 智能动态内存管理器
pub struct DynamicMemoryManager {
    /// 系统信息监控
    system: Arc<Mutex<System>>,

    /// 平台配置
    platform_profile: PlatformMemoryProfile,

    /// 当前配置
    current_config: Arc<Mutex<DynamicMemoryConfig>>,

    /// 历史内存使用记录（用于趋势分析）
    memory_history: Arc<Mutex<Vec<(Instant, u64)>>>,
}

impl DynamicMemoryManager {
    /// 创建动态内存管理器
    pub fn new() -> Self {
        let mut system = System::new_with_specifics(
            RefreshKind::new().with_memory(MemoryRefreshKind::everything()),
        );
        system.refresh_memory();

        let platform_profile = Self::detect_platform_profile();
        let initial_config = Self::calculate_initial_config(&system, &platform_profile);

        Self {
            system: Arc::new(Mutex::new(system)),
            platform_profile,
            current_config: Arc::new(Mutex::new(initial_config)),
            memory_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 检测平台特性
    fn detect_platform_profile() -> PlatformMemoryProfile {
        #[cfg(target_os = "macos")]
        {
            PlatformMemoryProfile {
                platform: "macOS".to_string(),
                system_reserved: 2 * 1024 * 1024 * 1024, // macOS通常保留2GB
                safe_usage_ratio: 0.75,                  // 75%安全使用率
                fragmentation_factor: 1.2,               // 20%碎片化开销
            }
        }

        #[cfg(target_os = "linux")]
        {
            PlatformMemoryProfile {
                platform: "Linux".to_string(),
                system_reserved: 1024 * 1024 * 1024, // Linux较高效，保留1GB
                safe_usage_ratio: 0.80,              // 80%安全使用率
                fragmentation_factor: 1.15,          // 15%碎片化开销
            }
        }

        #[cfg(target_os = "windows")]
        {
            PlatformMemoryProfile {
                platform: "Windows".to_string(),
                system_reserved: 3 * 1024 * 1024 * 1024, // Windows保留更多，3GB
                safe_usage_ratio: 0.70,                  // 70%安全使用率
                fragmentation_factor: 1.3,               // 30%碎片化开销
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            PlatformMemoryProfile {
                platform: "Unknown".to_string(),
                system_reserved: 2 * 1024 * 1024 * 1024, // 保守估算
                safe_usage_ratio: 0.60,                  // 60%安全使用率
                fragmentation_factor: 1.4,               // 40%碎片化开销
            }
        }
    }

    /// 计算初始内存配置
    fn calculate_initial_config(
        system: &System,
        profile: &PlatformMemoryProfile,
    ) -> DynamicMemoryConfig {
        let total_memory = system.total_memory() * 1024; // sysinfo返回KB，转换为字节
        let available_memory = system.available_memory() * 1024;

        // 极端工况配置
        let absolute_min = 64 * 1024 * 1024; // 绝对最小64MB
        let emergency_threshold = 512 * 1024 * 1024; // 紧急阈值512MB

        // 考虑平台特性的可用内存
        let platform_available = if available_memory > profile.system_reserved {
            ((available_memory - profile.system_reserved) as f64 * profile.safe_usage_ratio
                / profile.fragmentation_factor) as u64
        } else {
            available_memory / 4 // 紧急情况下只用25%
        };

        // 分级内存配置
        let (min_memory, max_memory, current_memory) =
            match Self::classify_memory_tier(platform_available) {
                MemoryTier::Emergency => (
                    absolute_min,
                    emergency_threshold,
                    std::cmp::max(absolute_min, platform_available / 8),
                ),
                MemoryTier::Limited => {
                    (absolute_min, 2 * 1024 * 1024 * 1024, platform_available / 4)
                }
                MemoryTier::Standard => (
                    128 * 1024 * 1024,
                    8 * 1024 * 1024 * 1024,
                    platform_available / 3,
                ),
                MemoryTier::Abundant => (
                    256 * 1024 * 1024,
                    32 * 1024 * 1024 * 1024,
                    platform_available / 2,
                ),
                MemoryTier::Ultra => (
                    512 * 1024 * 1024,
                    64 * 1024 * 1024 * 1024,
                    platform_available * 2 / 3,
                ),
            };

        // 计算内存压力
        let memory_pressure = if available_memory > 0 {
            1.0 - (available_memory as f64 / total_memory as f64)
        } else {
            1.0
        };

        DynamicMemoryConfig {
            min_memory_bytes: min_memory,
            max_memory_bytes: max_memory,
            current_memory_bytes: current_memory,
            memory_pressure,
            emergency_mode: platform_available < emergency_threshold,
            last_check: Instant::now(),
        }
    }

    /// 分类内存等级
    fn classify_memory_tier(available_bytes: u64) -> MemoryTier {
        if available_bytes < 512 * 1024 * 1024 {
            MemoryTier::Emergency
        } else if available_bytes < 2 * 1024 * 1024 * 1024 {
            MemoryTier::Limited
        } else if available_bytes < 8 * 1024 * 1024 * 1024 {
            MemoryTier::Standard
        } else if available_bytes < 32 * 1024 * 1024 * 1024 {
            MemoryTier::Abundant
        } else {
            MemoryTier::Ultra
        }
    }

    /// 刷新内存状态（实时监控）
    pub fn refresh_memory_status(&self) -> AudioResult<DynamicMemoryConfig> {
        let mut system = self
            .system
            .lock()
            .map_err(|_| AudioError::CalculationError("内存监控锁定失败".to_string()))?;

        system.refresh_memory();

        let available_memory = system.available_memory() * 1024;
        let _total_memory = system.total_memory() * 1024;

        // 记录历史数据
        {
            let mut history = self
                .memory_history
                .lock()
                .map_err(|_| AudioError::CalculationError("内存历史记录锁定失败".to_string()))?;
            history.push((Instant::now(), available_memory));

            // 只保留最近10分钟的记录
            let cutoff = Instant::now() - Duration::from_secs(600);
            history.retain(|(time, _)| *time > cutoff);
        }

        // 重新计算配置
        let updated_config = Self::calculate_initial_config(&system, &self.platform_profile);

        // 更新当前配置
        {
            let mut current = self
                .current_config
                .lock()
                .map_err(|_| AudioError::CalculationError("内存配置锁定失败".to_string()))?;
            *current = updated_config.clone();
        }

        Ok(updated_config)
    }

    /// 根据音频格式获取自适应内存配置
    pub fn get_adaptive_config(&self, format: &AudioFormat) -> AudioResult<u64> {
        let config = self.refresh_memory_status()?;

        // 基于音频格式调整内存需求
        let format_factor = match format.bits_per_sample {
            16 => 1.0,
            24 => 1.5,
            32 => 2.0,
            _ => 1.2,
        };

        let channel_factor = match format.channels {
            1 => 1.0,
            2 => 1.2,
            6 => 2.0,
            8 => 2.5,
            _ => 1.5,
        };

        let sample_rate_factor = if format.sample_rate >= 96000 {
            2.0
        } else if format.sample_rate >= 48000 {
            1.5
        } else {
            1.0
        };

        // 综合调整系数
        let total_factor = format_factor * channel_factor * sample_rate_factor;

        // 基础内存配置
        let base_memory = if config.emergency_mode {
            config.min_memory_bytes
        } else {
            let target = (config.current_memory_bytes as f64 * total_factor) as u64;
            std::cmp::min(target, config.max_memory_bytes)
        };

        // 确保不低于绝对最小值
        Ok(std::cmp::max(base_memory, 32 * 1024 * 1024)) // 最少32MB
    }

    /// 获取内存状态报告
    pub fn get_memory_report(&self) -> AudioResult<String> {
        let config = self
            .current_config
            .lock()
            .map_err(|_| AudioError::CalculationError("配置锁定失败".to_string()))?;

        let tier = Self::classify_memory_tier(config.current_memory_bytes);

        Ok(format!(
            "🧠 动态内存管理报告:\n\
             平台: {}\n\
             内存等级: {:?}\n\
             当前可用: {:.1}MB\n\
             配置范围: {:.1}MB - {:.1}MB\n\
             内存压力: {:.1}%\n\
             紧急模式: {}",
            self.platform_profile.platform,
            tier,
            config.current_memory_bytes as f64 / (1024.0 * 1024.0),
            config.min_memory_bytes as f64 / (1024.0 * 1024.0),
            config.max_memory_bytes as f64 / (1024.0 * 1024.0),
            config.memory_pressure * 100.0,
            if config.emergency_mode { "是" } else { "否" }
        ))
    }

    /// 检查是否需要降级处理
    pub fn should_use_degraded_mode(&self) -> AudioResult<bool> {
        let config = self
            .current_config
            .lock()
            .map_err(|_| AudioError::CalculationError("配置检查失败".to_string()))?;

        Ok(config.emergency_mode || config.memory_pressure > 0.85)
    }

    /// 环境变量覆盖支持
    pub fn apply_env_overrides(&mut self) -> AudioResult<()> {
        // 支持强制设置内存限制
        if let Ok(max_memory_str) = std::env::var("MACINMETER_MAX_MEMORY_MB")
            && let Ok(max_memory_mb) = max_memory_str.parse::<u64>()
        {
            let mut config = self
                .current_config
                .lock()
                .map_err(|_| AudioError::CalculationError("环境变量配置失败".to_string()))?;
            config.max_memory_bytes = max_memory_mb * 1024 * 1024;
            config.current_memory_bytes =
                std::cmp::min(config.current_memory_bytes, config.max_memory_bytes);
        }

        // 支持强制紧急模式
        if std::env::var("MACINMETER_EMERGENCY_MODE").is_ok() {
            let mut config = self
                .current_config
                .lock()
                .map_err(|_| AudioError::CalculationError("紧急模式配置失败".to_string()))?;
            config.emergency_mode = true;
            config.current_memory_bytes = config.min_memory_bytes;
        }

        Ok(())
    }
}

impl Default for DynamicMemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static::lazy_static! {
    static ref GLOBAL_MEMORY_MANAGER: Arc<Mutex<DynamicMemoryManager>> = {
        let mut manager = DynamicMemoryManager::new();
        let _ = manager.apply_env_overrides(); // 应用环境变量配置
        Arc::new(Mutex::new(manager))
    };
}

/// 获取全局动态内存配置
pub fn get_adaptive_memory_for_format(format: &AudioFormat) -> AudioResult<u64> {
    let manager = GLOBAL_MEMORY_MANAGER
        .lock()
        .map_err(|_| AudioError::CalculationError("全局内存管理器访问失败".to_string()))?;
    manager.get_adaptive_config(format)
}

/// 获取内存状态报告
pub fn get_memory_status_report() -> AudioResult<String> {
    let manager = GLOBAL_MEMORY_MANAGER
        .lock()
        .map_err(|_| AudioError::CalculationError("全局内存管理器访问失败".to_string()))?;
    manager.get_memory_report()
}

/// 检查是否应该使用降级模式
pub fn should_use_emergency_mode() -> AudioResult<bool> {
    let manager = GLOBAL_MEMORY_MANAGER
        .lock()
        .map_err(|_| AudioError::CalculationError("全局内存管理器访问失败".to_string()))?;
    manager.should_use_degraded_mode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_tier_classification() {
        assert_eq!(
            DynamicMemoryManager::classify_memory_tier(100 * 1024 * 1024),
            MemoryTier::Emergency
        );
        assert_eq!(
            DynamicMemoryManager::classify_memory_tier(1024 * 1024 * 1024),
            MemoryTier::Limited
        );
        assert_eq!(
            DynamicMemoryManager::classify_memory_tier(4 * 1024 * 1024 * 1024),
            MemoryTier::Standard
        );
        assert_eq!(
            DynamicMemoryManager::classify_memory_tier(16 * 1024 * 1024 * 1024),
            MemoryTier::Abundant
        );
        assert_eq!(
            DynamicMemoryManager::classify_memory_tier(64 * 1024 * 1024 * 1024),
            MemoryTier::Ultra
        );
    }

    #[test]
    fn test_dynamic_memory_manager_creation() {
        let manager = DynamicMemoryManager::new();
        assert!(!manager.platform_profile.platform.is_empty());
        assert!(manager.platform_profile.safe_usage_ratio > 0.0);
        assert!(manager.platform_profile.fragmentation_factor >= 1.0);
    }
}
