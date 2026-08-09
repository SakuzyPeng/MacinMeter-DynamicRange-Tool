# MacinMeter 0.3.1 release draft / 发布草案

> Source for the GitHub release description. Keep it readable by someone who
> wants to measure audio files, without requiring knowledge of the implementation. /
> GitHub Release 说明的来源。面向只想测量音频文件的用户，不要求读者了解实现细节。

## English

MacinMeter measures the dynamic range (DR) of audio files, entirely on your own
machine.

**0.3.1 makes large batches smoother and more dependable.** Results now appear
while files are being analyzed, cancelling responds promptly, and JSON or image
exports work reliably even when the result list is very large.

### Download

| Your system | File |
| --- | --- |
| macOS on Apple Silicon (M1 or newer), macOS 11 or later | `macinmeter-gui-0.3.1-aarch64-apple-darwin.dmg` |
| Windows, 64-bit | `macinmeter-gui-0.3.1-x86_64-pc-windows-msvc-setup.exe` |

The `macinmeter-cli-0.3.1-…` archives contain the command-line tool on its own.
Its command is `mdrmeter`.

### What changed

- **See results sooner.** When analyzing a folder, completed tracks appear as
  the batch progresses instead of arriving all at once at the end.
- **Cancel the whole batch.** The Cancel button now responds promptly during a
  large analysis and keeps a clear cancelling status until the job stops.
- **Reliable exports.** JSON, PNG, and SVG exports now handle large result sets
  without silently doing nothing. The app shows feedback as soon as an export
  starts.
- **Readable long images.** Very tall result lists are saved as numbered image
  pages instead of being squeezed into one blurry image. You choose one folder,
  and MacinMeter writes the pages there in order.
- **A more responsive Windows app.** Large batches no longer flood the window
  with background updates, so controls remain usable when hundreds of files are
  involved.

The DR calculation, report numbers, supported audio formats, and JSON schema are
unchanged from 0.3.0. WAV, FLAC, AIFF, and supported Apple Lossless `.m4a` /
`.mp4` files continue to work as before.

### Your system will warn you the first time

**This is expected.** The downloads are not code-signed:

- **macOS** may refuse to open the app. Right-click it, choose **Open**, and
  confirm. Once is enough.
- **Windows** may show “Windows protected your PC”. Click **More info**, then
  **Run anyway**.

This is a standing privacy decision, not unfinished release work. Signing would
place the individual maintainer's legal name in every download. Building from
source avoids the warning entirely.

AAC, MP3, Ogg, Opus, and DSD remain unsupported. There are no Intel Mac, ARM64
Windows, or Linux GUI packages. The
[format guide](https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool/blob/v0.3.1/docs/SUPPORTED_FORMATS.md)
lists the exact supported variants.

## 中文

MacinMeter 用来测量音频文件的动态范围（DR），完全在你自己的机器上运行。

**0.3.1 让大批量分析更顺畅、更可靠。** 文件分析完成后会陆续显示结果；取消操作能及时
响应；即使结果很多，JSON 和图片也能正常导出。

### 下载

| 你的系统 | 文件 |
| --- | --- |
| macOS，Apple Silicon（M1 及更新），macOS 11 或更高 | `macinmeter-gui-0.3.1-aarch64-apple-darwin.dmg` |
| Windows 64 位 | `macinmeter-gui-0.3.1-x86_64-pc-windows-msvc-setup.exe` |

`macinmeter-cli-0.3.1-…` 压缩包里是单独的命令行工具，命令名为 `mdrmeter`。

### 本版本的变化

- **更早看到结果。** 分析文件夹时，已完成的曲目会随着进度陆续出现，不必等到全部结束
  后才一次显示。
- **取消整个批次。** 大批量分析过程中点击取消会及时响应，并持续显示“正在取消”，直到
  整个任务真正停止。
- **导出不再没反应。** JSON、PNG 和 SVG 可以可靠处理大量结果；点击导出后，界面会
  立即给出反馈。
- **长图片仍然清楚。** 很长的结果不会再被硬塞进一张模糊图片，而是自动保存为按顺序
  编号的多页图片。你只需选择一个文件夹。
- **Windows 大批量操作更流畅。** 即使一次处理数百个文件，后台进度也不会再把窗口拖到
  像卡死一样，按钮可以正常响应。

本版本没有改变 DR 算法、报告数值、支持的音频格式或 JSON schema。WAV、FLAC、AIFF，
以及受支持的 Apple Lossless `.m4a` / `.mp4` 文件都与 0.3.0 相同。

### 首次打开时系统会拦一下

**这是正常的。** 下载文件没有做代码签名：

- **macOS** 可能拒绝打开。在应用上点右键，选择**打开**，再确认一次即可。
- **Windows** 可能出现「Windows 已保护你的电脑」。点**更多信息**，再点**仍要运行**。

未签名是出于隐私考虑的长期决定，不是尚未完成的发布步骤。签名会把个人维护者的法定
姓名嵌入每一份下载文件；从源码构建则不会遇到这项系统提示。

AAC、MP3、Ogg、Opus 和 DSD 仍不支持；也不提供 Intel Mac、ARM64 Windows 或 Linux
图形界面安装包。完整规则见
[格式指南](https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool/blob/v0.3.1/docs/SUPPORTED_FORMATS_CN.md)。
