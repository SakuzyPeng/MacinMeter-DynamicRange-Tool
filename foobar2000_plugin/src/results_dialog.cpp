#include "results_dialog.h"
#include "foobar2000.h"
#include <algorithm>
#include <cmath>
#include <ctime>
#include <filesystem>
#include <iomanip>
#include <map>
#include <sstream>

#ifdef MAC_VERSION
#include <CoreServices/CoreServices.h>
#include <fstream>
#else
#include <commdlg.h>
#include <windows.h>
#endif

// 安全格式化数值的辅助函数
static std::string format_db_value(double value, int precision = 2) {
    if (std::isfinite(value)) {
        std::ostringstream oss;
        oss << std::fixed << std::setprecision(precision) << value;
        return oss.str();
    } else {
        return "--";
    }
}

void ResultsDialog::show_results(const std::vector<DrAnalysisResult>& results,
                                 const std::vector<AudioData>& audio_data_list) {
    if (results.empty()) {
        popup_message::g_complain("MacinMeter DR Plugin", "No DR analysis results to display");
        return;
    }

    std::string title = (results.size() == 1) ? "MacinMeter DR Analysis Result (foobar2000 decoded)"
                                              : PFC_string_formatter()
                                                    << "MacinMeter DR Analysis Results ("
                                                    << results.size()
                                                    << " tracks, foobar2000 decoded)";

    std::string content = generate_results_text(results, audio_data_list, false);
    show_results_dialog(title, content, audio_data_list);
}

void ResultsDialog::show_batch_results(const std::vector<DrAnalysisResult>& results,
                                       const std::vector<AudioData>& track_infos) {
    if (results.empty()) {
        popup_message::g_complain("MacinMeter DR Plugin", "No batch analysis results to display");
        return;
    }

    pfc::string8 title_str;
    title_str << "MacinMeter DR Batch Analysis Report (" << results.size() << " tracks)";
    std::string title = title_str.c_str();
    std::string content = generate_results_text(results, track_infos, true);

    // 为批量结果添加统计信息
    content += "\n\n" + generate_batch_statistics(results);

    show_results_dialog(title, content, track_infos);
}

std::string ResultsDialog::generate_results_text(const std::vector<DrAnalysisResult>& results,
                                                 const std::vector<AudioData>& track_infos,
                                                 bool batch_mode) {
    std::ostringstream oss;

    // 生成标准foobar2000 DR报告头部
    oss << "MacinMeter DR Tool v1.0.0 / Dynamic Range Meter (foobar2000 compatible)\n";
    oss << "log date: " << get_timestamp_string() << "\n\n";

    if (batch_mode) {
        // 批量模式：生成简化的表格格式
        oss << "================================================================================\n";
        oss << "MacinMeter DR Batch Analysis Report\n";
        oss << "================================================================================"
               "\n\n";

        oss << "File Name\tDR\tPeak(dB)\tRMS(dB)\tSample Rate\tChannels\tDuration\n";
        oss << "--------------------------------------------------------------------------------\n";

        for (size_t i = 0; i < results.size() && i < track_infos.size(); ++i) {
            const auto& result = results[i];
            const auto& track = track_infos[i];

            // 安全格式化官方DR值
            std::string dr_str =
                (std::isfinite(result.official_dr_value) && result.official_dr_value > 0)
                    ? "DR" + std::to_string((int)std::round(result.official_dr_value))
                    : "DR--";

            oss << track.file_name << "\t" << dr_str << "\t" << format_db_value(result.peak_db)
                << "\t" << format_db_value(result.rms_db) << "\t" << result.sample_rate << "Hz\t"
                << result.channels << "\t" << std::fixed << std::setprecision(1)
                << result.duration_seconds << "s\n";
        }
    } else {
        // 单文件模式：生成详细的foobar2000格式报告
        for (size_t i = 0; i < results.size() && i < track_infos.size(); ++i) {
            if (i > 0)
                oss << "\n\n";
            oss << format_single_result(results[i], track_infos[i]);
        }
    }

    return oss.str();
}

std::string ResultsDialog::format_single_result(const DrAnalysisResult& result,
                                                const AudioData& track) {
    std::ostringstream oss;

    // foobar2000标准格式
    oss << "--------------------------------------------------------------------------------\n";
    oss << "Statistics for: " << track.file_name << "\n";
    oss << "Number of samples: " << result.total_samples << "\n";

    // Duration: Use result.duration_seconds
    int minutes = (int)result.duration_seconds / 60;
    int seconds = (int)result.duration_seconds % 60;
    oss << "Duration: " << minutes << ":" << std::setfill('0') << std::setw(2) << seconds << " \n";
    oss << "--------------------------------------------------------------------------------\n\n";

    // 根据声道数选择显示格式
    if (result.channels == 1) {
        // 单声道 - 使用真实的声道数据
        oss << "                 Mono\n\n";
        oss << "Peak Value:     " << format_db_value(result.peak_db_per_channel[0]) << " dB   \n";
        oss << "Avg RMS:       " << format_db_value(result.rms_db_per_channel[0]) << " dB   \n";
        oss << "DR channel:      " << format_db_value(result.dr_db_per_channel[0]) << " dB   \n";
    } else if (result.channels == 2) {
        // 立体声 - 使用真实的每声道数据
        oss << "                 Left              Right\n\n";
        oss << "Peak Value:     " << format_db_value(result.peak_db_per_channel[0])
            << " dB         " << format_db_value(result.peak_db_per_channel[1]) << " dB   \n";
        oss << "Avg RMS:       " << format_db_value(result.rms_db_per_channel[0]) << " dB        "
            << format_db_value(result.rms_db_per_channel[1]) << " dB   \n";
        oss << "DR channel:      " << format_db_value(result.dr_db_per_channel[0]) << " dB         "
            << format_db_value(result.dr_db_per_channel[1]) << " dB   \n";
    } else {
        // 多声道 - 显示所有声道数据
        oss << "              Multi-channel (" << result.channels << " channels)\n\n";
        oss << "Overall Peak:   " << format_db_value(result.peak_db) << " dB\n";
        oss << "Overall RMS:    " << format_db_value(result.rms_db) << " dB\n\n";

        // 显示每声道详细信息
        for (unsigned int ch = 0; ch < result.channels && ch < 8; ++ch) {
            oss << "Channel " << (ch + 1) << ":\n";
            oss << "  Peak:   " << format_db_value(result.peak_db_per_channel[ch]) << " dB\n";
            oss << "  RMS:    " << format_db_value(result.rms_db_per_channel[ch]) << " dB\n";
            oss << "  DR:     " << format_db_value(result.dr_db_per_channel[ch]) << " dB\n\n";
        }
        oss << "DR channel:      " << format_db_value(result.precise_dr_value) << " dB\n";
    }

    oss << "--------------------------------------------------------------------------------\n\n";

    // 官方DR值 - 添加有效性检查
    if (std::isfinite(result.official_dr_value) && result.official_dr_value > 0) {
        oss << "Official DR Value: DR" << (int)std::round(result.official_dr_value) << "\n";
    } else {
        oss << "Official DR Value: DR--\n";
    }

    if (std::isfinite(result.precise_dr_value) && result.precise_dr_value > 0) {
        oss << "Precise DR Value: " << std::fixed << std::setprecision(2) << result.precise_dr_value
            << " dB\n\n";
    } else {
        oss << "Precise DR Value: -- dB\n\n";
    }

    // 技术信息
    oss << "Samplerate:        " << result.sample_rate << " Hz\n";
    oss << "Channels:          " << result.channels << "\n";
    // Bits per sample:
    unsigned int display_bits_per_sample = result.bits_per_sample;
    if (display_bits_per_sample == 0) { // If not set, assume float
        display_bits_per_sample = 32;
    }
    oss << "Bits per sample:   " << display_bits_per_sample << "\n";

    // Calculate bitrate
    int bitrate = 0;
    if (result.sample_rate > 0 && result.channels > 0 && display_bits_per_sample > 0) {
        bitrate = (result.sample_rate * result.channels * display_bits_per_sample) / 1000;
    }
    oss << "Bitrate:           " << bitrate << " kbps\n";
    oss << "Codec:             " << result.codec << "\n";

    oss << "================================================================================";

    return oss.str();
}

std::string ResultsDialog::generate_batch_statistics(const std::vector<DrAnalysisResult>& results) {
    if (results.empty())
        return "";

    std::ostringstream oss;

    // 计算统计信息
    double total_dr = 0.0;
    double min_dr = results[0].official_dr_value;
    double max_dr = results[0].official_dr_value;

    std::map<int, int> dr_distribution;

    for (const auto& result : results) {
        total_dr += result.official_dr_value;
        min_dr = std::min(min_dr, result.official_dr_value);
        max_dr = std::max(max_dr, result.official_dr_value);

        dr_distribution[(int)result.official_dr_value]++;
    }

    double avg_dr = total_dr / results.size();

    oss << "================================================================================\n";
    oss << "Batch Analysis Statistics:\n";
    oss << "================================================================================\n\n";

    oss << "Total Files Analyzed:     " << results.size() << "\n";
    oss << "Average DR Value:         DR" << std::fixed << std::setprecision(1) << avg_dr << "\n";
    oss << "DR Range:                 DR" << (int)min_dr << " - DR" << (int)max_dr << "\n\n";

    oss << "DR Distribution:\n";
    for (const auto& pair : dr_distribution) {
        int dr_value = pair.first;
        int count = pair.second;
        double percentage = (count * 100.0) / results.size();

        oss << "  DR" << dr_value << ":  " << count << " files (" << std::fixed
            << std::setprecision(1) << percentage << "%)\n";
    }

    oss << "\n";
    oss << "Analysis completed: " << get_timestamp_string() << "\n";
    oss << "MacinMeter DR Plugin v1.0.0 (foobar2000 compatible)";

    return oss.str();
}

void ResultsDialog::show_results_dialog(const std::string& title, const std::string& content,
                                        const std::vector<AudioData>& track_infos) {
    // 在控制台输出完整结果，不保存文件

    std::string display_content = content;

    // 添加引擎信息作为页脚
    display_content += "\n\n" + std::string(80, '-');
    display_content += "\nMacinMeter DR Engine v1.0.0 (foobar2000-plugin)";
    display_content += "\nDecoded by: foobar2000 native decoder";
    display_content += "\nPacket-by-packet processing with Sum Doubling enabled";

    // 在控制台输出完整结果
    console::printf("MacinMeter DR: === %s ===", title.c_str());
    console::printf("%s", display_content.c_str());

    // 给用户一个完成通知
    std::string summary = track_infos.size() == 1
                              ? "DR analysis completed! Check Console for detailed results."
                              : PFC_string_formatter()
                                    << "Batch DR analysis of " << track_infos.size()
                                    << " tracks completed! Check Console for detailed results.";

    popup_message::g_complain("MacinMeter DR Analysis Complete", summary.c_str());

    console::print("MacinMeter DR: Results displayed in Console (no file created)");
}

bool ResultsDialog::save_results_to_file(const std::string& content,
                                         const std::vector<AudioData>& track_infos) {
    if (track_infos.empty()) {
        popup_message::g_complain("MacinMeter DR Plugin",
                                  "No track information available for saving");
        return false;
    }

    // 简化文件保存逻辑（foobar2000解码模式）
    std::string first_file_name = track_infos[0].file_name;
#ifdef MAC_VERSION
    // 使用 foobar2000 配置目录作为默认保存位置（可写）
    std::string default_dir = core_api::get_profile_path();
#else
    std::string default_dir = "."; // 当前目录
#endif
    std::string base_filename;

    // 移除原始文件扩展名，添加DR分析后缀
    size_t last_dot = first_file_name.find_last_of('.');
    if (last_dot != std::string::npos) {
        base_filename = first_file_name.substr(0, last_dot);
    } else {
        base_filename = first_file_name;
    }

    // 生成建议的文件名
    std::string timestamp = get_timestamp_string();
    // 替换时间戳中的特殊字符
    std::replace(timestamp.begin(), timestamp.end(), ':', '-');
    std::replace(timestamp.begin(), timestamp.end(), ' ', '_');

    std::string suggested_filename;
    if (track_infos.size() == 1) {
        suggested_filename = base_filename + "_DR_" + timestamp + ".txt";
    } else {
        suggested_filename = "MacinMeter_Batch_DR_" + timestamp + ".txt";
    }

    std::string default_path = default_dir + "/" + suggested_filename;

#ifdef MAC_VERSION
    // Mac版本：使用简化的保存方式
    try {
        // 在 profile 路径下创建专用结果目录，避免当前目录不可写
        std::filesystem::path dir_path(default_dir);
        dir_path /= "MacinMeter_DR_Results";
        std::error_code ec;
        std::filesystem::create_directories(dir_path, ec);

        std::filesystem::path file_path = dir_path / suggested_filename;
        default_path = file_path.string();

        std::ofstream file(default_path, std::ios::out | std::ios::trunc);
        if (!file) {
            popup_message::g_complain("MacinMeter DR Plugin", PFC_string_formatter()
                                                                  << "Cannot create file: "
                                                                  << default_path.c_str());
            return false;
        }

        file << content;
        file.close();

        console::printf("MacinMeter DR: Results saved to %s", default_path.c_str());
        return true;
    } catch (const std::exception& e) {
        console::printf("MacinMeter DR: Error saving results: %s", e.what());
        pfc::string8 error_msg;
        error_msg << "Error saving results: " << e.what();
        popup_message::g_complain("MacinMeter DR Plugin", error_msg.c_str());
        return false;
    }

#else
    // Windows版本：使用文件保存对话框
    OPENFILENAME ofn;
    char file_path[MAX_PATH] = "";

    // 设置建议的完整路径
    strncpy_s(file_path, default_path.c_str(), _TRUNCATE);

    ZeroMemory(&ofn, sizeof(ofn));
    ofn.lStructSize = sizeof(ofn);
    ofn.hwndOwner = core_api::get_main_window();
    ofn.lpstrFilter = "Text Files (*.txt)\0*.txt\0All Files (*.*)\0*.*\0";
    ofn.lpstrFile = file_path;
    ofn.nMaxFile = MAX_PATH;
    ofn.lpstrTitle = "Save MacinMeter DR Analysis Results";
    ofn.Flags = OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT;
    ofn.lpstrDefExt = "txt";
    ofn.lpstrInitialDir = default_dir.c_str();

    if (GetSaveFileName(&ofn)) {
        try {
            HANDLE hFile = CreateFile(file_path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                                      FILE_ATTRIBUTE_NORMAL, NULL);
            if (hFile != INVALID_HANDLE_VALUE) {
                DWORD bytes_written;
                WriteFile(hFile, content.c_str(), (DWORD)content.length(), &bytes_written, NULL);
                CloseHandle(hFile);

                console::printf("MacinMeter DR: Results saved to %s", file_path);
                return true;
            }
        } catch (const std::exception& e) {
            console::printf("MacinMeter DR: Error saving results: %s", e.what());
            pfc::string8 win_error_msg;
            win_error_msg << "Error saving results: " << e.what();
            popup_message::g_complain("MacinMeter DR Plugin", win_error_msg.c_str());
        }
    }

    return false;
#endif
}

std::string ResultsDialog::get_timestamp_string() {
    auto now = std::time(nullptr);
    auto tm = *std::localtime(&now);

    std::ostringstream oss;
    oss << std::put_time(&tm, "%Y-%m-%d %H:%M:%S");
    return oss.str();
}