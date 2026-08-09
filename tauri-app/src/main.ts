import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { writeFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { toPng, toSvg } from "html-to-image";
import packageMetadata from "../package.json";
import { BatchProgress } from "./batch-progress";
import {
  changeLanguage,
  getCurrentLanguage,
  t,
  updateLanguageButtons,
  updateStaticTexts,
  type SupportedLanguage,
} from "./i18n";
import type {
  AnalysisReport,
  BatchItem,
  CapabilitySnapshot,
  ChannelResult,
  DiscoveryResponse,
  JobEvent,
  PublicError,
  WireEnvelope,
} from "./wire";

type SelectionKind = "files" | "directory" | "mixed";
type SortMode = "none" | "dr-asc" | "dr-desc";
type StatusState = {
  key: string;
  values?: Record<string, string | number>;
  error?: boolean;
  progress?: number;
};

type DisplayEntry = {
  key: number;
  displayPath: string;
  report: AnalysisReport | null;
  error: PublicError | null;
};

const element = <T extends HTMLElement>(id: string): T => {
  const found = document.getElementById(id);
  if (!found) throw new Error(`missing element #${id}`);
  return found as T;
};

const appVersion = element<HTMLSpanElement>("app-version");
const inputPath = element<HTMLInputElement>("input-path");
const pickFilesButton = element<HTMLButtonElement>("pick-files");
const scanDirectoryButton = element<HTMLButtonElement>("scan-dir");
const deepScanButton = element<HTMLButtonElement>("deep-scan-dir");
const clearButton = element<HTMLButtonElement>("clear-path");
const analyzeButton = element<HTMLButtonElement>("analyze-btn");
const scanResults = element<HTMLDivElement>("scan-results");
const statusElement = element<HTMLDivElement>("run-status");
const statusText = element<HTMLSpanElement>("status-text");
const statusDismissButton = element<HTMLButtonElement>("status-dismiss");
const progressFill = element<HTMLDivElement>("progress-fill");
const batchSummary = element<HTMLDivElement>("batch-summary");
const resultsElement = element<HTMLDivElement>("results");
const hidePathButton = element<HTMLButtonElement>("hide-path");
const copyMarkdownButton = element<HTMLButtonElement>("copy-md");
const exportJsonButton = element<HTMLButtonElement>("export-json");
const exportImageButton = element<HTMLButtonElement>("export-image");
const sortModeSelect = element<HTMLSelectElement>("sort-mode");
const searchInput = element<HTMLInputElement>("result-search");
const searchNextButton = element<HTMLButtonElement>("result-search-next");
const rawPanel = element<HTMLDetailsElement>("raw-panel");
const rawJson = element<HTMLPreElement>("raw-json");
const dropOverlay = element<HTMLDivElement>("drop-overlay");
const dropCard = dropOverlay.querySelector<HTMLDivElement>(".drop-card");
const dropTitle = element<HTMLElement>("drop-title");
const dropSubtitle = element<HTMLElement>("drop-subtitle");

if (!dropCard) throw new Error("missing drop card");

let selectedInputs: string[] = [];
let selectionKind: SelectionKind = "files";
let recursive = false;
let discoveredFiles: string[] | null = null;
let selectionRevision = 0;
let previewJobId: string | null = null;
let activeJobId: string | null = null;
let activeTotal = 1;
const activeProgress = new BatchProgress(activeTotal);
let lastEnvelope: WireEnvelope | null = null;
let hidePath = false;
let sortMode: SortMode = "none";
let searchQuery = "";
let searchIndex = -1;
let discoveryFilterExtensions: string[] = [];
let statusState: StatusState = { key: "status.ready", progress: 0 };

const escapeHtml = (value: string): string => {
  const replacements: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#039;",
  };
  return value.replace(/[&<>"']/g, (character) => replacements[character]);
};

const fileName = (path: string): string => {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : path;
};

const describeInvokeError = (error: unknown): string => {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return String(error);
};

const formatDbfs = (value: number | null): string => {
  if (value === null) return "−∞ dBFS";
  let corrected = value;
  if (corrected > -0.01 && corrected < 0.01) {
    const roundedCenti =
      corrected >= 0
        ? Math.floor(corrected * 100 + 0.5)
        : Math.ceil(corrected * 100 - 0.5);
    corrected = roundedCenti === 0 ? 0 : roundedCenti / 100;
  }
  return `${corrected.toFixed(2)} dBFS`;
};

const linearToDbfs = (value: number): number | null => {
  if (!(value > 0)) return null;
  return 20 * Math.log10(value);
};

const formatDuration = (frames: number, sampleRate: number): string => {
  const roundedSeconds = Math.round(frames / sampleRate);
  const weeks = Math.floor(roundedSeconds / 604_800);
  let remainder = roundedSeconds % 604_800;
  const days = Math.floor(remainder / 86_400);
  remainder %= 86_400;
  const hours = Math.floor(remainder / 3_600);
  remainder %= 3_600;
  const minutes = Math.floor(remainder / 60);
  const seconds = remainder % 60;
  const clock = `${hours > 0 ? `${hours}:` : ""}${String(minutes).padStart(hours > 0 ? 2 : 1, "0")}:${String(seconds).padStart(2, "0")}`;
  if (weeks > 0) return `${weeks}wk ${days}d ${clock}`;
  if (days > 0) return `${days}d ${clock}`;
  return clock;
};

const status = (
  key: string,
  values: Record<string, string | number> = {},
  options: { error?: boolean; progress?: number } = {},
): void => {
  statusState = { key, values, ...options };
  statusElement.hidden = false;
  renderStatus();
};

const renderStatus = (): void => {
  statusText.textContent = t(statusState.key, statusState.values);
  const isError = statusState.error === true;
  statusElement.classList.toggle("error", isError);
  statusDismissButton.hidden = !isError;
  const progress = Math.max(0, Math.min(100, statusState.progress ?? 0));
  progressFill.style.width = `${progress}%`;
};

const updateControls = (): void => {
  const running = activeJobId !== null;
  pickFilesButton.disabled = running;
  scanDirectoryButton.disabled = running;
  deepScanButton.disabled = running;
  clearButton.disabled = running;
  analyzeButton.disabled = !running && selectedInputs.length === 0;
  analyzeButton.classList.toggle("cancel-mode", running);
  analyzeButton.textContent = t(running ? "btn.cancel" : "btn.analyze");

  const hasResult = lastEnvelope !== null;
  copyMarkdownButton.disabled = !hasResult;
  exportJsonButton.disabled = !hasResult;
  exportImageButton.disabled = !hasResult;
  sortModeSelect.disabled = !hasResult || displayEntries().length < 2;
  searchNextButton.disabled = !hasResult || displayEntries().length === 0;
};

const selectionLabel = (): string => {
  if (selectedInputs.length === 0) return "";
  if (selectedInputs.length === 1) return selectedInputs[0];
  return `${selectedInputs.length} inputs · ${fileName(selectedInputs[0])} …`;
};

const renderSelection = (): void => {
  inputPath.value = selectionLabel();
  inputPath.title = selectedInputs.join("\n");
  if (selectedInputs.length === 0) {
    scanResults.classList.add("hidden");
    scanResults.innerHTML = "";
  } else {
    const count = discoveredFiles?.length;
    const countText =
      count === undefined
        ? t("status.selected", { count: selectedInputs.length })
        : t("scan.count", { count });
    scanResults.innerHTML = `<strong>${escapeHtml(countText)}</strong><span>${escapeHtml(selectedInputs.map(fileName).slice(0, 4).join(" · "))}${selectedInputs.length > 4 ? " …" : ""}</span><span class="scan-mode">${escapeHtml(t(recursive ? "scan.recursive" : "scan.nonRecursive"))}</span>`;
    scanResults.classList.remove("hidden");
  }
  updateControls();
};

const cancelPreview = async (): Promise<void> => {
  const jobId = previewJobId;
  if (!jobId) return;
  previewJobId = null;
  try {
    await invoke<boolean>("cancel_job", { jobId });
  } catch {
    // A completed discovery may already have left the registry.
  }
};

const previewSelection = async (revision: number): Promise<void> => {
  await cancelPreview();
  if (revision !== selectionRevision || activeJobId || selectedInputs.length === 0) return;

  const inputs = [...selectedInputs];
  const currentRecursive = recursive;
  const jobId = crypto.randomUUID();
  previewJobId = jobId;
  status("status.discovering");
  try {
    const response = await invoke<DiscoveryResponse>("discover_inputs", {
      request: { jobId, inputs, recursive: currentRecursive },
    });
    if (
      previewJobId !== jobId ||
      revision !== selectionRevision ||
      activeJobId
    ) {
      return;
    }
    discoveredFiles = response.files;
    if (
      selectionKind === "mixed" &&
      inputs.length === 1 &&
      response.files.length === 1 &&
      response.files[0] === inputs[0]
    ) {
      selectionKind = "files";
    }
    renderSelection();
    status("status.discovered", { count: response.files.length });
  } catch (error) {
    if (
      previewJobId !== jobId ||
      revision !== selectionRevision ||
      activeJobId
    ) {
      return;
    }
    discoveredFiles = [];
    renderSelection();
    status(
      "status.discoveryFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    if (previewJobId === jobId) previewJobId = null;
  }
};

const selectInputs = (
  inputs: string[],
  kind: SelectionKind,
  scanRecursively: boolean,
): void => {
  const unique = [...new Set(inputs.filter((path) => path.trim().length > 0))];
  if (unique.length === 0) {
    status("status.noDropped", {}, { error: true });
    return;
  }
  void cancelPreview();
  selectionRevision += 1;
  selectedInputs = unique;
  selectionKind = kind;
  recursive = scanRecursively;
  discoveredFiles = null;
  renderSelection();
  status("status.selected", { count: unique.length });
  void previewSelection(selectionRevision);
};

const clearResults = (): void => {
  lastEnvelope = null;
  resultsElement.innerHTML = "";
  batchSummary.classList.add("hidden");
  batchSummary.textContent = "";
  rawPanel.hidden = true;
  rawJson.textContent = "";
  sortMode = "none";
  sortModeSelect.value = "none";
  searchInput.value = "";
  searchQuery = "";
  searchIndex = -1;
  updateControls();
};

const clearAll = (): void => {
  void cancelPreview();
  selectionRevision += 1;
  selectedInputs = [];
  selectionKind = "files";
  recursive = false;
  discoveredFiles = null;
  clearResults();
  renderSelection();
  status("status.ready");
};

const channelRole = (report: AnalysisReport, index: number): string | null => {
  const layout = report.pcm.spec.channelLayout;
  if (layout.status !== "known") return null;
  const roles: Record<string, string> = {
    front_left: "FL",
    front_right: "FR",
    front_center: "FC",
    lfe: "LFE",
    back_left: "BL",
    back_right: "BR",
    side_left: "SL",
    side_right: "SR",
    other: "OTHER",
  };
  const role = layout.positions[index];
  return role ? (roles[role] ?? role) : null;
};

const renderChannel = (report: AnalysisReport, channel: ChannelResult): string => {
  const role = channelRole(report, channel.channelIndex);
  const roleTag = role
    ? `<span class="tag${role === "LFE" ? " warning" : ""}">${escapeHtml(role)}</span>`
    : "";
  const label = `<span class="channel-label">CH ${channel.channelIndex + 1}${roleTag}</span>`;
  if (channel.outcome.status === "measured") {
    const measurement = channel.outcome.measurement;
    return `<tr>
      <td>${label}</td>
      <td class="dr-value">DR${measurement.roundedDr}</td>
      <td>${measurement.drDb.toFixed(4)} dB</td>
      <td>${formatDbfs(channel.report.overallRmsDbfs)}</td>
      <td>${formatDbfs(linearToDbfs(measurement.drSelectedPeak))}</td>
    </tr>`;
  }
  if (channel.outcome.status === "silent") {
    return `<tr class="muted-row"><td>${label}</td><td class="dr-value">DR0</td><td colspan="3">${escapeHtml(t("table.silent"))}</td></tr>`;
  }
  return `<tr class="muted-row"><td>${label}</td><td colspan="4">${escapeHtml(t("table.insufficient"))}</td></tr>`;
};

const reportHeader = (entry: DisplayEntry): string => {
  const name = fileName(entry.displayPath);
  return `<div class="directory-entry-header">
    <div class="entry-title">
      <div class="entry-title-row"><h3>${escapeHtml(name)}</h3></div>
      ${hidePath ? "" : `<span class="entry-path">${escapeHtml(entry.displayPath)}</span>`}
    </div>
    <div class="entry-actions">
      <button type="button" class="entry-action copy-entry-md" data-entry="${entry.key}">${escapeHtml(t("btn.entryMd"))}</button>
      ${entry.report ? `<button type="button" class="entry-action copy-entry-png" data-entry="${entry.key}">${escapeHtml(t("btn.entryPng"))}</button>` : ""}
    </div>
  </div>`;
};

const renderReportEntry = (entry: DisplayEntry): string => {
  if (!entry.report) return renderErrorEntry(entry);
  const report = entry.report;
  const aggregate = report.analysis.aggregates.track;
  const metrics = report.analysis.report;
  const duration = formatDuration(
    metrics.duration.decodedFrames,
    metrics.duration.sampleRate,
  );
  const bits = report.source.bitsPerSample
    ? t("label.bits", { bits: report.source.bitsPerSample })
    : "—";
  const aggregateHtml =
    aggregate.roundedDr === null || aggregate.drDb === null
      ? `<div class="dr-hero"><span>${escapeHtml(t("result.track"))}</span><strong>—</strong><em>${escapeHtml(t("result.noAggregate"))}</em></div>`
      : `<div class="dr-hero"><span>${escapeHtml(t("result.track"))}</span><strong>DR${aggregate.roundedDr}</strong><em>${aggregate.drDb.toFixed(4)} dB</em></div>`;
  const warnings = report.diagnostics.warnings.length
    ? `<div class="diagnostics">${report.diagnostics.warnings.map(escapeHtml).join("<br>")}</div>`
    : "";
  return `<article id="entry-${entry.key}" class="directory-entry" data-search="${escapeHtml(`${fileName(entry.displayPath)} ${entry.displayPath}`.toLowerCase())}">
    ${reportHeader(entry)}
    <div class="track-summary">
      ${aggregateHtml}
      <div class="metric"><span>${escapeHtml(t("result.peak"))}</span><strong>${formatDbfs(metrics.primaryPeakDbfs)}</strong></div>
      <div class="metric"><span>${escapeHtml(t("result.rms"))}</span><strong>${formatDbfs(metrics.overallRmsDbfs)}</strong></div>
      <div class="metric"><span>${escapeHtml(t("result.duration"))}</span><strong>${duration}</strong></div>
      <div class="metric"><span>${escapeHtml(t("result.backend"))}</span><strong>${escapeHtml(report.diagnostics.backend)}</strong></div>
    </div>
    <div class="format-line">
      <span>${report.source.sampleRate.toLocaleString()} Hz</span>
      <span>${report.source.channels} ch</span>
      <span>${escapeHtml(bits)}</span>
      <span>${escapeHtml(report.source.container)} / ${escapeHtml(report.source.codec)}</span>
      <span>${report.analysis.framesSeen.toLocaleString()} frames</span>
    </div>
    <div class="table-wrap"><table>
      <thead><tr><th>${escapeHtml(t("table.channel"))}</th><th>${escapeHtml(t("table.rounded"))}</th><th>${escapeHtml(t("table.precise"))}</th><th>${escapeHtml(t("table.rms"))}</th><th>${escapeHtml(t("table.peak"))}</th></tr></thead>
      <tbody>${report.analysis.channels.map((channel) => renderChannel(report, channel)).join("")}</tbody>
    </table></div>
    ${warnings}
  </article>`;
};

const renderErrorEntry = (entry: DisplayEntry): string => {
  const error = entry.error;
  const message = error?.message ?? t("status.failed");
  return `<article id="entry-${entry.key}" class="directory-entry" data-search="${escapeHtml(`${fileName(entry.displayPath)} ${entry.displayPath}`.toLowerCase())}">
    ${reportHeader(entry)}
    <div class="error-entry"><strong>${escapeHtml(t("result.failed"))}${error ? ` · ${escapeHtml(error.code)}` : ""}</strong><span>${escapeHtml(message)}</span>${error?.details ? `<code>${escapeHtml(error.details)}</code>` : ""}</div>
  </article>`;
};

const displayEntries = (): DisplayEntry[] => {
  if (!lastEnvelope) return [];
  if (lastEnvelope.kind === "analysis") {
    return [
      {
        key: 0,
        displayPath: lastEnvelope.data.source.displayPath,
        report: lastEnvelope.data,
        error: null,
      },
    ];
  }
  if (lastEnvelope.kind === "error") {
    return [
      {
        key: 0,
        displayPath: lastEnvelope.data.displayPath ?? selectedInputs[0] ?? "MacinMeter",
        report: null,
        error: lastEnvelope.data,
      },
    ];
  }
  return lastEnvelope.data.items.map((item: BatchItem, key) => ({
    key,
    displayPath: item.displayPath,
    report: item.outcome.status === "success" ? item.outcome.report : null,
    error: item.outcome.status === "failure" ? item.outcome.error : null,
  }));
};

const sortedEntries = (): DisplayEntry[] => {
  const entries = [...displayEntries()];
  if (sortMode === "none") return entries;
  const direction = sortMode === "dr-asc" ? 1 : -1;
  return entries.sort((left, right) => {
    const leftDr = left.report?.analysis.aggregates.track.drDb;
    const rightDr = right.report?.analysis.aggregates.track.drDb;
    if (leftDr === null || leftDr === undefined) return 1;
    if (rightDr === null || rightDr === undefined) return -1;
    const order = (leftDr - rightDr) * direction;
    return order === 0 ? left.key - right.key : order;
  });
};

const bindEntryActions = (): void => {
  resultsElement
    .querySelectorAll<HTMLButtonElement>(".copy-entry-md")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const entry = displayEntries().find(
          (candidate) => candidate.key === Number(button.dataset.entry),
        );
        if (entry) void copyText(formatEntryMarkdown(entry), button);
      });
    });
  resultsElement
    .querySelectorAll<HTMLButtonElement>(".copy-entry-png")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const key = Number(button.dataset.entry);
        const target = document.getElementById(`entry-${key}`);
        if (target) void copyPngToClipboard(target, button);
      });
    });
};

const renderResults = (): void => {
  const entries = sortedEntries();
  resultsElement.innerHTML = entries.map(renderReportEntry).join("");
  bindEntryActions();

  if (lastEnvelope?.kind === "batch") {
    const summary = lastEnvelope.data.summary;
    batchSummary.textContent = t("result.summary", summary);
    batchSummary.classList.remove("hidden");
  } else {
    batchSummary.classList.add("hidden");
  }
  if (searchQuery) highlightSearch(false);
  updateControls();
};

const reportAudioSeconds = (report: AnalysisReport): number => {
  const { decodedFrames, sampleRate } = report.analysis.report.duration;
  return sampleRate > 0 ? decodedFrames / sampleRate : 0;
};

// Only analyzed audio counts, so a run that spent its time rejecting files
// reports the lower multiple it actually achieved.
const envelopeAudioSeconds = (envelope: WireEnvelope): number => {
  if (envelope.kind === "analysis") return reportAudioSeconds(envelope.data);
  if (envelope.kind === "batch") {
    return envelope.data.items.reduce(
      (total, item) =>
        item.outcome.status === "success"
          ? total + reportAudioSeconds(item.outcome.report)
          : total,
      0,
    );
  }
  return 0;
};

// Wall time measured around the command, which is what the user waited. The
// realtime multiple needs no baseline beyond the material's own length, but
// it describes this host at this moment, so it stays in the status line and
// never enters the envelope the GUI exports.
const elapsedValues = (
  elapsedMs: number,
  audioSeconds: number,
): { elapsed: string; realtime: string } | null => {
  const elapsedSeconds = elapsedMs / 1000;
  if (!(elapsedSeconds > 0) || !(audioSeconds > 0)) return null;
  return {
    elapsed: elapsedSeconds.toFixed(3),
    realtime: (audioSeconds / elapsedSeconds).toFixed(1),
  };
};

const renderEnvelope = (envelope: WireEnvelope, elapsedMs: number): void => {
  lastEnvelope = envelope;
  rawJson.textContent = JSON.stringify(envelope, null, 2);
  rawPanel.hidden = false;
  renderResults();
  if (envelope.kind === "error") {
    status(
      envelope.data.code === "cancelled" ? "status.cancelled" : "status.failed",
      {},
      { error: envelope.data.code !== "cancelled", progress: 100 },
    );
    return;
  }
  const timing = elapsedValues(elapsedMs, envelopeAudioSeconds(envelope));
  if (envelope.kind === "analysis") {
    status(
      timing ? "status.completeTimed" : "status.complete",
      timing ?? {},
      { progress: 100 },
    );
  } else {
    status(
      timing ? "status.batchCompleteTimed" : "status.batchComplete",
      {
        succeeded: envelope.data.summary.succeeded,
        failed: envelope.data.summary.failed,
        ...(timing ?? {}),
      },
      { error: envelope.data.summary.failed > 0, progress: 100 },
    );
  }
};

const channelMarkdown = (channel: ChannelResult): string => {
  const label = `CH ${channel.channelIndex + 1}`;
  if (channel.outcome.status === "measured") {
    return `| ${label} | DR${channel.outcome.measurement.roundedDr} | ${channel.outcome.measurement.drDb.toFixed(4)} dB |`;
  }
  if (channel.outcome.status === "silent") return `| ${label} | DR0 | Silent |`;
  return `| ${label} | — | Insufficient data |`;
};

const formatEntryMarkdown = (entry: DisplayEntry): string => {
  let markdown = `## ${fileName(entry.displayPath)}\n\n`;
  if (!hidePath) markdown += `**${t("md.path")}**: ${entry.displayPath}\n\n`;
  if (entry.error) return `${markdown}**${t("md.error")}**: ${entry.error.message}\n`;
  if (!entry.report) return markdown;
  const aggregate = entry.report.analysis.aggregates.track;
  markdown += `| ${t("md.channel")} | ${t("md.displayDr")} | ${t("md.preciseDr")} |\n`;
  markdown += "|---|---:|---:|\n";
  markdown += `${entry.report.analysis.channels.map(channelMarkdown).join("\n")}\n\n`;
  if (aggregate.roundedDr !== null && aggregate.drDb !== null) {
    markdown += `**Track DR${aggregate.roundedDr}** · ${aggregate.drDb.toFixed(4)} dB\n`;
  }
  const { warnings } = entry.report.diagnostics;
  if (warnings.length) {
    markdown += `\n**${t("md.warnings")}**:\n`;
    markdown += `${warnings.map((warning) => `- ${warning}`).join("\n")}\n`;
  }
  return markdown;
};

const formatAllMarkdown = (): string => {
  if (!lastEnvelope) return "";
  const header = `# ${t("md.title")}\n\nMacinMeter ${lastEnvelope.toolVersion}\n\n`;
  return header + displayEntries().map(formatEntryMarkdown).join("\n");
};

const copyText = async (
  text: string,
  button: HTMLButtonElement,
): Promise<void> => {
  if (!text) return;
  const original = button.textContent;
  try {
    await navigator.clipboard.writeText(text);
    button.classList.add("copied");
    button.textContent = t("status.copied");
  } catch (error) {
    status(
      "status.copyFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    window.setTimeout(() => {
      button.classList.remove("copied");
      button.textContent = original;
    }, 1300);
  }
};

const fileTimestamp = (): string => {
  const now = new Date();
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
};

const dataUrlBytes = (dataUrl: string): Uint8Array => {
  const encoded = dataUrl.split(",")[1] ?? "";
  const binary = atob(encoded);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
};

const showFormatDialog = (): Promise<"png" | "svg" | null> =>
  new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const dialog = document.createElement("div");
    dialog.className = "modal-dialog";
    dialog.innerHTML = `<h3>${escapeHtml(t("dialog.exportFormat"))}</h3><div class="modal-buttons"><button type="button" data-format="png">PNG</button><button type="button" data-format="svg">SVG</button><button type="button" data-format="cancel" class="ghost">${escapeHtml(t("dialog.cancel"))}</button></div>`;
    overlay.appendChild(dialog);
    document.body.appendChild(overlay);
    const finish = (format: "png" | "svg" | null): void => {
      overlay.remove();
      resolve(format);
    };
    dialog.querySelectorAll<HTMLButtonElement>("button").forEach((button) => {
      button.addEventListener("click", () => {
        const format = button.dataset.format;
        finish(format === "png" || format === "svg" ? format : null);
      });
    });
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) finish(null);
    });
  });

const imageOptions = (target: HTMLElement): Record<string, unknown> => {
  const bounds = target.getBoundingClientRect();
  const scale = Math.min(3, 16_384 / Math.max(bounds.width, bounds.height, 1));
  return {
    backgroundColor: "#fbf7f0",
    canvasWidth: Math.round(bounds.width * scale),
    canvasHeight: Math.round(bounds.height * scale),
    pixelRatio: 1,
    skipAutoScale: true,
    skipFonts: true,
    style: {
      fontFamily:
        "'Hiragino Sans', 'PingFang SC', 'Yu Gothic', Meiryo, 'Microsoft YaHei', system-ui, sans-serif",
      position: "static",
      top: "auto",
      left: "auto",
      zIndex: "auto",
    },
  };
};

const withImageCaptureFrame = async <T>(
  target: HTMLElement,
  capture: (frame: HTMLElement) => Promise<T>,
): Promise<T> => {
  const padding = 24;
  const frame = document.createElement("div");
  frame.className = "image-capture-frame";
  frame.style.width = `${Math.ceil(target.getBoundingClientRect().width) + padding * 2}px`;
  frame.setAttribute("aria-hidden", "true");

  const clone = target.cloneNode(true) as HTMLElement;
  clone.classList.add("exporting");
  clone.classList.remove("search-hit");
  clone.querySelectorAll(".search-hit").forEach((element) => {
    element.classList.remove("search-hit");
  });
  frame.appendChild(clone);
  document.body.appendChild(frame);

  try {
    return await capture(frame);
  } finally {
    frame.remove();
  }
};

const renderPng = (target: HTMLElement): Promise<string> =>
  withImageCaptureFrame(target, (frame) => toPng(frame, imageOptions(frame)));

const renderSvg = (target: HTMLElement): Promise<string> =>
  withImageCaptureFrame(target, (frame) =>
    toSvg(frame, {
      backgroundColor: "#fbf7f0",
      skipFonts: true,
      style: {
        position: "static",
        top: "auto",
        left: "auto",
        zIndex: "auto",
      },
    }),
  );

const exportImage = async (): Promise<void> => {
  if (!lastEnvelope || resultsElement.children.length === 0) return;
  const format = await showFormatDialog();
  if (!format) return;
  const name = `MacinMeter_v${lastEnvelope.toolVersion}_${fileTimestamp()}.${format}`;
  const path = await save({
    defaultPath: name,
    filters: [
      {
        name: format === "png" ? "PNG Image" : "SVG Image",
        extensions: [format],
      },
    ],
  });
  if (!path) return;
  try {
    if (format === "png") {
      await writeFile(path, dataUrlBytes(await renderPng(resultsElement)));
    } else {
      const dataUrl = await renderSvg(resultsElement);
      await writeTextFile(path, decodeURIComponent(dataUrl.split(",")[1] ?? ""));
    }
    status("status.exported", { name });
  } catch (error) {
    status(
      "status.exportFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  }
};

const copyPngToClipboard = async (
  target: HTMLElement,
  button: HTMLButtonElement,
): Promise<void> => {
  const original = button.textContent;
  button.disabled = true;
  try {
    const dataUrl = await renderPng(target);
    await writeImage(dataUrlBytes(dataUrl));
    button.classList.add("copied");
    button.textContent = "OK!";
    status("status.pngCopied");
  } catch (error) {
    status(
      "status.pngCopyFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    window.setTimeout(() => {
      button.disabled = false;
      button.classList.remove("copied");
      button.textContent = original;
    }, 1200);
  }
};

const highlightSearch = (advance: boolean): void => {
  const query = searchInput.value.trim().toLowerCase();
  const entries = [...resultsElement.querySelectorAll<HTMLElement>(".directory-entry")];
  entries.forEach((entry) => entry.classList.remove("search-hit"));
  if (!query) {
    searchQuery = "";
    searchIndex = -1;
    return;
  }
  const matches = entries.filter((entry) =>
    (entry.dataset.search ?? "").includes(query),
  );
  if (matches.length === 0) {
    status("status.searchNotFound", { query }, { error: true });
    searchQuery = query;
    searchIndex = -1;
    return;
  }
  if (query !== searchQuery) searchIndex = -1;
  searchQuery = query;
  if (advance) searchIndex = (searchIndex + 1) % matches.length;
  else if (searchIndex < 0 || searchIndex >= matches.length) searchIndex = 0;
  const target = matches[searchIndex];
  target.classList.add("search-hit");
  if (advance) target.scrollIntoView({ behavior: "smooth", block: "start" });
};

const setDropOverlay = (visible: boolean): void => {
  if (!visible) {
    dropOverlay.hidden = true;
    dropOverlay.setAttribute("aria-hidden", "true");
    return;
  }
  const busy = activeJobId !== null;
  dropCard.classList.toggle("busy", busy);
  dropTitle.textContent = t(busy ? "drop.busy" : "drop.title");
  dropSubtitle.textContent = t(busy ? "status.busyDrop" : "drop.subtitle");
  dropOverlay.hidden = false;
  dropOverlay.setAttribute("aria-hidden", "false");
};

const cancelActiveJob = async (): Promise<void> => {
  if (!activeJobId) return;
  const jobId = activeJobId;
  status("status.cancelling");
  try {
    const accepted = await invoke<boolean>("cancel_job", { jobId });
    if (activeJobId === jobId && !accepted) status("status.complete");
  } catch (error) {
    status(
      "status.discoveryFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  }
};

const runAnalysis = async (): Promise<void> => {
  if (activeJobId) {
    await cancelActiveJob();
    return;
  }
  if (selectedInputs.length === 0) return;

  await cancelPreview();
  const jobId = crypto.randomUUID();
  activeJobId = jobId;
  activeTotal = Math.max(discoveredFiles?.length ?? 1, 1);
  activeProgress.reset(activeTotal);
  clearResults();
  updateControls();
  status("status.running", {}, { progress: 0 });

  const startedAt = performance.now();
  try {
    const envelope =
      selectionKind === "files" && selectedInputs.length === 1
        ? await invoke<WireEnvelope>("run_analysis", {
            request: { jobId, path: selectedInputs[0] },
          })
        : await invoke<WireEnvelope>("run_batch", {
            request: {
              jobId,
              inputs: selectedInputs,
              recursive,
            },
          });
    renderEnvelope(envelope, performance.now() - startedAt);
  } catch (error) {
    status(
      "status.discoveryFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    if (activeJobId === jobId) activeJobId = null;
    updateControls();
  }
};

pickFilesButton.addEventListener("click", async () => {
  const result = await open({
    multiple: true,
    directory: false,
    filters:
      discoveryFilterExtensions.length > 0
        ? [
            {
              name: "MacinMeter audio",
              extensions: discoveryFilterExtensions,
            },
          ]
        : [],
  });
  if (!result) return;
  const paths = Array.isArray(result) ? result : [result];
  selectInputs(paths, "files", false);
});

scanDirectoryButton.addEventListener("click", async () => {
  const result = await open({ multiple: false, directory: true });
  if (!result || Array.isArray(result)) return;
  selectInputs([result], "directory", false);
});

deepScanButton.addEventListener("click", async () => {
  const result = await open({ multiple: false, directory: true });
  if (!result || Array.isArray(result)) return;
  const proceed = await confirm(t("dialog.deepScanMessage"), {
    title: t("dialog.deepScanTitle"),
    kind: "warning",
  });
  if (!proceed) return;
  selectInputs([result], "directory", true);
});

clearButton.addEventListener("click", clearAll);
analyzeButton.addEventListener("click", () => void runAnalysis());
statusDismissButton.addEventListener("click", () => {
  statusElement.hidden = true;
});

hidePathButton.addEventListener("click", () => {
  hidePath = !hidePath;
  hidePathButton.classList.toggle("active", hidePath);
  renderResults();
});

copyMarkdownButton.addEventListener("click", () => {
  void copyText(formatAllMarkdown(), copyMarkdownButton);
});

exportJsonButton.addEventListener("click", async () => {
  if (!lastEnvelope) return;
  const name = `MacinMeter_v${lastEnvelope.toolVersion}_${fileTimestamp()}.json`;
  const path = await save({
    defaultPath: name,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!path) return;
  try {
    await writeTextFile(path, JSON.stringify(lastEnvelope, null, 2));
    status("status.exported", { name });
  } catch (error) {
    status(
      "status.exportFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  }
});

exportImageButton.addEventListener("click", () => void exportImage());

sortModeSelect.addEventListener("change", () => {
  sortMode = sortModeSelect.value as SortMode;
  renderResults();
});

searchInput.addEventListener("input", () => highlightSearch(false));
searchInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") highlightSearch(true);
});
searchNextButton.addEventListener("click", () => highlightSearch(true));

const setLanguage = (language: SupportedLanguage): void => {
  changeLanguage(language);
  updateStaticTexts();
  updateLanguageButtons();
  document.title = t("title");
  renderSelection();
  renderStatus();
  if (lastEnvelope) renderResults();
};

element<HTMLButtonElement>("lang-zh").addEventListener("click", () =>
  setLanguage("zh-CN"),
);
element<HTMLButtonElement>("lang-en").addEventListener("click", () =>
  setLanguage("en-US"),
);

void listen<JobEvent>("analysis-event", ({ payload }) => {
  if (payload.jobId !== activeJobId) return;
  const event = payload.event;
  if (event.type === "discovery_started") {
    status("status.discovering", {}, { progress: 0 });
  } else if (event.type === "discovery_finished") {
    activeTotal = Math.max(event.files, 1);
    activeProgress.reset(activeTotal);
    status("status.running", {}, { progress: 0 });
  } else if (event.type === "file_started") {
    status(
      "status.file",
      {
        name: fileName(event.displayPath),
        current: event.index + 1,
        total: activeTotal,
      },
      { progress: activeProgress.update(event.index, 0) },
    );
  } else if (event.type === "decode_progress") {
    const fraction = event.progress.fraction ?? 0;
    status(
      "status.file",
      {
        name: fileName(event.displayPath),
        current: event.index + 1,
        total: activeTotal,
      },
      { progress: activeProgress.update(event.index, fraction) },
    );
  } else if (event.type === "file_finished") {
    status(
      "status.file",
      {
        name: fileName(event.displayPath),
        current: event.index + 1,
        total: activeTotal,
      },
      { progress: activeProgress.update(event.index, 1) },
    );
  } else if (event.type === "batch_finished") {
    status(
      "status.batchComplete",
      { succeeded: event.succeeded, failed: event.failed },
      { error: event.failed > 0, progress: 100 },
    );
  }
}).catch((error: unknown) => {
  status(
    "status.discoveryFailed",
    { message: describeInvokeError(error) },
    { error: true },
  );
});

void getCurrentWebview()
  .onDragDropEvent(({ payload }) => {
    if (payload.type === "enter" || payload.type === "over") {
      setDropOverlay(true);
    } else if (payload.type === "leave") {
      setDropOverlay(false);
    } else if (payload.type === "drop") {
      setDropOverlay(false);
      if (activeJobId) {
        status("status.busyDrop", {}, { error: true });
        return;
      }
      selectInputs(payload.paths, "mixed", false);
    }
  })
  .catch((error: unknown) => {
    status(
      "status.discoveryFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  });

const loadCapabilities = async (): Promise<void> => {
  try {
    const snapshot = await invoke<CapabilitySnapshot>("get_capabilities");
    discoveryFilterExtensions = snapshot.stableDiscoveryExtensions;
  } catch {
    discoveryFilterExtensions = [];
  }
};

appVersion.textContent = `v${packageMetadata.version}`;
setLanguage(getCurrentLanguage());
renderSelection();
status("status.ready");
void loadCapabilities();
