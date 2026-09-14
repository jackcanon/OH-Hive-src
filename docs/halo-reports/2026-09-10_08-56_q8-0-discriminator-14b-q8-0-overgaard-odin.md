# Q8_0 discriminator - 14B Q8_0 Overgaard+Odin

- date: 2026-09-10T15:56:54Z
- outcome: fail
- model: Qwen_Qwen3-14B-Q8_0.gguf
- placement: Midgaard host + Odin 8 GB
- tensor_split: 8/7.7
- filed_by: HaloBench 0.1.0

## Summary

Qwen_Qwen3-14B-Q8_0.gguf on Midgaard host + Odin 8 GB did not complete (exit -2 after 20s).

## Notes

Fails with res=-3 -> Q8_0-over-RPC bug, file upstream, Halo uses Q4/Q5/Q6. Passes -> it's the 32B; next rung 32B Q4_K_M.

llama.cpp tag: b10883

## Command

```

```

Raw log: `2026-09-10_08-56_q8-0-discriminator-14b-q8-0-overgaard-odin.log`
