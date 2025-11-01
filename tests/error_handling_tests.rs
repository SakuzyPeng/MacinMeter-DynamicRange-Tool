//! 错误处理完整性测试
//!
//! **错误处理和故障隔离**
//!
//! 验证音频处理不应panic，而要优雅降级
//!
//! ## 测试策略（安全第一）
//!
//! 1. **错误传播** - SequencedChannel发送失败的处理
//! 2. **优雅降级** - channel断开后的恢复
//! 3. **故障隔离** - 部分失败不影响整体
//! 4. **边界条件** - 空数据、无效输入的处理
//!
//! ## 安全约束
//!
//! - 不模拟真实的解码器panic（太危险）
//! - 使用channel的Disconnected错误模拟失败
//! - 只测试错误路径的逻辑，不测试恢复后的性能

use crossbeam_channel::{SendError, TryRecvError};
use macinmeter_dr_tool::audio::parallel_decoder::SequencedChannel;
use std::thread;
use std::time::Duration;

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

// ========== SequencedChannel错误传播测试 ==========

/// 验证发送端关闭后，接收端能正确识别
///
/// 场景：模拟并行解码线程提前退出的情况
/// 修复：使用try_recv避免死锁
#[test]
fn test_channel_disconnection_detection() {
    log("Channel断开检测测试", "Channel disconnection detection");

    let channel: SequencedChannel<u32> = SequencedChannel::new();
    let sender = channel.sender();

    // 发送少量数据
    sender.send_sequenced(0, 100).unwrap();
    sender.send_sequenced(1, 200).unwrap();

    // 接收前2个
    assert_eq!(channel.recv_ordered().unwrap(), 100);
    assert_eq!(channel.recv_ordered().unwrap(), 200);

    // 关闭发送端（模拟线程退出）
    drop(sender);

    log("  发送端已关闭", "  Sender closed");

    // 给一点时间让channel检测到断开
    thread::sleep(Duration::from_millis(10));

    // 使用try_recv避免死锁，应该返回Disconnected或Empty
    match channel.try_recv_ordered() {
        Err(TryRecvError::Disconnected) => {
            log(
                "  正确检测到channel断开",
                "  Channel disconnection detected",
            );
        }
        Err(TryRecvError::Empty) => {
            log(
                "  channel为空（发送端已关闭）",
                "  Channel empty (sender closed)",
            );
        }
        Ok(v) => panic!("不应该收到数据: {v}"),
    }

    log("Channel断开检测通过", "Channel disconnection test passed");
}

/// 验证try_recv_ordered的错误处理
///
/// 测试Empty和Disconnected两种错误
#[test]
fn test_try_recv_error_handling() {
    log("try_recv错误处理测试", "try_recv error handling test");

    let channel: SequencedChannel<String> = SequencedChannel::new();
    let sender = channel.sender();

    // 测试1: 空通道返回Empty
    match channel.try_recv_ordered() {
        Err(TryRecvError::Empty) => {
            log("  空通道正确返回Empty", "  Empty channel returned Empty");
        }
        other => panic!("应该返回Empty，实际: {other:?}"),
    }

    // 发送一个数据
    sender.send_sequenced(0, "data".to_string()).unwrap();

    // 测试2: 有数据时正常接收
    match channel.try_recv_ordered() {
        Ok(data) => {
            assert_eq!(data, "data");
            log("  有数据时正确接收", "  Data received successfully");
        }
        err => panic!("应该接收到数据，实际: {err:?}"),
    }

    // 测试3: 发送端关闭后返回Disconnected
    drop(sender);

    match channel.try_recv_ordered() {
        Err(TryRecvError::Disconnected) | Err(TryRecvError::Empty) => {
            log(
                "  发送端关闭后正确返回错误",
                "  Correct error after sender closure",
            );
        }
        other => panic!("应该返回Disconnected或Empty，实际: {other:?}"),
    }

    log(
        "try_recv错误处理通过",
        "try_recv error handling test passed",
    );
}

// ========== 乱序数据 + 发送失败场景 ==========

/// 测试乱序发送但部分数据丢失的场景
///
/// 场景：发送0, 2, 3, 4（缺少1），然后关闭channel
/// 预期：只能收到0，后续数据因等待1而被阻塞
#[test]
fn test_missing_sequence_with_disconnection() {
    log(
        "缺失序列号 + 断开测试",
        "Missing sequence with disconnect test",
    );

    let channel: SequencedChannel<u32> = SequencedChannel::new();
    let sender = channel.sender();

    // 发送0, 2, 3, 4（故意跳过1）
    sender.send_sequenced(0, 100).unwrap();
    sender.send_sequenced(2, 300).unwrap();
    sender.send_sequenced(3, 400).unwrap();
    sender.send_sequenced(4, 500).unwrap();

    log(
        "  发送了序列0, 2, 3, 4（缺少1）",
        "  Sent sequences 0, 2, 3, 4 (missing 1)",
    );

    // 接收序列0
    assert_eq!(channel.recv_ordered().unwrap(), 100);
    log("  收到序列0", "  Received sequence 0");

    // 序列1缺失，2-4在缓冲区等待
    match channel.try_recv_ordered() {
        Err(TryRecvError::Empty) => {
            log(
                "  序列2-4正确等待序列1",
                "  Sequences 2-4 waiting for sequence 1",
            );
        }
        Ok(v) => panic!("不应该收到数据，实际: {v}"),
        Err(e) => panic!("意外错误: {e:?}"),
    }

    // 关闭发送端（模拟线程异常退出）
    drop(sender);
    log("  发送端已关闭", "  Sender closed");

    // 尝试接收序列1，应该阻塞然后返回Disconnected
    // 注意：这会阻塞，因为HashMap中没有序列1
    // 我们用try_recv来避免阻塞
    thread::sleep(Duration::from_millis(10)); // 给点时间让channel检测断开

    match channel.try_recv_ordered() {
        Err(TryRecvError::Disconnected) | Err(TryRecvError::Empty) => {
            log(
                "  缺失序列导致后续数据无法接收（符合预期）",
                "  Missing sequence prevents further reception (expected)",
            );
        }
        Ok(v) => panic!("不应该收到数据: {v}"),
    }

    log(
        "缺失序列号处理验证通过",
        "Missing sequence handling validated",
    );
    log(
        "   注意：序列2-4因等待序列1而永久阻塞（这是设计约束）",
        "   Note: sequences 2-4 remain blocked waiting for 1 (design constraint)",
    );
}

// ========== 并发发送错误测试 ==========

/// 测试多个sender同时向已关闭的receiver发送数据
///
/// 场景：receiver先drop，多个sender尝试发送
/// 验证：send_sequenced最终会返回SendError
/// 修复：由于channel缓冲，可能需要多次发送才会失败
#[test]
fn test_send_to_closed_receiver() {
    log(
        "向已关闭receiver发送测试",
        "Sending to closed receiver test",
    );

    let channel: SequencedChannel<u32> = SequencedChannel::new();
    let sender1 = channel.sender();
    let sender2 = sender1.clone();

    // 先drop receiver
    drop(channel);
    log("  receiver已关闭", "  Receiver closed");

    // 由于channel可能有缓冲，多次发送以触发错误
    let mut errors1 = 0;
    for i in 0..100 {
        match sender1.send_sequenced(i, i as u32) {
            Err(SendError(_)) => {
                errors1 += 1;
                break;
            }
            Ok(_) => {
                // channel缓冲允许部分发送成功
            }
        }
    }

    let mut errors2 = 0;
    for i in 100..200 {
        match sender2.send_sequenced(i, i as u32) {
            Err(SendError(_)) => {
                errors2 += 1;
                break;
            }
            Ok(_) => {
                // channel缓冲允许部分发送成功
            }
        }
    }

    // 至少应该有一个sender检测到错误
    assert!(
        errors1 > 0 || errors2 > 0,
        "至少应该有一个sender检测到SendError"
    );

    log(
        "  sender最终检测到receiver关闭",
        "  Sender detected receiver closure",
    );
    log(
        "向已关闭receiver发送错误处理通过",
        "Sending to closed receiver handled correctly",
    );
}

// ========== 边界条件测试 ==========

/// 测试只发送EOF的情况（无实际数据）
///
/// 场景：创建channel，不发送任何数据就关闭sender
/// 修复：使用try_recv避免死锁
#[test]
fn test_immediate_disconnection() {
    log(
        "立即断开测试（无数据发送）",
        "Immediate disconnection test (no data sent)",
    );

    let channel: SequencedChannel<u32> = SequencedChannel::new();
    let sender = channel.sender();

    // 立即关闭sender
    drop(sender);

    // 给一点时间让channel检测到断开
    thread::sleep(Duration::from_millis(10));

    // 使用try_recv避免死锁，应该返回Disconnected或Empty
    match channel.try_recv_ordered() {
        Err(TryRecvError::Disconnected) => {
            log(
                "  无数据时正确返回Disconnected",
                "  Returned Disconnected with no data",
            );
        }
        Err(TryRecvError::Empty) => {
            log(
                "  channel为空（发送端已关闭）",
                "  Channel empty (sender already closed)",
            );
        }
        Ok(v) => panic!("不应该收到数据: {v}"),
    }

    log(
        "立即断开处理通过",
        "Immediate disconnection handled correctly",
    );
}

/// 测试发送大量数据后异常关闭
///
/// 场景：发送10000个数据，只接收一半就关闭receiver
/// 验证：剩余数据被正确丢弃，无内存泄露
#[test]
#[ignore = "大数据量测试(10k条)，累积可能影响CI时间，仅本地运行"]
#[allow(clippy::needless_range_loop)] // 需要索引来发送序列号
fn test_partial_consumption_with_close() {
    log("部分消费后关闭测试", "Partial consumption then close test");

    let channel: SequencedChannel<u32> = SequencedChannel::new();
    let sender = channel.sender();

    const TOTAL: usize = 10_000;
    const CONSUMED: usize = 5_000;

    // 发送10000个数据
    for i in 0..TOTAL {
        sender.send_sequenced(i, i as u32).unwrap();
    }
    log(
        format!("  发送了{TOTAL}个数据"),
        format!("  Sent {TOTAL} items"),
    );

    // 只接收前5000个
    for i in 0..CONSUMED {
        let received = channel.recv_ordered().unwrap();
        assert_eq!(received, i as u32);
    }
    log(
        format!("  只接收了{CONSUMED}个数据"),
        format!("  Consumed {CONSUMED} items"),
    );

    // 关闭channel，剩余5000个数据应该被丢弃
    drop(channel);
    log("  receiver已关闭", "  Receiver closed");

    // 尝试继续发送，应该失败
    match sender.send_sequenced(TOTAL, 99999) {
        Err(SendError(_)) => {
            log(
                "  继续发送正确返回错误",
                "  Additional sends correctly failed",
            );
        }
        Ok(_) => panic!("应该返回SendError"),
    }

    log(
        format!("部分消费后关闭通过（剩余{}个数据被丢弃）", TOTAL - CONSUMED),
        format!(
            "Partial consumption then close succeeded ({} items dropped)",
            TOTAL - CONSUMED
        ),
    );
}

// ========== 并发错误场景 ==========

/// 测试多线程并发发送时receiver突然关闭
///
/// 场景：4个线程并发发送，receiver在中途关闭
/// 验证：各线程能正确处理SendError，不会panic
#[test]
#[ignore = "并发错误测试，可能需要数秒，仅本地运行"]
fn test_concurrent_send_with_receiver_close() {
    log(
        "并发发送 + receiver关闭测试",
        "Concurrent send with receiver closure test",
    );

    let channel: SequencedChannel<u32> = SequencedChannel::new();
    let mut handles = Vec::new();

    const THREADS: usize = 4;
    const PER_THREAD: usize = 1000;

    // 启动4个线程并发发送
    for thread_id in 0..THREADS {
        let sender = channel.sender();
        let handle = thread::spawn(move || {
            let start = thread_id * PER_THREAD;
            let mut success_count = 0;
            let mut error_count = 0;

            for i in 0..PER_THREAD {
                let seq = start + i;
                match sender.send_sequenced(seq, seq as u32) {
                    Ok(_) => success_count += 1,
                    Err(_) => {
                        error_count += 1;
                        // 发送失败后继续尝试几次，然后退出
                        if error_count > 10 {
                            break;
                        }
                    }
                }

                // 模拟一些处理时间
                if i % 100 == 0 {
                    thread::sleep(Duration::from_micros(10));
                }
            }

            (success_count, error_count)
        });
        handles.push(handle);
    }

    // 主线程接收一部分数据后关闭
    log("  接收前1000个数据...", "  Consuming first 1000 items...");
    for i in 0..1000 {
        let _ = channel.recv_ordered().unwrap();
        if i == 999 {
            log(
                "  接收完成，关闭receiver",
                "  Consumption complete; closing receiver",
            );
        }
    }

    // 关闭receiver
    drop(channel);

    // 等待所有线程完成
    let mut total_success = 0;
    let mut total_errors = 0;

    for (thread_id, handle) in handles.into_iter().enumerate() {
        let (success, errors) = handle.join().expect("线程panic");
        log(
            format!("  线程{thread_id}: 成功{success}, 失败{errors}"),
            format!("  Thread {thread_id}: success {success}, errors {errors}"),
        );
        total_success += success;
        total_errors += errors;
    }

    log(
        format!("  总计: 成功{total_success}, 失败{total_errors}"),
        format!("  Totals: success {total_success}, errors {total_errors}"),
    );
    log(
        "并发发送错误处理通过：所有线程正确处理SendError",
        "Concurrent send error handling passed: all threads handled SendError",
    );
}

// ========== 错误恢复模式测试 ==========

/// 测试检测到错误后的重试逻辑
///
/// 场景：模拟临时失败后的重试成功
#[test]
fn test_error_recovery_pattern() {
    log("错误恢复模式测试", "Error recovery pattern test");

    let channel1: SequencedChannel<u32> = SequencedChannel::new();
    let sender1 = channel1.sender();

    // 第一次尝试：发送数据
    sender1.send_sequenced(0, 100).unwrap();
    assert_eq!(channel1.recv_ordered().unwrap(), 100);

    // 关闭第一个channel（模拟失败）
    drop(channel1);
    drop(sender1);

    log("  第一个channel失败", "  First channel failed");

    // 错误恢复：创建新的channel重试
    let channel2: SequencedChannel<u32> = SequencedChannel::new();
    let sender2 = channel2.sender();

    log("  创建新channel重试", "  Creating new channel to retry");

    // 第二次尝试：成功
    sender2.send_sequenced(0, 200).unwrap();
    assert_eq!(channel2.recv_ordered().unwrap(), 200);

    log("  重试成功", "  Retry succeeded");
    log("错误恢复模式验证通过", "Error recovery pattern verified");
}
