# R3b - 70B Q4_K_M three-way no Jotunheim

- date: 2026-09-10T19:04:02Z
- outcome: pass
- model: Llama-3.3-70B-Instruct-Q4_K_M.gguf
- placement: Overgaard host + Odin 13 GB, Heimdall 9 GB
- tensor_split: 13/9/21
- pp512: 67.9
- tg128: 6.7
- filed_by: HaloBench 0.1.0

## Summary

Llama-3.3-70B-Instruct-Q4_K_M.gguf ran on Overgaard host + Odin 13 GB, Heimdall 9 GB (tensor-split 13/9/21, RPC-first). Decode 6.7 tok/s, prefill 67.9 tok/s (median of 3, -p 512 -n 128). Took 313s including load.

## Results

| test | tok/s | ± |
|---|---:|---:|
| pp512 | 67.9 | 0.0 |
| tg128 | 6.7 | 0.0 |

## Notes

Compare against R3 (13/9/6/15): decode 4.97-4.99, prefill 51.2. Passes faster -> the Jotunheim hop's cost is measured. Fails with a host-side Metal OOM -> Overgaard's real no-mmap ceiling is under 21 GB + overhead; retry at 19 with Odin 15.

llama.cpp tag: b10883

## Command

```
~/halo/bin/llama-bench -m ~/halo/models/Llama-3.3-70B-Instruct-Q4_K_M.gguf -p 512 -n 128 -r 3 --rpc 192.168.1.196:50052,192.168.1.50:50052 --tensor-split 13/9/21 -lm none
```

Raw log: `2026-09-10_12-04_r3b-70b-q4-k-m-three-way-no-jotunheim.log`
