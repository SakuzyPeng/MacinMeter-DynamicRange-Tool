[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# M0 支持的音频格式

MacinMeter 0.2.0 有意只公开一小块以正确性为先的解码面。列为可用表示该路径进入
M0 契约，不表示它已经与参考 DR 插件兼容。

| 容器 | 接受的编码 | 送入分析器的 PCM |
|---|---|---|
| WAV / WAVE | 8/16/24/32-bit 线性整数 PCM | 有限、交错的 `f32` |
| WAV / WAVE | IEEE 32/64-bit 浮点 PCM | 有限、交错的 `f32` |
| FLAC | FLAC | 有限、交错的 `f32` |
| AIFF | 8/16/24/32-bit 线性整数 PCM | 有限、交错的 `f32` |

解码器不使用扩展名 hint，而是直接探测文件内容。`.wav`、`.wave`、`.flac`、
`.aif`、`.aiff` 只用于目录发现。显式传入的文件即使扩展名不同，只要内容受支持
也可以打开。

## M0 明确不可用

0.2.0 不包含：

- AIFC、压缩 WAV 变体以及受支持容器内的其他编码；
- MP1/MP2/MP3、AAC、ALAC、Vorbis、Opus、AC-3、E-AC-3、DTS、DSD；
- MP4/M4A、Ogg、Matroska/WebM、DSF、DFF 容器；
- FFmpeg 回退或任何外部解码进程；
- 重采样、增益、滤波、边缘裁切和静音预处理；
- 包级并行或文件级并行解码。

能够识别但不属于 M0 的内容返回稳定错误码 `unsupported_format`。受支持格式内的
损坏内容返回探测或解码错误，不会变成空的或部分成功的报告。

## 解码契约

打开后的 PCM stream 信息不可动态改变。`read_block` 只会返回非空、有限、完整
frame 对齐的 block，sticky EOF，或结构化错误；空等候和解码失败不能伪装成 EOF。
预期 frame 与已解码 frame 分开记录，M0 遇到坏包会失败，不会静默跳过后生成结果。

系统不会根据声道数猜布局。backend 无法确认布局时报告 `unknown`，因此也不会生成
`without_lfe` 聚合。即使没有可测声道，aggregate 对象仍保留全部排除原因，其 DR
字段使用显式 `null`。
