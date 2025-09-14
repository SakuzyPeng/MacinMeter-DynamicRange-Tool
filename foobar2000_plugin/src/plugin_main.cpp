#include "foobar2000.h"
#include "rust_bridge.h"

// 插件组件声明
DECLARE_COMPONENT_VERSION(
    "MacinMeter DR Meter",
    "1.0.0",
    "High-precision Dynamic Range analysis plugin for foobar2000\n"
    "Based on foobar2000 DR Meter reverse engineering\n"
    "Developed with Rust for maximum performance and accuracy"
);

// 插件初始化组件
class component_dr_init : public initquit {
public:
    void on_init() override {
        console::print("MacinMeter DR Plugin: Initializing...");
        
        // 初始化Rust DR计算引擎
        if (rust_dr_engine_init() == 0) {
            console::print("MacinMeter DR Plugin: Rust engine initialized successfully");
        } else {
            console::print("MacinMeter DR Plugin: Failed to initialize Rust engine");
        }
    }

    void on_quit() override {
        console::print("MacinMeter DR Plugin: Shutting down...");
        
        // 清理Rust引擎资源
        rust_dr_engine_cleanup();
        
        console::print("MacinMeter DR Plugin: Shutdown complete");
    }
};

// 注册初始化组件
static initquit_factory_t<component_dr_init> g_init_factory;