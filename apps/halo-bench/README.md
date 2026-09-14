# HaloBench

Native macOS test bench for **Project Halo** (ADR-012 decision 17): runs llama.cpp RPC split
benchmarks across Loki's Lab, shows live status and logs, and files a human-readable report for
every run.

## Build & run

```bash
cd apps/halo-bench
swift run                 # dev: launches the window
scripts/build-app.sh      # makes HaloBench.app with an icon (ad-hoc signed)
open HaloBench.app
```

macOS 27, Apple Silicon. No external dependencies. Run it from the app bundle (not `nohup` over
SSH) — macOS Local Network privacy blocks LAN connections from detached processes.

## What it does

- **Dashboard** — pass/fail counts, best decode, a decode-tok/s chart over every filed run, the
  next test up (loaded from Test 01b's plan), recent reports.
- **Run** — pick a preset or configure a run: model, workers in `--rpc` order with a GB share
  each, local share. The app emits `--tensor-split` **RPC-first, local-last** (the order llama.cpp
  actually uses — the inversion that cost Test 01 its first night can't happen here). Restarts
  every worker over SSH before launch, waits for its port, runs `llama-bench` locally with live
  output, parses `pp512`/`tg128`.
- **History** — every report, newest first, with the writeup. Reads straight from the reports
  folder; edit a file there and hit Reload.
- **Fleet** — hosts, wired IPs, SSH user, measured usable GPU memory, `ggml-rpc-server` path.
  "Probe RPC ports" checks who's listening.
- **Logs** — the current run's output, copy/save.
- **Import a log** — file a report from a `llama-bench` run done by hand.

## Reports

Filed into `docs/halo-reports/` (configurable in Settings) as
`YYYY-MM-DD_HH-mm_<title-slug>.md` + `.log`. The markdown has a small header the app parses back:

```
# <title>

- date: 2026-09-10T09:14:00-07:00
- outcome: pass | fail | partial | aborted
- model: Qwen_Qwen3-14B-Q8_0.gguf
- placement: Overgaard host + Odin 8 GB
- tensor_split: 8/7.7
- pp512: 210.4
- tg128: 24.1

## Summary
...
```

The folder is the source of truth; the app is a mirror of it. Reports written by hand in the
same shape show up in History too (that's how Test 01 was backfilled).

## Config

`~/Library/Application Support/HaloBench/config.json` — fleet, paths, reports folder, which
fleet entry is this Mac.
