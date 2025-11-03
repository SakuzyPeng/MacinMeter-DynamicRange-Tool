//! 块大小统计模块
//!
//! 提供音频块大小的实时统计和分析功能
//! 注意：此模块仅供universal_decoder协调器内部使用

/// 块大小统计信息
///
/// 此结构通过协调器对外提供服务，内部实现由协调器管理
#[derive(Debug, Clone)]
pub struct ChunkSizeStats {
    pub total_chunks: usize,
    pub min_size: usize,
    pub max_size: usize,
    pub mean_size: f64,
    sizes_sum: usize,
    // 包大小分布统计（仅在 debug 模式收集，避免 Release 开销）
    #[cfg(debug_assertions)]
    size_distribution: std::collections::HashMap<usize, usize>,
}

impl Default for ChunkSizeStats {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkSizeStats {
    pub fn new() -> Self {
        Self {
            total_chunks: 0,
            min_size: usize::MAX,
            max_size: 0,
            mean_size: 0.0,
            sizes_sum: 0,
            #[cfg(debug_assertions)]
            size_distribution: std::collections::HashMap::new(),
        }
    }

    /// 添加一个音频块的统计信息
    ///
    /// # 参数
    /// * `size` - 音频块大小（单位：每声道样本数，非交错样本总数）
    ///
    /// # 安全性
    /// 使用饱和加法防止 sizes_sum 溢出
    pub fn add_chunk(&mut self, size: usize) {
        self.total_chunks += 1;
        self.sizes_sum = self.sizes_sum.saturating_add(size);
        self.min_size = self.min_size.min(size);
        self.max_size = self.max_size.max(size);

        // 调试模式：收集包大小分布并输出进度
        #[cfg(debug_assertions)]
        {
            *self.size_distribution.entry(size).or_insert(0) += 1;

            let is_500_mult = (self.total_chunks / 500) * 500 == self.total_chunks;
            if self.total_chunks <= 5 || is_500_mult {
                eprintln!(
                    "Processed packet #{count}: {size} samples/channel (total {total}) / 处理包#{count}: {size}样本/声道 (总计{total}包)",
                    count = self.total_chunks,
                    total = self.total_chunks
                );
            }
        }
    }

    pub fn finalize(&mut self) {
        if self.total_chunks > 0 {
            self.mean_size = self.sizes_sum as f64 / self.total_chunks as f64;
        }
        // 修复边界情况
        if self.min_size == usize::MAX {
            self.min_size = 0;
        }

        // 调试模式：输出包大小分布统计
        #[cfg(debug_assertions)]
        {
            if self.total_chunks > 0 {
                eprintln!("\nPacket size distribution / 包大小分布统计:");

                // 按包大小排序
                let mut distribution: Vec<_> = self.size_distribution.iter().collect();
                distribution.sort_by_key(|&(size, _)| size);

                // 显示分布详情
                for (size, count) in &distribution {
                    let percentage = (**count as f64 / self.total_chunks as f64) * 100.0;
                    eprintln!(
                        "   {size} samples/channel: {count} packets ({percentage:.1}%) / {size}样本/声道: {count}个包 ({percentage:.1}%)"
                    );
                }

                // 找出最常见的包大小
                if let Some((most_common_size, most_count)) =
                    distribution.iter().max_by_key(|&(_, count)| count)
                {
                    eprintln!(
                        "   Most frequent: {most_common_size} samples/channel ({most_count} packets) / 最常见: {most_common_size}样本/声道 ({most_count}个包)"
                    );
                }

                eprintln!("\nStatistics summary / 统计摘要:");
                eprintln!(
                    "   Total packets: {count} / 总包数: {count}",
                    count = self.total_chunks
                );
                eprintln!(
                    "   Packet size range: {} ~ {} samples/channel / 包大小范围: {} ~ {} 样本/声道",
                    self.min_size, self.max_size, self.min_size, self.max_size
                );
                eprintln!(
                    "   Average size: {:.1} samples/channel / 平均大小: {:.1} 样本/声道",
                    self.mean_size, self.mean_size
                );
                eprintln!(
                    "   Total samples: {} samples/channel / 总样本: {} 样本/声道",
                    self.sizes_sum, self.sizes_sum
                );

                // 计算包大小变化系数（启发式判断）
                if self.max_size > 0 && self.min_size > 0 {
                    let variation_ratio = self.max_size as f64 / self.min_size as f64;
                    eprintln!(
                        "   Variation ratio: {variation_ratio:.2}x / 变化系数: {variation_ratio:.2}x"
                    );

                    if variation_ratio > 2.0 {
                        eprintln!(
                            "   Likely variable-size packets (e.g., FLAC/OGG) / 可能是可变包大小格式 (如 FLAC/OGG)"
                        );
                    } else {
                        eprintln!(
                            "   Likely fixed-size packets (e.g., MP3/AAC) / 可能是固定包大小格式 (如 MP3/AAC)"
                        );
                    }
                }
                eprintln!();
            }
        }
    }
}
