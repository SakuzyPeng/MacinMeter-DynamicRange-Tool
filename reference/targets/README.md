# Reference targets

目标档案必须固定插件及其宿主环境，不能只记录插件文件名。

当前目标：

- [`TARGET-foo-dr-meter-1.0.8-foobar2000-2.25.10-x64-win10-19045`](foo-dr-meter-1.0.8-foobar2000-2.25.10-x64.md)：
  完整 v2 safe-master 黑盒观测使用的固定 x64 runtime target。
- [`TARGET-foobar2000-2.0-x64-wave-decoder-ea6e9c52-cf5b2a86`](foobar2000-2.0-x64-wave-decoder.md)：
  固定 foobar2000 2.0 x64 host 与 `foo_input_std` WAV decoder 的静态分析对象。
- [`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](foo-dr-meter-1.0.8-x64-static.md)：
  当前 candidate spec 的 x64 静态分析对象。
- [`TARGET-foo-dr-meter-1.0.8-x86-static-6debd1d6`](foo-dr-meter-1.0.8-x86-static.md)：
  与 x64 做数据宽度和控制流比较的 1.0.8 x86 静态分析对象。
- [`TARGET-foo-dr-meter-1.0.8-foobar2000-2.0-x86-win10-19045`](foo-dr-meter-1.0.8-foobar2000-2.0-x86.md)：
  与两份静态结论交叉印证的 1.0.8 x86 黑盒运行目标。

静态 target 与 runtime target 必须分别登记。即使插件 DLL 哈希相同，不同
foobar2000 或 input component 版本的宿主行为也不能互相代替。

每个 target 文件至少包含：

- target ID；
- 插件名称、版本、架构和 SHA-256；
- foobar2000 版本、架构和 SHA-256；
- 操作系统版本、时区和关键区域设置；
- 插件与宿主配置；
- 获取来源、授权边界和带时区的记录日期；
- 可重复运行所需但不能公开的材料位置说明。

建议文件名：`foo-dr-meter-<plugin-version>-<host>-<arch>.md`。
