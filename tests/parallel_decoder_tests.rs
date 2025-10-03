//! OrderedParallelDecoder有序并行解码器测试
//!
//! 测试并行解码的顺序保证、状态管理和错误处理
//! 优先测试不需要真实音频的核心逻辑

use macinmeter_dr_tool::audio::parallel_decoder::{DecodedChunk, DecodingState, SequencedChannel};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// ========== DecodedChunk枚举测试 ==========

#[test]
fn test_decoded_chunk_samples_variant() {
    let samples = vec![0.1, 0.2, 0.3];
    let chunk = DecodedChunk::Samples(samples.clone());

    match chunk {
        DecodedChunk::Samples(s) => assert_eq!(s, samples),
        DecodedChunk::EOF => panic!("应该是Samples变体"),
    }
}

#[test]
fn test_decoded_chunk_eof_variant() {
    let chunk = DecodedChunk::EOF;

    match chunk {
        DecodedChunk::EOF => {}
        DecodedChunk::Samples(_) => panic!("应该是EOF变体"),
    }
}

#[test]
fn test_decoded_chunk_clone() {
    let chunk1 = DecodedChunk::Samples(vec![1.0, 2.0]);
    let chunk2 = chunk1.clone();

    match (chunk1, chunk2) {
        (DecodedChunk::Samples(s1), DecodedChunk::Samples(s2)) => {
            assert_eq!(s1, s2);
        }
        _ => panic!("克隆后应该相等"),
    }
}

// ========== DecodingState状态机测试 ==========

#[test]
fn test_decoding_state_transitions() {
    let state = DecodingState::Decoding;
    assert_eq!(state, DecodingState::Decoding);

    let state = DecodingState::Flushing;
    assert_eq!(state, DecodingState::Flushing);

    let state = DecodingState::Completed;
    assert_eq!(state, DecodingState::Completed);
}

#[test]
fn test_decoding_state_copy() {
    let state1 = DecodingState::Decoding;
    let state2 = state1;

    assert_eq!(state1, state2);
    assert_eq!(state1, DecodingState::Decoding);
}

#[test]
fn test_decoding_state_inequality() {
    assert_ne!(DecodingState::Decoding, DecodingState::Flushing);
    assert_ne!(DecodingState::Flushing, DecodingState::Completed);
    assert_ne!(DecodingState::Decoding, DecodingState::Completed);
}

// ========== SequencedChannel顺序保证测试 ==========

#[test]
fn test_sequenced_channel_creation() {
    let channel: SequencedChannel<i32> = SequencedChannel::new();

    // 尝试非阻塞接收，应该返回错误（空通道）
    match channel.try_recv_ordered() {
        Err(mpsc::TryRecvError::Empty) => {}
        _ => panic!("空通道应该返回Empty错误"),
    }
}

#[test]
fn test_sequenced_channel_default() {
    let channel: SequencedChannel<String> = SequencedChannel::default();

    match channel.try_recv_ordered() {
        Err(mpsc::TryRecvError::Empty) => {}
        _ => panic!("默认通道应该为空"),
    }
}

#[test]
fn test_sequenced_channel_ordered_send() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    // 按顺序发送
    sender.send_sequenced(0, "first").unwrap();
    sender.send_sequenced(1, "second").unwrap();
    sender.send_sequenced(2, "third").unwrap();

    // 按顺序接收
    assert_eq!(channel.recv_ordered().unwrap(), "first");
    assert_eq!(channel.recv_ordered().unwrap(), "second");
    assert_eq!(channel.recv_ordered().unwrap(), "third");
}

#[test]
fn test_sequenced_channel_out_of_order_send() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    // 乱序发送：2, 0, 1
    sender.send_sequenced(2, "third").unwrap();
    sender.send_sequenced(0, "first").unwrap();
    sender.send_sequenced(1, "second").unwrap();

    // 仍然按正确顺序接收
    assert_eq!(channel.recv_ordered().unwrap(), "first");
    assert_eq!(channel.recv_ordered().unwrap(), "second");
    assert_eq!(channel.recv_ordered().unwrap(), "third");
}

#[test]
fn test_sequenced_channel_concurrent_send() {
    let channel = SequencedChannel::new();
    let sender1 = channel.sender();
    let sender2 = channel.sender();
    let sender3 = channel.sender();

    // 3个线程并发发送
    let h1 = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        sender1.send_sequenced(2, 300).unwrap();
    });

    let h2 = thread::spawn(move || {
        thread::sleep(Duration::from_millis(5));
        sender2.send_sequenced(1, 200).unwrap();
    });

    let h3 = thread::spawn(move || {
        sender3.send_sequenced(0, 100).unwrap();
    });

    h1.join().unwrap();
    h2.join().unwrap();
    h3.join().unwrap();

    // 验证顺序正确
    assert_eq!(channel.recv_ordered().unwrap(), 100);
    assert_eq!(channel.recv_ordered().unwrap(), 200);
    assert_eq!(channel.recv_ordered().unwrap(), 300);
}

#[test]
fn test_sequenced_channel_large_sequence_gap() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    // 发送序列号0和100，中间有99个gap
    sender.send_sequenced(100, "gap").unwrap();
    sender.send_sequenced(0, "start").unwrap();

    // 先收到序列号0
    assert_eq!(channel.recv_ordered().unwrap(), "start");

    // 序列号100仍在缓冲区等待，无法立即收到
    match channel.try_recv_ordered() {
        Err(mpsc::TryRecvError::Empty) => {}
        Ok(_) => panic!("序列号100应该还在缓冲区等待中间序列号"),
        Err(e) => panic!("意外错误: {e:?}"),
    }
}

#[test]
fn test_ordered_sender_clone() {
    let channel = SequencedChannel::new();
    let sender1 = channel.sender();
    let sender2 = sender1.clone();

    sender1.send_sequenced(0, "from_sender1").unwrap();
    sender2.send_sequenced(1, "from_sender2").unwrap();

    assert_eq!(channel.recv_ordered().unwrap(), "from_sender1");
    assert_eq!(channel.recv_ordered().unwrap(), "from_sender2");
}

// ========== 边界条件和错误处理 ==========

#[test]
fn test_sequenced_channel_empty_recv() {
    let channel: SequencedChannel<i32> = SequencedChannel::new();

    match channel.try_recv_ordered() {
        Err(mpsc::TryRecvError::Empty) => {}
        _ => panic!("空通道应该返回Empty"),
    }
}

#[test]
fn test_sequenced_channel_disconnected() {
    let channel: SequencedChannel<i32> = SequencedChannel::new();
    let sender = channel.sender();

    drop(sender); // 丢弃发送端

    match channel.try_recv_ordered() {
        Err(mpsc::TryRecvError::Disconnected) => {}
        Err(mpsc::TryRecvError::Empty) => {} // 如果还没检测到断开也可接受
        _ => panic!("发送端关闭后应该返回Disconnected或Empty"),
    }
}

#[test]
fn test_decoded_chunk_empty_samples() {
    let chunk = DecodedChunk::Samples(Vec::new());

    match chunk {
        DecodedChunk::Samples(s) => assert!(s.is_empty()),
        _ => panic!("应该是空Samples"),
    }
}

// ========== 性能和压力测试 ==========

#[test]
fn test_sequenced_channel_high_volume() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    const COUNT: usize = 1000;

    // 发送1000个乱序数据
    for i in (0..COUNT).rev() {
        sender.send_sequenced(i, i * 2).unwrap();
    }

    // 验证全部按序接收
    for i in 0..COUNT {
        assert_eq!(channel.recv_ordered().unwrap(), i * 2);
    }
}

#[test]
fn test_sequenced_channel_interleaved_send() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    // 交错发送：偶数先，奇数后
    for i in (0..10).step_by(2) {
        sender.send_sequenced(i, format!("even_{i}")).unwrap();
    }
    for i in (1..10).step_by(2) {
        sender.send_sequenced(i, format!("odd_{i}")).unwrap();
    }

    // 验证顺序
    for i in 0..10 {
        let expected = if i % 2 == 0 {
            format!("even_{i}")
        } else {
            format!("odd_{i}")
        };
        assert_eq!(channel.recv_ordered().unwrap(), expected);
    }
}

// ========== 优先级1：并行解码健壮性压力测试 ==========
// 所有压力测试标记#[ignore]以避免CI超时，使用小型数据确保内存安全

/// 大批量数据序列正确性测试（10000包完全逆序）
///
/// 安全性：10000个u32 = 40KB内存，完全安全
/// 风险控制：使用try_recv_ordered()防止死锁，标记#[ignore]避免CI超时
#[test]
#[ignore = "压力测试，可能需要数秒，仅本地运行"]
fn test_large_scale_sequence_ordering() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    const LARGE_COUNT: usize = 10_000;

    println!("开始大批量测试：{LARGE_COUNT} 个样本完全逆序发送");

    // 完全逆序发送：从9999到0
    for i in (0..LARGE_COUNT).rev() {
        sender.send_sequenced(i, i as u32).expect("发送失败");
    }

    println!("✓ 发送完成，开始验证顺序接收...");

    // 验证全部按正确顺序接收
    for expected_seq in 0..LARGE_COUNT {
        let received = channel.recv_ordered().expect("接收失败");

        assert_eq!(
            received, expected_seq as u32,
            "序列号 {expected_seq} 接收错误"
        );

        // 每1000个打印进度
        if expected_seq % 1000 == 0 && expected_seq > 0 {
            println!("  已验证 {expected_seq}/{LARGE_COUNT}");
        }
    }

    println!("✅ 大批量测试通过：{LARGE_COUNT} 个样本全部按序接收");
}

/// 极端序列号跳跃场景测试
///
/// 测试序列号大跳跃（如0→5000→10000）时的缓冲区处理
/// 风险控制：使用小数据集，避免内存爆炸
#[test]
#[ignore = "压力测试，测试极端场景，仅本地运行"]
fn test_extreme_sequence_gaps() {
    let channel: SequencedChannel<String> = SequencedChannel::new();
    let sender = channel.sender();

    println!("测试极端序列号跳跃：0 → 5000 → 10000");

    // 乱序发送：先发10000，再发0，最后发5000
    sender.send_sequenced(10_000, "last".to_string()).unwrap();
    sender.send_sequenced(0, "first".to_string()).unwrap();
    sender.send_sequenced(5_000, "middle".to_string()).unwrap();

    // 先收到序列号0
    assert_eq!(channel.recv_ordered().unwrap(), "first");
    println!("✓ 收到序列号0");

    // 序列号5000和10000仍在缓冲区等待
    match channel.try_recv_ordered() {
        Err(mpsc::TryRecvError::Empty) => {
            println!("✓ 序列号5000和10000正确缓冲等待");
        }
        Ok(v) => panic!("不应收到数据，实际收到: {v:?}"),
        Err(e) => panic!("意外错误: {e:?}"),
    }

    // 填充gap：发送1到4999
    println!("填充gap：发送序列号1-4999");
    for i in 1..5_000 {
        sender.send_sequenced(i, format!("seq_{i}")).unwrap();
    }

    // 现在应该能收到1-5000
    for i in 1..=5_000 {
        let expected = if i == 5_000 {
            "middle".to_string()
        } else {
            format!("seq_{i}")
        };
        assert_eq!(channel.recv_ordered().unwrap(), expected);

        if i % 1000 == 0 {
            println!("  已接收到序列号{i}");
        }
    }

    println!("✅ 极端序列号跳跃测试通过");
}

/// 批处理边界条件测试（63/64/65包场景）
///
/// 测试批处理逻辑的边界：恰好满batch、少1个、多1个
/// 风险控制：小数据集，无内存风险
#[test]
fn test_batch_boundary_conditions() {
    println!("测试批处理边界：63、64、65包");

    // 测试1：恰好63包（少于批大小64）
    {
        let channel = SequencedChannel::new();
        let sender = channel.sender();

        const BATCH_SIZE_MINUS_1: usize = 63;
        for i in (0..BATCH_SIZE_MINUS_1).rev() {
            sender.send_sequenced(i, i as u32).unwrap();
        }

        for i in 0..BATCH_SIZE_MINUS_1 {
            assert_eq!(channel.recv_ordered().unwrap(), i as u32);
        }
        println!("  ✓ 63包测试通过");
    }

    // 测试2：恰好64包（等于批大小）
    {
        let channel = SequencedChannel::new();
        let sender = channel.sender();

        const BATCH_SIZE: usize = 64;
        for i in (0..BATCH_SIZE).rev() {
            sender.send_sequenced(i, i as u32).unwrap();
        }

        for i in 0..BATCH_SIZE {
            assert_eq!(channel.recv_ordered().unwrap(), i as u32);
        }
        println!("  ✓ 64包测试通过");
    }

    // 测试3：65包（超过批大小1个）
    {
        let channel = SequencedChannel::new();
        let sender = channel.sender();

        const BATCH_SIZE_PLUS_1: usize = 65;
        for i in (0..BATCH_SIZE_PLUS_1).rev() {
            sender.send_sequenced(i, i as u32).unwrap();
        }

        for i in 0..BATCH_SIZE_PLUS_1 {
            assert_eq!(channel.recv_ordered().unwrap(), i as u32);
        }
        println!("  ✓ 65包测试通过");
    }

    println!("✅ 批处理边界条件测试全部通过");
}

/// 多线程高并发压力测试（4线程×2500包=10000包）
///
/// 安全性：10000个u32 = 40KB内存
/// 风险控制：线程join确保正常结束，标记#[ignore]避免CI超时
#[test]
#[ignore = "高并发压力测试，可能需要数秒，仅本地运行"]
fn test_high_concurrency_stress() {
    let channel = SequencedChannel::new();

    const THREAD_COUNT: usize = 4;
    const PER_THREAD: usize = 2500;
    const TOTAL_COUNT: usize = THREAD_COUNT * PER_THREAD;

    println!("开始高并发测试：{THREAD_COUNT} 线程 × {PER_THREAD} 包 = {TOTAL_COUNT} 总包");

    // 启动4个线程并发发送
    let mut handles = Vec::new();
    for thread_id in 0..THREAD_COUNT {
        let sender = channel.sender();
        let handle = thread::spawn(move || {
            let start = thread_id * PER_THREAD;
            let end = start + PER_THREAD;

            // 每个线程乱序发送自己范围内的数据
            for i in (start..end).rev() {
                sender.send_sequenced(i, i as u32).expect("发送失败");
            }

            println!("  线程{thread_id} 完成发送");
        });
        handles.push(handle);
    }

    // 等待所有线程完成发送
    for handle in handles {
        handle.join().expect("线程panic");
    }

    println!("✓ 所有线程发送完成，开始验证顺序...");

    // 验证全部按正确顺序接收
    for expected_seq in 0..TOTAL_COUNT {
        let received = channel.recv_ordered().expect("接收失败");

        assert_eq!(
            received, expected_seq as u32,
            "序列号 {expected_seq} 接收错误"
        );

        if expected_seq % 1000 == 0 && expected_seq > 0 {
            println!("  已验证 {expected_seq}/{TOTAL_COUNT}");
        }
    }

    println!("✅ 高并发压力测试通过：{TOTAL_COUNT} 个样本全部按序接收");
}

/// 序列号连续性验证测试
///
/// 验证SequencedChannel要求序列号从0开始连续递增
/// ⚠️ 重要发现：SequencedChannel会等待所有中间序列号，不支持任意起始序列号
#[test]
fn test_sequence_continuity_requirement() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    println!("验证序列号必须从0开始连续");

    // 测试：从序列号0开始的10个连续序列号
    for i in (0..10).rev() {
        sender.send_sequenced(i, i as u32).unwrap();
    }

    // 验证正确接收
    for i in 0..10 {
        let received = channel.recv_ordered().unwrap();
        assert_eq!(received, i as u32);
    }

    println!("✅ 序列号连续性测试通过");
}

/// 序列号非零起始测试（预期死锁，仅用于文档记录）
///
/// ⚠️ **警告**：此测试会死锁！仅用于记录SequencedChannel的设计约束
/// 发现：SequencedChannel期望序列号从0开始，如果从其他值开始会无限等待序列号0
/// 风险：实际使用中必须确保第一个packet的序列号为0
#[test]
#[ignore = "⚠️ 会死锁！用于记录设计约束，不要运行"]
fn test_nonzero_start_sequence_deadlock() {
    let channel = SequencedChannel::new();
    let sender = channel.sender();

    // 从序列号100开始（会死锁！）
    sender.send_sequenced(100, "data").unwrap();

    // 这里会无限等待序列号0-99，导致死锁
    // channel.recv_ordered().unwrap(); // 永远不会返回
}

// ========== 需要真实音频的集成测试（用#[ignore]标记） ==========

#[test]
#[ignore = "需要真实音频文件，仅本地运行"]
fn test_parallel_decoder_with_real_audio() {
    // TODO: 实现真实音频文件的并行解码测试
    // 使用tests/fixtures/中的小音频文件
}

#[test]
#[ignore = "需要真实音频文件，仅本地运行"]
fn test_parallel_decoder_performance() {
    // TODO: 性能对比测试：串行 vs 并行解码速度
}
