export type ErrorCode =
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

export type AnalysisStage =
  | "validation"
  | "discovery"
  | "probe"
  | "decode"
  | "analysis"
  | "output"
  | "cancellation"
  | "internal";

export type PublicError = {
  code: ErrorCode;
  stage: AnalysisStage;
  message: string;
  displayPath: string | null;
  backend: string | null;
  recoverable: boolean;
  details: string | null;
};

export type ChannelRole =
  | "front_left"
  | "front_right"
  | "front_center"
  | "lfe"
  | "back_left"
  | "back_right"
  | "side_left"
  | "side_right"
  | "other";

export type ChannelLayout =
  | { status: "unknown" }
  | { status: "known_no_lfe" }
  | { status: "known"; positions: ChannelRole[] };

export type StreamSpec = {
  sampleRate: number;
  channels: number;
  channelLayout: ChannelLayout;
};

export type DecodeProgress = {
  decodedFrames: number;
  expectedFrames: number | null;
  fraction: number | null;
  eof: boolean;
};

export type Measurement = {
  drDb: number;
  roundedDr: number;
  loudWindowRms: number;
  drSelectedPeak: number;
  drPrimaryPeak: number;
  drSecondaryPeak: number | null;
  validWindows: number;
  frames: number;
};

export type ChannelReportMetrics = {
  overallRmsLinear: number;
  overallRmsDbfs: number | null;
  primaryPeakLinear: number;
};

export type ChannelOutcome =
  | { status: "measured"; measurement: Measurement }
  | { status: "silent"; frames: number; validWindows: number }
  | { status: "insufficient_data"; frames: number };

export type ChannelResult = {
  channelIndex: number;
  outcome: ChannelOutcome;
  report: ChannelReportMetrics;
};

export type TrackAggregate = {
  drDb: number | null;
  roundedDr: number | null;
  contributingChannels: number[];
  excludedChannels: {
    channelIndex: number;
    reason: "insufficient_data";
  }[];
};

export type TrackReportMetrics = {
  overallRmsLinear: number;
  overallRmsDbfs: number | null;
  primaryPeakLinear: number;
  primaryPeakDbfs: number | null;
  duration: {
    decodedFrames: number;
    sampleRate: number;
  };
};

export type AnalysisReport = {
  source: {
    displayPath: string;
    container: string;
    codec: string;
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
    report: TrackReportMetrics;
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

export type BatchItem = {
  displayPath: string;
  outcome:
    | { status: "success"; report: AnalysisReport }
    | { status: "failure"; error: PublicError };
};

export type BatchReport = {
  status: "succeeded" | "partially_succeeded" | "failed";
  items: BatchItem[];
  summary: { total: number; succeeded: number; failed: number };
};

export type WireEnvelope =
  | {
      schemaVersion: 4;
      toolVersion: string;
      kind: "analysis";
      data: AnalysisReport;
    }
  | {
      schemaVersion: 4;
      toolVersion: string;
      kind: "batch";
      data: BatchReport;
    }
  | {
      schemaVersion: 4;
      toolVersion: string;
      kind: "error";
      data: PublicError;
    };

export type AnalysisEvent =
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

export type JobEvent = {
  jobId: string;
  event: AnalysisEvent;
};

export type DiscoveryResponse = {
  files: string[];
};

export type CapabilityRoute = {
  container: string;
  codec: string;
  status: string;
  backend: string;
  discoveryExtensions: string[];
  limitations: string[];
};

export type CapabilitySnapshot = {
  routes: CapabilityRoute[];
  stableDiscoveryExtensions: string[];
};
