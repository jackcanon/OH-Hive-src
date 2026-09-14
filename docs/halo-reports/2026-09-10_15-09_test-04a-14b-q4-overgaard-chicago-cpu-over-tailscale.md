# Test 04a - 14B Q4 Overgaard + Chicago CPU over Tailscale

- date: 2026-09-10T22:09:25Z
- outcome: pass
- model: Qwen_Qwen3-14B-Q4_K_M.gguf
- placement: Overgaard host + Chicago (cloud) 0.5 GB
- tensor_split: 0.5/8
- pp512: 20.6
- tg128: 6.7
- filed_by: HaloBench 0.1.0

## Summary

**First pooled inference over the real internet.** Overgaard (lab, M4 Max) hosting, a Linode 4 GB / 2 vCPU box in Chicago holding a 0.5 GB CPU shard, connected over Tailscale via a DERP relay (path not direct: Linode firewall drops inbound UDP), 44.4 ms RTT ±0.2 steady-state. Decode 6.66 tok/s, prefill 20.6 (median of 3, -p 512 -n 128). 235 s total including a short load.

**Prediction graded: wrong, informatively.** Predicted ~11 tok/s (~90 ms/token = 25 solo + ~24 latency + ~40 CPU). Measured 150 ms/token: the round trip can account for at most ~50 ms, so the 2-vCPU box's compute for a 0.5 GB slice cost ~75-100 ms — I priced CPU work as bandwidth-bound like a GPU; on two cores it is compute-bound. Prefill collapsed from 388 (Overgaard solo) to 20.6: a 512-token batched matmul on two cores is hopeless regardless of slice size.

**Conclusions.** (1) The pipeline works end to end over a real WAN path with Tailscale in the middle — the mechanism is proven. (2) This run cannot isolate latency because CPU compute dominated; the real-path latency measurement needs a GPU on the far end (volunteer, Test 04b) or Heimdall's CUDA card routed through the relay. (3) Cloud CPU boxes are not viable shard-holders at any slice size — skip Sydney-as-CPU. (4) The scheduler must price per-node *compute* (prefill especially), not only memory bandwidth and RTT: a node's prefill throughput is its own capability field.

Qwen_Qwen3-14B-Q4_K_M.gguf ran on Overgaard host + Chicago (cloud) 0.5 GB (tensor-split 0.5/8, RPC-first). Overgaard 100.80.147.109, Chicago 100.100.2.120 (chicago-hive), Midgaard 100.82.231.90 on the tailnet.

## Results

| test | tok/s | ± |
|---|---:|---:|
| pp512 | 20.6 | 0.7 |
| tg128 | 6.7 | 0.1 |

## Notes

Compare ms/token against Overgaard solo (39.4 tok/s = 25 ms) + 1.1 x one-way RTT to Chicago. Then repeat with Sydney (cloud) for the far-member floor.

llama.cpp tag: b10883

## Command

```
~/halo/bin/llama-bench -m ~/halo/models/Qwen_Qwen3-14B-Q4_K_M.gguf -p 512 -n 128 -r 3 --rpc 100.100.2.120:50052 --tensor-split 0.5/8 -lm none
```

Raw log: `2026-09-10_15-09_test-04a-14b-q4-overgaard-chicago-cpu-over-tailscale.log`
