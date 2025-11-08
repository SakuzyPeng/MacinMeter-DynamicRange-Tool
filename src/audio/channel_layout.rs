//! 标准声道布局定义和LFE检测
//!
//! 基于Apple CoreAudio AudioChannelLayoutTag规范，提供精确的声道布局识别。
//! 参考文档：docs/channel_doc.md

/// 标准声道布局信息
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelLayoutInfo {
    /// 布局名称（如"5.1", "7.1", "5.1(side)"等）
    pub layout_name: &'static str,
    /// 声道总数
    pub channel_count: u16,
    /// 声道顺序（使用标准缩写：L/R/C/LFE/Ls/Rs等）
    pub channel_order: &'static [&'static str],
    /// LFE声道索引（0-based），使用静态切片
    pub lfe_indices: &'static [usize],
}

impl ChannelLayoutInfo {
    /// 获取LFE索引的Vec副本（用于函数返回）
    pub fn lfe_indices_vec(&self) -> Vec<usize> {
        self.lfe_indices.to_vec()
    }
}

/// 标准布局定义（优先级从高到低）
///
/// 声道缩写说明：
/// - L/R: 左/右主声道
/// - C: 中置声道
/// - LFE: 低频效果声道
/// - Ls/Rs: 左/右环绕（侧面）
/// - Cs: 中央环绕
/// - Rls/Rrs: 后左/后右环绕
/// - Lc/Rc: 左中/右中
/// - Lw/Rw: 左宽/右宽
/// - Vhl/Vhc/Vhr: 垂直高度 左/中/右
/// - Ltm/Rtm: 顶部中间 左/右
/// - Ltr/Rtr: 顶部后方 左/右
#[allow(dead_code)]
mod standard_layouts {
    use super::ChannelLayoutInfo;

    // ========== MPEG标准（最常见，优先级最高） ==========

    /// MPEG 5.1 A - 最常见的5.1布局（ITU标准）
    pub const MPEG_5_1_A: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "5.1",
        channel_count: 6,
        channel_order: &["L", "R", "C", "LFE", "Ls", "Rs"],
        lfe_indices: &[3],
    };

    /// MPEG 5.1 B - LFE在末尾
    pub const MPEG_5_1_B: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "5.1_B",
        channel_count: 6,
        channel_order: &["L", "R", "Ls", "Rs", "C", "LFE"],
        lfe_indices: &[5],
    };

    /// MPEG 5.1 C
    pub const MPEG_5_1_C: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "5.1_C",
        channel_count: 6,
        channel_order: &["L", "C", "R", "Ls", "Rs", "LFE"],
        lfe_indices: &[5],
    };

    /// MPEG 5.1 D - AAC标准
    pub const MPEG_5_1_D: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "5.1_D",
        channel_count: 6,
        channel_order: &["C", "L", "R", "Ls", "Rs", "LFE"],
        lfe_indices: &[5],
    };

    /// MPEG 6.1 A
    pub const MPEG_6_1_A: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "6.1",
        channel_count: 7,
        channel_order: &["L", "R", "C", "LFE", "Ls", "Rs", "Cs"],
        lfe_indices: &[3],
    };

    /// MPEG 7.1 A - 前置扩展（SDDS）
    pub const MPEG_7_1_A: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "7.1_A",
        channel_count: 8,
        channel_order: &["L", "R", "C", "LFE", "Ls", "Rs", "Lc", "Rc"],
        lfe_indices: &[3],
    };

    /// MPEG 7.1 B - AAC标准
    pub const MPEG_7_1_B: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "7.1_B",
        channel_count: 8,
        channel_order: &["C", "Lc", "Rc", "L", "R", "Ls", "Rs", "LFE"],
        lfe_indices: &[7],
    };

    /// MPEG 7.1 C - 最常见的7.1布局（ITU标准）
    pub const MPEG_7_1_C: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "7.1",
        channel_count: 8,
        channel_order: &["L", "R", "C", "LFE", "Ls", "Rs", "Rls", "Rrs"],
        lfe_indices: &[3],
    };

    // ========== EAC3/AC3标准 ==========

    /// EAC3 7.1 A - 标准后环绕
    pub const EAC3_7_1_A: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "EAC3_7.1_A",
        channel_count: 8,
        channel_order: &["L", "C", "R", "Ls", "Rs", "LFE", "Rls", "Rrs"],
        lfe_indices: &[5],
    };

    /// EAC3 7.1 B - 前置扩展
    pub const EAC3_7_1_B: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "EAC3_7.1_B",
        channel_count: 8,
        channel_order: &["L", "C", "R", "Ls", "Rs", "LFE", "Lc", "Rc"],
        lfe_indices: &[5],
    };

    /// EAC3 7.1 E - 高度声道
    pub const EAC3_7_1_E: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "EAC3_7.1_E",
        channel_count: 8,
        channel_order: &["L", "C", "R", "Ls", "Rs", "LFE", "Vhl", "Vhr"],
        lfe_indices: &[5],
    };

    // ========== Dolby Atmos标准 ==========

    /// Atmos 5.1.2 - 5.1 + 2高度
    pub const ATMOS_5_1_2: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "Atmos_5.1.2",
        channel_count: 8,
        channel_order: &["L", "R", "C", "LFE", "Ls", "Rs", "Ltm", "Rtm"],
        lfe_indices: &[3],
    };

    /// Atmos 5.1.4 - 5.1 + 4高度
    pub const ATMOS_5_1_4: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "Atmos_5.1.4",
        channel_count: 10,
        channel_order: &["L", "R", "C", "LFE", "Ls", "Rs", "Vhl", "Vhr", "Ltr", "Rtr"],
        lfe_indices: &[3],
    };

    /// Atmos 7.1.2 - 7.1 + 2高度
    pub const ATMOS_7_1_2: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "Atmos_7.1.2",
        channel_count: 10,
        channel_order: &["L", "R", "C", "LFE", "Ls", "Rs", "Rls", "Rrs", "Ltm", "Rtm"],
        lfe_indices: &[3],
    };

    /// Atmos 7.1.4 - 7.1 + 4高度（最常见Atmos格式）
    pub const ATMOS_7_1_4: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "Atmos_7.1.4",
        channel_count: 12,
        channel_order: &[
            "L", "R", "C", "LFE", "Ls", "Rs", "Rls", "Rrs", "Vhl", "Vhr", "Ltr", "Rtr",
        ],
        lfe_indices: &[3],
    };

    /// Atmos 9.1.6 - DTS:X Pro级别
    pub const ATMOS_9_1_6: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "Atmos_9.1.6",
        channel_count: 16,
        channel_order: &[
            "L", "R", "C", "LFE", "Ls", "Rs", "Rls", "Rrs", "Lw", "Rw", "Vhl", "Vhr", "Ltm", "Rtm",
            "Ltr", "Rtr",
        ],
        lfe_indices: &[3],
    };

    // ========== DTS标准 ==========

    /// DTS 7.1
    pub const DTS_7_1: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "DTS_7.1",
        channel_count: 8,
        channel_order: &["Lc", "C", "Rc", "L", "R", "Ls", "Rs", "LFE"],
        lfe_indices: &[7],
    };

    // ========== 其他常见格式 ==========

    /// 2.1 - 立体声 + LFE
    pub const STEREO_2_1: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "2.1",
        channel_count: 3,
        channel_order: &["L", "R", "LFE"],
        lfe_indices: &[2],
    };

    /// 3.1 - 3.0 + LFE
    pub const SURROUND_3_1: ChannelLayoutInfo = ChannelLayoutInfo {
        layout_name: "3.1",
        channel_count: 4,
        channel_order: &["L", "R", "C", "LFE"],
        lfe_indices: &[3],
    };
}

/// 从FFmpeg的channel_layout字符串精确检测LFE位置
///
/// 优先级策略：
/// 1. 精确匹配标准布局名称
/// 2. 模糊匹配常见格式（如"5.1(side)" → "5.1"）
/// 3. 回退到基于声道数的通用推断
pub fn detect_lfe_from_layout(layout_str: &str, channels: u16) -> Option<Vec<usize>> {
    // 精确匹配
    if let Some(indices) = exact_match_layout(layout_str, channels) {
        return Some(indices);
    }

    // 模糊匹配
    if let Some(indices) = fuzzy_match_layout(layout_str, channels) {
        return Some(indices);
    }

    // 回退到通用推断
    Some(fallback_lfe_indices(channels))
}

/// 精确匹配标准布局
fn exact_match_layout(layout_str: &str, channels: u16) -> Option<Vec<usize>> {
    use standard_layouts::*;

    // 标准化输入（去除空格、转小写）
    let normalized = layout_str.trim().to_lowercase();

    match (normalized.as_str(), channels) {
        // MPEG 5.1标准（最常见）
        ("5.1" | "5.1(side)" | "itu_3_2_1" | "mpeg_5_1_a", 6) => Some(MPEG_5_1_A.lfe_indices_vec()),

        // MPEG 7.1标准（最常见）
        ("7.1" | "7.1(wide)" | "itu_3_4_1" | "mpeg_7_1_c", 8) => Some(MPEG_7_1_C.lfe_indices_vec()),

        // MPEG 6.1
        ("6.1" | "mpeg_6_1_a", 7) => Some(MPEG_6_1_A.lfe_indices_vec()),

        // Atmos格式
        ("atmos_5.1.2" | "5.1.2", 8) => Some(ATMOS_5_1_2.lfe_indices_vec()),
        ("atmos_5.1.4" | "5.1.4", 10) => Some(ATMOS_5_1_4.lfe_indices_vec()),
        ("atmos_7.1.2" | "7.1.2", 10) => Some(ATMOS_7_1_2.lfe_indices_vec()),
        ("atmos_7.1.4" | "7.1.4", 12) => Some(ATMOS_7_1_4.lfe_indices_vec()),
        ("atmos_9.1.6" | "9.1.6", 16) => Some(ATMOS_9_1_6.lfe_indices_vec()),

        // 其他常见格式
        ("2.1", 3) => Some(STEREO_2_1.lfe_indices_vec()),
        ("3.1", 4) => Some(SURROUND_3_1.lfe_indices_vec()),

        _ => None,
    }
}

/// 模糊匹配（处理带后缀的布局名称）
fn fuzzy_match_layout(layout_str: &str, channels: u16) -> Option<Vec<usize>> {
    let normalized = layout_str.trim().to_lowercase();

    // 移除常见后缀再匹配
    let base = normalized
        .replace("(side)", "")
        .replace("(wide)", "")
        .replace("(back)", "")
        .trim()
        .to_string();

    if base != normalized {
        exact_match_layout(&base, channels)
    } else {
        None
    }
}

/// 回退方案：基于声道数的通用推断
///
/// 这是保守的回退策略，遵循最常见的布局规则：
/// - 大多数标准布局将LFE放在index 3（FL, FR, FC, LFE, ...）
/// - 2.1除外（L R LFE）
pub fn fallback_lfe_indices(channel_count: u16) -> Vec<usize> {
    match channel_count {
        0..=2 => Vec::new(), // 单声道/立体声无LFE
        3 => vec![2],        // 2.1: L R LFE
        4..=32 => vec![3],   // 大多数标准布局：FL FR FC LFE ...
        _ => Vec::new(),     // 未知格式
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match_5_1() {
        assert_eq!(detect_lfe_from_layout("5.1", 6), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("5.1(side)", 6), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("MPEG_5_1_A", 6), Some(vec![3]));
    }

    #[test]
    fn test_exact_match_7_1() {
        assert_eq!(detect_lfe_from_layout("7.1", 8), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("7.1(wide)", 8), Some(vec![3]));
    }

    #[test]
    fn test_atmos_formats() {
        assert_eq!(detect_lfe_from_layout("5.1.2", 8), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("7.1.4", 12), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("9.1.6", 16), Some(vec![3]));
    }

    #[test]
    fn test_fuzzy_match() {
        assert_eq!(detect_lfe_from_layout("5.1 (side)", 6), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("7.1 (wide)", 8), Some(vec![3]));
    }

    #[test]
    fn test_fallback() {
        // 未知6声道格式回退到index 3
        assert_eq!(detect_lfe_from_layout("unknown", 6), Some(vec![3]));
        // 未知8声道格式回退到index 3
        assert_eq!(detect_lfe_from_layout("unknown", 8), Some(vec![3]));
    }

    #[test]
    fn test_2_1_format() {
        assert_eq!(detect_lfe_from_layout("2.1", 3), Some(vec![2]));
    }

    #[test]
    fn test_no_lfe() {
        assert_eq!(detect_lfe_from_layout("stereo", 2), Some(vec![]));
        assert_eq!(detect_lfe_from_layout("mono", 1), Some(vec![]));
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(detect_lfe_from_layout("5.1", 6), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("5.1(SIDE)", 6), Some(vec![3]));
        assert_eq!(detect_lfe_from_layout("mpeg_5_1_a", 6), Some(vec![3]));
    }
}
