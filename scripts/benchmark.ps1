# 警告：请将此脚本文件另存为 "UTF-8 with BOM" 编码，否则中文会显示为乱码。
# (在记事本中：文件 -> 另存为 -> 编码 -> "UTF-8-BOM")

# 将所有操作放入一个 try...catch...finally 结构中
try {
    # --- 配置 ---
    $executablePrefix = "MacinMeter"
    $sampleInterval = 1 # 采样间隔（秒）用于内存/CPU统计

    # --- 脚本主体 ---
    Write-Host "正在当前目录查找以 '$executablePrefix' 开头的程序 (.exe)..." -ForegroundColor Yellow

    # 1. 查找可执行文件
    # --------------------------------------------------
    # $PSScriptRoot 是一个自动变量，代表脚本所在的目录
    $executableFile = Get-ChildItem -Path $PSScriptRoot -Filter "$executablePrefix*.exe"

    if ($null -eq $executableFile) {
        # 使用 throw 抛出一个会终止脚本的错误
        throw "错误: 未在脚本目录中找到以 '$executablePrefix' 开头的 .exe 文件。"
    }

    if ($executableFile.Count -gt 1) {
        throw "错误: 找到多个匹配的可执行文件，无法确定启动哪一个。"
    }

    Write-Host "找到目标程序: $($executableFile.Name)" -ForegroundColor Green
    Write-Host ""

    # 2. 启动与监控
    # --------------------------------------------------
    Write-Host "正在启动 $($executableFile.Name)..."
    
    # (优化) 启动高精度计时器
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

    $process = Start-Process -FilePath $executableFile.FullName -PassThru -ErrorAction Stop

    if ($null -eq $process) {
        throw "错误：启动进程 '$($executableFile.Name)' 失败！"
    }

    Write-Host "程序已启动 (PID: $($process.Id))。正在后台监控，请等待程序运行结束..." -ForegroundColor Cyan
    Write-Host "="*65

    $memorySamples = [System.Collections.Generic.List[double]]::new()
    $cpuSamples    = [System.Collections.Generic.List[double]]::new()
    $processorCount = [System.Environment]::ProcessorCount

    # 初始化CPU采样参考点
    $prevCpuTime  = [TimeSpan]::Zero
    $prevWallTime = [DateTime]::UtcNow
    try {
        $prevCpuTime = $process.TotalProcessorTime
    } catch { }

    while (-not $process.HasExited) {
        try {
            $process.Refresh()
            # WorkingSet64 是进程当前使用的物理内存
            $memorySamples.Add($process.WorkingSet64 / 1024) # 转换为 KB

            # 计算CPU占用率（基于采样区间）
            $nowWall = [DateTime]::UtcNow
            $nowCpu  = $process.TotalProcessorTime
            # 注意：Windows PowerShell 5.1 不支持数字分隔符 '_'，因此不要写 0.000_001
            $deltaWallSec = [Math]::Max(0.000001, ($nowWall - $prevWallTime).TotalSeconds)
            $deltaCpuSec  = [Math]::Max(0.0, ($nowCpu - $prevCpuTime).TotalSeconds)
            # 归一化到总CPU（所有逻辑处理器），确保不超过100%
            $cpuPct = ($deltaCpuSec / $deltaWallSec) * 100.0
            if ($processorCount -gt 0) { $cpuPct = $cpuPct / $processorCount }
            # 轻微抖动保护与边界限制
            if ($cpuPct -lt 0) { $cpuPct = 0 }
            if ($cpuPct -gt 100) { $cpuPct = [Math]::Min(100, $cpuPct) }
            $cpuSamples.Add([Math]::Round($cpuPct, 2))

            # 更新采样基线
            $prevWallTime = $nowWall
            $prevCpuTime  = $nowCpu
        } catch {
            # 进程可能在我们检查 HasExited 和 Refresh 之间退出，导致 Refresh 失败
            # 这种情况是正常的，直接跳出循环
            Write-Host " (进程已在采样间隙退出) " -NoNewline
            break
        }
        Start-Sleep -Seconds $sampleInterval
    }

    # (优化) 停止计时器
    $stopwatch.Stop()
    $elapsedTimeSpan = $stopwatch.Elapsed

    # 3. 计算并生成报告
    # --------------------------------------------------
    Write-Host ""
    Write-Host "======================= 运行总结报告 =======================" -ForegroundColor Green
    Write-Host "程序 '$($executableFile.Name)' (PID: $($process.Id)) 已停止运行。"
    Write-Host ""

    # (优化) 使用高精度时间，格式为 hh:mm:ss.fff (毫秒)
    Write-Host ("  - 总运行时长: {0} (精确到毫秒)" -f $elapsedTimeSpan.ToString("hh\:mm\:ss\.fff"))

    if ($memorySamples.Count -gt 0) {
        $stats = $memorySamples | Measure-Object -Average -Maximum
        $peakMemoryMb = $stats.Maximum / 1024
        $averageMemoryMb = $stats.Average / 1024

        Write-Host ("  - 内存使用峰值: {0:N2} MB ({1:N0} KB)" -f $peakMemoryMb, $stats.Maximum)
        Write-Host ("  - 内存使用平均值: {0:N2} MB ({1:N0} KB)" -f $averageMemoryMb, $stats.Average)
    } else {
        Write-Host "  - 内存使用情况: 程序运行时间不足 $sampleInterval 秒，未采集到有效内存数据。"
    }

    # CPU 统计（平均与峰值）
    try {
        $finalCpuTime = $process.TotalProcessorTime
    } catch {
        $finalCpuTime = [TimeSpan]::Zero
    }

    $overallCpuPct = 0.0
    if ($elapsedTimeSpan.TotalMilliseconds -gt 0) {
        $overallCpuPct = ($finalCpuTime.TotalMilliseconds / $elapsedTimeSpan.TotalMilliseconds) * 100.0
        if ($processorCount -gt 0) { $overallCpuPct = $overallCpuPct / $processorCount }
        if ($overallCpuPct -lt 0) { $overallCpuPct = 0 }
        if ($overallCpuPct -gt 100) { $overallCpuPct = [Math]::Min(100, $overallCpuPct) }
    }

    if ($cpuSamples.Count -gt 0) {
        $cpuStats = $cpuSamples | Measure-Object -Average -Maximum
        Write-Host ("  - CPU逻辑核心数: {0}" -f $processorCount)
        Write-Host ("  - CPU使用平均值(全程): {0:N2}%" -f $overallCpuPct)
        Write-Host ("  - CPU使用峰值(采样): {0:N2}%" -f $cpuStats.Maximum)
    } else {
        Write-Host ("  - CPU逻辑核心数: {0}" -f $processorCount)
        Write-Host ("  - CPU使用平均值(全程): {0:N2}%" -f $overallCpuPct)
        Write-Host ("  - CPU使用峰值(采样): 无（采样数为 0）")
    }

    Write-Host "="*65
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
