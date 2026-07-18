# Reference fixtures

本目录尚未加入 fixture。优先提交可由短小参数精确生成的 PCM/WAV 输入；大型、
私有或版权状态不清的音频只记录生成方式、属性和 SHA-256，不直接提交。

每个 fixture 应记录：

- fixture ID；
- 生成器版本、命令、参数和随机种子；
- sample format、采样率、声道布局和帧数；
- 波形/幅度/分段的精确定义；
- 文件 SHA-256；
- 许可或可再生成性说明。

Fixture 本身不是 golden。只有关联到固定 target 的 observation 才能提供参考结果。
