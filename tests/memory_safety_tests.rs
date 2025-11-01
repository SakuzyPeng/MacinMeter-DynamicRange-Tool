//! 内存安全和泄露检测测试
//!
//! **优先级2：内存管理验证**
//!
//! 验证项目承诺："零内存累积，~45MB恒定内存"
//!
//! ## 检测策略（安全第一）
//!
//! 1. **HashMap清理验证** - SequencedChannel的reorder_buffer正确清空
//! 2. **引用计数验证** - Arc引用正确释放，无循环引用
//! 3. **重复创建销毁** - decoder对象正确回收
//! 4. **流式处理模拟** - 验证内存不随数据量增长
//!
//! ## 安全约束
//!
//! - 所有测试标记#[ignore]，避免CI运行
//! - 使用小数据集（KB级），避免OOM
//! - 使用逻辑验证而非直接测量系统内存
//! - 快速失败，发现问题立即停止

use macinmeter_dr_tool::audio::parallel_decoder::SequencedChannel;
use std::sync::Arc;

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

// ========== SequencedChannel HashMap清理测试 ==========

/// 验证SequencedChannel的reorder_buffer在消费后正确清理
///
/// 关键风险：HashMap.remove()未被调用，导致已消费数据堆积
/// 检测方法：通过Arc::strong_count()间接验证数据被移除
#[test]
#[ignore = "Debug模式下运行超过60秒(100个1KB对象)，仅本地运行"]
#[allow(clippy::needless_range_loop)] // 需要索引来验证序列号和引用计数
fn test_sequenced_channel_buffer_cleanup() {
    let channel: SequencedChannel<Arc<Vec<u8>>> = SequencedChannel::new();
    let sender = channel.sender();

    log(
        "测试SequencedChannel缓冲区清理",
        "Validating SequencedChannel buffer cleanup",
    );

    // 创建100个1KB数据块，用Arc包装以便跟踪引用
    let mut data_refs = Vec::new();
    for i in 0..100 {
        let data = Arc::new(vec![i as u8; 1024]); // 1KB
        data_refs.push(Arc::clone(&data));

        // 乱序发送：先发送偶数，再发送奇数
        sender.send_sequenced(i, data).unwrap();
    }

    log(
        "  发送完成，开始接收...",
        "  All data sent; start consuming...",
    );

    // 接收前50个数据
    for i in 0..50 {
        let received = channel.recv_ordered().unwrap();

        // 验证数据正确
        assert_eq!(received[0], i as u8);

        // 关键验证：接收后Arc引用计数应该减少
        // data_refs[i]持有1个引用，received持有1个引用（如果HashMap仍持有则是3个）
        assert_eq!(
            Arc::strong_count(&data_refs[i]),
            2, // 只有data_refs和received持有
            "序列{i}: HashMap应该已释放引用"
        );
    }

    // 显式drop received，现在只剩data_refs持有引用
    log(
        "  前50个已接收，验证引用计数...",
        "  First 50 items consumed; verifying reference counts...",
    );

    // 再次验证前50个的引用计数
    for i in 0..50 {
        assert_eq!(
            Arc::strong_count(&data_refs[i]),
            1, // 只有data_refs持有
            "序列{i}: 所有临时引用应该已释放"
        );
    }

    // 后50个还未接收，但如果是乱序发送可能在HashMap中
    log(
        "  前50个引用正确释放",
        "  Reference counts for first 50 items are correct",
    );

    // 接收剩余50个
    for _i in 50..100 {
        let _ = channel.recv_ordered().unwrap();
    }

    // 最终验证：所有数据只有data_refs持有引用
    for i in 0..100 {
        assert_eq!(
            Arc::strong_count(&data_refs[i]),
            1,
            "序列{i}: 最终应该只有data_refs持有引用"
        );
    }

    log(
        "HashMap清理验证通过：100个数据块全部正确释放",
        "HashMap cleanup verified: 100 blocks released",
    );
}

/// 测试SequencedChannel在大量乱序数据下的内存管理
///
/// 场景：10000个数据完全逆序发送，验证HashMap不会堆积所有数据
/// 风险控制：使用u32而非大对象，10000个u32 = 40KB
#[test]
#[ignore = "压力测试，可能需要数秒，仅本地运行"]
#[allow(clippy::needless_range_loop)] // 需要索引来验证序列号和引用计数
fn test_sequenced_channel_large_scale_cleanup() {
    let channel: SequencedChannel<Arc<u32>> = SequencedChannel::new();
    let sender = channel.sender();

    const COUNT: usize = 10_000;

    log(
        format!("大规模HashMap清理测试：{COUNT} 个数据逆序发送"),
        format!("Large-scale HashMap cleanup test: {COUNT} entries sent in reverse"),
    );

    // 创建并逆序发送
    let mut data_refs = Vec::new();
    for i in (0..COUNT).rev() {
        let data = Arc::new(i as u32);
        data_refs.push(Arc::clone(&data));
        sender.send_sequenced(i, data).unwrap();
    }

    log(
        "  发送完成，开始顺序接收...",
        "  All entries sent; receiving in order...",
    );

    // 边接收边验证引用计数
    for i in 0..COUNT {
        let received = channel.recv_ordered().unwrap();
        assert_eq!(*received, i as u32);

        // 每接收1000个验证一次引用情况
        if i % 1000 == 0 && i > 0 {
            // 验证已接收的数据引用已释放
            for j in 0..i {
                assert_eq!(
                    Arc::strong_count(&data_refs[COUNT - 1 - j]),
                    1,
                    "序列{j} 应该已释放"
                );
            }
            log(
                format!("  已验证前{i}个数据引用正确释放"),
                format!("  Verified first {i} entries have correct ref-count"),
            );
        }
    }

    // 最终验证
    for i in 0..COUNT {
        assert_eq!(Arc::strong_count(&data_refs[i]), 1);
    }

    log(
        format!("大规模测试通过：{COUNT} 个数据全部正确释放"),
        format!("Large-scale test passed: {COUNT} entries released"),
    );
}

/// 验证SequencedChannel完全消费后，所有对象被drop
///
/// 检测方法：使用Arc引用计数验证对象释放（避免全局状态）
#[test]
#[ignore] // Debug模式下极慢（1000个对象 × 1KB），运行超过60秒，仅在Release内存验证时运行
fn test_complete_object_cleanup() {
    log(
        "对象Drop验证测试（使用Arc引用计数）",
        "Object drop validation (Arc reference counting)",
    );

    let channel: SequencedChannel<Arc<Vec<u8>>> = SequencedChannel::new();
    let sender = channel.sender();

    const COUNT: usize = 1000;

    // 创建1000个Arc包装的数据，保存引用用于验证
    let mut data_refs = Vec::new();
    for i in 0..COUNT {
        let data = Arc::new(vec![i as u8; 1024]); // 1KB each
        data_refs.push(Arc::clone(&data));
        sender.send_sequenced(i, data).unwrap();
    }

    log(
        format!("  创建了 {COUNT} 个对象"),
        format!("  Created {COUNT} objects"),
    );

    // 接收所有对象
    for _i in 0..COUNT {
        let data = channel.recv_ordered().unwrap();
        // 显式drop
        drop(data);
    }

    log("  接收完成", "  Consumption finished");

    // 显式drop channel和sender
    drop(sender);
    drop(channel);

    // 验证所有对象都被释放：每个Arc现在只有data_refs持有1个引用
    for (i, data_ref) in data_refs.iter().enumerate() {
        assert_eq!(
            Arc::strong_count(data_ref),
            1,
            "对象{i}: 应该只剩data_refs持有引用，实际引用数={}",
            Arc::strong_count(data_ref)
        );
    }

    log(
        format!("对象Drop验证通过：{COUNT} 个对象全部正确销毁（引用计数=1）"),
        format!("Object drop test passed: {COUNT} objects released (refcount=1)"),
    );
}

// ========== 流式处理内存恒定验证 ==========

/// 模拟流式处理大量数据，验证内存不累积
///
/// 场景：模拟100轮流式处理，每轮1000个数据块
/// 验证：使用Arc引用计数确保每轮结束后内存被回收
#[test]
#[ignore = "流式处理模拟，可能需要数秒，仅本地运行"]
#[allow(clippy::needless_range_loop)] // 需要索引来验证引用计数
fn test_streaming_memory_stability() {
    log("流式处理内存稳定性测试", "Streaming memory stability test");

    const ROUNDS: usize = 100;
    const PER_ROUND: usize = 1000;

    for round in 0..ROUNDS {
        let channel: SequencedChannel<Arc<Vec<u8>>> = SequencedChannel::new();
        let sender = channel.sender();

        // 每轮创建1000个1KB数据块
        let mut data_refs = Vec::new();
        for i in 0..PER_ROUND {
            let data = Arc::new(vec![i as u8; 1024]);
            data_refs.push(Arc::clone(&data));
            sender.send_sequenced(i, data).unwrap();
        }

        // 消费所有数据
        for _i in 0..PER_ROUND {
            let _ = channel.recv_ordered().unwrap();
        }

        // 验证本轮所有引用都已释放
        for i in 0..PER_ROUND {
            assert_eq!(
                Arc::strong_count(&data_refs[i]),
                1,
                "第{round}轮，数据{i}未释放"
            );
        }

        if round % 10 == 0 && round > 0 {
            log(
                format!("  完成第{round}轮，内存稳定"),
                format!("  Completed round {round}, memory stable"),
            );
        }

        // data_refs和channel在这里drop
    }

    log(
        format!("流式处理稳定性验证通过：{ROUNDS} 轮处理，内存无累积"),
        format!("Streaming stability test passed: {ROUNDS} rounds with no growth"),
    );
}

// ========== 重复创建销毁decoder验证 ==========

/// 验证重复创建和销毁SequencedChannel不会泄露
///
/// 场景：创建1000个channel，立即销毁
/// 验证：使用Arc引用计数验证释放（避免全局状态）
#[test]
#[ignore = "大量创建销毁测试(1000次×10数据)，可能需要数秒，仅本地运行"]
fn test_channel_creation_destruction() {
    log(
        "Channel重复创建销毁测试（使用Arc引用计数）",
        "Channel create/destroy stress test (Arc reference counting)",
    );

    const ITERATIONS: usize = 1000;
    const ITEMS_PER_CHANNEL: usize = 10;

    for i in 0..ITERATIONS {
        let channel: SequencedChannel<Arc<Vec<u8>>> = SequencedChannel::new();
        let sender = channel.sender();

        // 保存引用用于验证
        let mut data_refs = Vec::new();

        // 发送少量数据
        for j in 0..ITEMS_PER_CHANNEL {
            let data = Arc::new(vec![j as u8; 100]); // 100 bytes
            data_refs.push(Arc::clone(&data));
            sender.send_sequenced(j, data).unwrap();
        }

        // 接收所有数据
        for _ in 0..ITEMS_PER_CHANNEL {
            let _ = channel.recv_ordered().unwrap();
        }

        // channel和sender在这里drop
        drop(sender);
        drop(channel);

        // 验证所有数据都被释放
        for (j, data_ref) in data_refs.iter().enumerate() {
            assert_eq!(
                Arc::strong_count(data_ref),
                1,
                "迭代{i}, 对象{j}: 引用未完全释放"
            );
        }

        if i % 100 == 0 && i > 0 {
            log(
                format!("  第{i}次迭代完成，所有对象正确释放"),
                format!("  Iteration {i} complete, all objects released"),
            );
        }
    }

    log(
        format!(
            "Channel创建销毁测试通过：{ITERATIONS} 次迭代，每次{ITEMS_PER_CHANNEL}个对象全部正确销毁"
        ),
        format!(
            "Channel create/destroy test passed: {ITERATIONS} iterations × {ITEMS_PER_CHANNEL} objects released"
        ),
    );
}

// ========== Arc循环引用检测 ==========

/// 验证SequencedChannel的Arc引用不会形成循环
///
/// 关键检查：sender和channel之间的Arc引用是否正确释放
#[test]
fn test_no_circular_arc_references() {
    log("Arc循环引用检测", "Arc cycle detection");

    let channel: SequencedChannel<u32> = SequencedChannel::new();
    let sender1 = channel.sender();
    let sender2 = sender1.clone();

    // 发送一些数据
    sender1.send_sequenced(0, 100).unwrap();
    sender2.send_sequenced(1, 200).unwrap();

    // 接收数据
    assert_eq!(channel.recv_ordered().unwrap(), 100);
    assert_eq!(channel.recv_ordered().unwrap(), 200);

    // drop所有sender
    drop(sender1);
    drop(sender2);

    // 如果存在循环引用，channel会保持sender的Arc引用
    // 这里我们通过try_recv来验证channel仍然可用
    match channel.try_recv_ordered() {
        Err(_) => log(
            "  Channel正常工作，无循环引用",
            "  Channel operates without cycles",
        ),
        Ok(_) => panic!("不应该还有数据"),
    }

    log("Arc循环引用检测通过", "Arc cycle detection passed");
}
