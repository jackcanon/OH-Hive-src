# R2 - 32B Q8_0 pooled attempts (six placements, all failed)

- date: 2026-09-09T19:30:00-07:00
- outcome: fail
- model: Qwen_Qwen3-32B-Q8_0.gguf
- placement: Midgaard/Overgaard host + Odin, Asgard, Heimdall in 2/3/4-way combinations
- tensor_split: (inverted - see summary)
- filed_by: backfill from HALO-TEST-01-LAN-SPLIT.md

## Summary

Every attempt died on whichever Mac was an RPC worker: Metal "command buffer failed with status 5 / Insufficient Memory" at first compute, then the worker asserted. Observed on Odin at 21, 15, 13, 12, 10, 8 and 4 GB nominal shares; Asgard at 8 and 5; Midgaard at 12. Root-caused later the same night by the shard-size ladder: --tensor-split assigns RPC devices first and the local GPU last, so every split gave the big share to the worker. Not a Metal bug, not the network.

## Notes

llama.cpp tag: b10883. Raw: halo-test-01-results/R2-32B-Q8-*.txt (six files). Things that did not fix it, tested in isolation: -b 128 -ub 128, removing -c, native build on Odin, GGML_METAL_NO_RESIDENCY=1.
