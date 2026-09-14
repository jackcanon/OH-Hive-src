# Jotunheim solo profile - model ladder

- date: 2026-09-09T21:05:00-07:00
- outcome: pass
- model: 8B / 12B / 14B Q4 ladder
- placement: Jotunheim alone (MacBook Pro M1 Pro 16 GB, macOS 26.6.2, on AC)
- tensor_split: 
- pp512: 134.9
- tg128: 13.7
- filed_by: backfill from HALO-TEST-01-LAN-SPLIT.md

## Summary

Jotunheim is half an Odin, consistently: 0.51-0.73x decode across the ladder, matching the M1 Pro's 200 GB/s bus vs the M4 Pro's ~273 GB/s. No thermal fade over 1024 tokens. Usable GPU memory ~11 GB (16 x ~0.7): the 8.4 GB 14B fits, the 16.8 GB Qwen 3.8 27B loads then dies at compute (res = -3). As a Halo worker plan on an 8-10 GB shard; it adds ~40 ms/token, about the same as Odin carrying 14 GB - worth pooling at a proportional cost.

## Results

| test | tok/s | ± |
|---|---:|---:|
| llama3.1 8B Q4_K_M pp512 | 256.6 | |
| llama3.1 8B Q4_K_M tg128 | 26.2 | |
| gemma4 12B Q4_0 pp512 | 178.6 | |
| gemma4 12B Q4_0 tg128 | 22.2 | |
| qwen3 14B Q4_K_M pp512 | 134.9 | |
| qwen3 14B Q4_K_M tg128 | 13.7 | |
| qwen3 14B tg1024 | 13.6 | |

## Notes

llama.cpp tag: b10883 (Odin's native macOS-26 build copied over). Wired IP 192.168.1.10 (0.4 ms). Raw: halo-test-01-results/jotunheim-solo-profile.log, jotunheim-solo-telemetry.csv.
