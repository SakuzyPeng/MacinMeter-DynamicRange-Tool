//! 峰值选择策略模块
//!
//! 提供多种峰值选择算法，支持主峰/次峰智能选择、削波检测等高级功能。
//! 独立的算法模块，可在DR计算和其他音频分析中复用。

/// 削波检测阈值（接近满幅度）
pub const CLIPPING_THRESHOLD: f64 = 0.99999;

/// 峰值选择策略枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeakSelectionStrategy {
    /// 标准模式：优先使用次峰(Pk_2nd)，仅在次峰无效时回退到主峰
    /// 对应 Measuring_DR_ENv3.md 标准
    PreferSecondary,

    /// 削波检测模式：优先使用主峰，仅在削波时使用次峰
    /// 对应 foobar2000 削波回退机制
    ClippingAware,

    /// 保守模式：总是使用主峰
    AlwaysPrimary,

    /// 次峰优先模式：总是使用次峰（如果可用）
    AlwaysSecondary,
}

impl Default for PeakSelectionStrategy {
    fn default() -> Self {
        Self::PreferSecondary
    }
}

/// 峰值选择器trait，定义峰值选择行为
pub trait PeakSelector {
    /// 从主峰和次峰中选择用于DR计算的峰值
    ///
    /// # 参数
    /// * `primary_peak` - 主峰值（最大绝对值）
    /// * `secondary_peak` - 次峰值（第二大绝对值）
    ///
    /// # 返回值
    /// 返回选择的峰值
    fn select_peak(&self, primary_peak: f64, secondary_peak: f64) -> f64;

    /// 获取策略描述（用于日志输出）
    fn strategy_name(&self) -> &'static str;

    /// 检查给定峰值是否被削波
    fn is_clipped(&self, peak: f64) -> bool {
        peak >= CLIPPING_THRESHOLD
    }
}

/// 峰值选择策略实现
impl PeakSelector for PeakSelectionStrategy {
    fn select_peak(&self, primary_peak: f64, secondary_peak: f64) -> f64 {
        match self {
            Self::PreferSecondary => {
                // 优先使用次峰，仅在次峰无效时回退到主峰
                if secondary_peak > 0.0 {
                    secondary_peak
                } else {
                    primary_peak
                }
            }

            Self::ClippingAware => {
                // 削波检测：主峰接近满幅度时使用次峰
                if self.is_clipped(primary_peak) && secondary_peak > 0.0 {
                    secondary_peak
                } else {
                    primary_peak
                }
            }

            Self::AlwaysPrimary => primary_peak,

            Self::AlwaysSecondary => {
                if secondary_peak > 0.0 {
                    secondary_peak
                } else {
                    primary_peak // 回退到主峰
                }
            }
        }
    }

    fn strategy_name(&self) -> &'static str {
        match self {
            Self::PreferSecondary => "PreferSecondary",
            Self::ClippingAware => "ClippingAware",
            Self::AlwaysPrimary => "AlwaysPrimary",
            Self::AlwaysSecondary => "AlwaysSecondary",
        }
    }
}

/// 峰值选择工具函数集合
pub mod utils {
    use super::*;

    /// 检查峰值是否被削波
    pub fn is_peak_clipped(peak: f64) -> bool {
        peak >= CLIPPING_THRESHOLD
    }

    /// 计算峰值比率（次峰/主峰）
    pub fn peak_ratio(primary_peak: f64, secondary_peak: f64) -> f64 {
        if primary_peak > 0.0 {
            secondary_peak / primary_peak
        } else {
            0.0
        }
    }

    /// 根据音频特征自动选择最优策略
    pub fn suggest_strategy(primary_peak: f64, secondary_peak: f64) -> PeakSelectionStrategy {
        if is_peak_clipped(primary_peak) {
            PeakSelectionStrategy::ClippingAware
        } else if peak_ratio(primary_peak, secondary_peak) > 0.8 {
            // 主次峰接近时使用次峰避免瞬态干扰
            PeakSelectionStrategy::PreferSecondary
        } else {
            PeakSelectionStrategy::AlwaysPrimary
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefer_secondary_strategy() {
        let strategy = PeakSelectionStrategy::PreferSecondary;

        // 次峰可用时使用次峰
        assert_eq!(strategy.select_peak(1.0, 0.8), 0.8);

        // 次峰无效时回退到主峰
        assert_eq!(strategy.select_peak(1.0, 0.0), 1.0);
    }

    #[test]
    fn test_clipping_aware_strategy() {
        let strategy = PeakSelectionStrategy::ClippingAware;

        // 未削波时使用主峰
        assert_eq!(strategy.select_peak(0.5, 0.3), 0.5);

        // 削波时使用次峰
        assert_eq!(strategy.select_peak(0.99999, 0.7), 0.7);

        // 削波但次峰无效时仍使用主峰
        assert_eq!(strategy.select_peak(1.0, 0.0), 1.0);
    }

    #[test]
    fn test_always_primary_strategy() {
        let strategy = PeakSelectionStrategy::AlwaysPrimary;
        assert_eq!(strategy.select_peak(1.0, 0.8), 1.0);
        assert_eq!(strategy.select_peak(0.5, 0.9), 0.5);
    }

    #[test]
    fn test_always_secondary_strategy() {
        let strategy = PeakSelectionStrategy::AlwaysSecondary;

        // 次峰可用时使用次峰
        assert_eq!(strategy.select_peak(1.0, 0.8), 0.8);

        // 次峰无效时回退到主峰
        assert_eq!(strategy.select_peak(1.0, 0.0), 1.0);
    }

    #[test]
    fn test_clipping_detection() {
        let strategy = PeakSelectionStrategy::ClippingAware;

        assert!(strategy.is_clipped(0.99999));
        assert!(strategy.is_clipped(1.0));
        assert!(!strategy.is_clipped(0.9));
    }

    #[test]
    fn test_strategy_names() {
        assert_eq!(
            PeakSelectionStrategy::PreferSecondary.strategy_name(),
            "PreferSecondary"
        );
        assert_eq!(
            PeakSelectionStrategy::ClippingAware.strategy_name(),
            "ClippingAware"
        );
        assert_eq!(
            PeakSelectionStrategy::AlwaysPrimary.strategy_name(),
            "AlwaysPrimary"
        );
        assert_eq!(
            PeakSelectionStrategy::AlwaysSecondary.strategy_name(),
            "AlwaysSecondary"
        );
    }

    #[test]
    fn test_utils_peak_ratio() {
        assert_eq!(utils::peak_ratio(1.0, 0.8), 0.8);
        assert_eq!(utils::peak_ratio(0.0, 0.5), 0.0);
    }

    #[test]
    fn test_utils_suggest_strategy() {
        // 削波情况
        assert_eq!(
            utils::suggest_strategy(1.0, 0.7),
            PeakSelectionStrategy::ClippingAware
        );

        // 主次峰接近（避免触发削波检测）
        assert_eq!(
            utils::suggest_strategy(0.9, 0.81),
            PeakSelectionStrategy::PreferSecondary
        );

        // 正常情况
        assert_eq!(
            utils::suggest_strategy(0.8, 0.4),
            PeakSelectionStrategy::AlwaysPrimary
        );
    }

    #[test]
    fn test_default_strategy() {
        assert_eq!(
            PeakSelectionStrategy::default(),
            PeakSelectionStrategy::PreferSecondary
        );
    }
}
