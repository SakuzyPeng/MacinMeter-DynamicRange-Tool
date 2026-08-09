# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter 用来测量音频文件的**动态范围（DR）**。它完全在本地运行，不上传任何
内容，也不需要联网。

它可以读取 WAV、FLAC、AIFF 和 Apple Lossless（`.m4a` / `.mp4`）文件，给出每个
声道的 DR 值和整轨的 DR 值。命令行工具和桌面应用都有，两者结果相同。

## 下载

到[发布页面](../../releases/latest)获取最新版本。

| 你的系统 | 文件 |
| --- | --- |
| macOS，Apple Silicon（M1 及更新），macOS 11 或更高 | `macinmeter-gui-…-aarch64-apple-darwin.dmg` |
| Windows 64 位 | `macinmeter-gui-…-x86_64-pc-windows-msvc-setup.exe` |

`macinmeter-cli-…` 压缩包里是单独的命令行工具。

### 首次打开时系统会拦一下

**这是正常的。** 这些文件没有做代码签名，所以：

- **macOS** 可能拒绝打开。在应用上点右键，选择**打开**，再确认一次。只需做这一次。
- **Windows** 可能出现蓝色的「Windows 已保护你的电脑」。点**更多信息**，再点**仍要运行**。

做签名需要一张颁发给具体个人的证书，而那个姓名会被嵌进这里发布的每一个文件。本项目
由一个人维护，不希望把法定姓名附在每份下载上，所以两个平台都不签名。如果你不接受这个
取舍，也可以[从源码构建](docs/INTERNALS_CN.md#从源码构建)。

发布页面上的 `SHA256SUMS` 可以用来校验下载是否完整。

## 使用桌面应用

把文件或文件夹拖进窗口，或者用按钮选择，然后点击分析。结果可以复制为 Markdown，
或导出为 JSON、PNG、SVG。界面支持中文和英文。

## 使用命令行

```bash
mdrmeter analyze "01 - Song.flac"
mdrmeter batch "My Album/" --recursive
```

`batch` 始终按你给出的顺序列出文件，无论它们以什么顺序完成；某个文件读不了也不会
影响其余文件。它逐轨独立报告，不计算整张专辑的 DR。

下面是仓库内一个测试文件的真实输出：

```text
MacinMeter
Source: tests/fixtures/edge_cases.wav
PCM: 44100 Hz, 2 channels, 308700 frames
Duration: 0:07

CH 1: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000
CH 2: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000

Track aggregate: DR2 (2.4300 dB; 2 contributing channels)
Report levels: peak 0.00 dBFS, RMS -2.43 dBFS

Elapsed: 0.002 s (2929.6x realtime)
```

那是一个合成的测试文件，不代表真实音乐发行物。

常用选项：

| 选项 | 作用 |
| --- | --- |
| `--format json` | 输出机器可读格式，而不是文本 |
| `--output PATH` | 把报告写入文件，而不是打印到屏幕 |
| `--recursive` | 配合 `batch`，同时搜索子文件夹 |
| `--timing` | 额外显示解码与分析各自耗时 |

支持 bash、zsh、fish、PowerShell、elvish 的命令补全：

```bash
mdrmeter completions zsh > "${fpath[1]}/_mdrmeter"
```

## 如何理解结果

- **`DR2`** 是这一轨的 DR 值。数值越大，表示峰值与响亮部分平均电平之间的差距越大。
  DR 高本身不代表录音好听；但 DR 很低往往意味着强压缩，对应母带受损的可能性更大。
  音乐类型和创作意图仍然会影响判断。
- 每条 **`CH`** 是该声道自己的 DR、overall RMS，以及 DR 计算所选取的峰值。
- **`Report levels`** 是整轨测量值。其中的峰值与上面的 selected DR peak 不是同一个量。
- **dBFS** 以幅度 `1.0` 作为 0 dB。浮点音频可以合法地包含超过该值的样本，所以
  0 dBFS 并不总是削波点。
- **静音声道**依然会显示，并按 DR0 计入。数据不足以测量的声道会被排除，并明确说明被排除。
- **`Elapsed`** 和其后的实时倍率描述的是本机这一次运行，而不是分析本身，因此只出现在
  文本输出里。同一个文件两次运行的 JSON 完全相同。

有些报告会附带警告 —— 例如文件短于一个分析窗口，或者多声道文件的声道布局未知、
整轨数值可能因此包含 LFE 声道。这些警告不会改变任何数字。

### 退出码

| 代码 | 含义 |
| ---: | --- |
| `0` | 全部分析成功 |
| `1` | 失败、无输入、batch 全部失败，或报告写入失败 |
| `2` | 命令行参数错误 |
| `3` | batch 同时存在成功与失败 |
| `130` | 已取消 |

## 哪些文件可以用

| 格式 | 支持范围 |
| --- | --- |
| WAV | 8/16/24/32-bit 整数，32/64-bit 浮点 |
| FLAC | 原生 FLAC 文件 |
| AIFF | 8/16/24/32-bit 整数 |
| `.m4a` / `.mp4` 里的 Apple Lossless | 16/24-bit，1–8 声道 |

文件夹扫描会寻找 `.wav`、`.wave`、`.flac`、`.aif`、`.aiff`、`.m4a` 与 `.mp4`。
其他后缀的受支持文件，只要直接传入路径同样可以分析，因为识别依据是文件内容而不是
文件名。

并不是所有带这些后缀的文件都能用。AAC、MP3、Ogg、Opus 和 DSD 完全不支持，上述格式
里也有一些较少见的变体不支持。MacinMeter 会直接说明，而不是猜测：它不会转换、不会
重采样，也不会退回到别的解码器。完整规则和全部排除项见[格式指南](docs/SUPPORTED_FORMATS_CN.md)。

## 准确度如何

DR 算法是在原作者许可下，从一个特定程序 `foo_dr_meter 1.0.8 x64` 重建的。在一组固定
的测试输入上，MacinMeter 的结果与该程序完全一致 —— 39 轨全部一致，62 个声道全部一致，
零差异。

这是一条关于特定测试集的精确陈述，不是对世界上每个文件的承诺。比较了什么、没有比较
什么，都列在[准确度记录](docs/INTERNALS_CN.md#准确度)里。

## 更多

- [格式指南](docs/SUPPORTED_FORMATS_CN.md) —— 具体哪些文件可用、为什么
- [技术说明](docs/INTERNALS_CN.md) —— 架构、Rust API、准确度证据、性能测量
- [发行与打包状态](docs/RELEASE_CN.md)
- [法律说明](docs/LEGAL_CN.md)与[第三方声明](THIRD_PARTY_NOTICES.md)

MacinMeter 以 [MIT License](LICENSE) 发布。

参考目标是 Janne Hyvärinen 的 `foo_dr_meter 1.0.8 x64` 组件。对它的逆向工程经过作者
许可；仓库中只保留一份[最小化的公开授权摘要](reference/authorization/README.md)。
