# 警告：请将此脚本文件另存为 "UTF-8 with BOM" 编码，否则中文会显示为乱码。
# (在记事本中：文件 -> 另存为 -> 编码 -> "UTF-8-BOM")

# 将所有操作放入一个 try...catch...finally 结构中
try {
    # --- 配置 ---
    $executablePrefix = "MacinMeter"
    $sampleInterval = 1 # 内存采样间隔（秒）

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

    while (-not $process.HasExited) {
        try {
            $process.Refresh()
            # WorkingSet64 是进程当前使用的物理内存
            $memorySamples.Add($process.WorkingSet64 / 1024) # 转换为 KB
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