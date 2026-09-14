# R1 - 14B Q4_K_M split Midgaard + Odin (RPC)

- date: 2026-09-09T17:15:00-07:00
- outcome: pass
- model: Qwen_Qwen3-14B-Q4_K_M.gguf
- placement: Midgaard host + Odin ~4 GB
- tensor_split: 4/8
- pp512: 165.2
- tg128: 18.7
- filed_by: backfill from HALO-TEST-01-LAN-SPLIT.md

## Summary

Pure RPC-overhead test on a model that did not need splitting. The split ran faster than the same host alone: +14% decode (18.7 vs 16.4) and +15% prefill (165.2 vs 143.9). A sub-millisecond wired hop costs less than halving each GPU's weight traffic saves. Prediction of a 20-40% decode loss was wrong.

## Notes

llama.cpp tag: b10883. Wired LAN: 935-939 Mbit/s, 0.3-0.7 ms RTT. Split recorded as "~4 GB on Odin"; exact tensor-split string not preserved in the source doc.
