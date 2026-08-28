import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { join } from "@tauri-apps/api/path";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { create } from "@tauri-apps/plugin-fs";
import { toBlob, toSvg } from "html-to-image";
import packageMetadata from "../package.json";
import { BatchProgress } from "./batch-progress";
import {
  byteRanges,
  numberedExportPath,
  paginateItemIndexes,
  PNG_PAGE_MAX_CONTENT_HEIGHT,
  safeImageScale,
  SVG_PAGE_MAX_CONTENT_HEIGHT,
} from "./export-utils";
import {
  changeLanguage,
  getCurrentLanguage,
  t,
  updateLanguageButtons,
  updateStaticTexts,
  type SupportedLanguage,
} from "./i18n";
import type {
  AnalysisEvent,
  AnalysisReport,
  BatchCompletion,
  BatchItem,
  BatchItemSummary,
  BatchRunResponse,
  CapabilitySnapshot,
  ChannelResult,
  DiscoveryResponse,
  FrontendMessage,
  JobTiming,
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
  summary: BatchItemSummary | null;
  report: AnalysisReport | null;
  error: PublicError | null;
  expanded: boolean;
};

type LazyBatchResult = {
  resultId: string;
  toolVersion: string;
  completion: BatchCompletion;
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
const showTimingButton = element<HTMLButtonElement>("show-timing");
const hideWarningsButton = element<HTMLButtonElement>("hide-warnings");
const hideFailedButton = element<HTMLButtonElement>("hide-failed");
const phaseTimings = element<HTMLDivElement>("phase-timings");
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
let cancellingJobId: string | null = null;
let activeTotal = 1;
const activeProgress = new BatchProgress(activeTotal);
let lastEnvelope: WireEnvelope | null = null;
let lastBatchResult: LazyBatchResult | null = null;
const batchItemSummaries = new Map<number, BatchItemSummary>();
const pendingBatchItemSummaries = new Map<number, BatchItemSummary>();
const loadedBatchItems = new Map<number, BatchItem>();
const expandedBatchItems = new Set<number>();
const loadingBatchItems = new Set<number>();
const renderedStreamedBatchNodes = new Map<number, HTMLElement>();
const renderedStreamedBatchIndexes: number[] = [];
let streamedBatchRenderFrame: number | null = null;
let hidePath = false;
// Off by default: each interval reads the clock at both start and stop, and an
// ordinary run should not pay for observation it did not ask for.
let showTiming = false;
// Warnings repeat on every multichannel file and a large scan can be mostly
// failures, so both are dismissible in one action rather than one per entry.
let hideWarnings = false;
let hideFailed = false;
let lastPhaseTimings: JobTiming | null = null;
let sortMode: SortMode = "none";
let searchQuery = "";
let searchIndex = -1;
let discoveryFilterExtensions: string[] = [];
let statusState: StatusState = { key: "status.ready", progress: 0 };
let exportInProgress = false;
let rawRequestRevision = 0;

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

const restoreStatus = (previous: StatusState, wasHidden: boolean): void => {
  statusState = previous;
  statusElement.hidden = wasHidden;
  renderStatus();
};

const updateControls = (): void => {
  const running = activeJobId !== null;
  const cancelling = running && cancellingJobId === activeJobId;
  const interactionLocked = running || exportInProgress;
  pickFilesButton.disabled = interactionLocked;
  scanDirectoryButton.disabled = interactionLocked;
  deepScanButton.disabled = interactionLocked;
  clearButton.disabled = interactionLocked;
  showTimingButton.disabled = interactionLocked;
  analyzeButton.disabled =
    exportInProgress || cancelling || (!running && selectedInputs.length === 0);
  analyzeButton.classList.toggle("cancel-mode", running);
  analyzeButton.textContent = t(running ? "btn.cancel" : "btn.analyze");

  const hasResult = lastEnvelope !== null || lastBatchResult !== null;
  copyMarkdownButton.disabled = !hasResult || interactionLocked;
  exportJsonButton.disabled = !hasResult || interactionLocked;
  exportImageButton.disabled = !hasResult || interactionLocked;
  hideWarningsButton.disabled = interactionLocked;
  hideFailedButton.disabled = interactionLocked;
  sortModeSelect.disabled =
    interactionLocked || !hasResult || visibleEntries().length < 2;
  searchInput.disabled = interactionLocked || !hasResult;
  searchNextButton.disabled =
    interactionLocked || !hasResult || visibleEntries().length === 0;
  resultsElement.toggleAttribute("inert", interactionLocked);
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

const resetStreamedBatch = (): void => {
  if (streamedBatchRenderFrame !== null) {
    cancelAnimationFrame(streamedBatchRenderFrame);
    streamedBatchRenderFrame = null;
  }
  batchItemSummaries.clear();
  pendingBatchItemSummaries.clear();
  loadedBatchItems.clear();
  expandedBatchItems.clear();
  loadingBatchItems.clear();
  renderedStreamedBatchNodes.clear();
  renderedStreamedBatchIndexes.length = 0;
};

const releaseRetainedBatch = (): void => {
  const resultId = lastBatchResult?.resultId;
  lastBatchResult = null;
  if (!resultId) return;
  void invoke<boolean>("release_batch_results", {
    request: { resultId },
  }).catch(() => {
    // The backend may already have discarded a failed or cancelled session.
  });
};

const clearResults = (): void => {
  releaseRetainedBatch();
  resetStreamedBatch();
  lastEnvelope = null;
  lastPhaseTimings = null;
  renderPhaseTimings();
  resultsElement.innerHTML = "";
  batchSummary.classList.add("hidden");
  batchSummary.textContent = "";
  rawPanel.open = false;
  rawPanel.hidden = true;
  rawJson.textContent = "";
  delete rawJson.dataset.loading;
  rawRequestRevision += 1;
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
  const lazySuccess = entry.summary?.outcome.status === "success";
  const loading = loadingBatchItems.has(entry.key);
  const detailAction = lazySuccess
    ? `<button type="button" class="entry-action toggle-entry-detail" data-entry="${entry.key}" ${loading ? "disabled" : ""}>${escapeHtml(t(loading ? "btn.loadingDetails" : entry.expanded ? "btn.hideDetails" : "btn.showDetails"))}</button>`
    : "";
  return `<div class="directory-entry-header">
    <div class="entry-title">
      <div class="entry-title-row"><h3>${escapeHtml(name)}</h3></div>
      ${hidePath ? "" : `<span class="entry-path">${escapeHtml(entry.displayPath)}</span>`}
    </div>
    <div class="entry-actions">
      ${detailAction}
      ${entry.report || entry.error ? `<button type="button" class="entry-action copy-entry-md" data-entry="${entry.key}">${escapeHtml(t("btn.entryMd"))}</button>` : ""}
      ${entry.report ? `<button type="button" class="entry-action copy-entry-png" data-entry="${entry.key}">${escapeHtml(t("btn.entryPng"))}</button>` : ""}
    </div>
  </div>`;
};

const renderReportEntry = (entry: DisplayEntry): string => {
  if (entry.summary?.outcome.status === "success" && !entry.report) {
    return renderBatchSummaryEntry(entry, entry.summary.outcome.metrics);
  }
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
  const warnings = !hideWarnings && report.diagnostics.warnings.length
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

const renderBatchSummaryEntry = (
  entry: DisplayEntry,
  metrics: Extract<BatchItemSummary["outcome"], { status: "success" }>["metrics"],
): string => {
  const duration = formatDuration(
    metrics.duration.decodedFrames,
    metrics.duration.sampleRate,
  );
  const bits = metrics.bitsPerSample
    ? t("label.bits", { bits: metrics.bitsPerSample })
    : "—";
  const aggregateHtml =
    metrics.roundedDr === null || metrics.trackDrDb === null
      ? `<div class="dr-hero"><span>${escapeHtml(t("result.track"))}</span><strong>—</strong><em>${escapeHtml(t("result.noAggregate"))}</em></div>`
      : `<div class="dr-hero"><span>${escapeHtml(t("result.track"))}</span><strong>DR${metrics.roundedDr}</strong><em>${metrics.trackDrDb.toFixed(4)} dB</em></div>`;
  return `<article id="entry-${entry.key}" class="directory-entry batch-list-entry" data-search="${escapeHtml(`${fileName(entry.displayPath)} ${entry.displayPath}`.toLowerCase())}">
    ${reportHeader(entry)}
    <div class="track-summary batch-list-summary">
      ${aggregateHtml}
      <div class="metric"><span>${escapeHtml(t("result.peak"))}</span><strong>${formatDbfs(metrics.primaryPeakDbfs)}</strong></div>
      <div class="metric"><span>${escapeHtml(t("result.rms"))}</span><strong>${formatDbfs(metrics.overallRmsDbfs)}</strong></div>
      <div class="metric"><span>${escapeHtml(t("result.duration"))}</span><strong>${duration}</strong></div>
      <div class="metric"><span>${escapeHtml(t("result.backend"))}</span><strong>${escapeHtml(metrics.backend)}</strong></div>
    </div>
    <div class="format-line">
      <span>${metrics.sampleRate.toLocaleString()} Hz</span>
      <span>${metrics.channels} ch</span>
      <span>${escapeHtml(bits)}</span>
      <span>${escapeHtml(metrics.container)} / ${escapeHtml(metrics.codec)}</span>
      ${metrics.warningCount > 0 ? `<span>${escapeHtml(t("result.warningCount", { count: metrics.warningCount }))}</span>` : ""}
    </div>
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

const batchDisplayEntry = (item: BatchItem, key: number): DisplayEntry => ({
  key,
  displayPath: item.displayPath,
  summary: batchItemSummaries.get(key) ?? null,
  report: item.outcome.status === "success" ? item.outcome.report : null,
  error: item.outcome.status === "failure" ? item.outcome.error : null,
  expanded: true,
});

const batchSummaryDisplayEntry = (
  item: BatchItemSummary,
  key: number,
): DisplayEntry => {
  const loaded = loadedBatchItems.get(key);
  const expanded = expandedBatchItems.has(key);
  return {
    key,
    displayPath: item.displayPath,
    summary: item,
    report:
      expanded && loaded?.outcome.status === "success"
        ? loaded.outcome.report
        : null,
    error:
      item.outcome.status === "failure"
        ? item.outcome.error
        : expanded && loaded?.outcome.status === "failure"
          ? loaded.outcome.error
          : null,
    expanded,
  };
};

const displayEntries = (): DisplayEntry[] => {
  if (lastBatchResult || (!lastEnvelope && batchItemSummaries.size > 0)) {
    return [...batchItemSummaries.entries()]
      .sort(([left], [right]) => left - right)
      .map(([key, item]) => batchSummaryDisplayEntry(item, key));
  }
  if (!lastEnvelope) {
    return [];
  }
  if (lastEnvelope.kind === "analysis") {
    return [
      {
        key: 0,
        displayPath: lastEnvelope.data.source.displayPath,
        summary: null,
        report: lastEnvelope.data,
        error: null,
        expanded: true,
      },
    ];
  }
  if (lastEnvelope.kind === "error") {
    return [
      {
        key: 0,
        displayPath: lastEnvelope.data.displayPath ?? selectedInputs[0] ?? "MacinMeter",
        summary: null,
        report: null,
        error: lastEnvelope.data,
        expanded: true,
      },
    ];
  }
  return lastEnvelope.data.items.map(batchDisplayEntry);
};

const visibleEntries = (): DisplayEntry[] =>
  hideFailed
    ? displayEntries().filter(
        (entry) =>
          entry.summary?.outcome.status === "success" || entry.report !== null,
      )
    : displayEntries();

const sortedEntries = (): DisplayEntry[] => {
  const entries = [...visibleEntries()];
  if (sortMode === "none") return entries;
  const direction = sortMode === "dr-asc" ? 1 : -1;
  return entries.sort((left, right) => {
    const leftDr =
      left.summary?.outcome.status === "success"
        ? left.summary.outcome.metrics.trackDrDb
        : left.report?.analysis.aggregates.track.drDb;
    const rightDr =
      right.summary?.outcome.status === "success"
        ? right.summary.outcome.metrics.trackDrDb
        : right.report?.analysis.aggregates.track.drDb;
    const leftMissing = leftDr === null || leftDr === undefined;
    const rightMissing = rightDr === null || rightDr === undefined;
    if (leftMissing && rightMissing) return left.key - right.key;
    if (leftMissing) return 1;
    if (rightMissing) return -1;
    const order = (leftDr - rightDr) * direction;
    return order === 0 ? left.key - right.key : order;
  });
};

const rerenderBatchEntry = (key: number): void => {
  const current = renderedStreamedBatchNodes.get(key);
  const summary = batchItemSummaries.get(key);
  if (!current || !summary) return;
  const template = document.createElement("template");
  template.innerHTML = renderReportEntry(batchSummaryDisplayEntry(summary, key));
  const replacement = template.content.firstElementChild;
  if (!(replacement instanceof HTMLElement)) return;
  bindEntryActions(replacement);
  current.replaceWith(replacement);
  renderedStreamedBatchNodes.set(key, replacement);
};

const toggleBatchDetail = async (key: number): Promise<void> => {
  if (expandedBatchItems.delete(key)) {
    rerenderBatchEntry(key);
    return;
  }
  if (loadedBatchItems.has(key)) {
    expandedBatchItems.add(key);
    rerenderBatchEntry(key);
    return;
  }
  const result = lastBatchResult;
  const summary = batchItemSummaries.get(key);
  if (!result || summary?.outcome.status !== "success" || loadingBatchItems.has(key)) {
    return;
  }

  loadingBatchItems.add(key);
  rerenderBatchEntry(key);
  try {
    const item = await invoke<BatchItem>("get_batch_item_detail", {
      request: { resultId: result.resultId, index: key },
    });
    if (lastBatchResult?.resultId !== result.resultId) return;
    if (item.displayPath !== summary.displayPath || item.outcome.status !== "success") {
      throw new Error("batch detail does not match its list summary");
    }
    loadedBatchItems.set(key, item);
    expandedBatchItems.add(key);
  } catch (error) {
    status(
      "status.detailFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    loadingBatchItems.delete(key);
    rerenderBatchEntry(key);
  }
};

const bindEntryActions = (root: ParentNode = resultsElement): void => {
  root
    .querySelectorAll<HTMLButtonElement>(
      ".toggle-entry-detail:not([data-actions-bound])",
    )
    .forEach((button) => {
      button.dataset.actionsBound = "true";
      button.addEventListener("click", () => {
        void toggleBatchDetail(Number(button.dataset.entry));
      });
    });
  root
    .querySelectorAll<HTMLButtonElement>(
      ".copy-entry-md:not([data-actions-bound])",
    )
    .forEach((button) => {
      button.dataset.actionsBound = "true";
      button.addEventListener("click", () => {
        const entry = displayEntries().find(
          (candidate) => candidate.key === Number(button.dataset.entry),
        );
        if (entry) void copyText(formatEntryMarkdown(entry), button);
      });
    });
  root
    .querySelectorAll<HTMLButtonElement>(
      ".copy-entry-png:not([data-actions-bound])",
    )
    .forEach((button) => {
      button.dataset.actionsBound = "true";
      button.addEventListener("click", () => {
        const key = Number(button.dataset.entry);
        const target = document.getElementById(`entry-${key}`);
        if (target) void copyPngToClipboard(target, button);
      });
    });
};

// Keep each paint bounded even when many tiny files finish between two WebView
// frames. This makes a backlog appear page by page instead of parsing hundreds
// of result cards in one long main-thread task.
const STREAMED_RENDER_ITEMS_PER_FRAME = 12;

const renderStreamedBatchSummary = (): void => {
  let succeeded = 0;
  let failed = 0;
  for (const item of batchItemSummaries.values()) {
    if (item.outcome.status === "success") succeeded += 1;
    else failed += 1;
  }
  batchSummary.textContent = t("result.summary", {
    total: activeTotal,
    succeeded,
    failed,
  });
  batchSummary.classList.remove("hidden");
};

const streamedInsertionIndex = (key: number): number => {
  let low = 0;
  let high = renderedStreamedBatchIndexes.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (renderedStreamedBatchIndexes[middle] < key) low = middle + 1;
    else high = middle;
  }
  return low;
};

const flushStreamedBatchItems = (): void => {
  const entries = [...pendingBatchItemSummaries.entries()]
    .sort(([left], [right]) => left - right)
    .slice(0, STREAMED_RENDER_ITEMS_PER_FRAME);
  if (entries.length === 0) return;
  for (const [key] of entries) pendingBatchItemSummaries.delete(key);

  const template = document.createElement("template");
  template.innerHTML = entries
    .map(([key, item]) => renderReportEntry(batchSummaryDisplayEntry(item, key)))
    .join("");
  bindEntryActions(template.content);
  const nodes = Array.from(template.content.children) as HTMLElement[];
  for (let offset = 0; offset < entries.length; offset += 1) {
    const [key] = entries[offset];
    const node = nodes[offset];
    if (!node || renderedStreamedBatchNodes.has(key)) continue;
    const insertion = streamedInsertionIndex(key);
    const followingKey = renderedStreamedBatchIndexes[insertion];
    const following =
      followingKey === undefined
        ? null
        : (renderedStreamedBatchNodes.get(followingKey) ?? null);
    resultsElement.insertBefore(node, following);
    renderedStreamedBatchIndexes.splice(insertion, 0, key);
    renderedStreamedBatchNodes.set(key, node);
  }
  renderStreamedBatchSummary();
};

const scheduleStreamedBatchRender = (): void => {
  if (streamedBatchRenderFrame !== null) return;
  streamedBatchRenderFrame = requestAnimationFrame(() => {
    streamedBatchRenderFrame = null;
    flushStreamedBatchItems();
    if (pendingBatchItemSummaries.size > 0) scheduleStreamedBatchRender();
  });
};

const queueStreamedBatchItem = (
  index: number,
  item: BatchItemSummary,
): void => {
  if (
    lastEnvelope !== null ||
    lastBatchResult !== null ||
    !Number.isInteger(index) ||
    index < 0 ||
    batchItemSummaries.has(index)
  ) {
    return;
  }
  batchItemSummaries.set(index, item);
  pendingBatchItemSummaries.set(index, item);
  scheduleStreamedBatchRender();
};

const finishBatchList = async (result: LazyBatchResult): Promise<void> => {
  if (streamedBatchRenderFrame !== null) {
    cancelAnimationFrame(streamedBatchRenderFrame);
    streamedBatchRenderFrame = null;
  }

  while (pendingBatchItemSummaries.size > 0) {
    status(
      "status.renderingResults",
      {
        current: renderedStreamedBatchNodes.size,
        total: result.completion.summary.total,
      },
      {
        progress:
          (renderedStreamedBatchNodes.size /
            Math.max(result.completion.summary.total, 1)) *
          100,
      },
    );
    await nextPaint();
    flushStreamedBatchItems();
  }

  const complete =
    batchItemSummaries.size === result.completion.summary.total &&
    Array.from(
      { length: result.completion.summary.total },
      (_, index) => index,
    ).every((index) => batchItemSummaries.has(index));
  let succeeded = 0;
  let failed = 0;
  for (const item of batchItemSummaries.values()) {
    if (item.outcome.status === "success") succeeded += 1;
    else failed += 1;
  }
  if (
    !complete ||
    succeeded !== result.completion.summary.succeeded ||
    failed !== result.completion.summary.failed
  ) {
    throw new Error("batch summary stream is incomplete");
  }
  lastBatchResult = result;
  rawPanel.hidden = false;
  batchSummary.textContent = t("result.summary", result.completion.summary);
  batchSummary.classList.remove("hidden");
  updateControls();
};

const renderResults = (): void => {
  const entries = sortedEntries();
  resultsElement.innerHTML = entries.map(renderReportEntry).join("");
  bindEntryActions();

  if (lastBatchResult) {
    renderedStreamedBatchNodes.clear();
    renderedStreamedBatchIndexes.length = 0;
    for (const entry of entries) {
      const node = document.getElementById(`entry-${entry.key}`);
      if (!node) continue;
      renderedStreamedBatchNodes.set(entry.key, node);
      renderedStreamedBatchIndexes.push(entry.key);
    }
  }

  const summary =
    lastBatchResult?.completion.summary ??
    (lastEnvelope?.kind === "batch" ? lastEnvelope.data.summary : null);
  if (summary) {
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

const lazyBatchAudioSeconds = (): number => {
  let total = 0;
  for (const item of batchItemSummaries.values()) {
    if (item.outcome.status !== "success") continue;
    const { decodedFrames, sampleRate } = item.outcome.metrics.duration;
    if (sampleRate > 0) total += decodedFrames / sampleRate;
  }
  return total;
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

const renderEnvelope = async (
  envelope: WireEnvelope,
  elapsedMs: number,
): Promise<void> => {
  lastEnvelope = envelope;
  rawJson.textContent = "";
  rawPanel.hidden = false;
  resetStreamedBatch();
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
  renderPhaseTimings();
};

const renderCompletedBatch = async (
  response: Extract<BatchRunResponse, { outcome: "completed" }>,
  elapsedMs: number,
): Promise<void> => {
  const result: LazyBatchResult = {
    resultId: response.resultId,
    toolVersion: response.toolVersion,
    completion: response.completion,
  };
  await finishBatchList(result);
  const timing = elapsedValues(elapsedMs, lazyBatchAudioSeconds());
  status(
    timing ? "status.batchCompleteTimed" : "status.batchComplete",
    {
      succeeded: response.completion.summary.succeeded,
      failed: response.completion.summary.failed,
      ...(timing ?? {}),
    },
    { error: response.completion.summary.failed > 0, progress: 100 },
  );
  renderPhaseTimings();
};

// A second line rather than more of the status sentence: the roles may overlap
// and omit other work, so printing them without the caveat makes them look like
// a split of elapsed time.
const renderPhaseTimings = (): void => {
  if (!lastPhaseTimings) {
    phaseTimings.hidden = true;
    phaseTimings.textContent = "";
    return;
  }
  const seconds = (roundedMs: number): string =>
    (roundedMs / 1000).toFixed(3);
  // Round the two displayed endpoints before deriving `other`. Rounding all
  // three independently can visibly turn an exact 1 + 1 = 2 partition into
  // 1 + 1 = 1 at millisecond precision.
  const roleValues = (
    activeMs: number,
    spanMs: number,
  ): Record<string, string> => {
    const active = Math.round(activeMs);
    const span = Math.round(spanMs);
    return {
      active: seconds(active),
      other: seconds(Math.max(0, span - active)),
      span: seconds(span),
    };
  };
  const batch = lastPhaseTimings.batch;
  phaseTimings.hidden = false;
  phaseTimings.textContent = [
    t(
      batch ? "status.phaseBatchDecode" : "status.phaseDecode",
      roleValues(lastPhaseTimings.decodeMs, lastPhaseTimings.decodeSpanMs),
    ),
    t(
      batch ? "status.phaseBatchAnalysis" : "status.phaseAnalysis",
      roleValues(lastPhaseTimings.analysisMs, lastPhaseTimings.analysisSpanMs),
    ),
    t(batch ? "status.phaseBatchCaveat" : "status.phaseCaveat"),
  ].join("\n");
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

const fullResultEnvelope = async (): Promise<WireEnvelope | null> => {
  if (lastEnvelope) return lastEnvelope;
  const resultId = lastBatchResult?.resultId;
  if (!resultId) return null;
  return invoke<WireEnvelope>("get_batch_report", {
    request: { resultId },
  });
};

const populateRawJson = async (): Promise<void> => {
  if (rawJson.textContent || rawJson.dataset.loading === "true") return;
  const revision = rawRequestRevision;
  rawJson.dataset.loading = "true";
  rawJson.textContent = t("status.loadingDetails");
  try {
    const envelope = await fullResultEnvelope();
    if (revision === rawRequestRevision) {
      rawJson.textContent = envelope
        ? JSON.stringify(envelope, null, 2)
        : t("status.detailUnavailable");
    }
  } catch (error) {
    if (revision === rawRequestRevision) {
      rawJson.textContent = describeInvokeError(error);
    }
  } finally {
    if (revision === rawRequestRevision) delete rawJson.dataset.loading;
  }
};

const envelopeEntries = (envelope: WireEnvelope): DisplayEntry[] => {
  if (envelope.kind === "analysis") {
    return [
      {
        key: 0,
        displayPath: envelope.data.source.displayPath,
        summary: null,
        report: envelope.data,
        error: null,
        expanded: true,
      },
    ];
  }
  if (envelope.kind === "error") {
    return [
      {
        key: 0,
        displayPath: envelope.data.displayPath ?? selectedInputs[0] ?? "MacinMeter",
        summary: null,
        report: null,
        error: envelope.data,
        expanded: true,
      },
    ];
  }
  return envelope.data.items.map(batchDisplayEntry);
};

const formatAllMarkdown = (envelope: WireEnvelope): string => {
  const entries = hideFailed
    ? envelopeEntries(envelope).filter((entry) => entry.report !== null)
    : envelopeEntries(envelope);
  const header = `# ${t("md.title")}\n\nMacinMeter ${envelope.toolVersion}\n\n`;
  return header + entries.map(formatEntryMarkdown).join("\n");
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
  const separator = dataUrl.indexOf(",");
  if (separator < 0) throw new Error("image encoder returned an invalid data URL");
  const encoded = dataUrl.slice(separator + 1);
  const binary = atob(encoded);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
};

const dataUrlText = (dataUrl: string): string => {
  const separator = dataUrl.indexOf(",");
  if (separator < 0) throw new Error("image encoder returned an invalid data URL");
  const metadata = dataUrl.slice(0, separator);
  if (metadata.endsWith(";base64")) {
    return new TextDecoder().decode(dataUrlBytes(dataUrl));
  }
  return decodeURIComponent(dataUrl.slice(separator + 1));
};

const writeBytesInChunks = async (
  path: string,
  bytes: Uint8Array,
): Promise<void> => {
  const file = await create(path);
  try {
    for (const range of byteRanges(bytes.byteLength)) {
      const chunk = bytes.subarray(range.start, range.end);
      const written = await file.write(chunk);
      if (written !== chunk.byteLength) {
        throw new Error(
          `file write stopped after ${written} of ${chunk.byteLength} bytes`,
        );
      }
    }
  } finally {
    await file.close();
  }
};

const nextPaint = (): Promise<void> =>
  new Promise((resolve) => window.requestAnimationFrame(() => resolve()));

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
  const scale = safeImageScale(bounds.width, bounds.height);
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

const prepareImageClone = (clone: HTMLElement): void => {
  clone.removeAttribute("id");
  clone.removeAttribute("inert");
  clone.classList.add("exporting");
  clone.classList.remove("search-hit");
  clone.querySelectorAll<HTMLElement>("[id]").forEach((element) => {
    element.removeAttribute("id");
  });
  clone.querySelectorAll(".search-hit").forEach((element) => {
    element.classList.remove("search-hit");
  });
};

const withMountedImageClone = async <T>(
  sourceWidth: number,
  clone: HTMLElement,
  capture: (frame: HTMLElement) => Promise<T>,
): Promise<T> => {
  const padding = 24;
  const frame = document.createElement("div");
  frame.className = "image-capture-frame";
  frame.style.width = `${Math.ceil(sourceWidth) + padding * 2}px`;
  frame.setAttribute("aria-hidden", "true");
  prepareImageClone(clone);
  frame.appendChild(clone);
  document.body.appendChild(frame);

  try {
    return await capture(frame);
  } finally {
    frame.remove();
  }
};

const withImageCaptureFrame = async <T>(
  target: HTMLElement,
  capture: (frame: HTMLElement) => Promise<T>,
): Promise<T> =>
  withMountedImageClone(
    target.getBoundingClientRect().width,
    target.cloneNode(true) as HTMLElement,
    capture,
  );

const withImageCapturePage = async <T>(
  container: HTMLElement,
  entries: readonly HTMLElement[],
  capture: (frame: HTMLElement) => Promise<T>,
): Promise<T> => {
  const clone = container.cloneNode(false) as HTMLElement;
  entries.forEach((entry) => clone.appendChild(entry.cloneNode(true)));
  return withMountedImageClone(
    container.getBoundingClientRect().width,
    clone,
    capture,
  );
};

const imageCapturePages = (
  container: HTMLElement,
  maxContentHeight: number,
): HTMLElement[][] => {
  const entries = Array.from(container.children).filter(
    (child): child is HTMLElement => child instanceof HTMLElement,
  );
  const gap = Number.parseFloat(window.getComputedStyle(container).rowGap);
  return paginateItemIndexes(
    entries.map((entry) => entry.getBoundingClientRect().height),
    gap,
    maxContentHeight,
  ).map((page) => page.map((index) => entries[index]));
};

const pngBytes = async (frame: HTMLElement): Promise<Uint8Array> => {
  const blob = await toBlob(frame, imageOptions(frame));
  if (!blob) throw new Error("PNG encoder returned no data");
  return new Uint8Array(await blob.arrayBuffer());
};

const renderPng = (target: HTMLElement): Promise<Uint8Array> =>
  withImageCaptureFrame(target, pngBytes);

const svgText = async (frame: HTMLElement): Promise<string> =>
  dataUrlText(
    await toSvg(frame, {
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

const renderPngPage = (
  container: HTMLElement,
  entries: readonly HTMLElement[],
): Promise<Uint8Array> => withImageCapturePage(container, entries, pngBytes);

const renderSvgPage = (
  container: HTMLElement,
  entries: readonly HTMLElement[],
): Promise<string> => withImageCapturePage(container, entries, svgText);

const currentToolVersion = (): string =>
  lastEnvelope?.toolVersion ?? lastBatchResult?.toolVersion ?? packageMetadata.version;

const materializeBatchDetails = (envelope: WireEnvelope): void => {
  if (!lastBatchResult || envelope.kind !== "batch") return;
  envelope.data.items.forEach((item, index) => {
    loadedBatchItems.set(index, item);
    if (item.outcome.status === "success") expandedBatchItems.add(index);
  });
  renderResults();
};

const exportImage = async (): Promise<void> => {
  if (
    (!lastEnvelope && !lastBatchResult) ||
    resultsElement.children.length === 0 ||
    exportInProgress
  ) {
    return;
  }
  const previousStatus = statusState;
  const statusWasHidden = statusElement.hidden;
  exportInProgress = true;
  updateControls();
  try {
    status("status.preparingExport", {}, { progress: 0 });
    await nextPaint();
    const format = await showFormatDialog();
    if (!format) {
      restoreStatus(previousStatus, statusWasHidden);
      return;
    }
    const envelope = await fullResultEnvelope();
    if (!envelope) throw new Error("result is no longer available");
    materializeBatchDetails(envelope);
    await nextPaint();
    const name = `MacinMeter_v${currentToolVersion()}_${fileTimestamp()}.${format}`;
    const pages = imageCapturePages(
      resultsElement,
      format === "png"
        ? PNG_PAGE_MAX_CONTENT_HEIGHT
        : SVG_PAGE_MAX_CONTENT_HEIGHT,
    );
    if (pages.length === 0) {
      restoreStatus(previousStatus, statusWasHidden);
      return;
    }
    let outputPaths: string[];
    if (pages.length === 1) {
      const path = await save({
        defaultPath: name,
        filters: [
          {
            name: format === "png" ? "PNG Image" : "SVG Image",
            extensions: [format],
          },
        ],
      });
      if (!path) {
        restoreStatus(previousStatus, statusWasHidden);
        return;
      }
      outputPaths = [path];
    } else {
      const directory = await open({
        title: t("dialog.exportFolder"),
        directory: true,
        multiple: false,
        recursive: false,
      });
      if (!directory || Array.isArray(directory)) {
        restoreStatus(previousStatus, statusWasHidden);
        return;
      }
      outputPaths = await Promise.all(
        pages.map((_, index) =>
          join(directory, numberedExportPath(name, index, pages.length)),
        ),
      );
    }
    for (let index = 0; index < pages.length; index += 1) {
      status(
        "status.exportingImage",
        { current: index + 1, total: pages.length },
        { progress: (index / pages.length) * 100 },
      );
      await nextPaint();
      const bytes =
        format === "png"
          ? await renderPngPage(resultsElement, pages[index])
          : new TextEncoder().encode(
              await renderSvgPage(resultsElement, pages[index]),
            );
      await writeBytesInChunks(outputPaths[index], bytes);
    }
    status(
      pages.length === 1 ? "status.exported" : "status.exportedMany",
      { name, count: pages.length },
      { progress: 100 },
    );
  } catch (error) {
    status(
      "status.exportFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    exportInProgress = false;
    updateControls();
  }
};

const exportJson = async (): Promise<void> => {
  if ((!lastEnvelope && !lastBatchResult) || exportInProgress) return;
  const previousStatus = statusState;
  const statusWasHidden = statusElement.hidden;
  exportInProgress = true;
  updateControls();
  try {
    status("status.preparingExport", {}, { progress: 0 });
    await nextPaint();
    const name = `MacinMeter_v${currentToolVersion()}_${fileTimestamp()}.json`;
    const path = await save({
      defaultPath: name,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) {
      restoreStatus(previousStatus, statusWasHidden);
      return;
    }

    status("status.exportingJson", {}, { progress: 0 });
    await nextPaint();
    const envelope = await fullResultEnvelope();
    if (!envelope) throw new Error("result is no longer available");
    const bytes = new TextEncoder().encode(JSON.stringify(envelope, null, 2));
    await writeBytesInChunks(path, bytes);
    status("status.exported", { name }, { progress: 100 });
  } catch (error) {
    status(
      "status.exportFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    exportInProgress = false;
    updateControls();
  }
};

const copyPngToClipboard = async (
  target: HTMLElement,
  button: HTMLButtonElement,
): Promise<void> => {
  const original = button.textContent;
  button.disabled = true;
  try {
    await writeImage(await renderPng(target));
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
  if (cancellingJobId === jobId) return;
  cancellingJobId = jobId;
  updateControls();
  status("status.cancelling");
  try {
    await invoke<boolean>("cancel_job", { jobId });
  } catch (error) {
    if (cancellingJobId === jobId) cancellingJobId = null;
    updateControls();
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
  const onEvent = new Channel<FrontendMessage>();
  onEvent.onmessage = (message) => handleFrontendMessage(jobId, message);
  try {
    if (selectionKind === "files" && selectedInputs.length === 1) {
      const envelope = await invoke<WireEnvelope>("run_analysis", {
        request: { jobId, path: selectedInputs[0], timing: showTiming },
        onEvent,
      });
      await renderEnvelope(envelope, performance.now() - startedAt);
    } else {
      const response = await invoke<BatchRunResponse>("run_batch", {
        request: {
          jobId,
          inputs: selectedInputs,
          recursive,
          timing: showTiming,
        },
        onEvent,
      });
      if (response.outcome === "error") {
        await renderEnvelope(response.envelope, performance.now() - startedAt);
      } else {
        try {
          await renderCompletedBatch(response, performance.now() - startedAt);
        } catch (error) {
          void invoke<boolean>("release_batch_results", {
            request: { resultId: response.resultId },
          });
          throw error;
        }
      }
    }
  } catch (error) {
    clearResults();
    status(
      "status.discoveryFailed",
      { message: describeInvokeError(error) },
      { error: true },
    );
  } finally {
    if (activeJobId === jobId) activeJobId = null;
    if (cancellingJobId === jobId) cancellingJobId = null;
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

hideWarningsButton.addEventListener("click", () => {
  hideWarnings = !hideWarnings;
  hideWarningsButton.classList.toggle("active", hideWarnings);
  renderResults();
});

hideFailedButton.addEventListener("click", () => {
  hideFailed = !hideFailed;
  hideFailedButton.classList.toggle("active", hideFailed);
  // The batch summary keeps reporting the real failure count, so hiding the
  // entries cannot make a partly failed run look like a clean one.
  renderResults();
});

showTimingButton.addEventListener("click", () => {
  showTiming = !showTiming;
  showTimingButton.classList.toggle("active", showTiming);
  if (!showTiming) {
    lastPhaseTimings = null;
    renderPhaseTimings();
  }
});

copyMarkdownButton.addEventListener("click", () => {
  void (async () => {
    try {
      const envelope = await fullResultEnvelope();
      if (envelope) {
        await copyText(formatAllMarkdown(envelope), copyMarkdownButton);
      }
    } catch (error) {
      status(
        "status.copyFailed",
        { message: describeInvokeError(error) },
        { error: true },
      );
    }
  })();
});

exportJsonButton.addEventListener("click", () => void exportJson());

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
rawPanel.addEventListener("toggle", () => {
  if (rawPanel.open) void populateRawJson();
});

const setLanguage = (language: SupportedLanguage): void => {
  changeLanguage(language);
  updateStaticTexts();
  updateLanguageButtons();
  document.title = t("title");
  renderSelection();
  renderStatus();
  renderPhaseTimings();
  if ((lastEnvelope || lastBatchResult) && activeJobId === null) renderResults();
};

element<HTMLButtonElement>("lang-zh").addEventListener("click", () =>
  setLanguage("zh-CN"),
);
element<HTMLButtonElement>("lang-en").addEventListener("click", () =>
  setLanguage("en-US"),
);

// The timing event arrives before the command resolves, so the status line the
// envelope produces already has the split available.
void listen<JobTiming>("analysis-timing", ({ payload }) => {
  if (payload.jobId !== activeJobId) return;
  lastPhaseTimings = payload;
});

const handleAnalysisEvent = (jobId: string, event: AnalysisEvent): void => {
  if (jobId !== activeJobId || cancellingJobId === jobId) return;
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
};

const handleFrontendMessage = (
  jobId: string,
  message: FrontendMessage,
): void => {
  if (message.type === "event") {
    handleAnalysisEvent(jobId, message.event);
    return;
  }
  if (jobId !== activeJobId || cancellingJobId === jobId) return;
  for (const { index, item } of message.items) {
    queueStreamedBatchItem(index, item);
  }
};

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

window.addEventListener("beforeunload", releaseRetainedBatch);

appVersion.textContent = `v${packageMetadata.version}`;
setLanguage(getCurrentLanguage());
renderSelection();
status("status.ready");
void loadCapabilities();
