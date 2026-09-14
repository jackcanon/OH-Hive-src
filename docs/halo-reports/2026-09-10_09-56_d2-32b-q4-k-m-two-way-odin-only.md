# D2 - 32B Q4_K_M two-way Odin only

- date: 2026-09-10T16:56:57Z
- outcome: fail
- model: Qwen_Qwen3-32B-Q4_K_M.gguf
- placement: Midgaard host + Odin 16 GB
- tensor_split: 16/4
- filed_by: HaloBench 0.1.0

## Summary

Qwen_Qwen3-32B-Q4_K_M.gguf on Midgaard host + Odin 16 GB did not complete (exit 1 after 219s). Metal reported insufficient memory at compute (`res = -3`). Either a shard exceeded that node's real usable budget (~65% of RAM), or -- if every node was inside budget and no worker compiled a kernel -- the Q8_0-over-RPC hypothesis from Test 01.

## Notes

Odin's real budget is ~15.5-18 GB; 16 GB share is deliberately near the edge -- if it OOMs on Odin that shows in the worker log, distinct from a host-side res=-3.

llama.cpp tag: b10883

## Command

```
/Users/dit1/halo/llama.cpp/build/bin/llama-bench -m /Users/dit1/halo/models/Qwen_Qwen3-32B-Q4_K_M.gguf -p 512 -n 128 -r 3 --rpc 192.168.1.196:50052 --tensor-split 16/4
```

Raw log: `2026-09-10_09-56_d2-32b-q4-k-m-two-way-odin-only.log`
