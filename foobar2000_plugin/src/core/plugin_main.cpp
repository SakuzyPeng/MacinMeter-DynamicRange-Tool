#include "../bridge/rust_bridge.h"
#include "foobar2000.h"

// 插件组件声明
DECLARE_COMPONENT_VERSION("MacinMeter DR Meter", "1.0.0",
                          "High-precision Dynamic Range analysis plugin for foobar2000\n"
                          "Based on foobar2000 DR Meter reverse engineering\n"
                          "Developed with Rust for maximum performance and accuracy");

// 🎯 插件初始化组件（零配置设计）
class component_dr_init : public initquit {
  public:
    void on_init() override {
        console::print("MacinMeter DR Plugin: Initialized (zero-config design)");
        console::print(
            "MacinMeter DR Plugin: Ready for DR analysis with auto-optimized performance");
    }

    void on_quit() override {
        console::print("MacinMeter DR Plugin: Shutdown complete");
    }
};

// 注册初始化组件
static initquit_factory_t<component_dr_init> g_init_factory;