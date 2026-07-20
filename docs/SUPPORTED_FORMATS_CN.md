[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# 0.2.0 稳定音频格式

MacinMeter 0.2.0 有意只公开一小块以正确性为先的解码面。列为可用表示该路径进入
0.2.0 稳定契约，不表示它已经与 foo_dr_meter 1.0.8 兼容。当前分析 profile 仍是
`Unverified` 的 Candidate。

| 容器 | 接受的编码 | 送入分析器的 PCM |
|---|---|---|
| 经典 RIFF/WAVE | 8/16/24/32-bit 线性整数 PCM | 有限、交错的 `f64` |
| 经典 RIFF/WAVE | IEEE 32/64-bit 浮点 PCM | 有限、交错的 `f64` |
| FLAC | FLAC | 有限、交错的 `f64` |
| AIFF | 8/16/24/32-bit 线性整数 PCM | 有限、交错的 `f64` |

固定的 x64 1.0.8 参考核心也接收 `f64`。产品 PCM 主链改为 `f64` 后，当前
safe-master corpus 暴露的两处 source-f64 偏差已关闭，公开可比核心字段达到
track DR 39/39、channel DR 62/62。这不证明所有宿主、容器、运行库边界或输入的
decoder 归一化已经逐位一致。

解码器不使用扩展名 hint，而是直接探测文件内容。`.wav`、`.wave`、`.flac`、
`.aif`、`.aiff` 只用于目录发现。显式传入的文件即使扩展名不同，只要内容受支持
也可以打开。

仓库提交的
[`native-pcm-v1`](../tests/fixtures/native-pcm-v1/README.md) 产品 corpus 以独立
raw-bit 归一化 oracle 固定了每一个已声明 PCM 位深；其中 FLAC 为 stereo、
multi-block，AIFF/FLAC 还通过 Rust API 与 CLI 的共享 report 边界。这些是产品
契约 fixture，不是参考插件 golden。

## 0.2.0 明确不可用

0.2.0 不包含：

- WAVE_FORMAT_EXTENSIBLE、AIFC、压缩 WAV 变体以及受支持容器内的其他编码；
- MP1/MP2/MP3、AAC、ALAC、Vorbis、Opus、AC-3、E-AC-3、DTS、DSD；
- MP4/M4A、Ogg、Matroska/WebM、DSF、DFF 容器；
- FFmpeg 回退或任何外部解码进程；
- 重采样、增益、滤波、边缘裁切和静音预处理；
- 包级并行或文件级并行解码。

稳定 AIFF 路径还要求 80-bit sample rate 为有限、正数、可由 `u32` 精确表示的
整数、COMM chunk 恰好为 18 bytes，且 SSND offset/block-size 均为零。稳定 FLAC
路径要求 STREAMINFO 声明非零总样本数：缺少该声明时流末帧数核对失效，若 MD5
签名同样缺失，整帧尾部丢失将原理上不可检测。产品会拒绝这些尚未毕业的容器
变体，而不会让 backend 静默舍入或自行扩张支持面。

能够识别但不属于 0.2.0 稳定面的内容返回稳定错误码 `unsupported_format`。受支持格式内可
检测的损坏内容返回探测或解码错误，不会变成空的或部分成功的报告。物理 EOF 只能
依据声明的 frame 数或 codec 完整性证据核对；输入同时缺失两者时，解码器不声称能
识别每一种恰好落在完整 frame 边界上的尾部截断。

## 解码契约

打开后的 PCM stream 信息不可动态改变。`read_block` 只会返回非空、有限、完整
frame 对齐的 block，sticky EOF，或结构化错误；空等候和解码失败不能伪装成 EOF。
预期 frame 与已解码 frame 分开记录，稳定路径遇到坏包会失败，不会静默跳过后生成结果。

产品分析最多接受 64 个 PCM 声道。这是资源契约，不表示每一种当前容器或 backend
都能表达 64 声道；格式自身的上限可能更低。声明超过 64 声道的源会在创建 decoder
之前，于探测阶段以 `unsupported_format` 拒绝。

系统不会根据声道数猜布局；backend 无法确认布局时报告 `unknown`。Candidate V1
只生成一个 `track` 聚合，并按照当前证据支持的候选规则纳入 LFE，而不再生成单独的
`without_lfe` 结果。静音声道仍明确显示为 `silent`，并以 DR0 参与聚合；只有数据
不足的声道会被排除。如果没有声道可以参与，aggregate 的 DR 字段使用显式 `null`。
