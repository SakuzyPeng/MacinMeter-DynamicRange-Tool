#!/usr/bin/env python3

import os
import subprocess
import tempfile
import time
import shutil
from pathlib import Path

SAMPLES_DIR = Path("/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/benchmark_samples")
EXE_PATH = Path("/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr")

def extract_metric(output, metric_name):
    """从输出中提取指标"""
    for line in output.split('\n'):
        if metric_name in line:
            # 尝试提取数字
            import re
            match = re.search(r'[\d.]+', line.replace(',', ''))
            if match:
                return float(match.group())
    return None

def run_benchmark(sample_path, serial=False):
    """运行单个基准测试"""
    with tempfile.TemporaryDirectory() as tmpdir:
        # 复制文件
        dest = Path(tmpdir) / sample_path.name
        shutil.copy2(sample_path, dest)
        
        # 构建命令
        cmd = [str(EXE_PATH), tmpdir]
        if serial:
            cmd.append("--serial")
        
        try:
            output = subprocess.check_output(cmd, stderr=subprocess.STDOUT, text=True, timeout=300)
            
            # 提取时间和速度
            time_sec = extract_metric(output, "运行时间")
            speed_mbs = extract_metric(output, "处理速度")
            
            return {
                'time': time_sec,
                'speed': speed_mbs,
                'output': output[:500]  # 前500字用于调试
            }
        except subprocess.TimeoutExpired:
            return {'error': 'Timeout'}
        except Exception as e:
            return {'error': str(e)}

def main():
    print("🎯 串行 vs 并行性能对比基准测试")
    print("=" * 70)
    
    samples = sorted(SAMPLES_DIR.glob("*.flac"))
    results = []
    
    print(f"📁 找到 {len(samples)} 个样本文件")
    print("")
    
    for idx, sample in enumerate(samples, 1):
        filesize_mb = sample.stat().st_size // (1024 * 1024)
        print(f"[{idx}/{len(samples)}] 📊 测试: {sample.name} ({filesize_mb}MB)")
        print("─" * 70)
        
        # 串行模式
        print(f"  ⏳ 串行模式 ... ", end='', flush=True)
        serial_result = run_benchmark(sample, serial=True)
        if 'error' in serial_result:
            print(f"❌ 失败: {serial_result['error']}")
            continue
        else:
            print(f"✓ ({serial_result['time']:.2f}s, {serial_result['speed']:.2f}MB/s)")
        
        time.sleep(2)
        
        # 并行模式
        print(f"  ⚡ 并行模式 ... ", end='', flush=True)
        parallel_result = run_benchmark(sample, serial=False)
        if 'error' in parallel_result:
            print(f"❌ 失败: {parallel_result['error']}")
            continue
        else:
            print(f"✓ ({parallel_result['time']:.2f}s, {parallel_result['speed']:.2f}MB/s)")
        
        # 计算加速比
        speedup = serial_result['time'] / parallel_result['time'] if parallel_result['time'] else 0
        print(f"  📈 加速比: {speedup:.2f}x")
        print("")
        
        results.append({
            'name': sample.name,
            'size_mb': filesize_mb,
            'serial_time': serial_result['time'],
            'parallel_time': parallel_result['time'],
            'serial_speed': serial_result['speed'],
            'parallel_speed': parallel_result['speed'],
            'speedup': speedup
        })
    
    # 输出结果表
    print("=" * 70)
    print("📊 完整性能对比表")
    print("")
    print(f"{'文件名':<28} | {'大小':<6} | {'串行':<9} | {'并行':<9} | {'加速比':<8}")
    print("─" * 70)
    
    for r in results:
        print(f"{r['name']:<28} | {r['size_mb']:>5}M | {r['serial_time']:>7.2f}s | {r['parallel_time']:>7.2f}s | {r['speedup']:>6.2f}x")
    
    # 性能分析
    print("")
    print("=" * 70)
    print("📈 性能分析:")
    print("")
    
    small = [r for r in results if r['size_mb'] < 100]
    medium = [r for r in results if 100 <= r['size_mb'] < 400]
    large = [r for r in results if r['size_mb'] >= 400]
    
    if small:
        avg_speedup_small = sum(r['speedup'] for r in small) / len(small)
        print(f"  小文件 (<100MB): {len(small)} 个文件, 平均加速比: {avg_speedup_small:.2f}x")
    
    if medium:
        avg_speedup_medium = sum(r['speedup'] for r in medium) / len(medium)
        print(f"  中等文件 (100-400MB): {len(medium)} 个文件, 平均加速比: {avg_speedup_medium:.2f}x")
    
    if large:
        avg_speedup_large = sum(r['speedup'] for r in large) / len(large)
        print(f"  大文件 (>400MB): {len(large)} 个文件, 平均加速比: {avg_speedup_large:.2f}x")
    
    print("")
    print("✅ 测试完成！")

if __name__ == '__main__':
    main()
