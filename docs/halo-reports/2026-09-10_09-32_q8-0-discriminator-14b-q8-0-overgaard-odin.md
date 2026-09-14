# Q8_0 discriminator - 14B Q8_0 Overgaard+Odin

- date: 2026-09-10T16:32:46Z
- outcome: pass
- model: Qwen_Qwen3-14B-Q8_0.gguf
- placement: Midgaard host + Odin 8 GB
- tensor_split: 8/7.7
- pp512: 201.7
- tg128: 12.4
- filed_by: HaloBench 0.1.0

## Summary

Qwen_Qwen3-14B-Q8_0.gguf ran on Midgaard host + Odin 8 GB (tensor-split 8/7.7, RPC-first). Decode 12.4 tok/s, prefill 201.7 tok/s (median of 3, -p 512 -n 128). Took 162s including load.

## Results

| test | tok/s | ± |
|---|---:|---:|
| pp512 | 201.7 | 0.8 |
| tg128 | 12.4 | 0.1 |

## Notes

Fails with res=-3 -> Q8_0-over-RPC bug, file upstream, Halo uses Q4/Q5/Q6. Passes -> it's the 32B; next rung 32B Q4_K_M.

llama.cpp tag: b10883

## Command

```
/Users/dit1/halo/llama.cpp/build/bin/llama-bench -m /Users/dit1/halo/models/Qwen_Qwen3-14B-Q8_0.gguf -p 512 -n 128 -r 3 --rpc 192.168.1.196:50052 --tensor-split 8/7.7
```

Raw log: `2026-09-10_09-32_q8-0-discriminator-14b-q8-0-overgaard-odin.log`
