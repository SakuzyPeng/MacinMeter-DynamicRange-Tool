# 警告：请将此脚本文件另存为 "UTF-8 with BOM" 编码，否则中文会显示为乱码。
# (在记事本中：文件 -> 另存为 -> 编码 -> "UTF-8-BOM")

# 将所有操作放入一个 try...catch...finally 结构中
try {
    # --- 配置 ---
    $executablePrefix = "MacinMeter"
    $sampleInterval = 1 # 内存采样间隔（秒）
    $runCount = 10      # 运行次数

    # 定义常见的音频文件扩展名 (PowerShell 数组)
    # 注意：所有扩展名都应为小写
    $audioExtensions = @(
        ".flac", ".wav", ".mp3", ".m4a", ".aac", 
        ".ogg", ".opus", ".aiff", ".ape", ".dsf", ".dff"
    )

    # 辅助函数：计算中位数
    function Get-Median {
        param([double[]]$numbers)
        
        if ($numbers.Count -eq 0) { return 0 }

        $sortedNumbers = $numbers | Sort-Object
        $count = $sortedNumbers.Count
        $midPoint = [Math]::Floor($count / 2)

        if ($count % 2 -eq 0) {
            # 偶数个，取中间两个的平均值
            return ($sortedNumbers[$midPoint - 1] + $sortedNumbers[$midPoint]) / 2
        } else {
            # 奇数个，取正中间的值
            return $sortedNumbers[$midPoint]
        }
    }

    # (*** 新 ***) 辅助函数：手写标准差 (Sample Standard Deviation)
    function Get-StandardDeviation {
        param([double[]]$numbers)
        
        $count = $numbers.Count
        # 样本数必须大于1才能计算标准差
        if ($count -le 1) { return $null }

        # 1. 计算平均值
        $sum = 0.0
        foreach ($num in $numbers) { $sum += $num }
        $average = $sum / $count

        # 2. 计算 (number - average)^2 的总和
        $sumOfSquares = 0.0
        foreach ($num in $numbers) {
            $sumOfSquares += [Math]::Pow($num - $average, 2)
        }

        # 3. 除以 (n-1) (这是"样本"标准差，是基准测试的标准做法)
        $variance = $sumOfSquares / ($count - 1)
        
        # 4. 开方
        return [Math]::Sqrt($variance)
    }

    # --- 脚本主体 ---
    Write-Host "正在当前目录查找以 '$executablePrefix' 开头的程序 (.exe)..." -ForegroundColor Yellow

    # 1. 查找可执行文件 (只需执行一次)
    # --------------------------------------------------
    $executableFile = Get-ChildItem -Path $PSScriptRoot -Filter "$executablePrefix*.exe"

    if ($null -eq $executableFile) {
        throw "错误: 未在脚本目录中找到以 '$executablePrefix' 开头的 .exe 文件。"
    }

    if ($executableFile.Count -gt 1) {
        throw "错误: 找到多个匹配的可执行文件，无法确定启动哪一个。"
    }

    Write-Host "找到目标程序: $($executableFile.Name)" -ForegroundColor Green

    # 2. (新) 预扫描音频文件
    # --------------------------------------------------
    Write-Host "正在扫描当前目录的音频文件..."
    $audioFiles = Get-ChildItem -Path . -File | Where-Object { 
        $audioExtensions -contains $_.Extension.ToLower() 
    }
    
    $audioFileCount = $audioFiles.Count
    if ($audioFileCount -eq 0) {
        throw "错误: 在当前目录中未找到任何音频文件，测试无法开始。"
    }

    $totalAudioSizeBytes = ($audioFiles | Measure-Object -Property Length -Sum).Sum
    $totalAudioSizeMb = $totalAudioSizeBytes / 1MB # 1MB 是 PowerShell 内置常量

    Write-Host ("找到 {0} 个音频文件，总大小 {1:N2} MB" -f $audioFileCount, $totalAudioSizeMb) -ForegroundColor Green
    Write-Host "准备执行 $runCount 轮基准测试..."
    Write-Host ""


    # (新) 创建列表来存储所有运行的结果
    $allRuntimesMs = [System.Collections.Generic.List[double]]::new()
    $allPeakMemoriesKb = [System.Collections.Generic.List[double]]::new()
    $allAverageMemoriesKb = [System.Collections.Generic.List[double]]::new()
    $allSpeedsMbPerSec = [System.Collections.Generic.List[double]]::new()


    # 3. 循环运行基准测试
    # --------------------------------------------------
    for ($i = 1; $i -le $runCount; $i++) {
        Write-Host "======================= 正在执行第 $i / $runCount 轮 =======================" -ForegroundColor Cyan
        
        # --- 3a. 启动与监控 ---
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $executableFile.FullName -PassThru -ErrorAction Stop
        
        if ($null -eq $process) {
            throw "错误：第 $i 轮启动进程 '$($executableFile.Name)' 失败！"
        }

        Write-Host "程序已启动 (PID: $($process.Id))。正在监控..."
        
        $memorySamples = [System.Collections.Generic.List[double]]::new()
        $runPeakMemoryKb = 0.0
        $runAverageMemoryKb = 0.0

        while (-not $process.HasExited) {
            try {
                $process.Refresh()
                $currentMemKb = $process.WorkingSet64 / 1024
                $memorySamples.Add($currentMemKb)
            } catch {
                Write-Host " (进程已在采样间隙退出) " -NoNewline -ForegroundColor Yellow
                break
            }
            Start-Sleep -Seconds $sampleInterval
        }

        $stopwatch.Stop()
        $elapsedTimeSpan = $stopwatch.Elapsed
        
        Write-Host ""
        Write-Host "程序 (PID: $($process.Id)) 已停止运行。"

        # --- 3b. 计算单轮结果 ---
        if ($memorySamples.Count -gt 0) {
            # 这里的 Measure-Object 只是为了单轮的 Average 和 Maximum
            $stats = $memorySamples | Measure-Object -Average -Maximum
            $runPeakMemoryKb = $stats.Maximum
            $runAverageMemoryKb = $stats.Average
        } else {
            Write-Host "  - 内存使用情况: 运行时间不足 $sampleInterval 秒，未采集到数据。" -ForegroundColor Yellow
        }

        # --- 3c. 打印并存储单轮结果 ---
        $runTimeMs = $elapsedTimeSpan.TotalMilliseconds
        $runTimeSec = $runTimeMs / 1000.0
        $runSpeedMbPerSec = if ($runTimeSec -gt 0) { $totalAudioSizeMb / $runTimeSec } else { 0 }

        $allRuntimesMs.Add($runTimeMs)
        $allPeakMemoriesKb.Add($runPeakMemoryKb)
        $allAverageMemoriesKb.Add($runAverageMemoryKb)
        $allSpeedsMbPerSec.Add($runSpeedMbPerSec)

        Write-Host ("  - 本轮运行时长: {0:N3} ms ({1})" -f $runTimeMs, $elapsedTimeSpan.ToString("hh\:mm\:ss\.fff"))
        Write-Host ("  - 本轮处理速度: {0:N2} MB/s" -f $runSpeedMbPerSec)
        Write-Host ("  - 本轮内存峰值: {0:N2} MB ({1:N0} KB)" -f ($runPeakMemoryKb / 1024), $runPeakMemoryKb)
        Write-Host ("  - 本轮内存均值: {0:N2} MB ({1:N0} KB)" -f ($runAverageMemoryKb / 1024), $runAverageMemoryKb)
        Write-Host "======================= 第 $i / $runCount 轮结束 ========================" -ForegroundColor Cyan
        Write-Host ""
        Start-Sleep -Seconds 1 # 在两次运行之间稍作停顿
    }

    # 4. (新) 计算并生成最终的统计报告
    # --------------------------------------------------
    Write-Host ""
    Write-Host "===================== $runCount 轮运行总结统计报告 =====================" -ForegroundColor Green
    
    # (新) 创建一个字符串列表用于存储报告
    $reportLines = [System.Collections.Generic.List[string]]::new()
    $reportTimestamp = Get-Date -Format "yyyy-MM-dd_HH-mm-ss"
    $reportTimestampFriendly = Get-Date -Format "yyyy-MM-dd HH:mm:ss"

    $reportLines.Add("===================== $runCount 轮运行总结统计报告 =====================")
    $reportLines.Add("  程序 (Executable): $($executableFile.Name)")
    $reportLines.Add("  目标 (Target): $audioFileCount 个音频文件, 总计 $($totalAudioSizeMb.ToString("N2")) MB")
    $reportLines.Add("  报告生成时间: $reportTimestampFriendly")
    $reportLines.Add("") # 空行

    # --- 4a. 总运行时长 (单位: 毫秒) ---
    # 我们仍然用 Measure-Object 来获取 Average，因为它很快
    $timeStats = $allRuntimesMs | Measure-Object -Average
    $medianTimeMs = Get-Median $allRuntimesMs.ToArray()
    # (*** 已修正: 调用我们自己的函数 ***)
    $stdDevTimeMs = Get-StandardDeviation $allRuntimesMs.ToArray()
    
    $stdDevTimeStr = if ($null -ne $stdDevTimeMs) { $stdDevTimeMs.ToString("N3") } else { "N/A" }
    
    $reportLines.Add("  --- 1. 总运行时长 (Time) ---")
    $reportLines.Add("    - 中位数 (Median): $($medianTimeMs.ToString("N3")) ms")
    $reportLines.Add("    - 平均值 (Average): $($timeStats.Average.ToString("N3")) ms")
    $reportLines.Add("    - 标准差 (StdDev) : $stdDevTimeStr ms")
    $reportLines.Add("")

    # --- 4b. 内存使用峰值 (单位: KB) ---
    $peakMemStats = $allPeakMemoriesKb | Measure-Object -Average
    $medianPeakMemKb = Get-Median $allPeakMemoriesKb.ToArray()
    # (*** 已修正: 调用我们自己的函数 ***)
    $stdDevPeakMemKb = Get-StandardDeviation $allPeakMemoriesKb.ToArray()

    $stdDevPeakMemStr = if ($null -ne $stdDevPeakMemKb) {
        "{0:N0} KB ({1:N2} MB)" -f $stdDevPeakMemKb, ($stdDevPeakMemKb / 1024.0)
    } else { "N/A" }

    $reportLines.Add("  --- 2. 内存使用峰值 (Peak Memory) ---")
    $reportLines.Add("    - 中位数 (Median): $($medianPeakMemKb.ToString("N0")) KB ($(($medianPeakMemKb / 1024.0).ToString("N2")) MB)")
    $reportLines.Add("    - 平均值 (Average): $($peakMemStats.Average.ToString("N0")) KB ($(($peakMemStats.Average / 1024.0).ToString("N2")) MB)")
    $reportLines.Add("    - 标准差 (StdDev) : $stdDevPeakMemStr")
    $reportLines.Add("")
    
    # --- 4c. 内存使用平均值 (单位: KB) ---
    $avgMemStats = $allAverageMemoriesKb | Measure-Object -Average
    $medianAvgMemKb = Get-Median $allAverageMemoriesKb.ToArray()
    # (*** 已修正: 调用我们自己的函数 ***)
    $stdDevAvgMemKb = Get-StandardDeviation $allAverageMemoriesKb.ToArray()

    $stdDevAvgMemStr = if ($null -ne $stdDevAvgMemKb) {
        "{0:N0} KB ({1:N2} MB)" -f $stdDevAvgMemKb, ($stdDevAvgMemKb / 1024.0)
    } else { "N/A" }

    $reportLines.Add("  --- 3. 内存使用平均值 (Average Memory) ---")
    $reportLines.Add("    - 中位数 (Median): $($medianAvgMemKb.ToString("N0")) KB ($(($medianAvgMemKb / 1024.0).ToString("N2")) MB)")
    $reportLines.Add("    - 平均值 (Average): $($avgMemStats.Average.ToString("N0")) KB ($(($avgMemStats.Average / 1024.0).ToString("N2")) MB)")
    $reportLines.Add("    - 标准差 (StdDev) : $stdDevAvgMemStr")
    $reportLines.Add("")

    # --- 4d. 处理速度 (单位: MB/s) ---
    $speedStats = $allSpeedsMbPerSec | Measure-Object -Average
    $medianSpeed = Get-Median $allSpeedsMbPerSec.ToArray()
    # (*** 已修正: 调用我们自己的函数 ***)
    $stdDevSpeedMbPerSec = Get-StandardDeviation $allSpeedsMbPerSec.ToArray()

    $stdDevSpeedStr = if ($null -ne $stdDevSpeedMbPerSec) { $stdDevSpeedMbPerSec.ToString("N2") } else { "N/A" }

    $reportLines.Add("  --- 4. 处理速度 (Processing Speed) ---")
    $reportLines.Add("    - 中位数 (Median): $($medianSpeed.ToString("N2")) MB/s")
    $reportLines.Add("    - 平均值 (Average): $($speedStats.Average.ToString("N2")) MB/s")
    $reportLines.Add("    - 标准差 (StdDev) : $stdDevSpeedStr MB/s")
    $reportLines.Add("")

    $reportLines.Add("="*70)

    # --- 4e. (新) 将报告打印到控制台 (带颜色) ---
    $reportLines | ForEach-Object { Write-Host $_ -ForegroundColor Green }


    # --- 4f. (新) 将报告写入文件 ---
    # 获取不带 .exe 的程序基本名称
    $exeBaseName = $executableFile.BaseName 
    $reportFileName = "$($exeBaseName)_$($reportTimestamp).txt"
    
    # 使用 -Encoding Utf8 将报告内容写入文件
    Set-Content -Path $reportFileName -Value $reportLines -Encoding Utf8

    Write-Host ""
    Write-Host "性能报告已保存到: $reportFileName" -ForegroundColor Yellow
    Write-Host "="*70

}
catch {
    # 如果 try {} 代码块中发生任何错误，代码会跳转到这里
    Write-Host ""
    Write-Host "!!!!!!!!!!!!!!!!!!!!!!! 脚本执行出错 !!!!!!!!!!!!!!!!!!!!!!!" -ForegroundColor Red
    # $_ 代表当前的错误对象
    Write-Host $_.Exception.Message -ForegroundColor Red
    Write-Host "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!" -ForegroundColor Red
}
finally {
    # 无论脚本是成功执行还是中途出错，finally {} 里的代码都一定会执行
    Write-Host ""
    Read-Host "脚本执行完毕。按 Enter 键退出..."
}