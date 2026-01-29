import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { writeTextFile, writeFile } from "@tauri-apps/plugin-fs";
import { toPng, toSvg } from "html-to-image";
import {
  t,
  changeLanguage,
  updateStaticTexts,
  updateLanguageButtons,
  type SupportedLanguage,
} from "./i18n";

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
let resultExcludeLfeBtn!: HTMLButtonElement;
let exportHidePathBtn!: HTMLButtonElement;
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
let hidePath = false;
let singlePanel!: AnalysisPanel;
let appVersion = "0.1.0";

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
    md += `\n*${t("md.boundaryRisk", { level: w.level, direction: w.direction, distance: w.distanceDb.toFixed(2) })}*\n`;
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

      // 汇总行
      if (aggregate.officialDr !== null && aggregate.preciseDr !== null) {
        const mode = aggregateExcludeLfe ? " (excl. LFE)" : "";
        md += `\n**Official DR${aggregate.officialDr}${mode}** · Precise ${decimals(aggregate.preciseDr)} dB\n`;
      }

      // 边界风险
      if (aggregate.boundaryWarning) {
        const w = aggregate.boundaryWarning;
        md += `\n*${t("md.boundaryRisk", { level: w.level, direction: w.direction, distance: w.distanceDb.toFixed(2) })}*\n`;
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
    md += `\n*${t("md.boundaryRisk", { level: w.level, direction: w.direction, distance: w.distanceDb.toFixed(2) })}*\n`;
  }

  return md;
};

// 格式化为JSON（用于导出）
const formatResultAsJson = (): object => {
  const timestamp = getTimestamp();
  const version = appVersion;

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

  const defaultName = `MacinMeter_v${appVersion}_${getFileTimestamp()}.json`;

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
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";

    const dialog = document.createElement("div");
    dialog.className = "modal-dialog";
    dialog.innerHTML = `
      <h3>${t("dialog.formatTitle")}</h3>
      <div class="modal-buttons">
        <button type="button" data-format="png">PNG</button>
        <button type="button" data-format="svg">SVG</button>
        <button type="button" data-format="cancel" class="ghost">${t("dialog.cancel")}</button>
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
  const defaultName = `MacinMeter_v${appVersion}_${getFileTimestamp()}.${ext}`;

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
        style: {
          // CJK 字体支持：macOS/Windows/Linux
          fontFamily: "'Hiragino Sans', 'Hiragino Kaku Gothic Pro', 'Yu Gothic', 'Meiryo', 'Microsoft YaHei', 'Noto Sans CJK JP', 'Noto Sans CJK SC', system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
        },
        skipFonts: true,
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
    if (!hidePath) {
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
  panel.statusEl.innerHTML = `<span class="status-text">${t("status.analyzingProgress", { current, total })}</span><div class="progress-fill" style="width: ${percent}%"></div>`;
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
  if (resultExcludeLfeBtn) {
    resultExcludeLfeBtn.classList.remove("active");
    resultExcludeLfeBtn.disabled = true;
  }
  updateCopyButtons();
  setStatus(singlePanel, t("status.ready"));
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
  analyzeButton.textContent = analyzing ? t("btn.cancelAnalyze") : t("btn.analyze");
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
    scanResultsEl.innerHTML = `<p>${t("scan.noFiles", { path: result.directory })}</p>`;
  } else {
    scanResultsEl.innerHTML = `
      <p>${t("scan.foundFiles", { path: `<strong>${result.directory}</strong>`, count: result.files.length })}</p>
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
    panel.tableEl.innerHTML = `<p>${t("table.noResults")}</p>`;
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
            <th>${t("table.channel")}</th>
            <th>${t("table.official")}</th>
            <th>${t("table.precise")}</th>
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
      t("warning.boundary", {
        level: warning.level,
        direction: warning.direction,
        distance: warning.distanceDb.toFixed(2),
      }),
    );
  }
  if (aggregate.warningText) {
    notes.push(aggregate.warningText.trim());
  }
  if (response.format.partialAnalysis) {
    notes.push(
      t("warning.partialAnalysis", { count: response.format.skippedPackets }),
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
    setStatus(singlePanel, t("status.ready"));
  }
};

const renderTrimReport = (panel: AnalysisPanel, report?: TrimReport | null) => {
  if (!report || !report.enabled) {
    panel.trimEl.innerHTML = "";
    return;
  }
  panel.trimEl.innerHTML = `
    <div class="warning-card">
      <strong>${t("trim.title")}</strong>
      <p>${t("trim.threshold", { db: report.thresholdDb.toFixed(1), ms: report.minRunMs.toFixed(0) })}</p>
      <p>${t("trim.result", {
        samples: report.totalSamplesTrimmed,
        leading: decimals(report.leadingSeconds),
        trailing: decimals(report.trailingSeconds),
      })}</p>
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
      <strong>${t("silence.title", { db: report.thresholdDb.toFixed(1) })}</strong>
      <table>
        <thead>
          <tr>
            <th>${t("table.channel")}</th>
            <th>${t("table.filtered")}</th>
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
  if (resultExcludeLfeBtn) {
    resultExcludeLfeBtn.disabled = false;
  }
  if (!response.files.length) {
    const empty = document.createElement("p");
    empty.textContent = t("noAudioInDirectory");
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
          if (hidePath && pathSpan) {
            pathSpan.style.display = "none";
          }

          const dataUrl = await toPng(card, {
            backgroundColor: "#ffffff",
            pixelRatio: 2,
            style: {
              // CJK 字体支持：macOS/Windows/Linux
              fontFamily: "'Hiragino Sans', 'Hiragino Kaku Gothic Pro', 'Yu Gothic', 'Meiryo', 'Microsoft YaHei', 'Noto Sans CJK JP', 'Noto Sans CJK SC', system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
            },
            skipFonts: true,
          });

          // 恢复显示
          buttons.forEach((btn) => (btn.style.display = ""));
          if (!hidePath && pathSpan) {
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
          copyPngBtn.textContent = "OK!";
          setTimeout(() => {
            copyPngBtn.classList.remove("copied");
            copyPngBtn.textContent = "PNG";
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
    if (hidePath) {
      pathText.style.display = "none";
    }
    header.appendChild(title);
    header.appendChild(pathText);
    card.appendChild(header);

    if (entry.error) {
      const err = document.createElement("div");
      err.className = "warning-card";
      const suggestion = entry.error.suggestion
        ? t("error.suggestion", { suggestion: entry.error.suggestion })
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
    const modeLabel = aggregateExcludeLfe ? t("label.excludeLfeMode") : "";
    setStatus(
      panel,
      t("status.officialDr", {
        dr: aggregate.officialDr,
        mode: modeLabel,
        precise: decimals(aggregate.preciseDr),
      }),
    );
  } else {
    setStatus(
      panel,
      aggregateExcludeLfe
        ? t("status.noChannelsExcludeLfe")
        : t("status.noChannels"),
    );
  }
};

const renderAnalysis = (response: AnalyzeResponse) => {
  // 单文件也用 renderDirectoryResults 渲染，统一逻辑
  // 清空 lastResponse，只用 lastDirectoryResponse 管理状态
  lastResponse = null;

  const fileName = response.sourcePath.split("/").pop() || response.sourcePath;
  const entry: DirectoryAnalysisEntry = {
    path: response.sourcePath,
    fileName,
    analysis: response,
  };
  const dirResponse: DirectoryAnalysisResponse = {
    directory: response.sourcePath,
    files: [entry],
  };
  renderDirectoryResults(dirResponse);

  // 单文件时隐藏排序和搜索
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
  return { message: t("error.unknown") };
};

const handleAnalyze = async () => {
  if (!selectedPath) {
    setStatus(singlePanel, t("error.selectFileFirst"), true);
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
    setStatus(singlePanel, t("error.selectFileFirst"), true);
    return;
  }
  const token = ++analysisToken;
  analyzing = true;
  updateAnalyzeButton();
  clearOutput();
  setStatus(singlePanel, t("status.analyzing"), false);
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
    if (resultExcludeLfeBtn) {
      resultExcludeLfeBtn.classList.toggle("active", aggregateExcludeLfe);
      resultExcludeLfeBtn.disabled = false;
    }
  } catch (error) {
    if (token !== analysisToken) {
      return;
    }
    const detail = parseInvokeError(error);
    setStatus(
      singlePanel,
      detail.suggestion
        ? `${detail.message}${t("error.suggestion", { suggestion: detail.suggestion })}`
        : detail.message,
      true,
    );
    if (detail.supportedFormats?.length) {
      singlePanel.warningsEl.innerHTML = `<div class="warning-card">${t("error.supportedFormats", { formats: detail.supportedFormats.join(", ") })}</div>`;
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
    setStatus(singlePanel, t("error.selectDirFirst"), true);
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
        t("status.directoryComplete", { count: event.payload.files.length }),
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
        ? `${detail.message}${t("error.suggestion", { suggestion: detail.suggestion })}`
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
    setStatus(singlePanel, t("error.selectAtLeastOne"), true);
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
        t("status.multiFileComplete", { count: event.payload.files.length }),
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
        ? `${detail.message}${t("error.suggestion", { suggestion: detail.suggestion })}`
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
  setStatus(singlePanel, t("status.directorySelected", { path: dir }));
  lastDirectoryResponse = null;
  directoryResultsEl.innerHTML = "";
  renderScanResults(result);
};

const handleDeepScanDir = async () => {
  const button = document.querySelector<HTMLButtonElement>("#deep-scan-dir");
  if (!button) return;

  if (deepScanning) {
    deepScanCancelled = true;
    setStatus(singlePanel, t("status.cancellingDeepScan"), false);
    try {
      await invoke("cancel_deep_scan");
    } catch {
      // ignore
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
    t("dialog.deepScanMessage", { path: dir }),
    {
      title: t("dialog.deepScanTitle"),
      kind: "warning",
      okLabel: t("dialog.deepScanConfirm"),
      cancelLabel: t("dialog.cancel"),
    },
  );

  if (!confirmed) {
    return;
  }

  deepScanning = true;
  deepScanCancelled = false;
  button.textContent = t("btn.cancelDeepScan");

  cleanupDeepScanListeners();
  scanResultsEl.classList.remove("hidden");
  scanResultsEl.innerHTML = `<p>${t("status.deepScanning", { path: `<strong>${dir}</strong>` })}</p>`;
  setStatus(
    singlePanel,
    t("status.deepScanning", { path: dir }),
    false,
  );

  deepScanProgressUnlisten = await listen<number>(
    "deep-scan-progress",
    (event) => {
      const count = event.payload ?? 0;
      scanResultsEl.innerHTML = `<p>${t("status.deepScanProgress", { path: `<strong>${dir}</strong>`, count })}</p>`;
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
        ? t("status.filesRecursive", { count: selectedFiles.length })
        : selectedFiles[0];

    const summaryText =
      result.files.length > 0
        ? deepScanCancelled
          ? t("status.deepScanCancelledWithFiles", { path: result.directory, count: result.files.length })
          : t("status.deepScanComplete", { path: result.directory, count: result.files.length })
        : deepScanCancelled
          ? t("status.deepScanCancelledEmpty", { path: result.directory })
          : t("status.deepScanEmpty", { path: result.directory });
    setStatus(singlePanel, summaryText);

    lastDirectoryResponse = null;
    directoryResultsEl.innerHTML = "";
    renderScanResults(result);
  } catch (error) {
    const detail = parseInvokeError(error);
    setStatus(
      singlePanel,
      detail.suggestion
        ? `${detail.message}${t("error.suggestion", { suggestion: detail.suggestion })}`
        : detail.message,
      true,
    );
  } finally {
    cleanupDeepScanListeners();
    deepScanning = false;
    deepScanCancelled = false;
    button.textContent = t("btn.deepScan");
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
        t("status.directorySelected", { path }),
      );
    } else {
      setStatus(singlePanel, t("status.directoryReadError", { path }), true);
    }
    lastDirectoryResponse = null;
    directoryResultsEl.innerHTML = "";
  } else {
    selectedFiles = [path];
    updateSelectedPath(path, "file");
    lastDirectoryResponse = null;
    directoryResultsEl.innerHTML = "";
    setStatus(singlePanel, t("status.fileSelected", { path }));
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
      t("status.allDirectoriesError"),
      true,
    );
    return;
  }
  selectedFiles = filePaths;
  selectedKind = filePaths.length > 1 ? "files" : "file";
  inputPathEl.value =
    filePaths.length > 1 ? t("status.filesCount", { count: filePaths.length }) : filePaths[0];
  selectedPath = filePaths[0];
  lastDirectoryResponse = null;
  directoryResultsEl.innerHTML = "";
  if (ignoredDirs > 0) {
    setStatus(singlePanel, t("status.filesSelectedWithIgnored", { count: filePaths.length, ignored: ignoredDirs }));
  } else {
    setStatus(singlePanel, t("status.filesSelected", { count: filePaths.length }));
  }
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
    setStatus(singlePanel, t("status.noAudioInDropped"), true);
    return;
  }
  await handleMultiFileSelection(filePaths);
};

document.addEventListener("DOMContentLoaded", () => {
  inputPathEl = document.querySelector<HTMLInputElement>("#input-path")!;
  scanResultsEl = document.querySelector<HTMLElement>("#scan-results")!;
  resultExcludeLfeBtn = document.querySelector<HTMLButtonElement>(
    "#result-exclude-lfe-btn",
  )!;
  exportHidePathBtn = document.querySelector<HTMLButtonElement>(
    "#export-hide-path-btn",
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

  // 从 Tauri 获取应用版本号
  invoke<{ version: string }>("load_app_metadata")
    .then((meta) => {
      if (meta?.version) {
        appVersion = meta.version;
      }
    })
    .catch(() => {
      // 保持默认版本号
    });

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
      setStatus(singlePanel, t("status.pathCleared"));
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
        setStatus(singlePanel, t("status.cancelling"));

        // 如果已有部分结果，启用相关按钮
        if (currentDirectoryEntries.length > 0) {
          lastDirectoryResponse = {
            directory: selectedPath || "selected-files",
            files: currentDirectoryEntries.slice(),
          };
          if (resultExcludeLfeBtn) {
            resultExcludeLfeBtn.disabled = false;
          }
          updateCopyButtons();
        }

        invoke("cancel_analysis")
          .then(() => {
            const count = currentDirectoryEntries.length;
            setStatus(singlePanel, count > 0
              ? t("status.cancelledWithCount", { count })
              : t("status.cancelled"));
          })
          .catch(() => {
            const count = currentDirectoryEntries.length;
            setStatus(singlePanel, count > 0
              ? t("status.cancelledWithCount", { count })
              : t("status.cancelled"));
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
          ? t("status.ffmpegSet", { path: value })
          : t("status.ffmpegCleared"),
        false,
      );
    } catch (error) {
      const detail = parseInvokeError(error);
      setStatus(
        singlePanel,
        detail.suggestion
          ? `${detail.message}${t("error.suggestion", { suggestion: detail.suggestion })}`
          : detail.message,
        true,
      );
    }
  });

  resultExcludeLfeBtn.disabled = true;
  resultExcludeLfeBtn.addEventListener("click", () => {
    aggregateExcludeLfe = !aggregateExcludeLfe;
    resultExcludeLfeBtn.classList.toggle("active", aggregateExcludeLfe);
    updateAggregateView();
  });

  exportHidePathBtn.addEventListener("click", () => {
    hidePath = !hidePath;
    exportHidePathBtn.classList.toggle("active", hidePath);
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
      setStatus(singlePanel, t("status.searchNotFound", { query }), true);
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

  // 语言切换初始化
  const langZhBtn = document.getElementById("lang-zh");
  const langEnBtn = document.getElementById("lang-en");

  const handleLanguageChange = (lng: SupportedLanguage) => {
    changeLanguage(lng).then(() => {
      updateStaticTexts();
      updateLanguageButtons();
      // 更新动态内容
      if (lastResponse || lastDirectoryResponse) {
        updateAggregateView();
      } else {
        setStatus(singlePanel, t("status.ready"));
      }
    });
  };

  langZhBtn?.addEventListener("click", () => handleLanguageChange("zh-CN"));
  langEnBtn?.addEventListener("click", () => handleLanguageChange("en-US"));

  // 初始化语言状态
  updateStaticTexts();
  updateLanguageButtons();
});
