# R8 - cross-backend determinism (32B Q4, temp 0, seed 1, 64 tokens)

- date: 2026-09-10T13:44:09-07:00
- outcome: pass
- model: Qwen_Qwen3-32B-Q4_K_M.gguf
- placement: four configurations, Overgaard host throughout (see Results)
- tensor_split: B/C: 8,12
- filed_by: Loki, run over SSH from Midgaard (llama-cli, four sequential runs, same prompt)

## Summary

Same prompt, `--temp 0 -s 1 -n 64 --no-warmup`, four ways. A (Overgaard Metal, no RPC) and B (Overgaard Metal + Odin Metal over RPC, `-lm none`) produced **byte-identical** output (same md5). C (Overgaard Metal + Heimdall CUDA over RPC) matched A for the first 42 words, then diverged at a near-tie ("But I need to be precise" vs "But I'm not exactly sure"). D (Overgaard CPU-only, `-ngl 0`) diverged at word 24. Conclusion: the RPC transport is transparent -- identical placement gives identical tokens -- but mixing backends (Metal/CUDA/CPU) is not bit-reproducible, as expected from floating-point summation order. Divergence is a flipped argmax at a close call, not garbage; all four outputs are coherent and on-topic.

## Results

| run | backends | words | vs A |
|---|---|---:|---|
| A | Metal (solo, mmap) | 53 | -- |
| B | Metal + Metal(RPC), -lm none | 53 | identical (md5 fc2977a7…) |
| C | Metal + CUDA(RPC), -lm none | 52 | diverges at word 43 |
| D | CPU only (-ngl 0) | 52 | diverges at word 24 |

Timings: A 13 s (page-cached mmap); B 81 s and C 84 s including no-mmap load + 8 GB upload; D 40 s.

## What this means for Halo v2 (lesson L8)

1. Reproducibility requires recording the full placement -- which nodes, which backends, the split, the seed, the quant -- alongside the job. That is precisely what `hive.cards.shard_plan` is for; it is not optional metadata.
2. ADR-019's triangulated verification cannot use exact-match re-execution across different placements; a re-run on different hardware will legitimately differ token-for-token after the first near-tie. Verification of pooled jobs is semantic (judge) or same-placement, never byte-equality across backends.
3. Halo can still offer "same placement, same output" as a guarantee for reruns on the same pool -- useful for debugging and for cached results.
4. Mixed-quant across shards remains untested (every field tool assumes one quant per instance). Rule stands: one quant per pooled job.

## Notes

llama.cpp tag: b10883. Qwen3 emitted its thinking preamble ("Okay, so I need to…") in all four -- the divergence is inside that reasoning text, which is a fair test of determinism. Raw outputs: `2026-09-10_13-44_r8-*.txt` alongside this file.
