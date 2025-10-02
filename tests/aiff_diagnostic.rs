//! AIFF格式诊断测试

use macinmeter_dr_tool::audio::UniversalDecoder;
use std::path::PathBuf;

#[test]
fn test_aiff_decoding() {
    let aiff_file = PathBuf::from("audio/test_compatibility.aiff");

    if !aiff_file.exists() {
        eprintln!("AIFF文件不存在");
        return;
    }

    println!("=== 测试AIFF串行解码 ===");

    // 使用串行解码器
    let decoder = UniversalDecoder;
    let mut serial_decoder = decoder
        .create_streaming(&aiff_file)
        .expect("创建解码器失败");

    let mut total_samples = 0;
    let mut chunk_count = 0;
    let mut first_samples = Vec::new();

    while let Some(chunk) = serial_decoder.next_chunk().expect("解码失败") {
        chunk_count += 1;
        total_samples += chunk.len();

        if chunk_count == 1 && chunk.len() >= 10 {
            first_samples = chunk[0..10].to_vec();
        }

        if chunk_count <= 3 {
            println!("Chunk {chunk_count}: {} 样本", chunk.len());
            if !chunk.is_empty() {
                println!("  前6个样本: {:?}", &chunk[0..chunk.len().min(6)]);
            }
        }
    }

    println!("\n总计: {chunk_count} 个chunk, {total_samples} 个样本");
    if !first_samples.is_empty() {
        println!("第一个chunk的前10个样本: {first_samples:?}");
    }

    assert!(total_samples > 0, "AIFF串行解码失败：样本数为0");
    assert!(chunk_count > 0, "AIFF串行解码失败：chunk数为0");
}

#[test]
fn test_aiff_parallel_decoding() {
    let aiff_file = PathBuf::from("audio/test_compatibility.aiff");

    if !aiff_file.exists() {
        eprintln!("AIFF文件不存在");
        return;
    }

    println!("\n=== 测试AIFF并行解码 ===");

    // 使用并行解码器
    let decoder = UniversalDecoder;
    let mut parallel_decoder = decoder
        .create_streaming_parallel(
            &aiff_file,
            true,     // parallel_enabled
            Some(64), // batch_size
            Some(4),  // thread_count
        )
        .expect("创建并行解码器失败");

    let mut total_samples = 0;
    let mut chunk_count = 0;
    let mut first_samples = Vec::new();

    while let Some(chunk) = parallel_decoder.next_chunk().expect("并行解码失败") {
        chunk_count += 1;
        total_samples += chunk.len();

        if chunk_count == 1 && chunk.len() >= 10 {
            first_samples = chunk[0..10].to_vec();
        }

        if chunk_count <= 3 {
            println!("Chunk {chunk_count}: {} 样本", chunk.len());
            if !chunk.is_empty() {
                println!("  前6个样本: {:?}", &chunk[0..chunk.len().min(6)]);
            }
        }
    }

    println!("\n总计: {chunk_count} 个chunk, {total_samples} 个样本");
    if !first_samples.is_empty() {
        println!("第一个chunk的前10个样本: {first_samples:?}");
    }

    assert!(total_samples > 0, "AIFF并行解码失败：样本数为0");
    assert!(chunk_count > 0, "AIFF并行解码失败：chunk数为0");
}
