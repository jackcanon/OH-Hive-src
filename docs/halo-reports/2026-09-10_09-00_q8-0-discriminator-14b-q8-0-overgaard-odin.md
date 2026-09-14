# Q8_0 discriminator - 14B Q8_0 Overgaard+Odin

- date: 2026-09-10T16:00:14Z
- outcome: fail
- model: Qwen_Qwen3-14B-Q8_0.gguf
- placement: Midgaard host + Odin 8 GB
- tensor_split: 8/7.7
- filed_by: HaloBench 0.1.0

## Summary

**NOT a Q8 result -- HaloBench harness bug (fixed 09:10).** Qwen_Qwen3-14B-Q8_0.gguf on Midgaard host + Odin 8 GB did not complete (exit 6 after 202s): the host loaded the model and shipped Odin's share, then Odin's worker crashed. Odin's `rpc-server.log` shows `ggml_backend_blas_graph_compute: unsupported op RMS_NORM` -- HaloBench started the worker without `-d MTL0`, so it served its BLAS/CPU device instead of Metal. Odin had already compiled the Q8_0 Metal kernels (`kernel_mul_mm_q8_0_f32`) before that, so nothing here bears on the Q8-over-RPC question. Runner now passes `-d <device>` (MTL0/CUDA0 by backend) and pulls worker log tails on failure. Rerun pending.

## Notes

Fails with res=-3 -> Q8_0-over-RPC bug, file upstream, Halo uses Q4/Q5/Q6. Passes -> it's the 32B; next rung 32B Q4_K_M.

llama.cpp tag: b10883

## Command

```
/Users/dit1/halo/llama.cpp/build/bin/llama-bench -m /Users/dit1/halo/models/Qwen_Qwen3-14B-Q8_0.gguf -p 512 -n 128 -r 3 --rpc 192.168.1.196:50052 --tensor-split 8/7.7
```

Raw log: `2026-09-10_09-00_q8-0-discriminator-14b-q8-0-overgaard-odin.log`
