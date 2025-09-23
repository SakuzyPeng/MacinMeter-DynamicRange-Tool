#include "results_dialog.h"
#include "foobar2000.h"
#include <sstream>

void ResultsDialog::show_results(const std::vector<std::string>& formatted_reports,
                                 const std::vector<AudioData>& audio_data_list) {
    if (formatted_reports.empty()) {
        popup_message::g_complain("MacinMeter DR Plugin", "No DR analysis results to display");
        return;
    }

    // 🚀 极简标题生成
    std::string title = (formatted_reports.size() == 1)
        ? "MacinMeter DR Analysis Result"
        : PFC_string_formatter() << "MacinMeter DR Analysis Results (" << formatted_reports.size() << " tracks)";

    // 🚀 直接合并所有Rust格式化的报告（零处理）
    std::ostringstream content_stream;
    for (const auto& report : formatted_reports) {
        content_stream << report;
        if (&report != &formatted_reports.back()) {
            content_stream << "\n" << std::string(70, '-') << "\n"; // 分隔线
        }
    }

    // 🚀 直接显示，无任何额外处理
    popup_message::g_show(content_stream.str().c_str(), title.c_str());
}

// 🚀 所有复杂格式化代码已删除 - 直接使用Rust端格式化结果！