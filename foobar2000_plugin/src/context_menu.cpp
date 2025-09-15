#include "context_menu.h"
#include "audio_accessor.h"
#include "foobar2000.h"
#include "results_dialog.h"
#include "rust_bridge.h"

// 菜单节点实现
class context_dr_menu_node : public contextmenu_item_node_root_leaf {
  private:
    unsigned m_index;
    pfc::string8 m_name;

  public:
    context_dr_menu_node(unsigned index, const char* name) : m_index(index), m_name(name) {}

    bool get_display_data(pfc::string_base& p_out, unsigned& p_displayflags,
                          metadb_handle_list_cref p_data, const GUID& p_caller) override {
        console::printf("MacinMeter DR: Menu node get_display_data called for: %s",
                        m_name.get_ptr());
        p_out = m_name;
        p_displayflags = 0;
        return true;
    }

    void execute(metadb_handle_list_cref p_data, const GUID& p_caller) override {
        console::printf("MacinMeter DR: Executing menu item %u: %s", m_index, m_name.get_ptr());

        try {
            if (m_index == 0) {
                // 单个文件DR分析
                execute_dr_analysis_single(p_data);
            } else if (m_index == 1) {
                // 批量DR分析
                execute_dr_analysis_batch(p_data);
            }
        } catch (const std::exception& e) {
            console::printf("MacinMeter DR Plugin Error: %s", e.what());
            popup_message::g_complain("MacinMeter DR Plugin",
                                      PFC_string_formatter()
                                          << "Error analyzing dynamic range: " << e.what());
        }
    }

  private:
    // 执行单个文件DR分析（使用foobar2000解码器）
    void execute_dr_analysis_single(metadb_handle_list_cref data) {
        if (data.get_count() == 0)
            return;

        console::printf(
            "MacinMeter DR: Analyzing %d track(s) using packet-by-packet DR analysis...",
            (int)data.get_count());

        // 直接进行DR分析（逐包会话模式）
        AudioAccessor accessor;
        auto results = accessor.analyze_dr_data_list(data);

        if (results.empty()) {
            popup_message::g_complain(
                "MacinMeter DR Plugin",
                "No valid DR analysis results from packet-by-packet processing");
            return;
        }

        // 从 DR结果创建简化的AudioData对象以兼容显示接口
        std::vector<AudioData> audio_data_list;
        audio_data_list.reserve(results.size());

        for (const auto& result : results) {
            AudioData audio_data;
            audio_data.file_name = std::string(result.file_name);
            audio_data.sample_rate = result.sample_rate;
            audio_data.channels = result.channels;
            audio_data.duration = result.duration_seconds;
            audio_data.sample_count = result.total_samples;
            // samples留空，因为逐包模式不保存所有样本

            audio_data_list.push_back(audio_data);

            console::printf("MacinMeter DR: %s - DR%.0f (packet-by-packet analysis)",
                            result.file_name, result.official_dr_value);
        }

        // 显示结果
        if (!results.empty()) {
            ResultsDialog dialog;
            dialog.show_results(results, audio_data_list);
        } else {
            popup_message::g_complain("MacinMeter DR Plugin",
                                      "Failed to analyze any of the decoded audio tracks");
        }
    }

    // 执行批量DR分析（使用foobar2000解码器）
    void execute_dr_analysis_batch(metadb_handle_list_cref data) {
        if (data.get_count() == 0)
            return;

        console::printf("MacinMeter DR: Starting batch analysis for %d track(s) using "
                        "packet-by-packet analysis...",
                        (int)data.get_count());

        // 直接进行批量DR分析（逐包会话模式）
        AudioAccessor accessor;
        auto results = accessor.analyze_dr_data_list(data);

        if (results.empty()) {
            popup_message::g_complain("MacinMeter DR Plugin",
                                      "Batch analysis failed - no tracks could be processed using "
                                      "packet-by-packet analysis");
            return;
        }

        // 从 DR结果创建简化的AudioData对象以兼容显示接口
        std::vector<AudioData> audio_data_list;
        audio_data_list.reserve(results.size());

        for (const auto& result : results) {
            AudioData audio_data;
            audio_data.file_name = std::string(result.file_name);
            audio_data.sample_rate = result.sample_rate;
            audio_data.channels = result.channels;
            audio_data.duration = result.duration_seconds;
            audio_data.sample_count = result.total_samples;
            // samples留空，因为逐包模式不保存所有样本

            audio_data_list.push_back(audio_data);

            console::printf("MacinMeter DR: %s - DR%.0f (batch packet-by-packet analysis)",
                            result.file_name, result.official_dr_value);
        }

        // 显示批量结果
        console::printf(
            "MacinMeter DR: Batch analysis completed - %d track(s) processed successfully",
            (int)results.size());

        ResultsDialog dialog;
        dialog.show_results(results, audio_data_list);
    }

    bool get_description(pfc::string_base& p_out) override {
        if (m_index == 0) {
            p_out = "Analyze dynamic range using MacinMeter's high-precision DR engine (foobar2000 "
                    "compatible)";
        } else {
            p_out = "Batch analyze dynamic range and generate comprehensive report with MacinMeter "
                    "DR engine";
        }
        return true;
    }

    GUID get_guid() override {
        static const GUID guids[] = {
            {0xe1f2a3b4, 0xc5d6, 0xe7f8, {0xa9, 0xb0, 0xc1, 0xd2, 0xe3, 0xf4, 0xa5, 0xb6}},
            {0xf2a3b4c5, 0xd6e7, 0xf8a9, {0xb0, 0xc1, 0xd2, 0xe3, 0xf4, 0xa5, 0xb6, 0xc7}}};
        return guids[m_index];
    }

    bool is_mappable_shortcut() override {
        return true;
    }
};

// 右键菜单项实现
class context_dr_meter : public contextmenu_item {
  public:
    // 菜单项数量
    enum { cmd_analyze_dr_single = 0, cmd_analyze_dr_batch = 1, cmd_total };

    // 获取菜单项数量
    unsigned get_num_items() override {
        console::printf("MacinMeter DR: get_num_items() returning %u", (unsigned)cmd_total);
        return cmd_total;
    }

    // 创建菜单节点实例（关键方法）
    contextmenu_item_node_root* instantiate_item(unsigned p_index, metadb_handle_list_cref p_data,
                                                 const GUID& p_caller) override {
        console::printf("MacinMeter DR: instantiate_item called with index=%u, data_count=%u",
                        p_index, (unsigned)p_data.get_count());

        if (p_index >= cmd_total)
            return nullptr;

        const char* name = (p_index == cmd_analyze_dr_single) ? "MacinMeter DR Analysis"
                                                              : "MacinMeter DR Batch Analysis";
        return new context_dr_menu_node(p_index, name);
    }

    // 获取菜单项名称
    void get_item_name(unsigned index, pfc::string_base& out) override {
        switch (index) {
        case cmd_analyze_dr_single:
            out = "MacinMeter DR Analysis";
            break;
        case cmd_analyze_dr_batch:
            out = "MacinMeter DR Batch Analysis";
            break;
        default:
            uBugCheck();
        }
    }

    // 获取菜单项默认路径
    void get_item_default_path(unsigned index, pfc::string_base& out) override {
        // 直接在右键菜单顶层显示，不创建子菜单
        out = "";
    }

    // 获取菜单项描述
    bool get_item_description(unsigned index, pfc::string_base& out) override {
        switch (index) {
        case cmd_analyze_dr_single:
            out = "Analyze dynamic range using MacinMeter's high-precision DR engine (foobar2000 "
                  "compatible)";
            return true;
        case cmd_analyze_dr_batch:
            out = "Batch analyze dynamic range and generate comprehensive report with MacinMeter "
                  "DR engine";
            return true;
        default:
            return false;
        }
    }

    // 获取菜单项GUID
    GUID get_item_guid(unsigned index) override {
        static const GUID guids[cmd_total] = {
            // {E1F2A3B4-C5D6-E7F8-A9B0-C1D2E3F4A5B6}
            {0xe1f2a3b4, 0xc5d6, 0xe7f8, {0xa9, 0xb0, 0xc1, 0xd2, 0xe3, 0xf4, 0xa5, 0xb6}},
            // {F2A3B4C5-D6E7-F8A9-B0C1-D2E3F4A5B6C7}
            {0xf2a3b4c5, 0xd6e7, 0xf8a9, {0xb0, 0xc1, 0xd2, 0xe3, 0xf4, 0xa5, 0xb6, 0xc7}}};
        return guids[index];
    }

    // 获取启用状态（新增必需方法）
    t_enabled_state get_enabled_state(unsigned p_index) override {
        console::printf("MacinMeter DR: get_enabled_state called for index %u", p_index);
        return DEFAULT_ON; // 默认启用
    }

    // 简单执行方法（新增必需方法）
    void item_execute_simple(unsigned p_index, const GUID& p_node, metadb_handle_list_cref p_data,
                             const GUID& p_caller) override {
        console::printf("MacinMeter DR: item_execute_simple called for index %u", p_index);
        // 这里可以调用实际的DR分析功能
        // 暂时只打印日志
    }

  private:
    // 检查是否为音频文件 (简化实现 - 先让菜单总是显示)
    bool is_audio_file(const char* path) {
        // 暂时总是返回true，让菜单显示
        // 实际的格式检查在AudioAccessor中进行
        return true;

        // TODO: 使用foobar2000的正确API检查音频格式
        /*
        pfc::string8 path_str(path);

        // 查找最后一个点号
        t_size last_dot = path_str.find_last('.');
        if (last_dot == pfc_infinite) return false;

        // 提取扩展名并转为小写
        pfc::string8 ext;
        path_str.subString(ext, last_dot + 1);
        ext.toLower();

        // 支持的音频格式
        const char* supported_formats[] = {
            "flac", "mp3", "wav", "aac", "m4a", "ogg", "wma", "ape", "wv"
        };

        for (const char* format : supported_formats) {
            if (ext.equals(format)) return true;
        }

        return false;
        */
    }

    // 执行单个文件DR分析（使用foobar2000解码器）
    void execute_dr_analysis_single(metadb_handle_list_cref data) {
        if (data.get_count() == 0)
            return;

        console::printf(
            "MacinMeter DR: Analyzing %d track(s) using packet-by-packet DR analysis...",
            (int)data.get_count());

        // 直接进行DR分析（逐包会话模式）
        AudioAccessor accessor;
        auto results = accessor.analyze_dr_data_list(data);

        if (results.empty()) {
            popup_message::g_complain(
                "MacinMeter DR Plugin",
                "No valid DR analysis results from packet-by-packet processing");
            return;
        }

        // 从 DR结果创建简化的AudioData对象以兼容显示接口
        std::vector<AudioData> audio_data_list;
        audio_data_list.reserve(results.size());

        for (const auto& result : results) {
            AudioData audio_data;
            audio_data.file_name = std::string(result.file_name);
            audio_data.sample_rate = result.sample_rate;
            audio_data.channels = result.channels;
            audio_data.duration = result.duration_seconds;
            audio_data.sample_count = result.total_samples;
            // samples留空，因为逐包模式不保存所有样本

            audio_data_list.push_back(audio_data);

            console::printf("MacinMeter DR: %s - DR%.0f (packet-by-packet analysis)",
                            result.file_name, result.official_dr_value);
        }

        // 显示结果
        if (!results.empty()) {
            ResultsDialog dialog;
            dialog.show_results(results, audio_data_list);
        } else {
            popup_message::g_complain("MacinMeter DR Plugin",
                                      "Failed to analyze any of the decoded audio tracks");
        }
    }

    // 执行批量DR分析（使用foobar2000解码器）
    void execute_dr_analysis_batch(metadb_handle_list_cref data) {
        if (data.get_count() == 0)
            return;

        console::printf("MacinMeter DR: Starting batch analysis for %d track(s) using "
                        "packet-by-packet analysis...",
                        (int)data.get_count());

        try {
            // 直接进行批量DR分析（逐包会话模式）
            AudioAccessor accessor;
            auto results = accessor.analyze_dr_data_list(data);

            if (results.empty()) {
                popup_message::g_complain("MacinMeter DR Plugin",
                                          "Batch analysis failed - no tracks could be processed "
                                          "using packet-by-packet analysis");
                return;
            }

            // 从 DR结果创建简化的AudioData对象以兼容显示接口
            std::vector<AudioData> audio_data_list;
            audio_data_list.reserve(results.size());

            for (size_t i = 0; i < results.size(); ++i) {
                const auto& result = results[i];
                AudioData audio_data;
                audio_data.file_name = std::string(result.file_name);
                audio_data.sample_rate = result.sample_rate;
                audio_data.channels = result.channels;
                audio_data.duration = result.duration_seconds;
                audio_data.sample_count = result.total_samples;
                // samples留空，因为逐包模式不保存所有样本

                audio_data_list.push_back(audio_data);

                console::printf("MacinMeter DR: %d/%d: %s - DR%.0f (packet-by-packet)",
                                (int)(i + 1), (int)results.size(), result.file_name,
                                result.official_dr_value);
            }

            console::print(
                "MacinMeter DR: Generating batch report from packet-by-packet analysis...");

            // 生成批量报告
            ResultsDialog dialog;
            dialog.show_batch_results(results, audio_data_list);
            console::printf("MacinMeter DR: Batch analysis completed successfully (%d tracks, "
                            "packet-by-packet mode)",
                            (int)results.size());
        } catch (const std::exception& e) {
            console::printf("MacinMeter DR: Batch analysis error: %s", e.what());
            popup_message::g_complain("MacinMeter DR Plugin", PFC_string_formatter()
                                                                  << "Batch analysis failed: "
                                                                  << e.what());
        } catch (...) {
            console::print("MacinMeter DR: Unknown batch analysis error");
            popup_message::g_complain("MacinMeter DR Plugin",
                                      "Batch analysis failed with unknown error");
        }
    }
};

// 注册右键菜单
static service_factory_single_t<context_dr_meter> g_context_dr_meter;