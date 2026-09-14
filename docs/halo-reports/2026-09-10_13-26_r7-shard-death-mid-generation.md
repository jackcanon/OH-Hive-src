# R7 - shard death mid-generation (70B three-way, Odin killed)

- date: 2026-09-10T13:26:29-07:00
- outcome: pass
- model: Llama-3.3-70B-Instruct-Q4_K_M.gguf
- placement: Overgaard host 21 + Odin 13 + Heimdall (CUDA) 9, -lm none, llama-cli -n 2000 --temp 0 -s 1
- tensor_split: 13,9,21
- filed_by: Loki, run over SSH from Midgaard (llama-cli, not llama-bench; a "pass" here means the experiment ran and answered its question)

## Summary

Generation was streaming at ~6.7 tok/s (352 words, ~460 tokens, in 69 s). Odin's ggml-rpc-server was SIGKILLed at 13:26:29. Within one second (13:26:30) the host aborted mid-word ("Notable events during this") with `ggml-rpc.cpp:566: Remote RPC server crashed or returned malformed response` -> `recv failed (bytes_recv=0, size_to_recv=8)` -> `ggml_abort` from the RPC dispatcher thread. No retry, no timeout, no error returned to the caller: the whole llama-cli process died with SIGABRT (macOS crash report llama-cli-2026-09-10-132631.ips). The partial output survived only because the run was under a pty; a buffered run would have lost it. The surviving worker (Heimdall) saw "Client connection closed", released its 9 GB shard immediately (GPU back to 197 MiB), and was ready for a new client with no cleanup.

## What this means for Halo v2 (lesson L6)

1. llama.cpp's RPC backend has no failover: a lost shard is a hard abort of the host process, detected instantly. The checkpoint/resume layer therefore sits entirely above the engine.
2. The engine must run in a child process of the node core, never in-process -- otherwise a member's laptop lid takes the node core down with it.
3. Checkpoint = prompt + generated-so-far text, persisted as tokens stream (not at exit). Resume = re-plan placement without the dead node (L2), reload (minutes at no-mmap: this placement took ~3.5 min to load), re-prefill prompt + partial, continue. The cost of a shard loss is one full reload, not a lost job.
4. Worker death is visible to the host in < 1 s, so no worker heartbeat is needed for that direction. Host death is the case that needs a coordinator-side lease/heartbeat (ADR-005), since the workers just see a closed connection and idle.
5. Surviving workers self-clean on disconnect: no reaper action needed for shard memory.

## Notes

llama.cpp tag: b10883 (build 91f6a6c). llama-cli's --tensor-split takes commas; llama-bench's takes slashes. The first attempt at this run was restarted because llama-cli buffers output without a pty -- same lesson as HaloBench's `script` wrapper, and it matters more here because the partial output IS the evidence.

## Command

```
script -q /dev/null ~/halo/bin/llama-cli -m ~/halo/models/Llama-3.3-70B-Instruct-Q4_K_M.gguf -p "Write a long, detailed history of the city of Sydney..." -n 2000 -st --no-display-prompt --temp 0 -s 1 --rpc 192.168.1.196:50052,192.168.1.50:50052 -ts 13,9,21 -lm none
# then, 69 s into generation, on Odin: pkill -9 -x ggml-rpc-server
```

Raw log: `2026-09-10_13-26_r7-shard-death-mid-generation.log`
