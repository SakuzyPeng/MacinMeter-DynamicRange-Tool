# MacinMeter 0.3.0 release draft / 发布草案

> Source for the GitHub release description. Keep it readable by someone who
> has just found this page and wants to measure a file. /
> GitHub release 说明的来源。写给刚找到这个页面、只想测一个文件的人。

## English

MacinMeter measures the dynamic range (DR) of audio files, entirely on your own
machine.

**New in 0.3.0: Apple Lossless.** `.m4a` and `.mp4` files containing Apple
Lossless audio can now be analyzed, alongside the existing WAV, FLAC, and AIFF
support. This release is also the first to publish Windows builds.

### Download

| Your system | File |
| --- | --- |
| macOS on Apple Silicon (M1 or newer), macOS 11 or later | `macinmeter-gui-0.3.0-aarch64-apple-darwin.dmg` |
| Windows, 64-bit | `macinmeter-gui-0.3.0-x86_64-pc-windows-msvc-setup.exe` |

The `macinmeter-cli-0.3.0-…` archives contain the command-line tool on its own.
Its command is `mdrmeter`.

### Your system will warn you the first time

**This is expected.** These files are not code-signed:

- **macOS** may refuse to open the app. Right-click it, choose **Open**, and
  confirm. Once is enough.
- **Windows** may show "Windows protected your PC". Click **More info**, then
  **Run anyway**.

Signing requires a certificate issued to a named individual, and that name is
then embedded in every published file. This project is maintained by one person
who would rather not attach their legal name to every download, so neither
platform is signed. Building from source avoids this entirely.

### Also in this release

- The command-line tool is now called `mdrmeter` (it was `macinmeter`), so the
  name says what it measures. Shell completions are included for bash, zsh,
  fish, PowerShell, and elvish: `mdrmeter completions zsh`.
- Reports show how long the run took and how much faster than real time it was,
  and `--timing` breaks that into decoding and analysis.
- Reports now warn — without changing any number — when a file is shorter than
  one analysis window, when a multichannel file's speaker layout is unknown so
  the track value may include an LFE channel, and when silent channels count as
  DR0.
- Several files that were previously rejected with an unhelpful message now
  either work or explain themselves: RF64/BW64 files are named rather than
  called "not a WAV", files with more channels than the decoder supports say so,
  and non-Apple-Lossless `.mp4` files report the actual codec instead of blaming
  their edit list.
- The desktop app supports zooming, and its text is larger and more consistent.

### Not included

AAC, MP3, Ogg, Opus, DSD, and several less common variants of the supported
formats are still unsupported, and MacinMeter says so rather than converting or
resampling. There are no Intel Mac, ARM64 Windows, or Linux packages. The
[format guide](https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool/blob/v0.3.0/docs/SUPPORTED_FORMATS.md)
lists the exact rules.

## 中文

MacinMeter 用来测量音频文件的动态范围（DR），完全在你自己的机器上运行。

**0.3.0 新增：Apple Lossless。** 现在可以分析包含 Apple Lossless 音频的 `.m4a` 与
`.mp4` 文件，此外原有的 WAV、FLAC、AIFF 支持不变。本版本也是第一个提供 Windows
构建的版本。

### 下载

| 你的系统 | 文件 |
| --- | --- |
| macOS，Apple Silicon（M1 及更新），macOS 11 或更高 | `macinmeter-gui-0.3.0-aarch64-apple-darwin.dmg` |
| Windows 64 位 | `macinmeter-gui-0.3.0-x86_64-pc-windows-msvc-setup.exe` |

`macinmeter-cli-0.3.0-…` 压缩包里是单独的命令行工具，命令名为 `mdrmeter`。

### 首次打开时系统会拦一下

**这是正常的。** 这些文件没有做代码签名：

- **macOS** 可能拒绝打开。在应用上点右键，选择**打开**，再确认一次即可。
- **Windows** 可能出现「Windows 已保护你的电脑」。点**更多信息**，再点**仍要运行**。

做签名需要一张颁发给具体个人的证书，而那个姓名会被嵌进每一个发布的文件。本项目由
一个人维护，不希望把法定姓名附在每份下载上，所以两个平台都不签名。从源码构建则完全
不涉及这个问题。

### 本版本的其他变化

- 命令行工具改名为 `mdrmeter`（原为 `macinmeter`），让名字说明它测的是什么。附带
  bash、zsh、fish、PowerShell、elvish 的补全：`mdrmeter completions zsh`。
- 报告会显示本次运行耗时以及相当于实时的多少倍；`--timing` 可进一步拆出解码与分析。
- 报告会在**不改变任何数字**的前提下给出提示：文件短于一个分析窗口、多声道文件的
  声道布局未知因而整轨数值可能包含 LFE、以及静音声道按 DR0 计入。
- 一些此前被无用信息拒绝的文件，现在要么可用、要么说清了原因：RF64/BW64 会被点名，
  而不是被说成「不是 WAV」；声道数超出解码器支持范围会直接说明；非 Apple Lossless
  的 `.mp4` 会报出真正的编码格式，而不是归咎于 edit list。
- 桌面应用支持缩放，文字更大也更统一。

### 不包含

AAC、MP3、Ogg、Opus、DSD，以及上述格式中一些较少见的变体仍然不支持；MacinMeter 会
直接说明，而不会转换或重采样。不提供 Intel Mac、ARM64 Windows 或 Linux 的安装包。
完整规则见[格式指南](https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool/blob/v0.3.0/docs/SUPPORTED_FORMATS_CN.md)。
