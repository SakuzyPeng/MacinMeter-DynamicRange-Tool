//! 常量和默认配置集中管理
//!
//! 将所有重要常量集中定义，避免"默认值漂移"和重复定义

/// DR分析算法常量
pub mod dr_analysis {
    /// 窗口时长系数 - foobar2000精确值
    ///
    /// 根据foobar2000 DR Meter逆向分析（sub_180007FB0），窗口长度计算公式：
    /// `window_samples = floor(sample_rate * WINDOW_DURATION_COEFFICIENT)`
    ///
    /// 系数值从官方44.1kHz标准反推：132480 / 44100 = 3.00408163265306127343...
    /// 使用f64最高精度表示以保证所有采样率的计算精度。
    ///
    /// 该系数确保与foobar2000的窗口对齐：
    /// - 44.1 kHz: 132,480样本（vs 常规3.0秒的132,000样本，+180样本）
    /// - 48 kHz: 144,195样本（vs 常规3.0秒的144,000样本，+195样本）
    /// - 96 kHz: 288,391样本（vs 常规3.0秒的288,000样本，+391样本）
    ///
    /// 窗口长度差异会影响：
    /// - 窗口数量统计
    /// - 20%分位点计算
    /// - 声道级DR精度（±0.16-1.17 dB偏差的关键因素）
    pub const WINDOW_DURATION_COEFFICIENT: f64 = 3.0040816326530613;

    /// 窗口时长（秒）- 显示用近似值
    ///
    /// 用于用户可见的日志输出和进度显示，保持简洁易读。
    /// 实际计算使用 WINDOW_DURATION_COEFFICIENT 的精确值。
    pub const WINDOW_DURATION_SECONDS: f64 = 3.0;

    /// 削波检测阈值（接近满幅度）
    ///
    /// 用于判断峰值是否被削波（clipping）。0.99999 表示当峰值达到满幅度的99.999%时，
    /// 视为削波。此阈值与 foobar2000 削波检测机制保持一致。
    ///
    /// **使用场景**：
    /// - ClippingAware 策略：当主峰被削波时，选择次峰以避免削波干扰
    /// - 削波状态诊断：判断音频是否存在削波现象
    ///
    /// **设计考量**：
    /// - 0.99999 留余地避免浮点精度导致的误判
    /// - 比 1.0 更稳健，考虑浮点加法/乘法的舍入误差
    /// - 与 foobar2000 标准一致
    pub const CLIPPING_THRESHOLD: f64 = 0.99999;

    /// 峰值相等性判断的浮点数容差阈值
    ///
    /// 用于判断两个f64峰值是否相等，考虑浮点数精度限制。
    /// 1e-15是足够严格的阈值，能够区分几乎所有实际音频样本的差异。
    ///
    /// **使用场景**：
    /// - 流式双峰跟踪：判断新样本是否等于当前最大值
    /// - 尾窗Peak调整：判断最后样本是否为最大值
    ///
    /// **设计考量**：
    /// - f64精度约为15-16位十进制数字
    /// - 1e-15阈值远小于音频样本的量化误差（16bit: ~1/32768 ≈ 3e-5）
    /// - 可以统一调整（例如改为1e-12）而不影响实际结果
    pub const PEAK_EQUALITY_EPSILON: f64 = 1e-15;

    /// 静音输入判断的RMS阈值（单位：能量值）
    ///
    /// 用于判断音频RMS是否足够小，应该视为静音。当RMS <= DR_ZERO_EPS时，
    /// DR计算应该直接返回0.0而不进行对数运算，避免log(0)错误。
    ///
    /// **使用场景**：
    /// - 静音检测：判断是否为完全或接近完全的静音
    /// - DR零化：当输入为静音时，直接返回DR=0.0而不计算
    /// - 数值安全：防止对数计算中的边界情况
    ///
    /// **设计考量**：
    /// - 1e-12是浮点精度和实际音频噪声底线的平衡
    /// - 16bit音频的量化噪声约为 1/32768 ≈ 3e-5（远大于1e-12）
    /// - 完全静音（全0样本）的RMS为 0.0
    /// - 为测试预留容差时，通常使用 DR_ZERO_EPS * 100.0 ≈ 1e-10
    pub const DR_ZERO_EPS: f64 = 1e-12;
}

/// 音频格式约束常量
pub mod format_constraints {
    /// 支持的最大声道数（架构限制）
    ///
    /// 基于foobar2000 DR Meter实测行为：支持任意声道数，通过算术平均计算Official DR。
    /// 每个声道独立计算DR值，最终Official DR为所有声道DR的算术平均（四舍五入到整数）。
    pub const MAX_CHANNELS: u16 = 32;
}

/// 解码器性能优化常量
pub mod decoder_performance {
    /// BatchPacketReader批量预读包数
    ///
    /// 通过批量预读减少系统调用次数，64包是经过性能测试的最优值：
    /// - 减少99%的I/O系统调用
    /// - 良好的缓存局部性（完全fit进L2缓存）
    /// - 与并行解码器batch_size保持一致
    pub const BATCH_PACKET_SIZE: usize = 64;

    /// BatchPacketReader预读触发阈值
    ///
    /// 当缓冲区包数量低于此阈值时触发批量预读，
    /// 20个包的阈值确保缓冲区始终有足够数据，避免解码器空等
    pub const PREFETCH_THRESHOLD: usize = 20;

    /// 并行解码器批量处理大小
    ///
    /// 用于OrderedParallelDecoder的批量解码配置，
    /// 与 defaults::PARALLEL_BATCH_SIZE 保持一致，避免漂移。
    pub const PARALLEL_DECODE_BATCH_SIZE: usize = super::defaults::PARALLEL_BATCH_SIZE;

    /// 并行解码器线程数
    ///
    /// 用于OrderedParallelDecoder的工作线程数，
    /// 与 defaults::PARALLEL_THREADS 保持一致，避免漂移。
    pub const PARALLEL_DECODE_THREADS: usize = super::defaults::PARALLEL_THREADS;

    /// 有序通道容量乘数
    ///
    /// SequencedChannel的容量 = PARALLEL_DECODE_THREADS × SEQUENCED_CHANNEL_CAPACITY_MULTIPLIER
    ///
    /// **设计原则**：
    /// - 核心洞察：乱序样本缓冲峰值取决于并发度（线程数），而非批次大小
    /// - 推荐值：3-4（平衡性能与内存，避免过度背压）
    /// - 最小值：2（确保基本吞吐，但可能引发频繁背压和栈溢出风险）
    /// - 过大风险：reorder_buffer峰值内存随容量线性增长
    ///
    /// **实测数据**：
    /// - 容量=128（batch_size×2）：218.78 MB/s，62.79 MB（基线）
    /// - 容量=16（threads×4）：236.69 MB/s，68.81 MB（+8.2%速度，+9.6%内存）
    /// - 容量=12（threads×3）：栈溢出崩溃（背压过度）
    /// - 容量=8（threads×2）：栈溢出崩溃（背压过度）
    pub const SEQUENCED_CHANNEL_CAPACITY_MULTIPLIER: usize = 4;

    /// drain_all_samples() 接收超时时间（毫秒）
    ///
    /// 用于 drain_all_samples() 中的 recv_timeout，5ms 是经过性能测试的最优值：
    /// - 避免CPU空轮询开销（相比 try_recv + sleep(1ms)）
    /// - 保持良好的响应性（尾部样本延迟 < 5ms）
    /// - 实测性能提升：+10% 吞吐量（213.27 MB/s → 234.65 MB/s）
    ///
    /// **性能验证**：
    /// - 优化前（try_recv + sleep(1ms)）：213.27 MB/s
    /// - 优化后（recv_timeout(5ms)）：234.65 MB/s（中位数 236.535 MB/s）
    /// - 性能提升：+10.0% ~ +10.9%
    /// - 稳定性：标准差 5.76 MB/s（变异系数 2.45%）
    pub const DRAIN_RECV_TIMEOUT_MS: u64 = 5;

    /// 线程本地样本缓冲区初始容量
    ///
    /// 用于并行解码器中每个工作线程的样本缓冲区预分配，
    /// 8192样本（32KB）是经过性能测试的最优值：
    /// - 单声道 3秒@44.1kHz ≈ 132,300样本，双声道 ≈ 264,600样本
    /// - 每包通常包含少量帧（几百到几千样本）
    /// - 8192容量可容纳大部分包，减少resize开销
    /// - 内存开销：每线程 32KB（4线程 = 128KB总计）
    ///
    /// **优化 #8**：线程本地缓冲区复用
    /// - 避免每包创建新Vec的分配开销
    /// - 通过clear()保留容量，实现跨包复用
    /// - 预期收益：内存峰值-20%，分配开销-10-15%
    pub const THREAD_LOCAL_SAMPLE_BUFFER_CAPACITY: usize = 8192;
}

/// 默认配置值
pub mod defaults {
    /// 默认并行批大小
    ///
    /// 用于并行解码时的批量处理大小，
    /// 64包是经过性能测试的最优值
    pub const PARALLEL_BATCH_SIZE: usize = 64;

    /// 默认并行线程数
    ///
    /// 用于多线程并行处理的默认线程数，
    /// 4线程在多数场景下提供良好的性能/资源平衡
    pub const PARALLEL_THREADS: usize = 4;

    /// 默认多文件并行并发度
    ///
    /// 用于批量处理多个文件时的并行度，
    /// 4并发度在多数场景下提供良好的性能/资源平衡
    pub const PARALLEL_FILES_DEGREE: usize = 4;
}

/// 并发度限制常量
pub mod parallel_limits {
    /// 最小并发度
    ///
    /// 任何并行处理至少需要1个线程/工作单元
    pub const MIN_PARALLEL_DEGREE: usize = 1;

    /// 最大并发度
    ///
    /// 限制最大并发度为16，避免过度并发导致的：
    /// - 上下文切换开销
    /// - 内存占用过高
    /// - 系统资源竞争
    pub const MAX_PARALLEL_DEGREE: usize = 16;

    /// 并行批大小最小值（用于CLI与解析验证）
    pub const MIN_PARALLEL_BATCH_SIZE: usize = 1;

    /// 并行批大小最大值（用于CLI与解析验证）
    pub const MAX_PARALLEL_BATCH_SIZE: usize = 256;
}

/// 缓冲区内存优化常量（硬上限策略）
pub mod buffers {
    /// 样本缓冲区容量预分配倍数
    ///
    /// 为 sample_buffer 预分配 window_size_samples * BUFFER_CAPACITY_MULTIPLIER 容量，
    /// 减少扩容次数和内存抖动。值为 3 可在多数场景下避免扩容：
    /// - 窗口重叠处理时有足够预留空间
    /// - 避免频繁的 Vec reallocation
    /// - 对内存峰值影响可控（≈ +2-3 MB per worker）
    pub const BUFFER_CAPACITY_MULTIPLIER: usize = 3;

    /// 样本缓冲区硬上限比例
    ///
    /// 当 sample_buffer.len() 超过 window_size_samples * MAX_BUFFER_RATIO 时，
    /// 触发强制 compact 操作，防止缓冲区无限增长。值为 3.5：
    /// - 提供足够的弹性空间（50% 预留）
    /// - 及时回收过度累积的内存
    /// - 与预分配倍数配合形成"预分配→正常使用→硬上限压缩"的完整循环
    pub const MAX_BUFFER_RATIO: f64 = 3.5;

    /// 窗口对齐优化开关（内部策略，面向开发者）
    ///
    /// **默认行为（Release）**：固定返回 true，启用硬上限内存优化（预分配+容量限制）
    ///
    /// **调试模式（Debug/Test）**：读取环境变量 `DR_DISABLE_WINDOW_ALIGN`
    /// - `DR_DISABLE_WINDOW_ALIGN=1` 或 `=true` → 禁用优化（用于A/B对比测试）
    /// - 未设置或其他值 → 启用优化（默认）
    ///
    /// **设计原则**：
    /// - 不污染用户CLI，保持接口简洁
    /// - Release行为确定性（固定启用），避免环境变量引入非确定性
    /// - Debug模式提供灵活性，便于性能对比和回归测试
    ///
    /// **使用示例**：
    /// ```bash
    /// # Debug模式：禁用优化进行对比测试（注意：不加--release）
    /// DR_DISABLE_WINDOW_ALIGN=1 cargo run -- /path/to/audio
    ///
    /// # Release二进制：环境变量被忽略，始终启用优化
    /// DR_DISABLE_WINDOW_ALIGN=1 ./target/release/MacinMeter-... /path  # 无效
    /// ```
    #[inline]
    pub fn window_alignment_enabled() -> bool {
        #[cfg(any(test, debug_assertions))]
        {
            // Debug/Test模式：支持环境变量控制
            std::env::var("DR_DISABLE_WINDOW_ALIGN")
                .map(|v| v != "1" && v != "true")
                .unwrap_or(true) // 默认启用
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            // Release模式：固定启用
            true
        }
    }
}

/// 应用程序信息常量（统一文案，避免漂移）
pub mod app_info {
    /// Git 分支信息（用于显示和输出）
    pub const BRANCH_INFO: &str = "main (默认批处理模式)";

    /// 基础描述信息
    pub const BASE_DESCRIPTION: &str = "基于foobar2000 DR Meter逆向分析 (Measuring_DR_ENv3规范) / Based on foobar2000 DR Meter Reverse Analysis (Measuring_DR_ENv3 Specification)";

    /// 计算模式描述
    pub const CALCULATION_MODE: &str = "使用批处理DR计算模式 / Using Batch DR Calculation Mode";

    /// 应用程序完整名称
    pub const APP_NAME: &str = "MacinMeter DR Tool";

    /// 应用程序版本后缀
    pub const VERSION_SUFFIX: &str = "(foobar2000兼容版)";

    /// 输出报告的兼容性标识
    ///
    /// 用于输出文件头部，表明与 foobar2000 DR Meter 的兼容性
    pub const OUTPUT_COMPATIBILITY: &str = "Dynamic Range Meter (foobar2000 compatible)";

    /// 生成完整的输出头部标识
    ///
    /// 格式：`MacinMeter DR Tool v{VERSION} / Dynamic Range Meter (foobar2000 compatible)`
    ///
    /// # 参数
    /// - `version`: 应用程序版本号（通常来自 `env!("CARGO_PKG_VERSION")`）
    ///
    /// # 示例
    /// ```ignore
    /// let header = app_info::format_output_header("0.1.0");
    /// // 输出: "MacinMeter DR Tool v0.1.0 / Dynamic Range Meter (foobar2000 compatible)"
    /// ```
    pub fn format_output_header(version: &str) -> String {
        format!("{APP_NAME} v{version} / {OUTPUT_COMPATIBILITY}")
    }
}

/// 文本与表格格式相关的可配置常量
pub mod formatting {
    /// 边界风险警告标题左右空格（可根据视觉需求调整）
    pub const WARNINGS_TITLE_LEFT_PAD: usize = 3;
    pub const WARNINGS_TITLE_RIGHT_PAD: usize = 3;

    /// 顶部主标题左右空格（可根据视觉需求调整）
    pub const HEADER_TITLE_LEFT_PAD: usize = 3;
    pub const HEADER_TITLE_RIGHT_PAD: usize = 3;

    /// 顶部副标题左右空格（可根据视觉需求调整）
    pub const SUBTITLE_LEFT_PAD: usize = 3;
    pub const SUBTITLE_RIGHT_PAD: usize = 3;
}
