#include "context_menu.h"
#include "../core/dr_analysis_controller.h"
#include "foobar2000.h"
#include "results_dialog.h"
#include "progress_dialog.h"

// 🎯 MacinMeter DR菜单组GUID
static const GUID guid_macinmeter_group = { 0xb8c5a9f0, 0x8f5a, 0x4b2a, { 0x9c, 0x7d, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab } };

// 🎯 创建弹出菜单组（按SDK标准）
static contextmenu_group_popup_factory g_macinmeter_group(guid_macinmeter_group, contextmenu_groups::root, "MacinMeter DR", 0);

// 🎯 标准菜单项实现（按SDK模式）
class context_dr_menu : public contextmenu_item_simple {
public:
    enum {
        cmd_analyze = 0,
        cmd_total
    };

    // ✅ 关键：指定父菜单组
    GUID get_parent() override { return guid_macinmeter_group; }

    unsigned get_num_items() override { return cmd_total; }

    void get_item_name(unsigned p_index, pfc::string_base& p_out) override {
        switch(p_index) {
            case cmd_analyze:
                p_out = "Analyze Dynamic Range";
                break;
            default:
                uBugCheck();
        }
    }

    void context_command(unsigned p_index, metadb_handle_list_cref p_data, const GUID& p_caller) override {
        switch(p_index) {
            case cmd_analyze:
                execute_dr_analysis(p_data);
                break;
            default:
                uBugCheck();
        }
    }

    GUID get_item_guid(unsigned p_index) override {
        static const GUID guid_analyze = { 0xb8c5a9f1, 0x8f5a, 0x4b2a, { 0x9c, 0x7d, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab } };

        switch(p_index) {
            case cmd_analyze: return guid_analyze;
            default: uBugCheck();
        }
    }

    bool get_item_description(unsigned p_index, pfc::string_base& p_out) override {
        switch(p_index) {
            case cmd_analyze:
                p_out = "High-precision Dynamic Range analysis compatible with foobar2000 DR Meter";
                return true;
            default:
                return false;
        }
    }

  private:
    // 🛡️ 极简稳定版本：避免所有复杂性，专注不崩溃
    void execute_dr_analysis(metadb_handle_list_cref data) {
        if (data.get_count() == 0) {
            popup_message::g_complain("MacinMeter DR", "No tracks selected for analysis");
            return;
        }

        // 🎯 使用最简单的异步分析器（已验证稳定）
        StableAsyncAnalyzer::startAsync(data,
            [](const std::string& result_text, bool success) {
                // 🚀 直接显示结果（期望popup_message是线程安全的）
                if (success) {
                    popup_message::g_show(result_text.c_str(), "MacinMeter DR Analysis Result");
                } else {
                    popup_message::g_complain("MacinMeter DR", result_text.c_str());
                }
            });

        // 🎯 立即返回，零复杂性
    }

};

// 🎯 注册菜单项（使用SDK标准factory）
static contextmenu_item_factory_t<context_dr_menu> g_contextmenu_item_factory;