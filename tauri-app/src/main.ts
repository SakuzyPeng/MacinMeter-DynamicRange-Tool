import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import packageMetadata from "../package.json";

type ErrorCode =
  | "invalid_request"
  | "input_not_found"
  | "permission_denied"
  | "no_inputs"
  | "unsupported_format"
  | "malformed_media"
  | "decode_failed"
  | "analysis_failed"
  | "resource_exhausted"
  | "output_failed"
  | "cancelled"
  | "internal";

type AnalysisStage =
  | "validation"
  | "discovery"
  | "probe"
  | "decode"
  | "analysis"
  | "output"
  | "cancellation"
  | "internal";

type PublicError = {
  code: ErrorCode;
  stage: AnalysisStage;
  message: string;
  displayPath: string | null;
  backend: string | null;
  recoverable: boolean;
  details: string | null;
};

type ChannelRole =
  | "front_left"
  | "front_right"
  | "front_center"
  | "lfe"
  | "back_left"
  | "back_right"
  | "side_left"
  | "side_right"
  | "other";

type ChannelLayout =
  | { status: "unknown" }
  | { status: "known_no_lfe" }
  | { status: "known"; positions: ChannelRole[] };

type StreamSpec = {
  sampleRate: number;
  channels: number;
  channelLayout: ChannelLayout;
};

type DecodeProgress = {
  decodedFrames: number;
  expectedFrames: number | null;
  fraction: number | null;
  eof: boolean;
};

type Measurement = {
  drDb: number;
  roundedDr: number;
  loudWindowRms: number;
  selectedPeak: number;
  primaryPeak: number;
  secondaryPeak: number | null;
  validWindows: number;
  frames: number;
};

type ChannelOutcome =
  | { status: "measured"; measurement: Measurement }
  | { status: "silent"; frames: number; validWindows: number }
  | { status: "insufficient_data"; frames: number };

type ChannelResult = {
  channelIndex: number;
  outcome: ChannelOutcome;
};

type TrackAggregate = {
  drDb: number | null;
  roundedDr: number | null;
  contributingChannels: number[];
  excludedChannels: {
    channelIndex: number;
    reason: "insufficient_data";
  }[];
};

type AnalysisReport = {
  source: {
    displayPath: string;
    container: "wave" | "flac" | "aiff";
    codec: "pcm_integer" | "pcm_float" | "flac";
    sampleRate: number;
    channels: number;
    bitsPerSample: number | null;
    expectedFrames: number | null;
  };
  pcm: {
    spec: StreamSpec;
    expectedFrames: number | null;
  };
  analysis: {
    algorithm: {
      profile: "foo_dr_meter_1_0_8_candidate_v1";
      profileVersion: number;
      compatibility: "unverified";
      parameters: {
        windowDurationCoefficient: number;
        rmsSumMultiplier: number;
        histogramBins: number;
        rmsHistogramMinDb: number;
        rmsHistogramMaxDb: number;
        histogramBinWidthDb: number;
        peakKeyBinWidthDb: number;
        loudFraction: number;
        minimumTailFrames: number;
        includeEntireBoundaryBin: boolean;
        exactWindowVirtualZeroPeak: boolean;
        drFloorDb: number;
        silentChannelDrDb: number;
        includesLfeInTrackAggregate: boolean;
        resultPrecisionBits: number;
      };
    };
    stream: StreamSpec;
    framesSeen: number;
    channels: ChannelResult[];
    aggregates: {
      track: TrackAggregate;
    };
  };
  diagnostics: {
    backend: string;
    decodedFrames: number;
    warnings: string[];
  };
};

type BatchItem = {
  displayPath: string;
  outcome:
    | { status: "success"; report: AnalysisReport }
    | { status: "failure"; error: PublicError };
};

type BatchReport = {
  status: "succeeded" | "partially_succeeded" | "failed";
  items: BatchItem[];
  summary: { total: number; succeeded: number; failed: number };
};

type WireEnvelope =
  | {
      schemaVersion: 2;
      toolVersion: string;
      kind: "analysis";
      data: AnalysisReport;
    }
  | {
      schemaVersion: 2;
      toolVersion: string;
      kind: "batch";
      data: BatchReport;
    }
  | {
      schemaVersion: 2;
      toolVersion: string;
      kind: "error";
      data: PublicError;
    };

type AnalysisEvent =
  | { type: "discovery_started" }
  | { type: "discovery_finished"; files: number }
  | { type: "file_started"; index: number; displayPath: string }
  | {
      type: "decode_progress";
      index: number;
      displayPath: string;
      progress: DecodeProgress;
    }
  | {
      type: "file_finished";
      index: number;
      displayPath: string;
      success: boolean;
    }
  | { type: "batch_finished"; succeeded: number; failed: number };

type JobEvent = {
  jobId: string;
  event: AnalysisEvent;
};

type DiscoveryResponse = {
  files: string[];
};

type SelectionKind = "files" | "folder";

const element = <T extends HTMLElement>(id: string): T => {
  const found = document.getElementById(id);
  if (!found) throw new Error(`missing element #${id}`);
  return found as T;
};

const chooseFilesButton = element<HTMLButtonElement>("choose-files");
const chooseFolderButton = element<HTMLButtonElement>("choose-folder");
const clearButton = element<HTMLButtonElement>("clear-inputs");
const analyzeButton = element<HTMLButtonElement>("analyze");
const cancelButton = element<HTMLButtonElement>("cancel");
const copyButton = element<HTMLButtonElement>("copy-json");
const recursiveInput = element<HTMLInputElement>("recursive");
const selectionElement = element<HTMLDivElement>("selection");
const statusElement = element<HTMLDivElement>("run-status");
const resultsElement = element<HTMLDivElement>("results");
const rawPanel = element<HTMLDetailsElement>("raw-panel");
const rawJson = element<HTMLPreElement>("raw-json");
const versionLabel = element<HTMLSpanElement>("version-label");

let selectedInputs: string[] = [];
let selectionKind: SelectionKind = "files";
let activeJobId: string | null = null;
let previewJobId: string | null = null;
let lastEnvelope: WireEnvelope | null = null;
let selectionRevision = 0;
// This only narrows the native file picker. Rust discovery and content probing
// remain authoritative.
const discoveryFilterExtensions = ["wav", "wave", "flac", "aif", "aiff"];

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

const setRunning = (running: boolean): void => {
  analyzeButton.disabled = running || selectedInputs.length === 0;
  cancelButton.disabled = !running;
  chooseFilesButton.disabled = running;
  chooseFolderButton.disabled = running;
  clearButton.disabled = running;
  recursiveInput.disabled = running || selectionKind !== "folder";
};

const updateSelection = (): void => {
  analyzeButton.disabled = selectedInputs.length === 0;
  recursiveInput.disabled = selectionKind !== "folder";
  if (selectedInputs.length === 0) {
    selectionElement.className = "selection empty";
    selectionElement.textContent = "尚未选择输入";
    return;
  }
  selectionElement.className = "selection";
  selectionElement.innerHTML = selectedInputs
    .map(
      (path) =>
        `<div class="selected-item"><strong>${escapeHtml(fileName(path))}</strong><span>${escapeHtml(path)}</span></div>`,
    )
    .join("");
};

const cancelPreview = async (): Promise<void> => {
  const jobId = previewJobId;
  if (!jobId) return;
  previewJobId = null;
  try {
    await invoke<boolean>("cancel_job", { jobId });
  } catch {
    // A completed preview may have already removed its registry entry.
  }
};

const previewDirectory = async (
  directory: string,
  recursive: boolean,
  revision: number,
): Promise<void> => {
  await cancelPreview();
  if (revision !== selectionRevision || activeJobId) return;

  const jobId = crypto.randomUUID();
  previewJobId = jobId;
  statusElement.textContent = recursive
    ? "正在递归发现输入…"
    : "正在发现输入…";
  try {
    const discovery = await invoke<DiscoveryResponse>("discover_inputs", {
      request: { jobId, inputs: [directory], recursive },
    });
    if (
      previewJobId !== jobId ||
      revision !== selectionRevision ||
      activeJobId
    )
      return;
    statusElement.textContent = `已发现 ${discovery.files.length} 个可分析文件`;
  } catch (error) {
    if (
      previewJobId !== jobId ||
      revision !== selectionRevision ||
      activeJobId
    )
      return;
    statusElement.textContent = `发现输入失败：${describeInvokeError(error)}`;
  } finally {
    if (previewJobId === jobId) previewJobId = null;
  }
};

const renderChannel = (channel: ChannelResult): string => {
  const label = `CH ${channel.channelIndex + 1}`;
  if (channel.outcome.status === "measured") {
    const value = channel.outcome.measurement;
    return `<tr>
      <td>${label}</td>
      <td class="dr">DR${value.roundedDr}</td>
      <td>${value.drDb.toFixed(4)} dB</td>
      <td>${value.loudWindowRms.toFixed(8)}</td>
      <td>${value.selectedPeak.toFixed(8)}</td>
    </tr>`;
  }
  if (channel.outcome.status === "silent") {
    return `<tr class="muted-row">
      <td>${label}</td>
      <td class="dr">DR0</td>
      <td colspan="3">Silent · DR0 contribution</td>
    </tr>`;
  }
  return `<tr class="muted-row"><td>${label}</td><td colspan="4">Insufficient data</td></tr>`;
};

const renderAnalysis = (report: AnalysisReport): string => {
  const aggregate = report.analysis.aggregates.track;
  const aggregateHtml =
    aggregate.roundedDr !== null && aggregate.drDb !== null
    ? `<div class="aggregate"><span>Track aggregate</span><strong>DR${aggregate.roundedDr}</strong><em>${aggregate.drDb.toFixed(4)} dB</em></div>`
    : `<div class="aggregate unavailable">Track aggregate unavailable</div>`;
  return `<article class="report">
    <div class="report-header">
      <div><h3>${escapeHtml(fileName(report.source.displayPath))}</h3><p>${escapeHtml(report.source.displayPath)}</p></div>
      <span>${report.pcm.spec.sampleRate.toLocaleString()} Hz · ${report.pcm.spec.channels} ch</span>
    </div>
    ${aggregateHtml}
    <div class="table-wrap"><table>
      <thead><tr><th>Channel</th><th>Rounded</th><th>DR</th><th>Loud-window RMS</th><th>Selected DR peak</th></tr></thead>
      <tbody>${report.analysis.channels.map(renderChannel).join("")}</tbody>
    </table></div>
  </article>`;
};

const renderEnvelope = (envelope: WireEnvelope): void => {
  lastEnvelope = envelope;
  versionLabel.textContent = `v${envelope.toolVersion} · schema ${envelope.schemaVersion}`;
  rawJson.textContent = JSON.stringify(envelope, null, 2);
  rawPanel.hidden = false;
  copyButton.disabled = false;

  if (envelope.kind === "error") {
    const path = envelope.data.displayPath
      ? `<p>${escapeHtml(envelope.data.displayPath)}</p>`
      : "";
    resultsElement.innerHTML = `<div class="error-box"><strong>${escapeHtml(envelope.data.code)}</strong><span>${escapeHtml(envelope.data.message)}</span>${path}</div>`;
    statusElement.textContent =
      envelope.data.code === "cancelled" ? "任务已取消" : "分析失败";
    return;
  }
  if (envelope.kind === "analysis") {
    resultsElement.innerHTML = renderAnalysis(envelope.data);
    statusElement.textContent = "分析完成";
    return;
  }

  resultsElement.innerHTML = envelope.data.items
    .map((item) =>
      item.outcome.status === "success"
        ? renderAnalysis(item.outcome.report)
        : `<div class="error-box"><strong>${escapeHtml(fileName(item.displayPath))}</strong><span>${escapeHtml(item.outcome.error.message)}</span></div>`,
    )
    .join("");
  statusElement.textContent = `批量完成：${envelope.data.summary.succeeded} 成功，${envelope.data.summary.failed} 失败`;
};

chooseFilesButton.addEventListener("click", async () => {
  const result = await open({
    multiple: true,
    directory: false,
    filters: [
      {
        name: "MacinMeter audio",
        extensions: discoveryFilterExtensions,
      },
    ],
  });
  if (!result) return;
  void cancelPreview();
  selectionRevision += 1;
  selectedInputs = Array.isArray(result) ? result : [result];
  selectionKind = "files";
  recursiveInput.checked = false;
  updateSelection();
  statusElement.textContent = `已选择 ${selectedInputs.length} 个文件`;
});

chooseFolderButton.addEventListener("click", async () => {
  const result = await open({ multiple: false, directory: true });
  if (!result || Array.isArray(result)) return;
  selectionRevision += 1;
  const revision = selectionRevision;
  selectedInputs = [result];
  selectionKind = "folder";
  recursiveInput.checked = false;
  updateSelection();
  void previewDirectory(result, false, revision);
});

clearButton.addEventListener("click", () => {
  void cancelPreview();
  selectionRevision += 1;
  selectedInputs = [];
  resultsElement.innerHTML = "";
  rawPanel.hidden = true;
  copyButton.disabled = true;
  lastEnvelope = null;
  updateSelection();
  statusElement.textContent = "等待任务";
});

recursiveInput.addEventListener("change", () => {
  if (selectionKind !== "folder" || selectedInputs.length !== 1) return;
  selectionRevision += 1;
  void previewDirectory(
    selectedInputs[0],
    recursiveInput.checked,
    selectionRevision,
  );
});

analyzeButton.addEventListener("click", async () => {
  if (selectedInputs.length === 0 || activeJobId) return;
  activeJobId = crypto.randomUUID();
  setRunning(true);
  statusElement.textContent = "正在分析…";
  resultsElement.innerHTML = "";
  rawPanel.hidden = true;
  copyButton.disabled = true;

  try {
    await cancelPreview();
    const envelope =
      selectionKind === "files" && selectedInputs.length === 1
        ? await invoke<WireEnvelope>("run_analysis", {
            request: { jobId: activeJobId, path: selectedInputs[0] },
          })
        : await invoke<WireEnvelope>("run_batch", {
            request: {
              jobId: activeJobId,
              inputs: selectedInputs,
              recursive: selectionKind === "folder" && recursiveInput.checked,
            },
          });
    renderEnvelope(envelope);
  } catch (error) {
    statusElement.textContent = "调用失败";
    resultsElement.innerHTML = `<div class="error-box"><span>${escapeHtml(describeInvokeError(error))}</span></div>`;
  } finally {
    activeJobId = null;
    setRunning(false);
  }
});

cancelButton.addEventListener("click", async () => {
  if (!activeJobId) return;
  const jobId = activeJobId;
  statusElement.textContent = "正在取消…";
  try {
    const accepted = await invoke<boolean>("cancel_job", { jobId });
    if (activeJobId === jobId && !accepted) {
      statusElement.textContent = "任务已完成，取消请求未生效";
    }
  } catch (error) {
    if (activeJobId === jobId) {
      statusElement.textContent = `取消失败：${describeInvokeError(error)}`;
    }
  }
});

copyButton.addEventListener("click", async () => {
  if (!lastEnvelope) return;
  await navigator.clipboard.writeText(JSON.stringify(lastEnvelope, null, 2));
  copyButton.textContent = "已复制";
  window.setTimeout(() => {
    copyButton.textContent = "复制 JSON";
  }, 1200);
});

void listen<JobEvent>("analysis-event", ({ payload }) => {
  if (payload.jobId !== activeJobId) return;
  const event = payload.event;
  if (event.type === "file_started") {
    statusElement.textContent = `正在分析 ${fileName(event.displayPath)}`;
  } else if (event.type === "batch_finished") {
    statusElement.textContent = `正在整理结果：${event.succeeded} 成功，${event.failed} 失败`;
  }
}).catch((error: unknown) => {
  statusElement.textContent = `事件监听失败：${describeInvokeError(error)}`;
});

versionLabel.textContent = `v${packageMetadata.version} · foo_dr_meter 1.0.8 Candidate V1 / Unverified`;

updateSelection();
