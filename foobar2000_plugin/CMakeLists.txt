cmake_minimum_required(VERSION 3.20)
project(MacinMeter_DR_Plugin)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# 🚀 强制Release构建（优化大小）
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

# 平台特定设置
if(APPLE)
    message(STATUS "Building for macOS foobar2000")
    set(CMAKE_OSX_DEPLOYMENT_TARGET "11.0")
    # 使用当前Mac的原生架构
    set(CMAKE_OSX_ARCHITECTURES "arm64")
endif()

# foobar2000 SDK 路径
set(FOOBAR2000_SDK_PATH "${CMAKE_CURRENT_SOURCE_DIR}/../SDK-2025-03-07")

# 包含foobar2000 SDK头文件（标记为SYSTEM以减少IDE误报）
include_directories(SYSTEM
    "${FOOBAR2000_SDK_PATH}"
    "${FOOBAR2000_SDK_PATH}/foobar2000"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK"
    "${FOOBAR2000_SDK_PATH}/foobar2000/helpers"
)

# 平台特定的预定义宏
if(APPLE)
    add_definitions(-DMAC_VERSION)
    # 🎯 平衡的大小优化（保持异常处理，目标：减少30%的文件大小）
    set(CMAKE_CXX_FLAGS_RELEASE "${CMAKE_CXX_FLAGS_RELEASE} -Os -flto -ffunction-sections -fdata-sections -fvisibility=hidden -DNDEBUG")
    set(CMAKE_SHARED_LINKER_FLAGS_RELEASE "${CMAKE_SHARED_LINKER_FLAGS_RELEASE} -Wl,-dead_strip -Wl,-x -flto -Wl,-S")
elseif(WIN32)
    add_definitions(-DUNICODE -D_UNICODE -DWIN32_LEAN_AND_MEAN)
endif()

# PFC源文件
set(PFC_SOURCES
    "${FOOBAR2000_SDK_PATH}/pfc/printf.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/string_base.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/string-lite.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/string-compare.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/other.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/pathUtils.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/splitString2.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/utf8.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/bit_array.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/sort.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/crashWithMessage.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/timers.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/nix-objects.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/synchro_nix.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/pfc-fb2k-hooks.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/unicode-normalize.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/audio_math.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/audio_sample.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/wildcard.cpp"
)

# SDK源文件
set(SDK_SOURCES
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/service.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/console.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/popup_message.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/completion_notify.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/input.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/guids.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/filesystem.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/filesystem_helper.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/file_info.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/file_info_impl.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/file_info_merge.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/abort_callback.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/audio_chunk.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/audio_chunk_channel_config.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/playable_location.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/metadb_handle.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/replaygain.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/replaygain_info.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/shared/shared-nix.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/shared/audio_math.cpp"
    "${FOOBAR2000_SDK_PATH}/foobar2000/foobar2000_component_client/component_client.cpp"
    "${FOOBAR2000_SDK_PATH}/pfc/guid.cpp"
    ${PFC_SOURCES}
)

# 插件源文件
set(PLUGIN_SOURCES
    src/plugin_main.cpp
    src/context_menu.cpp
    src/audio_accessor.cpp
    src/rust_bridge.cpp
    src/results_dialog.cpp
    src/foobar2000_stub.cpp
    ${SDK_SOURCES}
)

# 添加custom target来构建Rust核心库
set(RUST_CORE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/rust_core")
set(RUST_BUILD_DIR "${RUST_CORE_PATH}/target/release")

if(APPLE)
    set(RUST_LIB_NAME "libmacinmeter_dr_core.dylib")
    set(RUST_BUILD_TARGET "")  # 默认target，使用本机架构
elseif(WIN32)
    set(RUST_LIB_NAME "macinmeter_dr_core.dll")
    set(RUST_BUILD_TARGET "--target x86_64-pc-windows-msvc")
endif()

set(RUST_LIB_FULL_PATH "${RUST_BUILD_DIR}/${RUST_LIB_NAME}")

# 获取Rust源文件列表用于依赖检查
file(GLOB_RECURSE RUST_SOURCES
    "${RUST_CORE_PATH}/src/*.rs"
)
list(APPEND RUST_SOURCES
    "${RUST_CORE_PATH}/Cargo.toml"
)

# 创建Rust库输出的custom command（带源文件依赖）
add_custom_command(
    OUTPUT ${RUST_LIB_FULL_PATH}
    COMMAND cargo build --release ${RUST_BUILD_TARGET}
    DEPENDS ${RUST_SOURCES}
    WORKING_DIRECTORY ${RUST_CORE_PATH}
    COMMENT "Building Rust core library with output tracking..."
    VERBATIM
)

# 创建基于输出文件的构建目标
add_custom_target(build_rust_core DEPENDS ${RUST_LIB_FULL_PATH})

# 创建导入库目标来表示Rust库
add_library(rust_core_lib SHARED IMPORTED)
set_target_properties(rust_core_lib PROPERTIES
    IMPORTED_LOCATION ${RUST_LIB_FULL_PATH}
)

# 确保Rust库在导入前构建完成
add_dependencies(rust_core_lib build_rust_core)

# 创建共享库/插件
add_library(foo_dr_macinmeter SHARED ${PLUGIN_SOURCES})

# 为目标补充SYSTEM头，进一步减少IDE/clangd对第三方头的误报
target_include_directories(foo_dr_macinmeter SYSTEM PRIVATE
    "${FOOBAR2000_SDK_PATH}"
    "${FOOBAR2000_SDK_PATH}/foobar2000"
    "${FOOBAR2000_SDK_PATH}/foobar2000/SDK"
    "${FOOBAR2000_SDK_PATH}/foobar2000/helpers"
)

# 确保插件依赖Rust库构建
add_dependencies(foo_dr_macinmeter rust_core_lib)

# 平台特定的库链接
if(APPLE)
    # Mac版本：查找SDK静态库
    find_library(FOOBAR2000_SDK_LIB 
        NAMES libfoobar2000_SDK.a foobar2000_SDK
        PATHS "${FOOBAR2000_SDK_PATH}/foobar2000/SDK"
        NO_DEFAULT_PATH
    )
    
    find_library(FOOBAR2000_HELPERS_LIB
        NAMES libfoobar2000_sdk_helpers.a foobar2000_sdk_helpers  
        PATHS "${FOOBAR2000_SDK_PATH}/foobar2000/helpers"
        NO_DEFAULT_PATH
    )
    
    find_library(PFC_LIB
        NAMES libpfc.a pfc
        PATHS "${FOOBAR2000_SDK_PATH}/pfc"
        NO_DEFAULT_PATH
    )
    
    # 如果找到SDK库就链接，否则使用目标文件
    if(FOOBAR2000_SDK_LIB AND FOOBAR2000_HELPERS_LIB AND PFC_LIB)
        target_link_libraries(foo_dr_macinmeter ${FOOBAR2000_SDK_LIB} ${FOOBAR2000_HELPERS_LIB} ${PFC_LIB})
    else()
        message(WARNING "foobar2000 SDK libraries not found, plugin may need manual linking")
    endif()
    
    # Rust核心库 (通过导入目标链接)
    target_link_libraries(foo_dr_macinmeter rust_core_lib)
    
    # Mac系统框架
    find_library(COCOA_FRAMEWORK Cocoa)
    find_library(COREAUDIO_FRAMEWORK CoreAudio)
    find_library(AUDIOTOOLBOX_FRAMEWORK AudioToolbox)
    target_link_libraries(foo_dr_macinmeter 
        ${COCOA_FRAMEWORK}
        ${COREAUDIO_FRAMEWORK} 
        ${AUDIOTOOLBOX_FRAMEWORK}
    )
    
    # 设置Mac插件属性
    set_target_properties(foo_dr_macinmeter PROPERTIES
        PREFIX ""
        SUFFIX ".dylib"
        OUTPUT_NAME "foo_dr_macinmeter"
        BUNDLE FALSE
        MACOSX_RPATH TRUE
    )

    # 创建foobar2000兼容的macOS Bundle插件包
    add_custom_command(TARGET foo_dr_macinmeter POST_BUILD
        # 创建Bundle目录结构
        COMMAND ${CMAKE_COMMAND} -E make_directory ${CMAKE_CURRENT_BINARY_DIR}/plugin_bundle/mac/foo_dr_macinmeter.component/Contents/MacOS
        COMMAND ${CMAKE_COMMAND} -E make_directory ${CMAKE_CURRENT_BINARY_DIR}/plugin_bundle/mac/foo_dr_macinmeter.component/Contents/Resources

        # 复制主可执行文件
        COMMAND ${CMAKE_COMMAND} -E copy $<TARGET_FILE:foo_dr_macinmeter> ${CMAKE_CURRENT_BINARY_DIR}/plugin_bundle/mac/foo_dr_macinmeter.component/Contents/MacOS/foo_dr_macinmeter

        # 复制Rust核心库到Resources
        COMMAND ${CMAKE_COMMAND} -E copy ${RUST_LIB_FULL_PATH} ${CMAKE_CURRENT_BINARY_DIR}/plugin_bundle/mac/foo_dr_macinmeter.component/Contents/Resources/

        # 复制Info.plist
        COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_SOURCE_DIR}/Info.plist.template ${CMAKE_CURRENT_BINARY_DIR}/plugin_bundle/mac/foo_dr_macinmeter.component/Contents/Info.plist

        # 使用store方法（无压缩）创建插件包
        COMMAND ${CMAKE_COMMAND} -E chdir ${CMAKE_CURRENT_BINARY_DIR}/plugin_bundle zip -0 -r ../foo_dr_macinmeter.fb2k-component mac/

        COMMENT "Creating macOS foobar2000 Bundle plugin package (.fb2k-component)"
    )

elseif(WIN32)
    # Windows版本：使用.lib文件
    target_link_libraries(foo_dr_macinmeter
        "${FOOBAR2000_SDK_PATH}/foobar2000/SDK/foobar2000_SDK.lib"
        "${FOOBAR2000_SDK_PATH}/foobar2000/helpers/foobar2000_sdk_helpers.lib"
        "${FOOBAR2000_SDK_PATH}/pfc/pfc.lib"
        "${FOOBAR2000_SDK_PATH}/libPPUI/libPPUI.lib"
    )
    
    # Rust核心库 (通过导入目标链接)
    target_link_libraries(foo_dr_macinmeter rust_core_lib)
    
    # Windows系统库
    target_link_libraries(foo_dr_macinmeter
        user32 kernel32 gdi32 comctl32 comdlg32 ole32 oleaut32 uuid
    )
    
    # 设置Windows DLL属性
    set_target_properties(foo_dr_macinmeter PROPERTIES
        PREFIX ""
        SUFFIX ".dll"
        OUTPUT_NAME "foo_dr_macinmeter"
    )
endif()

# 复制到foobar2000插件目录（开发时使用）
if(DEFINED FOOBAR2000_PLUGIN_DIR)
    add_custom_command(TARGET foo_dr_macinmeter POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy $<TARGET_FILE:foo_dr_macinmeter> ${FOOBAR2000_PLUGIN_DIR}
        COMMENT "Copying plugin to foobar2000 directory"
    )
endif()