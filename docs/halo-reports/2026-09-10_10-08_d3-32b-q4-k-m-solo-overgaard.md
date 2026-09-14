# D3 - 32B Q4_K_M solo

- date: 2026-09-10T10:08:00-07:00
- outcome: pass
- model: Qwen_Qwen3-32B-Q4_K_M.gguf
- placement: Overgaard alone (M4 Max 36 GB), no RPC
- tensor_split: 
- pp512: 164.0
- tg128: 18.2
- filed_by: Loki, run over SSH from Midgaard (HaloBench was on Midgaard, which can't hold 19.8 GB)

## Summary

Qwen3-32B Q4_K_M runs cleanly on llama.cpp b10883 with no RPC involved: 164.0 pp512, 18.2 tg128, exit 0, Metal working set 30.15 GB. Combined with R2 (3-way, fail), D2 (2-way Metal-only, fail) and last night's Overgaard-hosted 32B Q8 (fail), this isolates the failure to **Qwen3-32B over the RPC backend** -- any split, any quant, any host, any topology -- while the 14B (same family) runs over RPC everywhere. Odin's raw worker log from D2 shows 18 compiled kernels (a full layer's ops) and no error before the client disconnected, so the worker did start computing; the host-side `res = -3` comes after dispatch, not before.

## Results

| test | tok/s | ± |
|---|---:|---:|
| pp512 | 164.02 | 0.04 |
| tg128 | 18.17 | 0.01 |

## Notes

llama.cpp tag: b10883 (build 91f6a6c). Overgaard's ~/halo/bin/llama-bench. Midgaard's SSH key was authorized on Overgaard for this run; the 32B Q4 file was copied from Midgaard (sha not re-verified; size matches 19762149696).

## Command

```
~/halo/bin/llama-bench -m ~/halo/models/Qwen_Qwen3-32B-Q4_K_M.gguf -p 512 -n 128 -r 3
```
