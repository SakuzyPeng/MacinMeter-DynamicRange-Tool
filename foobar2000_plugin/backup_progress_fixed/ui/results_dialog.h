#pragma once
#include "foobar2000.h"
#include "../audio/audio_accessor.h"
#include <vector>
#include <string>

// 🚀 极简结果显示对话框（零复杂性设计）
class ResultsDialog {
  public:
    // 🚀 唯一接口：直接显示Rust格式化的DR报告
    void show_results(const std::vector<std::string>& formatted_reports,
                      const std::vector<AudioData>& audio_data_list);
};