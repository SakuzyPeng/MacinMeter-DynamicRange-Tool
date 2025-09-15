#include "foobar2000.h"
#include <cstdarg>
#include <cstring>
#include <functional>
#include <iostream>
#include <stdexcept>
#include <strings.h>
#include <thread>
#include <unistd.h>

// 只提供链接时真正缺失的符号实现
// 已验证：所有保留的函数都是必需的，删除会导致链接错误
// - stricmp_utf8_ex: 字符串比较函数，Mac系统缺失
// - apple*函数: Mac特定的辅助函数
// - album_art GUID: input_entry需要这些符号
// - filesystem函数: SDK引用但未实现
// - fb2k函数: SDK引用但未实现

// contextmenu_item implementation
GUID contextmenu_item::get_parent_fallback() {
    return pfc::guid_null;
}

// pfc::stringLite 函数现在在string-lite.cpp中定义了

// fb2k命名空间函数（只保留非冲突的）
namespace fb2k {
void inMainThread(std::function<void()> func) {
    // 简化实现：直接执行
    if (func) {
        func();
    }
}
} // namespace fb2k

// Mac特定函数实现（只保留非冲突的）
namespace pfc {
void appleDebugLog(const char* msg) {
    std::cerr << "[Apple Debug] " << msg << std::endl;
}

void appleSetThreadDescription(const char* desc) {
    // Mac thread naming - simplified
    (void)desc; // suppress unused parameter warning
}
} // namespace pfc

// Apple特定函数的简化实现
namespace pfc {
int appleNaturalSortCompareI(const char* str1, const char* str2) {
    // 简化实现：使用标准字符串比较
    return strcasecmp(str1, str2);
}

int appleNaturalSortCompare(const char* str1, const char* str2) {
    // 简化实现：使用标准字符串比较
    return strcmp(str1, str2);
}
} // namespace pfc

// 缺失的字符串比较函数 - 使用C linkage，函数名不带前导下划线
extern "C" int stricmp_utf8_ex(const char* str1, size_t len1, const char* str2,
                               size_t len2) throw() {
    // 简化实现：忽略长度，使用不区分大小写的UTF-8比较（此处用strcasecmp占位）
    (void)len1;
    (void)len2; // suppress unused parameter warnings
    return strcasecmp(str1, str2);
}

// audio_math函数可能在头文件中定义或者需要从不同的源文件中链接

// pfc文件处理函数
namespace pfc {
const char* unicodeNormalizeC(const char* str) {
    // 简化实现：直接返回原字符串
    return str;
}

fileHandle_t fileHandleDup(fileHandle_t handle) {
    return dup(handle);
}

void fileHandleClose(fileHandle_t handle) noexcept {
    if (handle != -1) {
        close(handle);
    }
}

// fileHandle成员函数实现
void fileHandle::close() noexcept {
    if (h != fileHandleInvalid) {
        ::close(h);
        h = fileHandleInvalid;
    }
}
} // namespace pfc

// foobar2000_io namespace functions
namespace foobar2000_io {

// filesystem类的静态成员函数（提供安全实现）
fsItemFile::ptr filesystem::makeItemFileStd(const char* pathCanonical, const t_filestats2& stats) {
    // 安全实现：数据分析插件不需要文件系统操作
    throw std::runtime_error("makeItemFileStd not implemented in DR analysis plugin");
}

fsItemFolder::ptr filesystem::makeItemFolderStd(const char* pathCanonical,
                                                const t_filestats2& stats) {
    // 安全实现：数据分析插件不需要文件系统操作
    throw std::runtime_error("makeItemFolderStd not implemented in DR analysis plugin");
}

// fsItemFile实现
service_ptr_t<file> fsItemFile::openRead(abort_callback& abort) {
    // 安全实现：数据分析插件使用foobar2000的解码器
    throw std::runtime_error("fsItemFile::openRead not implemented in DR analysis plugin");
}

} // namespace foobar2000_io

// album_art相关GUID - input_entry需要这些符号
GUID album_art_editor::get_guid() {
    static const GUID guid = {
        0xabcd1234, 0x5678, 0x9abc, {0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc}};
    return guid;
}

GUID album_art_extractor::get_guid() {
    static const GUID guid = {
        0xbcda2345, 0x6789, 0xabcd, {0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd}};
    return guid;
}

// fb2k命名空间函数的实现（只提供缺失的实现）
namespace fb2k {
// 安全实现：禁止返回nullptr，改为抛异常
service_ptr_t<arrayMutable> arrayMutable::arrayWithCapacity(unsigned long capacity) {
    throw std::runtime_error(
        "arrayMutable::arrayWithCapacity not implemented in DR analysis plugin");
}

service_ptr_t<string> string::stringWithString(const char* str) {
    throw std::runtime_error("string::stringWithString not implemented in DR analysis plugin");
}

service_ptr_t<memBlock> memBlock::blockWithData(pfc::mem_block&& data) {
    throw std::runtime_error("memBlock::blockWithData not implemented in DR analysis plugin");
}

service_ptr_t<array> array::makeConst() const {
    throw std::runtime_error("array::makeConst not implemented in DR analysis plugin");
}
} // namespace fb2k