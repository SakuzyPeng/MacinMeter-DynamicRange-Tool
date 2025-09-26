#include "context_menu.h"
#include "foobar2000.h"
#include "progress_worker.h"

// 🎯 MacinMeter DR菜单组GUID
static const GUID guid_macinmeter_group = {
    0xb8c5a9f0, 0x8f5a, 0x4b2a, {0x9c, 0x7d, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab}};

// ====================================================================
// 🚀 现代异步DR分析器 - Rust线程管理的革命性设计
// ====================================================================

// 🎯 不再需要AsyncDrAnalysis类，已经由MacinMeterProgressWorker替代

// 🎯 创建弹出菜单组（按SDK标准）
static contextmenu_group_popup_factory
    g_macinmeter_group(guid_macinmeter_group, contextmenu_groups::root, "MacinMeter DR", 0);

// 🎯 标准菜单项实现（按SDK模式）
class context_dr_menu : public contextmenu_item_simple {
  public:
    enum { cmd_analyze = 0, cmd_total };

    // ✅ 关键：指定父菜单组
    GUID get_parent() override {
        return guid_macinmeter_group;
    }

    unsigned get_num_items() override {
        return cmd_total;
    }

    void get_item_name(unsigned p_index, pfc::string_base& p_out) override {
        switch (p_index) {
        case cmd_analyze:
            p_out = "Analyze Dynamic Range";
            break;
        default:
            uBugCheck();
        }
    }

    void context_command(unsigned p_index, metadb_handle_list_cref p_data,
                         const GUID& p_caller) override {
        switch (p_index) {
        case cmd_analyze:
            execute_dr_analysis(p_data);
            break;
        default:
            uBugCheck();
        }
    }

    GUID get_item_guid(unsigned p_index) override {
        static const GUID guid_analyze = {
            0xb8c5a9f1, 0x8f5a, 0x4b2a, {0x9c, 0x7d, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab}};

        switch (p_index) {
        case cmd_analyze:
            return guid_analyze;
        default:
            uBugCheck();
        }
    }

    bool get_item_description(unsigned p_index, pfc::string_base& p_out) override {
        switch (p_index) {
        case cmd_analyze:
            p_out = "High-precision Dynamic Range analysis compatible with foobar2000 DR Meter";
            return true;
        default:
            return false;
        }
    }

  private:
    // 🚀 现代异步分析：Rust管理一切，零复杂性
    void execute_dr_analysis(metadb_handle_list_cref data) {
        if (data.get_count() == 0) {
            popup_message::g_complain("MacinMeter DR", "No tracks selected for analysis");
            return;
        }

        // 🚀 使用官方threaded_process进度对话框
        // 目前支持单文件分析，第一个文件
        MacinMeterProgressWorker::startAnalysis(data[0]);

        // 🎯 立即返回，UI永不阻塞，进度由threaded_process托管
    }
};

// 🎯 注册菜单项（使用SDK标准factory）
static contextmenu_item_factory_t<context_dr_menu> g_contextmenu_item_factory;