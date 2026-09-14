# R2 - 32B Q8_0 with corrected split (still fails, res = -3)

- date: 2026-09-09T20:35:00-07:00
- outcome: fail
- model: Qwen_Qwen3-32B-Q8_0.gguf
- placement: Overgaard host 14.8 GB + Odin 12 GB + Heimdall (CUDA) 8 GB
- tensor_split: 12/8/14.8
- filed_by: backfill from HALO-TEST-01-LAN-SPLIT.md

## Summary

With every node far inside its budget and the split RPC-first, the host returned res = -3 at warmup and no worker ever compiled a kernel - compute was rejected host-side before dispatch. The 14B Q4_K_M runs at every split. Leading hypothesis: Q8_0 quantization through the RPC backend (supports_op / scheduler), not memory. This is the one open blocker. Discriminator queued: 14B Q8_0 (same architecture, only the quant differs).

## Notes

llama.cpp tag: b10883. Also observed: Overgaard's usable Metal budget is well under 30 GB (24.8 GB and 21.8 GB shares both failed). Treat Apple Silicon usable GPU memory as ~65% of RAM under RPC. Raw: halo-test-01-results/R2-32B-Q8-CORRECTED.log.
