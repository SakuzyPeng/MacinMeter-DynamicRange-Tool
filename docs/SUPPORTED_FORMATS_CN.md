[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# 稳定音频格式

MacinMeter 有意只公开一小块以正确性为先的解码面。这里的“可用”描述当前开发线
的稳定能力；已发布版本的记录仍保持历史含义。参考算法证据与解码支持分别记录。

| 容器 | 接受的编码 | 送入分析器的 PCM |
|---|---|---|
| RIFF/WAVE（经典或受限 WAVE_FORMAT_EXTENSIBLE） | 8/16/24/32-bit 线性整数 PCM | 有限、交错的 `f64` |
| RIFF/WAVE（经典或受限 WAVE_FORMAT_EXTENSIBLE） | IEEE 32/64-bit 浮点 PCM | 有限、交错的 `f64` |
| FLAC | FLAC | 有限、交错的 `f64` |
| AIFF | 8/16/24/32-bit 线性整数 PCM | 有限、交错的 `f64` |
| 非 fragmented ISO BMFF（`.m4a` / `.mp4`） | ALAC compatible version 0、16/24-bit、1–8 声道 | 有限、交错的 `f64` |

固定的 x64 1.0.8 参考核心也接收 `f64`。产品 PCM 主链改为 `f64` 后，当前
safe-master corpus 暴露的两处 source-f64 偏差已关闭，公开可比核心字段达到
track DR 39/39、channel DR 62/62。这不证明所有宿主、容器、运行库边界或输入的
decoder 归一化已经逐位一致。

解码器不使用扩展名 hint，而是直接探测文件内容。`.wav`、`.wave`、`.flac`、
`.aif`、`.aiff`、`.m4a`、`.mp4` 只用于目录发现。显式传入的文件即使扩展名
不同，只要内容受支持也可以打开。

仓库提交的
[`native-pcm-v1`](../tests/fixtures/native-pcm-v1/README.md) 产品 corpus 以独立
raw-bit 归一化 oracle 固定了每一个已声明 PCM 位深；其中 FLAC 为 stereo、
multi-block，AIFF/FLAC 还通过 Rust API 与 CLI 的共享 report 边界。这些是产品
契约 fixture，不是参考插件 golden。

独立的
[`native-pcm-extensible-v1`](../tests/fixtures/native-pcm-extensible-v1/README.md)
corpus 为每一种接受的 Extensible 形状提供携带相同 PCM 的经典 WAV 孪生。Extensible
输入要求 `fmt` chunk 恰好 40 bytes、`cbSize=22`、完整匹配 PCM 或 IEEE-float
sub-format GUID，且 valid bits 等于容器位宽。零 channel mask 作为未指定/direct-out
接受；非零 mask 只能使用标准低 18 个 speaker bits，且置位数必须等于声道数。
稳定 Extensible 路径接受 1–26 声道，报告中的 channel layout 仍为 `unknown`。

仓库提交的 [`native-alac-v1`](../tests/fixtures/native-alac-v1/README.md) 为每个稳定
ALAC 样本提供 PCM 逐位一致的 WAV 孪生。ISO BMFF 路径要求一个非 fragmented、
仅音频的 track、一个 `alac` sample entry、ALAC compatible version 0、4096-frame
cookie、16/24-bit、1–8 声道和标准声道几何，并要求 `mdhd`、`stts`、`stsz` 声明
一致。`moov` 可以位于 `mdat` 前后，普通 metadata 与 `free` box 可以存在；edit
list 只能缺失或是一条 identity mapping。报告中的 channel layout 仍为 `unknown`。

## 明确不可用

当前稳定面不包含：

- padded 或 valid bits 未指定的 WAVE_FORMAT_EXTENSIBLE、超过 26 声道的 Extensible、
  使用保留 channel-mask bits 的 Extensible、AIFC、压缩 WAV 变体及可识别容器内
  未支持的其他编码；
- AAC（包括 M4A/MP4 内的 AAC）、MP1/MP2/MP3、Vorbis、Opus、AC-3、E-AC-3、
  DTS、DSD；
- fragmented MP4、带视频或额外 track 的 MP4、多音轨、裁剪 edit list、ALAC
  20/32-bit 或非 version-0、非标准 ALAC layout、raw/CAF ALAC，以及 Ogg、
  Matroska/WebM、DSF、DFF 容器；
- FFmpeg 回退或任何外部解码进程；
- 重采样、增益、滤波、边缘裁切和静音预处理。

并行属于执行策略，不是 codec 能力。已接受的
[`ADR-0014`](adr/0014-deterministic-decode-analysis-pipeline.md) 解除有界 packet、
文件与窗口级并行的永久禁令，各条路径在独立毕业前都不是稳定功能。0.3.0 中，ALAC
route 与可证明有界的 FLAC 流以 packet worker 解码，解码在未花掉的 permit 上与分析
重叠，批量条目跨 file lane 运行，三者同出于一份 application 自有的计划；窗口级并行
尚未实现。走哪条路径由该计划逐 route 决定，因此既不扩张上面的格式矩阵，也不增加
公共线程调参，更不改变任何报告。

稳定 AIFF 路径还要求 80-bit sample rate 为有限、正数、可由 `u32` 精确表示的
整数、COMM chunk 恰好为 18 bytes，且 SSND offset/block-size 均为零。稳定 FLAC
路径要求 STREAMINFO 声明非零总样本数：缺少该声明时流末帧数核对失效，若 MD5
签名同样缺失，整帧尾部丢失将原理上不可检测。产品会拒绝这些尚未毕业的容器
变体，而不会让 backend 静默舍入或自行扩张支持面。

能够识别但不属于当前稳定面的内容返回稳定错误码 `unsupported_format`。受支持格式内可
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

系统不会根据声道数猜布局；backend 无法确认布局时报告 `unknown`。分析
只生成一个 `track` 聚合，并按照已记录的数值规则纳入 LFE，而不再生成单独的
`without_lfe` 结果。静音声道仍明确显示为 `silent`，并以 DR0 参与聚合；只有数据
不足的声道会被排除。如果没有声道可以参与，aggregate 的 DR 字段使用显式 `null`。
