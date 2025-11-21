import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { writeTextFile, writeFile } from "@tauri-apps/plugin-fs";
import { toPng, toSvg } from "html-to-image";

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
let analysisProgressUnlisten: (() => void) | null = null;
let deepScanProgressUnlisten: (() => void) | null = null;
let deepScanning = false;
let deepScanCancelled = false;
let sortModeSelect!: HTMLSelectElement;
let resultSearchInput!: HTMLInputElement;
let resultSearchNextBtn!: HTMLButtonElement;
let lastSearchQuery = "";
let lastSearchIndex = -1;
let currentDirectoryEntries: DirectoryAnalysisEntry[] = [];

let inputPathEl!: HTMLInputElement;
let scanResultsEl!: HTMLElement;
let analyzeButton!: HTMLButtonElement;
let resultExcludeToggleEl!: HTMLInputElement;
let exportHidePathEl!: HTMLInputElement;
let directoryResultsEl!: HTMLElement;
let ffmpegPathInput!: HTMLInputElement;
let applyFfmpegBtn!: HTMLButtonElement;
let copyMdBtn!: HTMLButtonElement;
let copyJsonBtn!: HTMLButtonElement;
let exportImageBtn!: HTMLButtonElement;
let selectedFiles: string[] | null = null;
let lastResponse: AnalyzeResponse | null = null;
let lastDirectoryResponse: DirectoryAnalysisResponse | null = null;
let aggregateExcludeLfe = false;
let singlePanel!: AnalysisPanel;

const decimals = (value: number, digits = 2): string =>
  Number.isFinite(value) ? value.toFixed(digits) : "-";

// 获取格式化的时间戳
const getTimestamp = (): string => {
  const now = new Date();
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
};

// 获取文件名格式的时间戳
const getFileTimestamp = (): string => {
  const now = new Date();
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
};

// 格式化单文件结果为Markdown
const formatSingleResultAsMd = (response: AnalyzeResponse): string => {
  const aggregate = aggregateExcludeLfe
    ? response.aggregates.excludeLfe
    : response.aggregates.includeLfe;
  const lfeSet = new Set(response.format.lfeIndices ?? []);
  const fileName = response.sourcePath.split("/").pop() || response.sourcePath;

  let md = `# MacinMeter DR Analysis - ${getTimestamp()}\n\n`;
  md += `## ${fileName}\n\n`;
  md += "| Channel | Official | Precise |\n";
  md += "|---------|----------|----------|\n";

  for (const ch of response.drResults) {
    const isSilent = ch.peak <= 1e-6 || ch.rms <= 1e-6;
    const isLfe = lfeSet.has(ch.channel);
    const isExcluded = isSilent || (aggregateExcludeLfe && isLfe);

    let label = `CH ${ch.channel + 1}`;
    const tags: string[] = [];
    if (isLfe) tags.push("LFE");
    if (isSilent) tags.push("Silent");
    if (isExcluded) tags.push("excluded");
    if (tags.length) label += ` [${tags.join(", ")}]`;

    const drRounded = isSilent ? "-" : `DR${ch.drValueRounded}`;
    const drPrecise = isSilent ? "-" : decimals(ch.drValue);
    md += `| ${label} | ${drRounded} | ${drPrecise} |\n`;
  }

  // 汇总行
  if (aggregate.officialDr !== null && aggregate.preciseDr !== null) {
    const mode = aggregateExcludeLfe ? " (excl. LFE)" : "";
    md += `\n**Official DR${aggregate.officialDr}${mode}** · Precise ${decimals(aggregate.preciseDr)} dB\n`;
  }

  // 边界风险用斜体
  if (aggregate.boundaryWarning) {
    const w = aggregate.boundaryWarning;
    md += `\n*边界风险 (${w.level}): 距${w.direction}边界 ${w.distanceDb.toFixed(2)} dB*\n`;
  }

  return md;
};

// 格式化批量结果为Markdown
const formatDirectoryResultsAsMd = (
  response: DirectoryAnalysisResponse,
): string => {
  let md = `# MacinMeter DR Analysis - ${getTimestamp()}\n\n`;

  for (const entry of response.files) {
    if (entry.analysis) {
      const aggregate = aggregateExcludeLfe
        ? entry.analysis.aggregates.excludeLfe
        : entry.analysis.aggregates.includeLfe;
      const lfeSet = new Set(entry.analysis.format.lfeIndices ?? []);
      const hasBoundaryWarning = aggregate.boundaryWarning !== null;

      // 文件名：有边界风险用斜体
      const fileTitle = hasBoundaryWarning
        ? `*${entry.fileName}*`
        : entry.fileName;
      md += `## ${fileTitle}\n\n`;
      md += "| Channel | Official | Precise |\n";
      md += "|---------|----------|----------|\n";

      for (const ch of entry.analysis.drResults) {
        const isSilent = ch.peak <= 1e-6 || ch.rms <= 1e-6;
        const isLfe = lfeSet.has(ch.channel);
        const isExcluded = isSilent || (aggregateExcludeLfe && isLfe);

        let label = `CH ${ch.channel + 1}`;
        const tags: string[] = [];
        if (isLfe) tags.push("LFE");
        if (isSilent) tags.push("Silent");
        if (isExcluded) tags.push("excluded");
        if (tags.length) label += ` [${tags.join(", ")}]`;

        const drRounded = isSilent ? "-" : `DR${ch.drValueRounded}`;
        const drPrecise = isSilent ? "-" : decimals(ch.drValue);
        md += `| ${label} | ${drRounded} | ${drPrecise} |\n`;
      }

      // 汇总行
      if (aggregate.officialDr !== null && aggregate.preciseDr !== null) {
        const mode = aggregateExcludeLfe ? " (excl. LFE)" : "";
        md += `\n**Official DR${aggregate.officialDr}${mode}** · Precise ${decimals(aggregate.preciseDr)} dB\n`;
      }

      // 边界风险
      if (aggregate.boundaryWarning) {
        const w = aggregate.boundaryWarning;
        md += `\n*边界风险 (${w.level}): 距${w.direction}边界 ${w.distanceDb.toFixed(2)} dB*\n`;
      }

      md += "\n";
    } else if (entry.error) {
      md += `## ${entry.fileName}\n\n`;
      md += `**Error**: ${entry.error.message}\n\n`;
    }
  }

  return md;
};

// 格式化单个entry为MD（用于批量分析中的单个文件复制）
const formatEntryAsMd = (entry: DirectoryAnalysisEntry, hidePath = false): string => {
  if (!entry.analysis) return "";

  const aggregate = aggregateExcludeLfe
    ? entry.analysis.aggregates.excludeLfe
    : entry.analysis.aggregates.includeLfe;
  const lfeSet = new Set(entry.analysis.format.lfeIndices ?? []);

  let md = `# ${entry.fileName} - ${getTimestamp()}\n\n`;
  if (!hidePath) {
    md += `**Path**: ${entry.path}\n\n`;
  }
  md += "| Channel | Official | Precise |\n";
  md += "|---------|----------|----------|\n";

  for (const ch of entry.analysis.drResults) {
    const isSilent = ch.peak <= 1e-6 || ch.rms <= 1e-6;
    const isLfe = lfeSet.has(ch.channel);
    const isExcluded = isSilent || (aggregateExcludeLfe && isLfe);

    let label = `CH ${ch.channel + 1}`;
    const tags: string[] = [];
    if (isLfe) tags.push("LFE");
    if (isSilent) tags.push("Silent");
    if (isExcluded) tags.push("excluded");
    if (tags.length) label += ` [${tags.join(", ")}]`;

    const drRounded = isSilent ? "-" : `DR${ch.drValueRounded}`;
    const drPrecise = isSilent ? "-" : decimals(ch.drValue);
    md += `| ${label} | ${drRounded} | ${drPrecise} |\n`;
  }

  if (aggregate.officialDr !== null && aggregate.preciseDr !== null) {
    const mode = aggregateExcludeLfe ? " (excl. LFE)" : "";
    md += `\n**Official DR${aggregate.officialDr}${mode}** · Precise ${decimals(aggregate.preciseDr)} dB\n`;
  }

  if (aggregate.boundaryWarning) {
    const w = aggregate.boundaryWarning;
    md += `\n*边界风险 (${w.level}): 距${w.direction}边界 ${w.distanceDb.toFixed(2)} dB*\n`;
  }

  return md;
};

// 格式化为JSON（用于导出）
const formatResultAsJson = (): object => {
  const timestamp = getTimestamp();
  const version = "0.1.0"; // TODO: 从app metadata获取

  const formatChannelInfo = (ch: DrChannelResult, lfeIndices: number[]) => {
    const isSilent = ch.peak <= 1e-6 || ch.rms <= 1e-6;
    const isLfe = lfeIndices.includes(ch.channel);
    const isExcluded = isSilent || (aggregateExcludeLfe && isLfe);

    return {
      channel: ch.channel + 1,
      drRounded: ch.drValueRounded,
      drPrecise: ch.drValue,
      status: isSilent ? "silent" : isLfe ? "lfe" : "normal",
      excluded: isExcluded,
    };
  };

  if (lastDirectoryResponse) {
    const files = lastDirectoryResponse.files.map((entry) => {
      if (entry.analysis) {
        const aggregate = aggregateExcludeLfe
          ? entry.analysis.aggregates.excludeLfe
          : entry.analysis.aggregates.includeLfe;
        const lfeIndices = entry.analysis.format.lfeIndices ?? [];

        return {
          file: entry.fileName,
          path: entry.path,
          officialDr: aggregate.officialDr,
          preciseDr: aggregate.preciseDr,
          boundaryWarning: aggregate.boundaryWarning
            ? {
                level: aggregate.boundaryWarning.level,
                direction: aggregate.boundaryWarning.direction,
                distanceDb: aggregate.boundaryWarning.distanceDb,
              }
            : null,
          channels: entry.analysis.drResults.map((ch) =>
            formatChannelInfo(ch, lfeIndices),
          ),
        };
      }
      return {
        file: entry.fileName,
        path: entry.path,
        error: entry.error?.message,
      };
    });

    return {
      tool: "MacinMeter DR Tool",
      version,
      timestamp,
      excludeLfe: aggregateExcludeLfe,
      files,
    };
  }

  if (lastResponse) {
    const aggregate = aggregateExcludeLfe
      ? lastResponse.aggregates.excludeLfe
      : lastResponse.aggregates.includeLfe;
    const lfeIndices = lastResponse.format.lfeIndices ?? [];
    const fileName =
      lastResponse.sourcePath.split("/").pop() || lastResponse.sourcePath;

    return {
      tool: "MacinMeter DR Tool",
      version,
      timestamp,
      excludeLfe: aggregateExcludeLfe,
      file: fileName,
      path: lastResponse.sourcePath,
      officialDr: aggregate.officialDr,
      preciseDr: aggregate.preciseDr,
      boundaryWarning: aggregate.boundaryWarning
        ? {
            level: aggregate.boundaryWarning.level,
            direction: aggregate.boundaryWarning.direction,
            distanceDb: aggregate.boundaryWarning.distanceDb,
          }
        : null,
      channels: lastResponse.drResults.map((ch) =>
        formatChannelInfo(ch, lfeIndices),
      ),
    };
  }

  return {};
};

// 导出JSON到文件
const exportJsonToFile = async () => {
  const data = formatResultAsJson();
  if (Object.keys(data).length === 0) return;

  const defaultName = `MacinMeter_v0.1.0_${getFileTimestamp()}.json`;

  const filePath = await save({
    defaultPath: defaultName,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });

  if (filePath) {
    await writeTextFile(filePath, JSON.stringify(data, null, 2));
  }
};

// 显示格式选择对话框
const showFormatDialog = (): Promise<"png" | "svg" | null> => {
  return new Promise((resolve) => {
    // 创建模态对话框
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";

    const dialog = document.createElement("div");
    dialog.className = "modal-dialog";
    dialog.innerHTML = `
      <h3>选择导出格式</h3>
      <div class="modal-buttons">
        <button type="button" data-format="png">PNG</button>
        <button type="button" data-format="svg">SVG</button>
        <button type="button" data-format="cancel" class="ghost">取消</button>
      </div>
    `;

    overlay.appendChild(dialog);
    document.body.appendChild(overlay);

    const cleanup = () => {
      document.body.removeChild(overlay);
    };

    // 点击按钮
    dialog.querySelectorAll("button").forEach((btn) => {
      btn.addEventListener("click", () => {
        const format = btn.dataset.format;
        cleanup();
        if (format === "png") resolve("png");
        else if (format === "svg") resolve("svg");
        else resolve(null);
      });
    });

    // 点击遮罩关闭
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) {
        cleanup();
        resolve(null);
      }
    });
  });
};

// 导出图片到文件
const exportImageToFile = async () => {
  const hasResult = lastResponse !== null || lastDirectoryResponse !== null;
  if (!hasResult) return;

  // 先选择格式
  const format = await showFormatDialog();
  if (!format) return;

  // 获取导出区域：单文件导出单个分析面板，多文件导出目录结果列表
  const resultPanel = lastDirectoryResponse
    ? document.querySelector<HTMLElement>("#directory-results")
    : document.querySelector<HTMLElement>("#single-analysis");
  if (!resultPanel) return;

  const ext = format === "svg" ? "svg" : "png";
  const defaultName = `MacinMeter_v0.1.0_${getFileTimestamp()}.${ext}`;

  const filePath = await save({
    defaultPath: defaultName,
    filters: [
      { name: format === "svg" ? "SVG Image" : "PNG Image", extensions: [ext] },
    ],
  });

  if (!filePath) return;

  // 临时隐藏按钮和路径
  const buttons = resultPanel.querySelectorAll<HTMLElement>(".copy-entry-btn");
  buttons.forEach((btn) => (btn.style.display = "none"));

  const hidePath = exportHidePathEl?.checked ?? false;
  const pathSpans = resultPanel.querySelectorAll<HTMLElement>(
    ".directory-entry-header > span:last-child",
  );
  if (hidePath) {
    pathSpans.forEach((span) => (span.style.display = "none"));
  }

  try {
    if (format === "svg") {
      const svgData = await toSvg(resultPanel, {
        backgroundColor: "#ffffff",
      });
      let svgContent = decodeURIComponent(svgData.split(",")[1]);
      // 让独立打开的 SVG 在浏览器中水平居中显示
      svgContent = svgContent.replace(
        /<svg([^>]*)>/,
        '<svg$1 style="display:block;margin:0 auto;">',
      );
      await writeTextFile(filePath, svgContent);
    } else {
      // PNG：通过放大导出画布尺寸来提高像素分辨率
      const rect = resultPanel.getBoundingClientRect();
      const baseWidth = rect.width;
      const baseHeight = rect.height;
      const pixelRatio = 4;

      // 避免生成过大的画布，和 html-to-image 内部 16384 限制保持一致
      const maxCanvas = 16384;
      const scale =
        baseWidth > 0 && baseHeight > 0
          ? Math.min(pixelRatio, maxCanvas / Math.max(baseWidth, baseHeight))
          : 1;

      const canvasWidth = Math.round(baseWidth * scale);
      const canvasHeight = Math.round(baseHeight * scale);

      const dataUrl = await toPng(resultPanel, {
        backgroundColor: "#ffffff",
        canvasWidth,
        canvasHeight,
        // 固定内部像素比为 1，全部缩放由 canvasWidth/Height 控制
        pixelRatio: 1,
        skipAutoScale: true,
      });
      const base64 = dataUrl.split(",")[1];
      const binaryString = atob(base64);
      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }
      await writeFile(filePath, bytes);
    }
  } catch (error) {
    console.error("Export image failed:", error);
  } finally {
    // 恢复显示
    buttons.forEach((btn) => (btn.style.display = ""));
    if (hidePath) {
      pathSpans.forEach((span) => (span.style.display = ""));
    }
  }
};

// 拷贝到剪贴板
const copyToClipboard = async (text: string, btn: HTMLButtonElement) => {
  try {
    await navigator.clipboard.writeText(text);
    btn.classList.add("copied");
    const original = btn.textContent;
    btn.textContent = "Copied!";
    setTimeout(() => {
      btn.classList.remove("copied");
      btn.textContent = original;
    }, 1500);
  } catch {
    // 静默失败
  }
};

// 更新拷贝按钮状态
const updateCopyButtons = () => {
  const hasResult = lastResponse !== null || lastDirectoryResponse !== null;
  if (copyMdBtn) copyMdBtn.disabled = !hasResult;
  if (copyJsonBtn) copyJsonBtn.disabled = !hasResult;
  if (exportImageBtn) exportImageBtn.disabled = !hasResult;
};

// 进度状态
let analysisTotal = 0;

const setStatus = (panel: AnalysisPanel, message: string, isError = false) => {
  panel.statusEl.innerHTML = `<span class="status-text">${message}</span><div class="progress-fill" style="width: 0%"></div>`;
  panel.statusEl.classList.toggle("error", isError);
};

const setStatusWithProgress = (
  panel: AnalysisPanel,
  current: number,
  total: number,
) => {
  analysisTotal = total;

  const percent = total > 0 ? Math.round((current / total) * 100) : 0;
  panel.statusEl.innerHTML = `<span class="status-text">分析中... ${current}/${total}</span><div class="progress-fill" style="width: ${percent}%"></div>`;
  panel.statusEl.classList.remove("error");
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
  updateCopyButtons();
  setStatus(singlePanel, "请选择音频文件后运行分析。");
};

const cleanupAnalysisListeners = () => {
  if (analysisEntryUnlisten) {
    analysisEntryUnlisten();
    analysisEntryUnlisten = null;
  }
  if (analysisProgressUnlisten) {
    analysisProgressUnlisten();
    analysisProgressUnlisten = null;
  }
  if (analysisFinishedUnlisten) {
    analysisFinishedUnlisten();
    analysisFinishedUnlisten = null;
  }
};

const cleanupDeepScanListeners = () => {
  if (deepScanProgressUnlisten) {
    deepScanProgressUnlisten();
    deepScanProgressUnlisten = null;
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

const disableWhile = async (
  flagSetter: (state: boolean) => void,
  task: () => Promise<void>,
) => {
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
        const result = await invoke<ScanResult>("scan_audio_directory", {
          path: p,
        });
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

const updateSelectedPath = (
  path: string,
  kind: "file" | "directory" | "files",
) => {
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
  highlightExcludedLfe: boolean,
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
  aggregate: AggregateView,
) => {
  const notes: string[] = [];
  if (aggregate.boundaryWarning) {
    const warning = aggregate.boundaryWarning;
    notes.push(
      `边界风险 (${warning.level}): 距 ${warning.direction} 边界 ${warning.distanceDb.toFixed(
        2,
      )} dB。`,
    );
  }
  if (aggregate.warningText) {
    notes.push(aggregate.warningText.trim());
  }
  if (response.format.partialAnalysis) {
    notes.push(
      `警告：解码时跳过 ${response.format.skippedPackets} 个损坏包，结果仅供参考。`,
    );
  }
  if (!notes.length) {
    panel.warningsEl.innerHTML = "";
    return;
  }
  panel.warningsEl.innerHTML = notes
    .map(
      (note) =>
        `<div class="warning-card">${note.replace(/\n/g, "<br>")}</div>`,
    )
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
        0,
      )} ms。</p>
      <p>裁切 ${report.totalSamplesTrimmed} 个样本（首部 ${decimals(
        report.leadingSeconds,
      )}s / 尾部 ${decimals(report.trailingSeconds)}s）。</p>
    </div>
  `;
};

const renderSilenceReport = (
  panel: AnalysisPanel,
  report?: SilenceReport | null,
) => {
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
    `,
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
  remember: boolean = true,
) => {
  if (remember) {
    lastDirectoryResponse = response;
    updateCopyButtons();
    if (sortModeSelect) {
      sortModeSelect.disabled = response.files.length === 0;
    }
    if (resultSearchNextBtn) {
      resultSearchNextBtn.disabled = response.files.length === 0;
    }
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

  const files = response.files.slice();

  const sortMode = sortModeSelect ? sortModeSelect.value : "none";
  const getEntryPreciseDr = (entry: DirectoryAnalysisEntry): number | null => {
    if (!entry.analysis) return null;
    const aggregate = aggregateExcludeLfe
      ? entry.analysis.aggregates.excludeLfe
      : entry.analysis.aggregates.includeLfe;
    return aggregate.preciseDr;
  };

  if (sortMode === "dr-asc" || sortMode === "dr-desc") {
    files.sort((a, b) => {
      const da = getEntryPreciseDr(a);
      const db = getEntryPreciseDr(b);
      if (da === null && db === null) return 0;
      if (da === null) return 1;
      if (db === null) return -1;
      return sortMode === "dr-asc" ? da - db : db - da;
    });
  }

  files.forEach((entry: DirectoryAnalysisEntry) => {
    const card = document.createElement("div");
    card.className = "directory-entry";
    card.dataset.path = entry.path;
    card.dataset.fileName = entry.fileName;

    const header = document.createElement("div");
    header.className = "directory-entry-header";
    const title = document.createElement("h3");
    const titleText = document.createElement("span");
    titleText.textContent = entry.fileName;
    title.appendChild(titleText);

    // 添加单个文件复制按钮
    if (entry.analysis) {
      const copyMdBtn = document.createElement("button");
      copyMdBtn.className = "copy-entry-btn";
      copyMdBtn.textContent = "MD";
      copyMdBtn.type = "button";
      copyMdBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        const hidePath = exportHidePathEl?.checked ?? false;
        const md = formatEntryAsMd(entry, hidePath);
        if (md) {
          void copyToClipboard(md, copyMdBtn);
        }
      });
      title.appendChild(copyMdBtn);

      const copyPngBtn = document.createElement("button");
      copyPngBtn.className = "copy-entry-btn";
      copyPngBtn.textContent = "PNG";
      copyPngBtn.type = "button";
      copyPngBtn.addEventListener("click", async (e) => {
        e.stopPropagation();

        try {
          // 临时隐藏按钮
          const buttons = card.querySelectorAll<HTMLElement>(".copy-entry-btn");
          buttons.forEach((btn) => (btn.style.display = "none"));

          // 根据设置隐藏路径
          const pathSpan = card.querySelector<HTMLElement>(
            ".directory-entry-header > span:last-child",
          );
          const hidePath = exportHidePathEl?.checked ?? false;
          if (hidePath && pathSpan) {
            pathSpan.style.display = "none";
          }

          const dataUrl = await toPng(card, {
            backgroundColor: "#ffffff",
            pixelRatio: 2,
          });

          // 恢复显示
          buttons.forEach((btn) => (btn.style.display = ""));
          if (hidePath && pathSpan) {
            pathSpan.style.display = "";
          }

          // 用 canvas 扩大画布，添加边距
          const img = new Image();
          await new Promise<void>((resolve, reject) => {
            img.onload = () => resolve();
            img.onerror = reject;
            img.src = dataUrl;
          });

          const padding = 32; // 边距像素
          const canvas = document.createElement("canvas");
          canvas.width = img.width + padding * 2;
          canvas.height = img.height + padding * 2;
          const ctx = canvas.getContext("2d")!;
          ctx.fillStyle = "#ffffff";
          ctx.fillRect(0, 0, canvas.width, canvas.height);
          ctx.drawImage(img, padding, padding);

          const paddedDataUrl = canvas.toDataURL("image/png");

          // 从 data URL 提取 base64
          const base64 = paddedDataUrl.split(",")[1];

          // 调用后端命令复制到剪贴板
          await invoke("copy_image_to_clipboard", { base64Data: base64 });

          copyPngBtn.classList.add("copied");
          const original = copyPngBtn.textContent;
          copyPngBtn.textContent = "OK!";
          setTimeout(() => {
            copyPngBtn.classList.remove("copied");
            copyPngBtn.textContent = original;
          }, 1500);
        } catch (err) {
          console.error("Copy PNG failed:", err);
          // 显示错误提示
          copyPngBtn.textContent = "Fail";
          setTimeout(() => {
            copyPngBtn.textContent = "PNG";
          }, 1500);
        }
      });
      title.appendChild(copyPngBtn);
    }

    const pathText = document.createElement("span");
    pathText.textContent = entry.path;
    header.appendChild(title);
    header.appendChild(pathText);
    card.appendChild(header);

    if (entry.error) {
      const err = document.createElement("div");
      err.className = "warning-card";
      const suggestion = entry.error.suggestion
        ? ` 建议：${entry.error.suggestion}`
        : "";
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

const renderAnalysisPanelContent = (
  panel: AnalysisPanel,
  response: AnalyzeResponse,
) => {
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
      `Official DR ${aggregate.officialDr}${modeLabel} · Precise ${decimals(aggregate.preciseDr)} dB`,
    );
  } else {
    setStatus(
      panel,
      aggregateExcludeLfe
        ? "没有有效声道（排除 LFE）"
        : "没有有效声道参与计算。",
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
  updateCopyButtons();
  if (sortModeSelect) {
    sortModeSelect.disabled = true;
    sortModeSelect.value = "none";
  }
  if (resultSearchNextBtn && resultSearchInput) {
    resultSearchNextBtn.disabled = true;
    resultSearchInput.value = "";
    lastSearchQuery = "";
    lastSearchIndex = -1;
  }
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
    if (
      "message" in error &&
      typeof (error as { message: unknown }).message === "string"
    ) {
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
      detail.suggestion
        ? `${detail.message}（建议：${detail.suggestion}）`
        : detail.message,
      true,
    );
    if (detail.supportedFormats?.length) {
      singlePanel.warningsEl.innerHTML = `<div class="warning-card">支持格式：${detail.supportedFormats.join(
        ", ",
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

  // 先扫描获取文件总数
  try {
    const scanResult = await invoke<ScanResult>("scan_audio_directory", {
      path: selectedPath,
    });
    analysisTotal = scanResult.files.length;
  } catch {
    analysisTotal = 0;
  }

  setStatusWithProgress(singlePanel, 0, analysisTotal);

  cleanupAnalysisListeners();
  analysisProgressUnlisten = await listen<number>(
    "analysis-progress",
    (event) => {
      if (token !== analysisToken) {
        return;
      }
      const completed = event.payload ?? 0;
      setStatusWithProgress(singlePanel, completed, analysisTotal);
    },
  );
  analysisEntryUnlisten = await listen<DirectoryAnalysisEntry>(
    "analysis-entry",
    (event) => {
      if (token !== analysisToken) {
        return;
      }
      currentDirectoryEntries.push(event.payload);
      const response: DirectoryAnalysisResponse = {
        directory: selectedPath!,
        files: currentDirectoryEntries.slice(),
      };
      renderDirectoryResults(response, false);
    },
  );
  analysisFinishedUnlisten = await listen<DirectoryAnalysisResponse>(
    "analysis-finished",
    (event) => {
      if (token !== analysisToken) {
        return;
      }
      renderDirectoryResults(event.payload);
      setStatus(
        singlePanel,
        `目录分析完成，共 ${event.payload.files.length} 个结果。`,
      );
      analyzing = false;
      updateAnalyzeButton();
      cleanupAnalysisListeners();
    },
  );

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
      detail.suggestion
        ? `${detail.message}（建议：${detail.suggestion}）`
        : detail.message,
      true,
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

  // 设置总数
  analysisTotal = files.length;
  setStatusWithProgress(singlePanel, 0, analysisTotal);

  cleanupAnalysisListeners();
  analysisProgressUnlisten = await listen<number>(
    "analysis-progress",
    (event) => {
      if (token !== analysisToken) {
        return;
      }
      const completed = event.payload ?? 0;
      setStatusWithProgress(singlePanel, completed, analysisTotal);
    },
  );
  analysisEntryUnlisten = await listen<DirectoryAnalysisEntry>(
    "analysis-entry",
    (event) => {
      if (token !== analysisToken) {
        return;
      }
      currentDirectoryEntries.push(event.payload);
      const response: DirectoryAnalysisResponse = {
        directory: "selected-files",
        files: currentDirectoryEntries.slice(),
      };
      renderDirectoryResults(response, false);
    },
  );
  analysisFinishedUnlisten = await listen<DirectoryAnalysisResponse>(
    "analysis-finished",
    (event) => {
      if (token !== analysisToken) {
        return;
      }
      renderDirectoryResults(event.payload);
      setStatus(
        singlePanel,
        `多文件分析完成，共 ${event.payload.files.length} 个结果。`,
      );
      analyzing = false;
      updateAnalyzeButton();
      cleanupAnalysisListeners();
    },
  );

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
      detail.suggestion
        ? `${detail.message}（建议：${detail.suggestion}）`
        : detail.message,
      true,
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
    },
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
  const result = await invoke<ScanResult>("scan_audio_directory", {
    path: dir,
  });
  updateSelectedPath(dir, "directory");
  setStatus(singlePanel, `目录 ${dir} 已选，可点击“开始分析”执行批量处理。`);
  lastDirectoryResponse = null;
  directoryResultsEl.innerHTML = "";
  renderScanResults(result);
};

const handleDeepScanDir = async () => {
  const button = document.querySelector<HTMLButtonElement>("#deep-scan-dir");
  if (!button) return;

  // 如果正在深度扫描，则此次点击视为“取消递归”
  if (deepScanning) {
    deepScanCancelled = true;
    setStatus(singlePanel, "正在请求取消深度扫描...", false);
    try {
      await invoke("cancel_deep_scan");
    } catch {
      // 忽略取消失败，后端扫描函数会在下次调用时重置状态
    }
    return;
  }

  const dir = await open({
    directory: true,
    multiple: false,
  });
  if (typeof dir !== "string") {
    return;
  }

  const confirmed = await confirm(
    `将对目录及其所有子目录执行递归扫描：\n${dir}\n\n` +
      "在包含大量文件的目录上，这可能会非常缓慢，并占用较高的磁盘与CPU资源。\n" +
      "建议仅对主要存放音频文件的专用目录使用此功能，避免选择整个磁盘或用户主目录。",
    {
      title: "深度扫描目录（递归）风险提示",
      kind: "warning",
      okLabel: "继续深度扫描",
      cancelLabel: "取消",
    },
  );

  if (!confirmed) {
    return;
  }

  deepScanning = true;
  deepScanCancelled = false;
  button.textContent = "取消递归扫描";

  cleanupDeepScanListeners();
  scanResultsEl.classList.remove("hidden");
  scanResultsEl.innerHTML = `<p>正在递归扫描目录 <strong>${dir}</strong> ...</p>`;
  setStatus(
    singlePanel,
    `正在递归扫描目录 ${dir} ... 这可能需要一段时间。`,
    false,
  );

  deepScanProgressUnlisten = await listen<number>(
    "deep-scan-progress",
    (event) => {
      const count = event.payload ?? 0;
      scanResultsEl.innerHTML = `<p>正在递归扫描目录 <strong>${dir}</strong> ... 已发现 ${count} 个音频文件。</p>`;
    },
  );

  try {
    const result = await invoke<ScanResult>("deep_scan_audio_directory", {
      path: dir,
    });

    updateSelectedPath(dir, "directory");
    selectedFiles = result.files.map((f) => f.path);
    selectedKind = selectedFiles.length > 1 ? "files" : "file";
    inputPathEl.value =
      selectedFiles.length > 1
        ? `${selectedFiles.length} 个文件（递归）`
        : selectedFiles[0];

    const summaryText =
      result.files.length > 0
        ? deepScanCancelled
          ? `深度扫描已取消：在 ${result.directory} 及其子目录中已发现 ${result.files.length} 个音频文件。`
          : `深度扫描完成：在 ${result.directory} 及其子目录中找到 ${result.files.length} 个音频文件，可点击“开始分析”执行批量处理。`
        : deepScanCancelled
          ? `深度扫描已取消：在 ${result.directory} 及其子目录中未发现可分析的音频文件。`
          : `深度扫描完成：在 ${result.directory} 及其子目录中未找到可分析的音频文件。`;
    setStatus(singlePanel, summaryText);

    lastDirectoryResponse = null;
    directoryResultsEl.innerHTML = "";
    renderScanResults(result);
  } catch (error) {
    const detail = parseInvokeError(error);
    setStatus(
      singlePanel,
      detail.suggestion
        ? `${detail.message}（建议：${detail.suggestion}）`
        : detail.message,
      true,
    );
  } finally {
    cleanupDeepScanListeners();
    deepScanning = false;
    deepScanCancelled = false;
    button.textContent = "深度扫描目录";
  }
};

const handleSinglePathSelection = async (path: string) => {
  const isDir = await checkIsDirectory(path);
  if (isDir) {
    selectedFiles = null;
    updateSelectedPath(path, "directory");
    const result = await invoke<ScanResult>("scan_audio_directory", {
      path,
    }).catch(() => null);
    if (result) {
      renderScanResults(result);
      setStatus(
        singlePanel,
        `目录 ${path} 已选，可点击“开始分析”执行批量处理。`,
      );
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
      "所选项目均为目录，请使用“选择目录（批量分析）”按钮或仅选择音频文件。",
      true,
    );
    return;
  }
  selectedFiles = filePaths;
  selectedKind = filePaths.length > 1 ? "files" : "file";
  inputPathEl.value =
    filePaths.length > 1 ? `${filePaths.length} 个文件` : filePaths[0];
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
    return;
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
  resultExcludeToggleEl = document.querySelector<HTMLInputElement>(
    "#result-exclude-lfe",
  )!;
  exportHidePathEl = document.querySelector<HTMLInputElement>(
    "#export-hide-path",
  )!;
  analyzeButton = document.querySelector<HTMLButtonElement>("#analyze-btn")!;
  directoryResultsEl =
    document.querySelector<HTMLElement>("#directory-results")!;
  ffmpegPathInput = document.querySelector<HTMLInputElement>("#ffmpeg-path")!;
  applyFfmpegBtn = document.querySelector<HTMLButtonElement>("#apply-ffmpeg")!;
  copyMdBtn = document.querySelector<HTMLButtonElement>("#copy-md")!;
  copyJsonBtn = document.querySelector<HTMLButtonElement>("#copy-json")!;
  exportImageBtn = document.querySelector<HTMLButtonElement>("#export-image")!;
  sortModeSelect = document.querySelector<HTMLSelectElement>("#sort-mode")!;
  resultSearchInput =
    document.querySelector<HTMLInputElement>("#result-search")!;
  resultSearchNextBtn = document.querySelector<HTMLButtonElement>(
    "#result-search-next",
  )!;
  singlePanel = {
    container: document.querySelector<HTMLElement>("#single-analysis")!,
    statusEl: document.querySelector<HTMLElement>("#status")!,
    tableEl: document.querySelector<HTMLElement>("#dr-table")!,
    warningsEl: document.querySelector<HTMLElement>("#warnings")!,
    trimEl: document.querySelector<HTMLElement>("#trim-report")!,
    silenceEl: document.querySelector<HTMLElement>("#silence-report")!,
  };

  document
    .querySelector<HTMLButtonElement>("#pick-file")
    ?.addEventListener("click", handlePickFile);
  document
    .querySelector<HTMLButtonElement>("#scan-dir")
    ?.addEventListener("click", handleScanDir);
  document
    .querySelector<HTMLButtonElement>("#deep-scan-dir")
    ?.addEventListener("click", () => {
      void handleDeepScanDir();
    });
  document
    .querySelector<HTMLButtonElement>("#clear-path")
    ?.addEventListener("click", () => {
      selectedPath = null;
      selectedKind = null;
      inputPathEl.value = "";
      setStatus(singlePanel, "已清除输入路径。");
      clearOutput();
      scanResultsEl.classList.add("hidden");
    });

  document
    .querySelector<HTMLButtonElement>("#analyze-btn")
    ?.addEventListener("click", () => {
      if (analyzing) {
        // 取消当前分析：通知后端停止处理
        analysisToken++;
        analyzing = false;
        updateAnalyzeButton();
        cleanupAnalysisListeners();
        setStatus(singlePanel, "正在取消分析...");
        invoke("cancel_analysis")
          .then(() => {
            setStatus(singlePanel, "已取消当前分析。");
          })
          .catch(() => {
            setStatus(singlePanel, "已取消当前分析。");
          });
        return;
      }
      void handleAnalyze();
    });
  applyFfmpegBtn.addEventListener("click", async () => {
    const value = ffmpegPathInput.value.trim();
    try {
      await invoke("set_ffmpeg_override", {
        path: value.length ? value : null,
      });
      setStatus(
        singlePanel,
        value.length
          ? `已设置自定义 ffmpeg 路径：${value}`
          : "已清除自定义 ffmpeg 路径，将使用系统默认 PATH。",
        false,
      );
    } catch (error) {
      const detail = parseInvokeError(error);
      setStatus(
        singlePanel,
        detail.suggestion
          ? `${detail.message}（建议：${detail.suggestion}）`
          : detail.message,
        true,
      );
    }
  });

  resultExcludeToggleEl.disabled = true;
  resultExcludeToggleEl.addEventListener("change", () => {
    aggregateExcludeLfe = resultExcludeToggleEl.checked;
    updateAggregateView();
  });

  sortModeSelect.disabled = true;
  sortModeSelect.addEventListener("change", () => {
    updateAggregateView();
  });

  resultSearchNextBtn.disabled = true;
  resultSearchNextBtn.addEventListener("click", () => {
    const query = resultSearchInput.value.trim();
    if (!query) return;
    const cards = Array.from(
      document.querySelectorAll<HTMLElement>(".directory-entry"),
    );
    if (!cards.length) return;

    const lowered = query.toLowerCase();
    const matches = cards.filter((card) => {
      const name = (card.dataset.fileName ?? "").toLowerCase();
      const path = (card.dataset.path ?? "").toLowerCase();
      return name.includes(lowered) || path.includes(lowered);
    });
    if (!matches.length) {
      setStatus(singlePanel, `未找到包含「${query}」的结果。`, true);
      lastSearchQuery = query;
      lastSearchIndex = -1;
      return;
    }

    if (lastSearchQuery !== query) {
      lastSearchQuery = query;
      lastSearchIndex = 0;
    } else {
      lastSearchIndex = (lastSearchIndex + 1) % matches.length;
    }

    const target = matches[lastSearchIndex];
    target.scrollIntoView({ behavior: "smooth", block: "start" });
  });

  // 拷贝按钮事件
  copyMdBtn.addEventListener("click", () => {
    let md = "";
    if (lastDirectoryResponse) {
      md = formatDirectoryResultsAsMd(lastDirectoryResponse);
    } else if (lastResponse) {
      md = formatSingleResultAsMd(lastResponse);
    }
    if (md) {
      void copyToClipboard(md, copyMdBtn);
    }
  });

  copyJsonBtn.addEventListener("click", () => {
    void exportJsonToFile();
  });

  exportImageBtn.addEventListener("click", () => {
    void exportImageToFile();
  });

  void getCurrentWindow().onDragDropEvent((event) => {
    if (event.payload.type === "drop" && Array.isArray(event.payload.paths)) {
      void handleDroppedPaths(event.payload.paths);
    }
  });
});
