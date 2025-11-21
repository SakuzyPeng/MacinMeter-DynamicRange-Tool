import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";

type UiAnalyzeOptions = {
  parallelDecoding: boolean;
  excludeLfe: boolean;
  showRmsPeak: boolean;
};

type DrChannelResult = {
  channel: number;
  drValue: number;
  drValueRounded: number;
  rms: number;
  peak: number;
  primaryPeak: number;
  secondaryPeak: number;
  sampleCount: number;
};

type AudioFormatView = {
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  sampleCount: number;
  durationSeconds: number;
  codec: string | null;
  processedSampleRate: number | null;
  dsdNativeRateHz: number | null;
  dsdMultipleOf44k: number | null;
  hasChannelLayoutMetadata: boolean;
  lfeIndices: number[];
  partialAnalysis: boolean;
  skippedPackets: number;
};

type BoundaryWarning = {
  level: string;
  direction: string;
  distanceDb: number;
  message: string;
};

type TrimReport = {
  enabled: boolean;
  thresholdDb: number;
  minRunMs: number;
  hysteresisMs: number;
  leadingSeconds: number;
  trailingSeconds: number;
  totalSeconds: number;
  totalSamplesTrimmed: number;
};

type SilenceChannel = {
  channelIndex: number;
  validWindows: number;
  filteredWindows: number;
  totalWindows: number;
  filteredPercent: number;
};

type SilenceReport = {
  thresholdDb: number;
  channels: SilenceChannel[];
};

type AnalyzeResponse = {
  sourcePath: string;
  format: AudioFormatView;
  drResults: DrChannelResult[];
  aggregates: AnalysisAggregates;
  trimReport?: TrimReport | null;
  silenceReport?: SilenceReport | null;
};

type AggregateView = {
  officialDr: number | null;
  preciseDr: number | null;
  excludedChannels: number;
  excludedLfe: number;
  boundaryWarning: BoundaryWarning | null;
  warningText: string | null;
};

type AnalysisAggregates = {
  includeLfe: AggregateView;
  excludeLfe: AggregateView;
};

type DirectoryAnalysisEntry = {
  path: string;
  fileName: string;
  analysis?: AnalyzeResponse | null;
  error?: CommandError | null;
};

type DirectoryAnalysisResponse = {
  directory: string;
  files: DirectoryAnalysisEntry[];
};

type ScanResult = {
  directory: string;
  files: { fileName: string; path: string }[];
  supportedFormats: string[];
};

type CommandError = {
  message: string;
  suggestion?: string;
  category?: string;
  supportedFormats?: string[];
};

type AnalysisPanel = {
  container: HTMLElement;
  statusEl: HTMLElement;
  tableEl: HTMLElement;
  warningsEl: HTMLElement;
  trimEl: HTMLElement;
  silenceEl: HTMLElement;
};

let selectedPath: string | null = null;
let selectedKind: "file" | "directory" | "files" | null = null;
let analyzing = false;
let analysisToken = 0;
let analysisEntryUnlisten: (() => void) | null = null;
let analysisFinishedUnlisten: (() => void) | null = null;
let currentDirectoryEntries: DirectoryAnalysisEntry[] = [];

let inputPathEl!: HTMLInputElement;
let scanResultsEl!: HTMLElement;
let analyzeButton!: HTMLButtonElement;
let resultExcludeToggleEl!: HTMLInputElement;
let directoryResultsEl!: HTMLElement;
let ffmpegPathInput!: HTMLInputElement;
let applyFfmpegBtn!: HTMLButtonElement;
let selectedFiles: string[] | null = null;
let lastResponse: AnalyzeResponse | null = null;
let lastDirectoryResponse: DirectoryAnalysisResponse | null = null;
let aggregateExcludeLfe = false;
let singlePanel!: AnalysisPanel;

const decimals = (value: number, digits = 2): string =>
  Number.isFinite(value) ? value.toFixed(digits) : "-";

const setStatus = (panel: AnalysisPanel, message: string, isError = false) => {
  panel.statusEl.textContent = message;
  panel.statusEl.classList.toggle("error", isError);
};

const clearOutput = () => {
  singlePanel.tableEl.innerHTML = "";
  singlePanel.warningsEl.innerHTML = "";
  singlePanel.trimEl.innerHTML = "";
  singlePanel.silenceEl.innerHTML = "";
  directoryResultsEl.innerHTML = "";
  lastResponse = null;
  lastDirectoryResponse = null;
  aggregateExcludeLfe = false;
  selectedFiles = null;
  if (resultExcludeToggleEl) {
    resultExcludeToggleEl.checked = false;
    resultExcludeToggleEl.disabled = true;
  }
  setStatus(singlePanel, "请选择音频文件后运行分析。");
};

const cleanupAnalysisListeners = () => {
  if (analysisEntryUnlisten) {
    analysisEntryUnlisten();
    analysisEntryUnlisten = null;
  }
  if (analysisFinishedUnlisten) {
    analysisFinishedUnlisten();
    analysisFinishedUnlisten = null;
  }
};

const updateAnalyzeButton = () => {
  if (!analyzeButton) return;
  analyzeButton.textContent = analyzing ? "取消分析" : "开始分析";
};

const createAnalysisPanelElement = (): AnalysisPanel => {
  const container = document.createElement("div");
  container.className = "analysis-panel";

  const statusEl = document.createElement("div");
  statusEl.className = "status";
  container.appendChild(statusEl);

  const tableEl = document.createElement("div");
  tableEl.className = "dr-table";
  container.appendChild(tableEl);

  const warningsEl = document.createElement("div");
  container.appendChild(warningsEl);

  const trimEl = document.createElement("div");
  container.appendChild(trimEl);

  const silenceEl = document.createElement("div");
  container.appendChild(silenceEl);

  return { container, statusEl, tableEl, warningsEl, trimEl, silenceEl };
};

const disableWhile = async (flagSetter: (state: boolean) => void, task: () => Promise<void>) => {
  flagSetter(true);
  try {
    await task();
  } finally {
    flagSetter(false);
  }
};

const checkIsDirectory = async (path: string): Promise<boolean> => {
  try {
    return await invoke<boolean>("path_is_directory", { path });
  } catch {
    return false;
  }
};

const collectFilesFromPaths = async (paths: string[]): Promise<string[]> => {
  const filePaths: string[] = [];
  for (const p of paths) {
    const isDir = await checkIsDirectory(p);
    if (isDir) {
      try {
        const result = await invoke<ScanResult>("scan_audio_directory", { path: p });
        for (const f of result.files) {
          filePaths.push(f.path);
        }
      } catch {
        // ignore directory scan errors for drag-and-drop aggregation
      }
    } else {
      filePaths.push(p);
    }
  }
  // 去重
  return Array.from(new Set(filePaths));
};

const updateSelectedPath = (path: string, kind: "file" | "directory" | "files") => {
  selectedPath = path;
  selectedKind = kind;
  inputPathEl.value = path;
};

const renderScanResults = (result: ScanResult) => {
  if (!result.files.length) {
    scanResultsEl.innerHTML = `<p>目录 ${result.directory} 中未发现受支持的音频文件。</p>`;
  } else {
    scanResultsEl.innerHTML = `
      <p>在 <strong>${result.directory}</strong> 中找到 ${result.files.length} 个文件，可点击“开始分析”执行批量处理。</p>
    `;
  }
  scanResultsEl.classList.remove("hidden");
};

const gatherOptions = (): UiAnalyzeOptions => ({
  parallelDecoding: true,
  excludeLfe: false,
  showRmsPeak: false,
});

const renderDrTable = (
  panel: AnalysisPanel,
  response: AnalyzeResponse,
  highlightExcludedLfe: boolean
) => {
  const channels = response.drResults;
  if (!channels.length) {
    panel.tableEl.innerHTML = "<p>未产生有效声道结果。</p>";
    return;
  }
  const lfeSet = new Set(response.format.lfeIndices ?? []);
  const rows = channels
    .map((ch) => {
      const isSilent = ch.peak <= 1e-6 || ch.rms <= 1e-6;
      const isLfe = lfeSet.has(ch.channel);
      const classes: string[] = [];
      if (highlightExcludedLfe && isLfe) {
        classes.push("lfe-row");
      }
      if (isSilent) {
        classes.push("silent-row");
      }
      let channelLabel = `CH ${ch.channel + 1}`;
      if (isLfe) {
        channelLabel += " [LFE]";
      } else if (isSilent) {
        channelLabel += " [Silent]";
      }
      return `
      <tr class="${classes.join(" ")}">
        <td>${channelLabel}</td>
        <td>DR${ch.drValueRounded}</td>
        <td>${decimals(ch.drValue)}</td>
      </tr>
    `;
    })
    .join("");

  panel.tableEl.innerHTML = `
    <div class="dr-table">
      <table>
        <thead>
          <tr>
            <th>通道</th>
            <th>Official</th>
            <th>Precise</th>
          </tr>
        </thead>
        <tbody>${rows}</tbody>
      </table>
    </div>
  `;
};


const renderWarnings = (
  panel: AnalysisPanel,
  response: AnalyzeResponse,
  aggregate: AggregateView
) => {
  const notes: string[] = [];
  if (aggregate.boundaryWarning) {
    const warning = aggregate.boundaryWarning;
    notes.push(
      `边界风险 (${warning.level}): 距 ${warning.direction} 边界 ${warning.distanceDb.toFixed(
        2
      )} dB。`
    );
  }
  if (aggregate.warningText) {
    notes.push(aggregate.warningText.trim());
  }
  if (response.format.partialAnalysis) {
    notes.push(
      `警告：解码时跳过 ${response.format.skippedPackets} 个损坏包，结果仅供参考。`
    );
  }
  if (!notes.length) {
    panel.warningsEl.innerHTML = "";
    return;
  }
  panel.warningsEl.innerHTML = notes
    .map((note) => `<div class="warning-card">${note.replace(/\n/g, "<br>")}</div>`)
    .join("");
};

const updateAggregateView = () => {
  let hasRendered = false;
  if (lastResponse) {
    renderAnalysisPanelContent(singlePanel, lastResponse);
    hasRendered = true;
  }

  if (lastDirectoryResponse) {
    renderDirectoryResults(lastDirectoryResponse, false);
    hasRendered = true;
  }

  if (!hasRendered) {
    setStatus(singlePanel, "请选择音频文件后运行分析。");
  }
};

const renderTrimReport = (panel: AnalysisPanel, report?: TrimReport | null) => {
  if (!report || !report.enabled) {
    panel.trimEl.innerHTML = "";
    return;
  }
  panel.trimEl.innerHTML = `
    <div class="warning-card">
      <strong>首尾静音裁切</strong>
      <p>阈值 ${report.thresholdDb.toFixed(1)} dBFS，最小时长 ${report.minRunMs.toFixed(
        0
      )} ms。</p>
      <p>裁切 ${report.totalSamplesTrimmed} 个样本（首部 ${decimals(
    report.leadingSeconds
  )}s / 尾部 ${decimals(report.trailingSeconds)}s）。</p>
    </div>
  `;
};

const renderSilenceReport = (panel: AnalysisPanel, report?: SilenceReport | null) => {
  if (!report) {
    panel.silenceEl.innerHTML = "";
    return;
  }
  const rows = report.channels
    .map(
      (ch) => `
      <tr>
        <td>CH ${ch.channelIndex + 1}</td>
        <td>${ch.filteredWindows}/${ch.totalWindows}</td>
        <td>${decimals(ch.filteredPercent)}</td>
      </tr>
    `
    )
    .join("");
  panel.silenceEl.innerHTML = `
    <div class="dr-table">
      <strong>静音窗口过滤（阈值 ${report.thresholdDb.toFixed(1)} dBFS）</strong>
      <table>
        <thead>
          <tr>
            <th>通道</th>
            <th>Filtered</th>
            <th>%</th>
          </tr>
        </thead>
        <tbody>${rows}</tbody>
      </table>
    </div>
  `;
};

const renderDirectoryResults = (
  response: DirectoryAnalysisResponse,
  remember: boolean = true
) => {
  if (remember) {
    lastDirectoryResponse = response;
  }
  directoryResultsEl.innerHTML = "";
  if (resultExcludeToggleEl) {
    resultExcludeToggleEl.disabled = false;
  }
  if (!response.files.length) {
    const empty = document.createElement("p");
    empty.textContent = "目录中未找到可分析的音频文件。";
    directoryResultsEl.appendChild(empty);
    return;
  }

  response.files.forEach((entry: DirectoryAnalysisEntry) => {
    const card = document.createElement("div");
    card.className = "directory-entry";

    const header = document.createElement("div");
    header.className = "directory-entry-header";
    const title = document.createElement("h3");
    title.textContent = entry.fileName;
    const pathText = document.createElement("span");
    pathText.textContent = entry.path;
    header.appendChild(title);
    header.appendChild(pathText);
    card.appendChild(header);

    if (entry.error) {
      const err = document.createElement("div");
      err.className = "warning-card";
      const suggestion = entry.error.suggestion ? ` 建议：${entry.error.suggestion}` : "";
      err.textContent = `${entry.error.message}${suggestion}`;
      card.appendChild(err);
    } else if (entry.analysis) {
      const panel = createAnalysisPanelElement();
      renderAnalysisPanelContent(panel, entry.analysis);
      card.appendChild(panel.container);
    }

    directoryResultsEl.appendChild(card);
  });
};

const renderAnalysisPanelContent = (panel: AnalysisPanel, response: AnalyzeResponse) => {
  const aggregate = aggregateExcludeLfe
    ? response.aggregates.excludeLfe
    : response.aggregates.includeLfe;
  renderDrTable(panel, response, aggregateExcludeLfe);
  renderWarnings(panel, response, aggregate);
  renderTrimReport(panel, response.trimReport);
  renderSilenceReport(panel, response.silenceReport);
  if (aggregate.officialDr !== null && aggregate.preciseDr !== null) {
    const modeLabel = aggregateExcludeLfe ? "（排除 LFE）" : "";
    setStatus(
      panel,
      `Official DR ${aggregate.officialDr}${modeLabel} · Precise ${decimals(aggregate.preciseDr)} dB`
    );
  } else {
    setStatus(
      panel,
      aggregateExcludeLfe ? "没有有效声道（排除 LFE）" : "没有有效声道参与计算。"
    );
  }
};

const renderAnalysis = (response: AnalyzeResponse) => {
  lastResponse = response;
  lastDirectoryResponse = null;
  directoryResultsEl.innerHTML = "";
  if (resultExcludeToggleEl) {
    resultExcludeToggleEl.disabled = false;
    resultExcludeToggleEl.checked = aggregateExcludeLfe;
  }
  renderAnalysisPanelContent(singlePanel, response);
};

const parseInvokeError = (error: unknown): CommandError => {
  if (typeof error === "string") {
    try {
      return JSON.parse(error);
    } catch {
      return { message: error };
    }
  }
  if (typeof error === "object" && error !== null) {
    // Tauri InvokeError 包含 payload
    const payload = (error as { payload?: unknown }).payload;
    if (typeof payload === "string") {
      try {
        return JSON.parse(payload);
      } catch {
        return { message: payload };
      }
    }
    if (typeof payload === "object" && payload !== null) {
      return payload as CommandError;
    }
    if ("message" in error && typeof (error as { message: unknown }).message === "string") {
      return { message: (error as { message: string }).message };
    }
  }
  return { message: "未知错误" };
};

const handleAnalyze = async () => {
  if (!selectedPath) {
    setStatus(singlePanel, "请先选择音频文件。", true);
    return;
  }
  if (selectedKind === "directory") {
    await startDirectoryAnalyze();
    return;
  }
  if (selectedKind === "files" && selectedFiles && selectedFiles.length > 0) {
    await startSelectedFilesAnalyze(selectedFiles);
    return;
  }
  await startSingleFileAnalyze();
};

const startSingleFileAnalyze = async () => {
  if (!selectedPath) {
    setStatus(singlePanel, "请先选择音频文件。", true);
    return;
  }
  const token = ++analysisToken;
  analyzing = true;
  updateAnalyzeButton();
  clearOutput();
  setStatus(singlePanel, "分析中...", false);
  try {
    const options = gatherOptions();
    const response = await invoke<AnalyzeResponse>("analyze_audio", {
      path: selectedPath,
      options,
    });
    if (token !== analysisToken) {
      // 已被取消，忽略结果
      return;
    }
    renderAnalysis(response);
    if (resultExcludeToggleEl) {
      resultExcludeToggleEl.checked = aggregateExcludeLfe;
      resultExcludeToggleEl.disabled = false;
    }
  } catch (error) {
    if (token !== analysisToken) {
      return;
    }
    const detail = parseInvokeError(error);
    setStatus(
      singlePanel,
      detail.suggestion ? `${detail.message}（建议：${detail.suggestion}）` : detail.message,
      true
    );
    if (detail.supportedFormats?.length) {
      singlePanel.warningsEl.innerHTML = `<div class="warning-card">支持格式：${detail.supportedFormats.join(
        ", "
      )}</div>`;
    }
  } finally {
    if (token === analysisToken) {
      analyzing = false;
      updateAnalyzeButton();
    }
  }
};

const startDirectoryAnalyze = async () => {
  if (!selectedPath) {
    setStatus(singlePanel, "请先选择目录。", true);
    return;
  }
  const token = ++analysisToken;
  analyzing = true;
  updateAnalyzeButton();
  clearOutput();
  currentDirectoryEntries = [];
  setStatus(singlePanel, "目录批量分析中...", false);

  cleanupAnalysisListeners();
  analysisEntryUnlisten = await listen<DirectoryAnalysisEntry>("analysis-entry", (event) => {
    if (token !== analysisToken) {
      return;
    }
    currentDirectoryEntries.push(event.payload);
    const response: DirectoryAnalysisResponse = {
      directory: selectedPath!,
      files: currentDirectoryEntries.slice(),
    };
    renderDirectoryResults(response, false);
  });
  analysisFinishedUnlisten = await listen<DirectoryAnalysisResponse>("analysis-finished", (event) => {
    if (token !== analysisToken) {
      return;
    }
    renderDirectoryResults(event.payload);
    setStatus(
      singlePanel,
      `目录分析完成，共 ${event.payload.files.length} 个结果。`
    );
    analyzing = false;
    updateAnalyzeButton();
    cleanupAnalysisListeners();
  });

  try {
    const options = gatherOptions();
    await invoke<DirectoryAnalysisResponse>("analyze_directory", {
      path: selectedPath,
      options,
    });
  } catch (error) {
    if (token !== analysisToken) {
      return;
    }
    const detail = parseInvokeError(error);
    setStatus(
      singlePanel,
      detail.suggestion ? `${detail.message}（建议：${detail.suggestion}）` : detail.message,
      true
    );
    analyzing = false;
    updateAnalyzeButton();
    cleanupAnalysisListeners();
  }
};

const startSelectedFilesAnalyze = async (files: string[]) => {
  if (!files.length) {
    setStatus(singlePanel, "请选择至少一个音频文件。", true);
    return;
  }
  const token = ++analysisToken;
  analyzing = true;
  updateAnalyzeButton();
  clearOutput();
  currentDirectoryEntries = [];
  setStatus(singlePanel, `多文件分析中（${files.length} 个文件）...`, false);

  cleanupAnalysisListeners();
  analysisEntryUnlisten = await listen<DirectoryAnalysisEntry>("analysis-entry", (event) => {
    if (token !== analysisToken) {
      return;
    }
    currentDirectoryEntries.push(event.payload);
    const response: DirectoryAnalysisResponse = {
      directory: "selected-files",
      files: currentDirectoryEntries.slice(),
    };
    renderDirectoryResults(response, false);
  });
  analysisFinishedUnlisten = await listen<DirectoryAnalysisResponse>("analysis-finished", (event) => {
    if (token !== analysisToken) {
      return;
    }
    renderDirectoryResults(event.payload);
    setStatus(
      singlePanel,
      `多文件分析完成，共 ${event.payload.files.length} 个结果。`
    );
    analyzing = false;
    updateAnalyzeButton();
    cleanupAnalysisListeners();
  });

  try {
    const options = gatherOptions();
    await invoke<DirectoryAnalysisResponse>("analyze_files", {
      paths: files,
      options,
    });
  } catch (error) {
    if (token !== analysisToken) {
      return;
    }
    const detail = parseInvokeError(error);
    setStatus(
      singlePanel,
      detail.suggestion ? `${detail.message}（建议：${detail.suggestion}）` : detail.message,
      true
    );
    analyzing = false;
    updateAnalyzeButton();
    cleanupAnalysisListeners();
  }
};

const handlePickFile = async () => {
  await disableWhile(
    (state) => {
      analyzeButton.disabled = analyzing || state;
    },
    async () => {
      const selection = await open({
        multiple: true,
        directory: false,
      });

      if (Array.isArray(selection)) {
        if (!selection.length) {
          return;
        }
        await handleMultiFileSelection(selection);
      } else if (typeof selection === "string") {
        await handleSinglePathSelection(selection);
      }
    }
  );
};

const handleScanDir = async () => {
  const dir = await open({
    directory: true,
    multiple: false,
  });
  if (typeof dir !== "string") {
    return;
  }
  const result = await invoke<ScanResult>("scan_audio_directory", { path: dir });
  updateSelectedPath(dir, "directory");
  setStatus(singlePanel, `目录 ${dir} 已选，可点击“开始分析”执行批量处理。`);
  lastDirectoryResponse = null;
  directoryResultsEl.innerHTML = "";
  renderScanResults(result);
};

const handleSinglePathSelection = async (path: string) => {
  const isDir = await checkIsDirectory(path);
  if (isDir) {
    selectedFiles = null;
    updateSelectedPath(path, "directory");
    const result = await invoke<ScanResult>("scan_audio_directory", { path }).catch(() => null);
    if (result) {
      renderScanResults(result);
      setStatus(singlePanel, `目录 ${path} 已选，可点击“开始分析”执行批量处理。`);
    } else {
      setStatus(singlePanel, `目录 ${path} 无法读取。`, true);
    }
    lastDirectoryResponse = null;
    directoryResultsEl.innerHTML = "";
  } else {
    selectedFiles = [path];
    updateSelectedPath(path, "file");
    lastDirectoryResponse = null;
    directoryResultsEl.innerHTML = "";
    setStatus(singlePanel, `已选择 ${path}`);
    scanResultsEl.classList.add("hidden");
  }
};

const handleMultiFileSelection = async (paths: string[]) => {
  if (!paths.length) {
    return;
  }
  const filePaths: string[] = [];
  let ignoredDirs = 0;
  for (const p of paths) {
    const isDir = await checkIsDirectory(p);
    if (isDir) {
      ignoredDirs += 1;
    } else {
      filePaths.push(p);
    }
  }
  if (!filePaths.length) {
    setStatus(
      singlePanel,
      "所选项目均为目录，请使用“扫描目录”或仅选择音频文件。",
      true
    );
    return;
  }
  selectedFiles = filePaths;
  selectedKind = filePaths.length > 1 ? "files" : "file";
  inputPathEl.value =
    filePaths.length > 1
      ? `${filePaths.length} 个文件`
      : filePaths[0];
  selectedPath = filePaths[0];
  lastDirectoryResponse = null;
  directoryResultsEl.innerHTML = "";
  const extra =
    ignoredDirs > 0
      ? `（忽略了 ${ignoredDirs} 个目录，目录分析请使用“扫描目录”按钮）`
      : "";
  setStatus(singlePanel, `已选择 ${filePaths.length} 个文件${extra}`);
  scanResultsEl.classList.add("hidden");
};

const handleDroppedPaths = async (paths: string[]) => {
  if (!paths.length) {
    return;
  }
  if (paths.length === 1) {
    await handleSinglePathSelection(paths[0]);
  }
  const filePaths = await collectFilesFromPaths(paths);
  if (!filePaths.length) {
    setStatus(singlePanel, "拖入的项目中未发现可分析的音频文件。", true);
    return;
  }
  await handleMultiFileSelection(filePaths);
};

document.addEventListener("DOMContentLoaded", () => {
  inputPathEl = document.querySelector<HTMLInputElement>("#input-path")!;
  scanResultsEl = document.querySelector<HTMLElement>("#scan-results")!;
  resultExcludeToggleEl = document.querySelector<HTMLInputElement>("#result-exclude-lfe")!;
  analyzeButton = document.querySelector<HTMLButtonElement>("#analyze-btn")!;
  directoryResultsEl = document.querySelector<HTMLElement>("#directory-results")!;
  ffmpegPathInput = document.querySelector<HTMLInputElement>("#ffmpeg-path")!;
  applyFfmpegBtn = document.querySelector<HTMLButtonElement>("#apply-ffmpeg")!;
  singlePanel = {
    container: document.querySelector<HTMLElement>("#single-analysis")!,
    statusEl: document.querySelector<HTMLElement>("#status")!,
    tableEl: document.querySelector<HTMLElement>("#dr-table")!,
    warningsEl: document.querySelector<HTMLElement>("#warnings")!,
    trimEl: document.querySelector<HTMLElement>("#trim-report")!,
    silenceEl: document.querySelector<HTMLElement>("#silence-report")!,
  };

  document.querySelector<HTMLButtonElement>("#pick-file")?.addEventListener("click", handlePickFile);
  document.querySelector<HTMLButtonElement>("#scan-dir")?.addEventListener("click", handleScanDir);
  document.querySelector<HTMLButtonElement>("#clear-path")?.addEventListener("click", () => {
    selectedPath = null;
    selectedKind = null;
    inputPathEl.value = "";
    setStatus(singlePanel, "已清除输入路径。");
    clearOutput();
    scanResultsEl.classList.add("hidden");
  });

  document.querySelector<HTMLButtonElement>("#analyze-btn")?.addEventListener("click", () => {
    if (analyzing) {
      // 取消当前分析：仅在前端层面生效（忽略后续结果）
      analysisToken++;
      analyzing = false;
      updateAnalyzeButton();
      cleanupAnalysisListeners();
      setStatus(singlePanel, "已取消当前分析。");
      return;
    }
    void handleAnalyze();
  });
  applyFfmpegBtn.addEventListener("click", async () => {
    const value = ffmpegPathInput.value.trim();
    try {
      await invoke("set_ffmpeg_override", { path: value.length ? value : null });
      setStatus(
        singlePanel,
        value.length
          ? `已设置自定义 ffmpeg 路径：${value}`
          : "已清除自定义 ffmpeg 路径，将使用系统默认 PATH。",
        false
      );
    } catch (error) {
      const detail = parseInvokeError(error);
      setStatus(
        singlePanel,
        detail.suggestion ? `${detail.message}（建议：${detail.suggestion}）` : detail.message,
        true
      );
    }
  });

  resultExcludeToggleEl.disabled = true;
  resultExcludeToggleEl.addEventListener("change", () => {
    aggregateExcludeLfe = resultExcludeToggleEl.checked;
    updateAggregateView();
  });

  void getCurrentWindow().onDragDropEvent((event) => {
    if (event.payload.type === "drop" && Array.isArray(event.payload.paths)) {
      void handleDroppedPaths(event.payload.paths);
    }
  });
});
