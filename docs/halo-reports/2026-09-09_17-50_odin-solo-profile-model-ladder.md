# Odin solo profile - model ladder

- date: 2026-09-09T17:50:00-07:00
- outcome: pass
- model: 8B / 12B / 14B / 24B Q4 ladder
- placement: Odin alone (MacBook Pro M4 Pro 24 GB, macOS 26.6.2, on AC)
- tensor_split: 
- pp512: 240.2
- tg128: 26.7
- filed_by: backfill from HALO-TEST-01-LAN-SPLIT.md

## Summary

Decode tracks file size almost exactly: ~220 GB/s effective on every rung (memory-bandwidth-bound). One number per chip predicts any model's decode from its size alone. No thermal fade over 1024-token generations (within 1-3% of tg128, CPU_Speed_Limit 100 throughout, battery 30.7 C). Real memory ceiling is ~18 GB and fails late: the 21.7 GB qwen3.6 allocated fine and died at first compute (res = -3).

## Results

| test | tok/s | ± |
|---|---:|---:|
| llama3.1 8B Q4_K_M pp512 | 452.8 | |
| llama3.1 8B Q4_K_M tg128 | 48.3 | |
| gemma4 12B Q4_0 pp512 | 296.1 | |
| gemma4 12B Q4_0 tg128 | 30.3 | |
| qwen3 14B Q4_K_M pp512 | 240.2 | |
| qwen3 14B Q4_K_M tg128 | 26.7 | |
| qwen3 14B tg1024 | 25.2 | |
| mistral-small 24B Q4_K_M pp512 | 144.7 | |
| mistral-small 24B Q4_K_M tg128 | 16.9 | |
| mistral-small 24B tg1024 | 16.7 | |

## Notes

llama.cpp tag: b10883 (native build on Odin). Header numbers are the 14B rung. Raw: halo-test-01-results/odin-solo-profile.log, odin-solo-telemetry.csv. mediaanalysisd was at 245% CPU before the run and was killed.
