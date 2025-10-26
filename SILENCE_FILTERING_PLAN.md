# Silence Filtering — Plan and Roadmap

Status: active (experimental feature shipped); Default: disabled (foobar2000‑compatible)

## Background

DR computation in this project follows foobar2000’s 3‑second window + top 20% RMS method. Real‑world files may contain encoder padding/silence (head/tail), especially lossy AAC/MP3/Opus. Those low‑energy windows can slightly bias the 20% selection (by inflating the denominator), yielding small DR uplifts (typically +0.01…+0.10 dB).

## Current State (shipped)

- Implementation scope
  - Window‑level filtering only; PCM samples are untouched.
  - Filtering is applied before top‑20% selection: windows whose RMS (dBFS) is below a threshold are excluded from the candidate set.
- Public API / CLI
  - CLI: `--filter-silence[=<DB>]` (default −70 dBFS if value omitted). Example:
    - Single file: `./MacinMeter… --filter-silence= -70 audio.wav`
    - Directory: `./MacinMeter… --filter-silence -- /path/to/dir` (note the `--`)
  - AppConfig: `silence_filter_threshold_db: Option<f64>` (present ⇒ enabled).
- Diagnostics (printed when enabled)
  - Per‑channel: filtered/total windows and percentage.
- Defaults / Compatibility
  - Default is OFF (100% foobar2000 behavior). Enabling is experimental.
- Observed effect (formatTEST)
  - Typical change: −0.01…−0.04 dB Precise DR vs default; Official DR unchanged.
  - After external trimming (e.g. ffmpeg silenceremove), our filter often reports 0 filtered windows and results match the non‑filtered path (as expected).
- Known limitations
  - Window‑level only: does not shorten the stream nor change sample counts; does not realign window phase between formats.
  - AAC/ADTS lacks container delay/padding metadata; filtering cannot fully remove AAC vs WAV micro‑differences.

## Rationale and Design Notes

- Why window filtering? Low risk, small code surface, transparent diagnostics, preserves core algorithm structure and foobar2000 comparability when disabled.
- Why −70 dBFS default? Conservative threshold that avoids accidental removal of very quiet musical passages; parameterizable.

## Roadmap (phased enhancements)

The plan below evolves precision from “window‑level robust” → “sample‑level precise” while keeping defaults safe.

### P0 — Edge‑only sample‑level trimming (experimental)

- Goal: remove head/tail silent runs at sample/frame granularity; keep mid‑track silence (artistic intent) intact.
- Method: streaming state machine over interleaved PCM frames (per‑frame amplitude = max(|L|,|R|))
  - Enter “trim mode” when below threshold for ≥ min_run frames; switch to “pass mode” after ≥ hysteresis frames above threshold.
  - Tail: ring buffer (size = min_run) holds recent frames; if sustained silence is detected, drop buffered tail; else flush.
- Config (new): `--trim-edges[=<DB>]` (default off), `--trim-min-run-ms` (e.g. 200–500 ms), channel aggregation = max.
- Complexity: O(N) time, O(min_run) memory; isolated to processing layer.
- Acceptance:
  - On formatTEST, enable trimming: filtered edge duration reported; Precise DR changes ≤ |0.00…0.05| dB and direction consistent with removing padding.
  - On silence‑heavy synthetic files, trimming removes head/tail but preserves mid‑track rests.

### P1 — Container‑aware precise alignment (lossless gapless)

- MP4/ALAC: read iTunSMPB / edts (encoder delay, padding) and crop samples accordingly.
- Opus: apply pre‑skip from OggOpusHead.
- FLAC/WAV: typically none; keep as is.
- Effect: sample‑accurate start/end; best‑effort gapless parity.
- Acceptance: DR delta vs WAV baseline shrinks to ≤ 0.01–0.02 dB for ALAC/Opus conversions.

### P2 — ADTS/AAC alignment without metadata (auto‑align)

- Use cross‑correlation (time‑domain; FFT if needed) between source and decoded AAC to estimate start offset; crop to equal length.
- For batch mode without source WAV: heuristics (e.g., 1024‑sample encoder delay + frame remainder padding) to approximate trimming.
- Acceptance: on paired WAV/AAC, equal‑length aligned DR delta ≤ 0.01–0.02 dB on average.

### P3 — Robust DR (optional dual report)

- Compute and optionally display “Robust DR” (top‑20% computed over non‑silent windows only).
- Show both “Official DR” (unaltered) and “Robust DR” when enabled; default OFF.
- Acceptance: clear labeling in output; no defaults changed.

### P4 — Diagnostics & Developer Tooling

- Optional dump of: selected top‑20% window indices, per‑window RMS dB, noise‑floor estimate (bottom percentile), effective threshold, counts before/after filtering.
- Verbose gating and `--debug` feature flag to avoid noisy production logs.

### P5 — Test & Bench Coverage

- Unit/property tests for:
  - Window filtering: threshold, hysteresis, multi‑channel behavior, boundary cases (remainder windows).
  - Edge trimming state machine: false‑positive protection (classical pianissimo), head/tail only.
  - Container‑aware alignment (fixture‑based): ALAC/Opus.
  - Auto‑align (paired WAV/AAC): tolerance assertions on offset & DR.
- Integration benches (ignored by default) to assert regression envelopes on DR deltas and runtime.

## Compatibility & Defaults

- Defaults remain foobar2000‑compatible (no filtering, no trimming, no alignment) unless explicitly enabled.
- All experimental options must:
  - Be OFF by default.
  - Print clear diagnostics when enabled.
  - Never relax error handling or change DR formula.

## Risks and Mitigations

- Risk: misclassifying quiet passages (classical) as silence.
  - Mitigate with conservative defaults (−70…−80 dBFS), min‑run duration, hysteresis, and edge‑only trimming.
- Risk: container metadata absent/incorrect.
  - Provide best‑effort auto‑align with explicit “estimated” labeling; do not overwrite Official DR.
- Risk: performance overhead in auto‑align.
  - Restrict correlation to a bounded search window; prefer P0/P1 in default workflows.

## Implementation Pointers

- Current code paths
  - CLI: `src/tools/cli.rs` (flag `--filter-silence[=<DB>]` → AppConfig.silence_filter_threshold_db)
  - Processing: `src/tools/processor.rs` (threshold routed to WindowRmsAnalyzer)
  - Core analyzer: `src/core/histogram.rs` (window‑level filter + per‑channel stats)
- Docs & examples
  - README “命令行选项”已包含 `--filter-silence` 使用示例
  - formatTEST summary: `docs/formatTEST_media_summary.md`（含默认/过滤对照表）

## Decision Log

- 2025‑10‑26: Shipped window‑level silence filtering under `--filter-silence[=<DB>]`, default OFF; simplified CLI; documented diagnostics and directory `--` usage.
- Next: implement P0 (edge‑only sample‑level trimming) behind experimental flag; then P1 (container‑aware alignment) for ALAC/Opus.

