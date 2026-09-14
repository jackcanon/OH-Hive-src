# R0 - 14B Q4_K_M on Overgaard alone

- date: 2026-09-09T17:00:00-07:00
- outcome: pass
- model: Qwen_Qwen3-14B-Q4_K_M.gguf
- placement: Overgaard alone (M4 Max 36 GB)
- tensor_split: 
- pp512: 388.6
- tg128: 39.4
- filed_by: backfill from HALO-TEST-01-LAN-SPLIT.md

## Summary

Best single machine in the lab. Usable Metal budget is well under the advertised 30.15 GB - a 24.8 GB Q8 share plus compute later failed with res = -3.

## Notes

llama.cpp tag: b10883. Median of 3, -p 512 -n 128.
