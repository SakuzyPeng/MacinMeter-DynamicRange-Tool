#pragma once
#include "foobar2000.h"
#include "rust_bridge.h"
#include "audio_accessor.h"
#include <vector>

// 结果显示对话框类（使用foobar2000解码的音频数据）
class ResultsDialog {
public:
    // 显示单个或多个文件的DR分析结果
    void show_results(const std::vector<DrAnalysisResult>& results, 
                     const std::vector<AudioData>& audio_data_list);
    
    // 显示批量分析结果
    void show_batch_results(const std::vector<DrAnalysisResult>& results,
                           const std::vector<AudioData>& audio_data_list);

private:
    // 生成结果文本
    std::string generate_results_text(const std::vector<DrAnalysisResult>& results,
                                     const std::vector<AudioData>& audio_data_list,
                                     bool batch_mode = false);
    
    // 格式化单个结果
    std::string format_single_result(const DrAnalysisResult& result, 
                                    const AudioData& audio);
    
    // 生成批量统计信息
    std::string generate_batch_statistics(const std::vector<DrAnalysisResult>& results);
    
    // 保存结果到文件
    bool save_results_to_file(const std::string& content, const std::vector<AudioData>& audio_data_list);
    
    // 显示结果对话框
    void show_results_dialog(const std::string& title, const std::string& content, 
                            const std::vector<AudioData>& audio_data_list);
    
    // 获取时间戳字符串
    std::string get_timestamp_string();
};