# 代码审查与优化顺序（可追踪）

创建时间：2025-10-10
用途：仅用于追踪我们“按顺序审查/优化”的进度，不包含具体建议或实现细节。

说明
- 按下列顺序逐步审查；完成一项打勾即可。
- 如需记录具体问题或改进点，另起专门文档或在提交信息中说明。

—

一、入口与流程控制
- [x] src/main.rs

二、Tools 层
- [x] src/tools/cli.rs
- [x] src/tools/scanner.rs
- [x] src/tools/constants.rs
- [x] src/tools/utils.rs
- [x] src/tools/processor.rs
- [x] src/tools/parallel_processor.rs
- [ ] src/tools/formatter.rs
- [ ] src/tools/batch_state.rs
- [ ] src/tools/mod.rs

三、Audio 解码层
- [ ] src/audio/universal_decoder.rs
- [ ] src/audio/streaming.rs
- [ ] src/audio/parallel_decoder.rs
- [ ] src/audio/opus_decoder.rs
- [ ] src/audio/format.rs
- [ ] src/audio/stats.rs
- [ ] src/audio/mod.rs

四、Processing 层
- [ ] src/processing/simd_core.rs
- [ ] src/processing/sample_conversion.rs
- [ ] src/processing/channel_separator.rs
- [ ] src/processing/dr_channel_state.rs
- [ ] src/processing/processing_coordinator.rs
- [ ] src/processing/performance_metrics.rs
- [ ] src/processing/mod.rs

五、Core 算法层
- [ ] src/core/dr_calculator.rs
- [ ] src/core/histogram.rs
- [ ] src/core/peak_selection.rs
- [ ] src/core/mod.rs

六、顶层库与错误
- [ ] src/lib.rs
- [ ] src/error.rs

七、测试覆盖
- [ ] tests/tools_integration_tests.rs
- [ ] tests/universal_decoder_tests.rs
- [ ] tests/parallel_decoder_tests.rs
- [ ] tests/parallel_diagnostic_tests.rs
- [ ] tests/simd_unit_tests.rs
- [ ] tests/simd_edge_case_tests.rs
- [ ] tests/simd_performance_tests.rs
- [ ] tests/audio_format_tests.rs
- [ ] tests/boundary_tests.rs
- [ ] tests/error_handling_tests.rs
- [ ] tests/error_path_tests.rs
- [ ] tests/memory_safety_tests.rs
- [ ] tests/aiff_diagnostic.rs
- [ ] tests/audio_test_fixtures.rs
- [ ] tests/README.md

八、文档与脚本
- [ ] README.md
- [ ] Performance_Tracking.md
- [ ] DR_Batch_Memory_Optimization_Plan.md
- [ ] optimization_log.md
- [ ] benchmark_10x.sh
- [ ] scripts/install-pre-commit.sh

—

当前起点：src/main.rs
下一步：依次审查 Tools → Audio → Processing → Core → 顶层库与错误 → Tests → 文档与脚本。
