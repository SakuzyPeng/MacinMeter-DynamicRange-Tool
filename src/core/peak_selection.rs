//! 峰值选择策略模块
//!
//! 提供多种峰值选择算法，支持主峰/次峰智能选择、削波检测等高级功能。
//! 独立的算法模块，可在DR计算和其他音频分析中复用。
//!
//! ## 职责边界
//!
//! 本模块仅负责：**给定 primary_peak 和 secondary_peak，选择哪个用于 DR 计算**。
//!
//! ### 本模块的职责：
//! - ✅ 定义不同的峰值选择策略（PreferSecondary, ClippingAware, 等）
//! - ✅ 实现策略的选择逻辑和削波检测
//! - ✅ 提供可观测的策略名称和工具函数
//!
//! ### 本模块不负责的事项：
//! - ❌ 峰值数据的存储和更新（由 `dr_channel_state::ChannelData` 负责）
//! - ❌ 峰值是否有效的判断（由调用方决定，通常通过检查 > 0.0）
//! - ❌ 峰值计算的算法细节（由 `WindowRmsAnalyzer` 负责）
//!
//! ### 调用方的职责：
//! - 调用方应通过 `PeakSelectionStrategy::select_peak()` 获取最终使用的峰值
//! - 不应直接使用 `ChannelData::get_effective_peak()`（该方法仅返回备选峰值）
//!
//! ## foobar2000 对齐
//!
//! - **默认策略**：`PreferSecondary`（优先使用次峰，符合标准）
//! - **削波检测**：与 foobar2000 削波回退机制一致
//! - **常量来源**：所有常量集中在 `tools::constants::dr_analysis` 中管理

use crate::tools::constants::dr_analysis;

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
    ///
    /// ℹ️ **注意**：此方法与 `utils::is_peak_clipped()` 逻辑完全相同。
    /// - Trait 版本：供 `PeakSelectionStrategy` 内部使用
    /// - Utils 版本：供外部工具函数使用（如 `suggest_strategy()`）
    ///
    /// 两个实现保持同步，都使用 `dr_analysis::CLIPPING_THRESHOLD`。
    #[inline]
    fn is_clipped(&self, peak: f64) -> bool {
        peak >= dr_analysis::CLIPPING_THRESHOLD
    }
}

/// 峰值选择策略实现
impl PeakSelector for PeakSelectionStrategy {
    #[inline]
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
    #[inline]
    pub fn is_peak_clipped(peak: f64) -> bool {
        peak >= dr_analysis::CLIPPING_THRESHOLD
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

    /// 削波阈值临界边界测试
    ///
    /// 验证在削波阈值（≈0.99999）附近的决策稳定性
    #[test]
    fn test_clipping_threshold_boundary() {
        let strategy = PeakSelectionStrategy::ClippingAware;

        // 案例1: 刚好低于削波阈值（不削波）
        assert!(!strategy.is_clipped(0.99998));
        assert_eq!(strategy.select_peak(0.99998, 0.7), 0.99998); // 使用主峰

        // 案例2: 精确削波阈值（削波）
        assert!(strategy.is_clipped(0.99999));
        assert_eq!(strategy.select_peak(0.99999, 0.7), 0.7); // 切换到次峰

        // 案例3: 全幅度削波（削波）
        assert!(strategy.is_clipped(1.0));
        assert_eq!(strategy.select_peak(1.0, 0.7), 0.7); // 切换到次峰

        // 案例4: 削波但次峰无效（回退到主峰）
        assert!(strategy.is_clipped(1.0));
        assert_eq!(strategy.select_peak(1.0, 0.0), 1.0);
    }

    /// 峰值比率临界边界测试
    ///
    /// 验证在 0.8 分界点（secondary/primary > 0.8）附近的策略稳定性
    /// 这影响 suggest_strategy 的决策（是否选择 AlwaysPrimary vs PreferSecondary）
    ///
    /// 注意：suggest_strategy 会优先检查削波，所以必须使用不被削波的主峰值
    #[test]
    fn test_peak_ratio_boundary() {
        // suggest_strategy 的逻辑：
        // 1. if 削波 → ClippingAware
        // 2. else if peak_ratio > 0.8 → PreferSecondary
        // 3. else → AlwaysPrimary

        // 案例1: 比率刚好低于 0.8（不满足 > 0.8）→ AlwaysPrimary
        let strategy_low = utils::suggest_strategy(0.9, 0.71); // 0.71/0.9 ≈ 0.789
        assert_eq!(
            strategy_low,
            PeakSelectionStrategy::AlwaysPrimary,
            "比率 ~0.789（不 > 0.8）应该返回 AlwaysPrimary"
        );

        // 案例2: 比率刚好高于 0.8（满足 > 0.8）→ PreferSecondary
        // 0.721/0.9 = 0.80111... > 0.8
        let strategy_high = utils::suggest_strategy(0.9, 0.721);
        assert_eq!(
            strategy_high,
            PeakSelectionStrategy::PreferSecondary,
            "比率 ~0.801（> 0.8）应该返回 PreferSecondary"
        );

        // 案例3: 明显高于 0.8（满足 > 0.8）→ PreferSecondary
        let strategy_clear = utils::suggest_strategy(0.9, 0.8); // 0.8/0.9 ≈ 0.889
        assert_eq!(
            strategy_clear,
            PeakSelectionStrategy::PreferSecondary,
            "比率 ~0.889（> 0.8）应该返回 PreferSecondary"
        );

        // 案例4: 削波优先（primary ≥ CLIPPING_THRESHOLD）
        // 削波检查优先于比率检查
        let strategy_clipped = utils::suggest_strategy(0.99999, 0.7);
        assert_eq!(
            strategy_clipped,
            PeakSelectionStrategy::ClippingAware,
            "削波情况下应该优先返回 ClippingAware（不论比率）"
        );
    }
}
