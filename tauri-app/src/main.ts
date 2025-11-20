import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

type MetadataResponse = {
  supportedFormats: string[];
};

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

let selectedPath: string | null = null;
let metadata: MetadataResponse | null = null;
let analyzing = false;

let inputPathEl: HTMLInputElement;
let statusEl: HTMLElement;
let drTableEl: HTMLElement;
let warningsEl: HTMLElement;
let trimReportEl: HTMLElement;
let silenceReportEl: HTMLElement;
let scanResultsEl: HTMLElement;
let analyzeButton: HTMLButtonElement;
let resultExcludeToggleEl: HTMLInputElement;
let lastResponse: AnalyzeResponse | null = null;
let aggregateExcludeLfe = false;

const decimals = (value: number, digits = 2): string =>
  Number.isFinite(value) ? value.toFixed(digits) : "-";

const setStatus = (message: string, isError = false) => {
  statusEl.textContent = message;
  statusEl.classList.toggle("error", isError);
};

const clearOutput = () => {
  drTableEl.innerHTML = "";
  warningsEl.innerHTML = "";
  trimReportEl.innerHTML = "";
  silenceReportEl.innerHTML = "";
  lastResponse = null;
  aggregateExcludeLfe = false;
  if (resultExcludeToggleEl) {
    resultExcludeToggleEl.checked = false;
    resultExcludeToggleEl.disabled = true;
  }
  setStatus("请选择音频文件后运行分析。");
};

const disableWhile = async (flagSetter: (state: boolean) => void, task: () => Promise<void>) => {
  flagSetter(true);
  try {
    await task();
  } finally {
    flagSetter(false);
  }
};

const renderScanResults = (result: ScanResult) => {
  if (!result.files.length) {
    scanResultsEl.innerHTML = `<p>目录 ${result.directory} 中未发现受支持的音频文件。</p>`;
  } else {
    const list = result.files
      .map(
        (file) =>
          `<li data-path="${file.path.replace(/"/g, "&quot;")}">${file.fileName}</li>`
      )
      .join("");
    scanResultsEl.innerHTML = `
      <p>在 <strong>${result.directory}</strong> 中找到 ${result.files.length} 个文件：</p>
      <ul>${list}</ul>
      <p class="help-text">点击列表即可将文件载入到分析输入。</p>
    `;
    scanResultsEl.querySelectorAll("li").forEach((item) => {
      item.addEventListener("click", () => {
        const path = (item as HTMLElement).dataset.path;
        if (path) {
          updateSelectedPath(path);
          setStatus(`已选择 ${path}`);
        }
      });
    });
  }
  scanResultsEl.classList.remove("hidden");
};

const gatherOptions = (): UiAnalyzeOptions => ({
  parallelDecoding: true,
  excludeLfe: false,
  showRmsPeak: false,
});

const renderDrTable = (response: AnalyzeResponse) => {
  const channels = response.drResults;
  if (!channels.length) {
    drTableEl.innerHTML = "<p>未产生有效声道结果。</p>";
    return;
  }
  const lfeSet = new Set(response.format.lfeIndices ?? []);
  const rows = channels
    .map(
      (ch) => {
        const isLfe = lfeSet.has(ch.channel);
        const channelLabel = isLfe ? `CH ${ch.channel + 1} [LFE]` : `CH ${ch.channel + 1}`;
        const rowClass = isLfe ? ' class="lfe-row"' : "";
        return `
      <tr${rowClass}>
        <td>${channelLabel}</td>
        <td>DR${ch.drValueRounded}</td>
        <td>${decimals(ch.drValue)}</td>
      </tr>
    `;
      }
    )
    .join("");

  drTableEl.innerHTML = `
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


const renderWarnings = (response: AnalyzeResponse, aggregate: AggregateView) => {
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
    warningsEl.innerHTML = "";
    return;
  }
  warningsEl.innerHTML = notes
    .map((note) => `<div class="warning-card">${note.replace(/\n/g, "<br>")}</div>`)
    .join("");
};

const getCurrentAggregate = (): AggregateView | null => {
  if (!lastResponse) {
    return null;
  }
  return aggregateExcludeLfe
    ? lastResponse.aggregates.excludeLfe
    : lastResponse.aggregates.includeLfe;
};

const updateAggregateView = () => {
  const response = lastResponse;
  const aggregate = getCurrentAggregate();
  if (!response || !aggregate) {
    setStatus("请选择音频文件后运行分析。");
    return;
  }

  if (aggregate.officialDr !== null && aggregate.preciseDr !== null) {
    const modeLabel = aggregateExcludeLfe ? "（排除 LFE）" : "";
    setStatus(
      `Official DR ${aggregate.officialDr}${modeLabel} · Precise ${decimals(aggregate.preciseDr)} dB`
    );
  } else {
    setStatus(
      aggregateExcludeLfe ? "没有有效声道（排除 LFE）" : "没有有效声道参与计算。"
    );
  }
  renderWarnings(response, aggregate);
};

const renderTrimReport = (report?: TrimReport | null) => {
  if (!report || !report.enabled) {
    trimReportEl.innerHTML = "";
    return;
  }
  trimReportEl.innerHTML = `
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

const renderSilenceReport = (report?: SilenceReport | null) => {
  if (!report) {
    silenceReportEl.innerHTML = "";
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
  silenceReportEl.innerHTML = `
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

const renderAnalysis = (response: AnalyzeResponse) => {
  lastResponse = response;
  aggregateExcludeLfe = false;
  if (resultExcludeToggleEl) {
    resultExcludeToggleEl.checked = false;
    resultExcludeToggleEl.disabled = false;
  }
  renderDrTable(response);
  renderTrimReport(response.trimReport);
  renderSilenceReport(response.silenceReport);
  updateAggregateView();
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

const updateSelectedPath = (path: string) => {
  selectedPath = path;
  inputPathEl.value = path;
};

const loadMetadata = async () => {
  metadata = await invoke<MetadataResponse>("load_app_metadata");
};

const handleAnalyze = async () => {
  if (!selectedPath) {
    setStatus("请先选择音频文件。", true);
    return;
  }
  await disableWhile(
    (state) => {
      analyzing = state;
      analyzeButton.disabled = state;
    },
    async () => {
      clearOutput();
      setStatus("分析中...", false);
      try {
        const options = gatherOptions();
        const response = await invoke<AnalyzeResponse>("analyze_audio", {
          path: selectedPath,
          options,
        });
        renderAnalysis(response);
      } catch (error) {
        const detail = parseInvokeError(error);
        setStatus(
          detail.suggestion ? `${detail.message}（建议：${detail.suggestion}）` : detail.message,
          true
        );
        if (detail.supportedFormats?.length) {
          warningsEl.innerHTML = `<div class="warning-card">支持格式：${detail.supportedFormats.join(
            ", "
          )}</div>`;
        }
      }
    }
  );
};

const handlePickFile = async () => {
  await disableWhile(
    (state) => {
      analyzeButton.disabled = analyzing || state;
    },
    async () => {
      const file = await open({
        multiple: false,
        directory: false,
        filters: metadata
          ? [
              {
                name: "Audio",
                extensions: metadata.supportedFormats.map((ext) => ext.toLowerCase()),
              },
            ]
          : undefined,
      });
      if (typeof file === "string") {
        updateSelectedPath(file);
        setStatus(`已选择 ${file}`);
        scanResultsEl.classList.add("hidden");
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
  renderScanResults(result);
};

document.addEventListener("DOMContentLoaded", () => {
  inputPathEl = document.querySelector<HTMLInputElement>("#input-path")!;
  statusEl = document.querySelector<HTMLElement>("#status")!;
  drTableEl = document.querySelector<HTMLElement>("#dr-table")!;
  warningsEl = document.querySelector<HTMLElement>("#warnings")!;
  trimReportEl = document.querySelector<HTMLElement>("#trim-report")!;
  silenceReportEl = document.querySelector<HTMLElement>("#silence-report")!;
  scanResultsEl = document.querySelector<HTMLElement>("#scan-results")!;
  resultExcludeToggleEl = document.querySelector<HTMLInputElement>("#result-exclude-lfe")!;
  analyzeButton = document.querySelector<HTMLButtonElement>("#analyze-btn")!;

  document.querySelector<HTMLButtonElement>("#pick-file")?.addEventListener("click", handlePickFile);
  document.querySelector<HTMLButtonElement>("#scan-dir")?.addEventListener("click", handleScanDir);
  document.querySelector<HTMLButtonElement>("#clear-path")?.addEventListener("click", () => {
    selectedPath = null;
    inputPathEl.value = "";
    setStatus("已清除输入路径。");
    clearOutput();
    scanResultsEl.classList.add("hidden");
  });

  document.querySelector<HTMLButtonElement>("#analyze-btn")?.addEventListener("click", () => {
    handleAnalyze();
  });

  resultExcludeToggleEl.disabled = true;
  resultExcludeToggleEl.addEventListener("change", () => {
    aggregateExcludeLfe = resultExcludeToggleEl.checked;
    updateAggregateView();
  });

  loadMetadata().catch((err) => {
    const detail = parseInvokeError(err);
    setStatus(`初始化失败：${detail.message}`, true);
  });
});
