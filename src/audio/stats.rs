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
    // 🔍 新增：包大小分布统计
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
            size_distribution: std::collections::HashMap::new(),
        }
    }

    pub fn add_chunk(&mut self, size: usize) {
        self.total_chunks += 1;
        self.sizes_sum += size;
        self.min_size = self.min_size.min(size);
        self.max_size = self.max_size.max(size);

        // 🔍 统计包大小分布
        *self.size_distribution.entry(size).or_insert(0) += 1;

        // 🔍 调试模式：简化包处理进度输出
        #[cfg(debug_assertions)]
        {
            if self.total_chunks <= 5 || (self.total_chunks % 500 == 0) {
                eprintln!(
                    "🎵 处理包#{}: {}样本/声道 (总计{}包)",
                    self.total_chunks, size, self.total_chunks
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

        // 🔍 调试模式：输出包大小分布统计
        #[cfg(debug_assertions)]
        {
            if self.total_chunks > 0 {
                eprintln!("\n📊 包大小分布统计:");

                // 按包大小排序
                let mut distribution: Vec<_> = self.size_distribution.iter().collect();
                distribution.sort_by_key(|&(size, _)| size);

                // 显示分布详情
                for (size, count) in &distribution {
                    let percentage = (**count as f64 / self.total_chunks as f64) * 100.0;
                    eprintln!("   {size}样本/声道: {count}个包 ({percentage:.1}%)");
                }

                // 找出最常见的包大小
                if let Some((most_common_size, most_count)) =
                    distribution.iter().max_by_key(|&(_, count)| count)
                {
                    eprintln!("   🎯 最常见: {most_common_size}样本/声道 ({most_count}个包)");
                }

                eprintln!("\n📋 统计摘要:");
                eprintln!("   总包数: {}", self.total_chunks);
                eprintln!(
                    "   包大小范围: {} ~ {} 样本/声道",
                    self.min_size, self.max_size
                );
                eprintln!("   平均大小: {:.1} 样本/声道", self.mean_size);
                eprintln!("   总样本: {} 样本/声道", self.sizes_sum);

                // 计算包大小变化系数
                if self.max_size > 0 && self.min_size > 0 {
                    let variation_ratio = self.max_size as f64 / self.min_size as f64;
                    eprintln!("   变化系数: {variation_ratio:.2}x");

                    if variation_ratio > 2.0 {
                        eprintln!("   📈 识别为可变包大小格式 (FLAC/OGG等)");
                    } else {
                        eprintln!("   📊 识别为固定包大小格式 (MP3/AAC等)");
                    }
                }
                eprintln!();
            }
        }
    }
}
