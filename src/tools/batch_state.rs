//! 批处理状态管理模块
//!
//! 提供统一的批处理统计管理，支持串行和并行两种模式。

use crate::error::ErrorCategory;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 批处理统计快照
///
/// 包含处理成功/失败计数和错误分类统计
#[derive(Debug, Clone)]
pub struct BatchStatsSnapshot {
    /// 成功处理的文件数
    pub processed: usize,
    /// 失败的文件数
    pub failed: usize,
    /// 错误分类统计（错误类型 -> 失败文件列表）
    pub error_stats: HashMap<ErrorCategory, Vec<String>>,
}

/// 串行批处理统计（单线程安全）
///
/// 使用普通类型，适用于单线程串行处理场景
#[derive(Debug, Default)]
pub struct SerialBatchStats {
    processed: usize,
    failed: usize,
    error_stats: HashMap<ErrorCategory, Vec<String>>,
}

impl SerialBatchStats {
    /// 创建新的串行统计实例
    pub fn new() -> Self {
        Self::default()
    }

    /// 增加成功处理计数
    #[inline]
    pub fn inc_processed(&mut self) -> usize {
        self.processed += 1;
        self.processed
    }

    /// 增加失败计数并记录错误分类
    #[inline]
    pub fn inc_failed(&mut self, category: ErrorCategory, filename: String) -> usize {
        self.failed += 1;
        self.error_stats.entry(category).or_default().push(filename);
        self.failed
    }

    /// 获取统计快照
    pub fn snapshot(&self) -> BatchStatsSnapshot {
        BatchStatsSnapshot {
            processed: self.processed,
            failed: self.failed,
            error_stats: self.error_stats.clone(),
        }
    }
}

/// 并行批处理统计（多线程安全）
///
/// 使用原子类型和锁，适用于多线程并行处理场景
#[derive(Debug, Clone)]
pub struct ParallelBatchStats {
    processed: Arc<AtomicUsize>,
    failed: Arc<AtomicUsize>,
    error_stats: Arc<Mutex<HashMap<ErrorCategory, Vec<String>>>>,
}

impl ParallelBatchStats {
    /// 创建新的并行统计实例
    pub fn new() -> Self {
        Self {
            processed: Arc::new(AtomicUsize::new(0)),
            failed: Arc::new(AtomicUsize::new(0)),
            error_stats: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 增加成功处理计数（线程安全）
    #[inline]
    pub fn inc_processed(&self) -> usize {
        self.processed.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 增加失败计数并记录错误分类（线程安全）
    pub fn inc_failed(&self, category: ErrorCategory, filename: String) -> usize {
        let count = self.failed.fetch_add(1, Ordering::Relaxed) + 1;

        // 更新错误分类统计（需要锁）
        if let Ok(mut stats) = self.error_stats.lock() {
            stats.entry(category).or_default().push(filename);
        }

        count
    }

    /// 获取统计快照（线程安全）
    pub fn snapshot(&self) -> BatchStatsSnapshot {
        BatchStatsSnapshot {
            processed: self.processed.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            error_stats: self
                .error_stats
                .lock()
                .map(|stats| stats.clone())
                .unwrap_or_default(),
        }
    }
}

impl Default for ParallelBatchStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioError;

    #[test]
    fn test_serial_stats_basic() {
        let mut stats = SerialBatchStats::new();

        // 测试初始状态
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.processed, 0);
        assert_eq!(snapshot.failed, 0);
        assert!(snapshot.error_stats.is_empty());

        // 测试递增成功计数
        assert_eq!(stats.inc_processed(), 1);
        assert_eq!(stats.inc_processed(), 2);

        // 测试递增失败计数和错误分类
        let category =
            ErrorCategory::from_audio_error(&AudioError::FormatError("test error".to_string()));
        assert_eq!(stats.inc_failed(category, "file1.wav".to_string()), 1);
        assert_eq!(stats.inc_failed(category, "file2.wav".to_string()), 2);

        // 验证快照
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.processed, 2);
        assert_eq!(snapshot.failed, 2);
        assert_eq!(snapshot.error_stats.len(), 1);
        assert_eq!(snapshot.error_stats[&category].len(), 2);
    }

    #[test]
    fn test_serial_stats_multiple_categories() {
        let mut stats = SerialBatchStats::new();

        let cat1 = ErrorCategory::from_audio_error(&AudioError::FormatError("format".to_string()));
        let cat2 =
            ErrorCategory::from_audio_error(&AudioError::DecodingError("decode".to_string()));

        stats.inc_failed(cat1, "file1.wav".to_string());
        stats.inc_failed(cat2, "file2.mp3".to_string());
        stats.inc_failed(cat1, "file3.wav".to_string());

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.failed, 3);
        assert_eq!(snapshot.error_stats.len(), 2);
        assert_eq!(snapshot.error_stats[&cat1].len(), 2);
        assert_eq!(snapshot.error_stats[&cat2].len(), 1);
    }

    #[test]
    fn test_parallel_stats_basic() {
        let stats = ParallelBatchStats::new();

        // 测试初始状态
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.processed, 0);
        assert_eq!(snapshot.failed, 0);

        // 测试递增成功计数
        assert_eq!(stats.inc_processed(), 1);
        assert_eq!(stats.inc_processed(), 2);

        // 测试递增失败计数
        let category =
            ErrorCategory::from_audio_error(&AudioError::FormatError("test error".to_string()));
        assert_eq!(stats.inc_failed(category, "file1.wav".to_string()), 1);

        // 验证快照
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.processed, 2);
        assert_eq!(snapshot.failed, 1);
    }

    #[test]
    fn test_parallel_stats_concurrent_updates() {
        use rayon::prelude::*;

        let stats = ParallelBatchStats::new();

        // 并发递增成功计数
        (0..100).into_par_iter().for_each(|_| {
            stats.inc_processed();
        });

        // 并发递增失败计数
        let category =
            ErrorCategory::from_audio_error(&AudioError::FormatError("test error".to_string()));
        (0..50).into_par_iter().for_each(|i| {
            stats.inc_failed(category, format!("file{i}.wav"));
        });

        // 验证并发累加正确性
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.processed, 100);
        assert_eq!(snapshot.failed, 50);
        assert_eq!(snapshot.error_stats[&category].len(), 50);
    }

    #[test]
    fn test_parallel_stats_clone() {
        let stats1 = ParallelBatchStats::new();
        stats1.inc_processed();

        // 克隆应该共享同一状态（Arc）
        let stats2 = stats1.clone();
        stats2.inc_processed();

        // 两个实例应该看到相同的计数
        assert_eq!(stats1.snapshot().processed, 2);
        assert_eq!(stats2.snapshot().processed, 2);
    }
}
