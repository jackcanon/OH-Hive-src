# Shard-size ladder - 14B Q4 Overgaard + Odin

- date: 2026-09-09T20:20:00-07:00
- outcome: pass
- model: Qwen_Qwen3-14B-Q4_K_M.gguf
- placement: Overgaard host + Odin 0.4 / 2.4 / 4.4 / 6.4 GB
- tensor_split: 0.4/8 · 2.4/6 · 4.4/4 · 6.4/2
- pp512: 
- tg128: 31.4
- filed_by: backfill from HALO-TEST-01-LAN-SPLIT.md

## Summary

Stepped Odin's share to find where a Metal worker fails. All four 14B rungs passed; Odin's wired-memory delta matched its share to the tenth of a GB, proving the device-order inversion. With 95% of the model on Odin the split ran at 31.4 tok/s - faster than Odin alone (26.8): the host handling sampling/output while the worker does layers is a real win on a 2.5G LAN. Worker overhead was 1.0-1.2x its shard; the ceiling is the machine's, not the protocol's.

## Results

| test | tok/s | ± |
|---|---:|---:|
| tg128 (Odin 0.4 GB) | 31.4 | |
| tg128 (Odin 2.4 GB) | 26.0 | |
| tg128 (Odin 4.4 GB) | 20.6 | |
| tg128 (Odin 6.4 GB) | 17.7 | |

## Notes

llama.cpp tag: b10883. Header tg128 is the best rung. Raw: halo-test-01-results/ladder-odin-worker.log, ladder-odin-worker.sh.
