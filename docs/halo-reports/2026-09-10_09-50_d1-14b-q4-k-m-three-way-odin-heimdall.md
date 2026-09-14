# D1 - 14B Q4_K_M three-way Odin+Heimdall

- date: 2026-09-10T16:50:40Z
- outcome: pass
- model: Qwen_Qwen3-14B-Q4_K_M.gguf
- placement: Midgaard host + Odin 4 GB, Heimdall 3 GB
- tensor_split: 4/3/2
- pp512: 251.0
- tg128: 14.6
- filed_by: HaloBench 0.1.0

## Summary

Qwen_Qwen3-14B-Q4_K_M.gguf ran on Midgaard host + Odin 4 GB, Heimdall 3 GB (tensor-split 4/3/2, RPC-first). Decode 14.6 tok/s, prefill 251.0 tok/s (median of 3, -p 512 -n 128). Took 123s including load.

## Results

| test | tok/s | ± |
|---|---:|---:|
| pp512 | 251.0 | 1.2 |
| tg128 | 14.6 | 0.5 |

## Notes

Passes -> CUDA worker + 3-way pipeline fine; the 32B is the problem. Fails -> the 3-way/CUDA path is broken and R2's failure is not about the 32B.

llama.cpp tag: b10883

## Command

```
/Users/dit1/halo/llama.cpp/build/bin/llama-bench -m /Users/dit1/halo/models/Qwen_Qwen3-14B-Q4_K_M.gguf -p 512 -n 128 -r 3 --rpc 192.168.1.196:50052,192.168.1.50:50052 --tensor-split 4/3/2
```

Raw log: `2026-09-10_09-50_d1-14b-q4-k-m-three-way-odin-heimdall.log`
