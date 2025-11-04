//! 音频测试固件生成器
//!
//! 为边界和异常测试生成各种特殊的音频文件

use fs2::FileExt;
use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

fn fixtures_base_dir() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Ok(custom) = std::env::var("MACINMETER_FIXTURES_DIR") {
            let path = PathBuf::from(custom);
            create_dir_all(&path).expect("无法创建自定义测试固件目录");
            path
        } else {
            let path = PathBuf::from("tests/fixtures");
            create_dir_all(&path).expect("无法创建测试固件目录");
            path
        }
    })
}

/// 公开获取固件根目录
pub fn fixtures_dir() -> PathBuf {
    fixtures_base_dir().clone()
}

/// 获取特定固件文件路径
pub fn fixture_path(name: &str) -> PathBuf {
    fixtures_base_dir().join(name)
}

/// 确保所有固件生成完毕（幂等）。
pub fn ensure_fixtures_generated() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let fixtures = AudioTestFixtures::new();
        fixtures.generate_all();
    });
}

/// 跨进程文件锁 + 进程内互斥，避免并发写入导致的截断文件。
struct FixtureLock {
    _mutex_guard: std::sync::MutexGuard<'static, ()>,
    lock_file: File,
}

impl FixtureLock {
    fn acquire() -> Self {
        static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        let mutex = MUTEX.get_or_init(|| Mutex::new(()));
        let guard = mutex.lock().expect("Fixture mutex poisoned");

        let lock_path = fixtures_base_dir().join(".lock");
        if let Some(parent) = lock_path.parent() {
            create_dir_all(parent).expect("无法创建锁文件目录");
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .expect("无法创建固件锁文件");
        file.lock_exclusive()
            .expect("无法获取固件文件锁，可能被其他进程占用");

        Self {
            _mutex_guard: guard,
            lock_file: file,
        }
    }
}

impl Drop for FixtureLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.lock_file);
    }
}

/// 测试固件生成器
pub struct AudioTestFixtures {
    fixtures_dir: PathBuf,
}

impl AudioTestFixtures {
    /// 创建固件生成器，默认目录为 tests/fixtures
    pub fn new() -> Self {
        let fixtures_dir = fixtures_dir();
        Self { fixtures_dir }
    }

    /// 自定义固件目录
    #[allow(dead_code)]
    pub fn with_dir(dir: impl AsRef<Path>) -> Self {
        let fixtures_dir = dir.as_ref().to_path_buf();
        create_dir_all(&fixtures_dir).expect("无法创建测试固件目录");
        Self { fixtures_dir }
    }

    /// 获取固件路径
    pub fn get_path(&self, filename: &str) -> PathBuf {
        fixture_path(filename)
    }

    /// 生成所有测试固件
    pub fn generate_all(&self) {
        // 同一时间只允许一个线程生成固件，避免并行测试造成中途读取未完成的文件。
        let _guard = FixtureLock::acquire();

        log(
            "开始生成音频测试固件...",
            "Generating audio test fixtures...",
        );

        self.create_zero_length();
        self.create_single_sample();
        self.create_silence();
        self.create_full_scale_clipping();
        self.create_high_sample_rate();
        self.create_empty_file();
        self.create_fake_audio();
        self.create_truncated_wav();
        self.create_3_channels();
        self.create_tiny_duration();
        self.create_nan_like_edge_cases();

        log(
            format!("所有测试固件已生成到: {:?}", self.fixtures_dir),
            format!("All fixtures generated at: {:?}", self.fixtures_dir),
        );
    }

    // ========== 边界条件测试文件 ==========

    /// 1. 零长度音频（0个样本，只有头）
    pub fn create_zero_length(&self) -> PathBuf {
        let path = self.get_path("zero_length.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        // 创建后立即关闭，不写入任何样本
        {
            let writer = WavWriter::create(&path, spec).expect("无法创建零长度文件");
            drop(writer); // 显式关闭，确保文件被刷新到磁盘
        }
        log(
            "  生成 zero_length.wav (0 样本)",
            "  Generated zero_length.wav (0 samples)",
        );
        path
    }

    /// 2. 单采样点文件
    pub fn create_single_sample(&self) -> PathBuf {
        let path = self.get_path("single_sample.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        {
            let mut writer = WavWriter::create(&path, spec).expect("无法创建单样本文件");
            writer.write_sample(16384i16).expect("无法写入样本"); // 半幅度样本
            writer.finalize().expect("无法完成写入");
        }
        log(
            "  生成 single_sample.wav (1 个样本)",
            "  Generated single_sample.wav (1 sample)",
        );
        path
    }

    /// 3. 极短音频（10ms，441个样本）
    pub fn create_tiny_duration(&self) -> PathBuf {
        let path = self.get_path("tiny_duration.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        {
            let mut writer = WavWriter::create(&path, spec).expect("无法创建极短文件");
            let samples = 441; // 10ms @ 44.1kHz
            for i in 0..samples {
                let sample = ((i as f64 / samples as f64) * 16384.0) as i16;
                writer.write_sample(sample).expect("无法写入样本");
                writer.write_sample(sample).expect("无法写入样本");
            }
            writer.finalize().expect("无法完成写入");
        }
        log(
            "  生成 tiny_duration.wav (10 毫秒)",
            "  Generated tiny_duration.wav (10 ms)",
        );
        path
    }

    // ========== 数值边界测试文件 ==========

    /// 4. 静音文件（全0样本，1秒）
    pub fn create_silence(&self) -> PathBuf {
        let path = self.get_path("silence.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        {
            let mut writer = WavWriter::create(&path, spec).expect("无法创建静音文件");
            let duration_samples = 44100; // 1秒
            for _ in 0..(duration_samples * 2) {
                // 立体声
                writer.write_sample(0i16).expect("无法写入样本");
            }
            writer.finalize().expect("无法完成写入");
        } // writer 在这里被 drop，确保文件被完全写入磁盘
        log(
            "  生成 silence.wav (1 秒静音)",
            "  Generated silence.wav (1 s silence)",
        );
        path
    }

    /// 5. 全削波（满刻度方波，1秒）
    pub fn create_full_scale_clipping(&self) -> PathBuf {
        let path = self.get_path("full_scale_clipping.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        {
            let mut writer = WavWriter::create(&path, spec).expect("无法创建削波文件");
            let duration_samples = 44100; // 1秒
            for i in 0..(duration_samples * 2) {
                // 交替最大/最小值，模拟严重削波
                let sample = if i % 4 < 2 { i16::MAX } else { i16::MIN };
                writer.write_sample(sample).expect("无法写入样本");
            }
            writer.finalize().expect("无法完成写入");
        }
        log(
            "  生成 full_scale_clipping.wav (满幅方波)",
            "  Generated full_scale_clipping.wav (full-scale square wave)",
        );
        path
    }

    /// 6. 边缘情况：极小值波形（测试精度）
    pub fn create_nan_like_edge_cases(&self) -> PathBuf {
        let path = self.get_path("edge_cases.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        {
            let mut writer = WavWriter::create(&path, spec).expect("无法创建边缘情况文件");

            // 组合各种边缘值
            let edge_values = vec![
                0i16,     // 静音
                1i16,     // 最小非零值
                -1i16,    // 最小负值
                i16::MAX, // 最大正值
                i16::MIN, // 最小负值
                100i16,   // 小值
                -100i16,  // 小负值
            ];

            for _ in 0..44100 {
                // 1秒，重复边缘值
                for &val in &edge_values {
                    writer.write_sample(val).expect("无法写入样本");
                    writer.write_sample(val).expect("无法写入样本");
                }
            }
            writer.finalize().expect("无法完成写入");
        }
        log(
            "  生成 edge_cases.wav (边缘值组合)",
            "  Generated edge_cases.wav (edge value patterns)",
        );
        path
    }

    // ========== 格式边界测试文件 ==========

    /// 7. 极高采样率（192kHz，24bit，1秒）
    pub fn create_high_sample_rate(&self) -> PathBuf {
        let path = self.get_path("high_sample_rate.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 192000,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        };
        {
            let mut writer = WavWriter::create(&path, spec).expect("无法创建高采样率文件");

            // 生成440Hz正弦波（24bit）
            let duration_samples = 192000; // 1秒
            for i in 0..duration_samples {
                let t = i as f64 / 192000.0;
                let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin() * 8388607.0; // 24bit max
                let sample_i32 = sample as i32;
                writer.write_sample(sample_i32).expect("无法写入样本");
                writer.write_sample(sample_i32).expect("无法写入样本");
            }
            writer.finalize().expect("无法完成写入");
        }
        log(
            "  生成 high_sample_rate.wav (192kHz, 24bit)",
            "  Generated high_sample_rate.wav (192 kHz, 24-bit)",
        );
        path
    }

    /// 8. 3声道文件（应该被拒绝）
    pub fn create_3_channels(&self) -> PathBuf {
        let path = self.get_path("3_channels.wav");
        let mut file = File::create(&path).expect("无法创建3声道文件");

        // 手动构造3声道WAV文件头
        let sample_rate = 44100u32;
        let channels = 3u16;
        let bits_per_sample = 16u16;
        let num_samples = 44100u32; // 1秒
        let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
        let block_align = channels * (bits_per_sample / 8);
        let data_size = num_samples * channels as u32 * (bits_per_sample as u32 / 8);

        // RIFF头
        file.write_all(b"RIFF").expect("写入失败");
        file.write_all(&(36 + data_size).to_le_bytes())
            .expect("写入失败");
        file.write_all(b"WAVE").expect("写入失败");

        // fmt chunk
        file.write_all(b"fmt ").expect("写入失败");
        file.write_all(&16u32.to_le_bytes()).expect("写入失败"); // chunk size
        file.write_all(&1u16.to_le_bytes()).expect("写入失败"); // PCM
        file.write_all(&channels.to_le_bytes()).expect("写入失败"); // 3声道
        file.write_all(&sample_rate.to_le_bytes())
            .expect("写入失败");
        file.write_all(&byte_rate.to_le_bytes()).expect("写入失败");
        file.write_all(&block_align.to_le_bytes())
            .expect("写入失败");
        file.write_all(&bits_per_sample.to_le_bytes())
            .expect("写入失败");

        // data chunk
        file.write_all(b"data").expect("写入失败");
        file.write_all(&data_size.to_le_bytes()).expect("写入失败");

        // 写入3声道样本数据（简单的1kHz正弦波）
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let sample = (t * 1000.0 * 2.0 * std::f64::consts::PI).sin() * 16384.0;
            let sample_i16 = sample as i16;
            for _ in 0..3 {
                file.write_all(&sample_i16.to_le_bytes()).expect("写入失败");
            }
        }

        log(
            "  生成 3_channels.wav (3 声道，应被拒绝)",
            "  Generated 3_channels.wav (3 channels, should be rejected)",
        );
        path
    }

    // ========== 异常文件测试 ==========

    /// 9. 空文件（0字节）
    pub fn create_empty_file(&self) -> PathBuf {
        let path = self.get_path("empty.wav");
        File::create(&path).expect("无法创建空文件");
        log(
            "  生成 empty.wav (0 字节)",
            "  Generated empty.wav (0 bytes)",
        );
        path
    }

    /// 10. 伪装文件（.txt内容伪装为.wav）
    pub fn create_fake_audio(&self) -> PathBuf {
        let path = self.get_path("fake_audio.wav");
        let mut file = File::create(&path).expect("无法创建伪装文件");
        file.write_all(b"This is not a valid WAV file!\n")
            .expect("写入失败");
        file.write_all(b"Just some text pretending to be audio.\n")
            .expect("写入失败");
        log(
            "  生成 fake_audio.wav (文本伪装为WAV)",
            "  Generated fake_audio.wav (text file disguised as WAV)",
        );
        path
    }

    /// 11. 截断WAV（头部正常，数据不完整）
    pub fn create_truncated_wav(&self) -> PathBuf {
        let path = self.get_path("truncated.wav");

        // 先创建一个正常的WAV文件
        {
            let spec = WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            };
            let mut writer = WavWriter::create(&path, spec).expect("无法创建文件");

            // 写入1000个样本
            for i in 0..1000 {
                let sample = (i as f64 / 1000.0 * 16384.0) as i16;
                writer.write_sample(sample).expect("写入失败");
                writer.write_sample(sample).expect("写入失败");
            }
            writer.finalize().expect("无法完成写入");
        } // 确保 writer 被 drop，文件被完全写入

        // 截断文件（只保留前200字节，包含头但数据不完整）
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("无法打开文件");
        file.set_len(200).expect("无法截断文件");

        log(
            "  生成 truncated.wav (截断数据)",
            "  Generated truncated.wav (incomplete data)",
        );
        path
    }

    /// 清理所有生成的测试文件
    #[allow(dead_code)]
    pub fn cleanup(&self) {
        if self.fixtures_dir.exists() {
            std::fs::remove_dir_all(&self.fixtures_dir).expect("无法删除测试固件目录");
            log("已清理测试固件目录", "Fixture directory cleaned");
        }
    }
}

impl Default for AudioTestFixtures {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixture_generation() {
        let fixtures = AudioTestFixtures::new();

        // 生成所有固件
        fixtures.generate_all();

        // 验证文件存在
        assert!(fixtures.get_path("zero_length.wav").exists());
        assert!(fixtures.get_path("silence.wav").exists());
        assert!(fixtures.get_path("full_scale_clipping.wav").exists());
        assert!(fixtures.get_path("3_channels.wav").exists());

        // 验证文件大小
        let empty = std::fs::metadata(fixtures.get_path("empty.wav")).unwrap();
        assert_eq!(empty.len(), 0, "空文件应该是0字节");

        let fake = std::fs::metadata(fixtures.get_path("fake_audio.wav")).unwrap();
        assert!(fake.len() < 100, "伪装文件应该很小");

        // 不自动清理，留给手动验证或后续测试使用
        // fixtures.cleanup();
    }
}
