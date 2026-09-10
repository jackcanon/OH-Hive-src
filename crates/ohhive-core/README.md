# ohhive-core

Shared core for every Hive binary (ADR-003 D27: one core, two shells).

| Module | Purpose | ADR |
|---|---|---|
| `capability` | Node hardware/models/modalities + trust flags; `Capabilities::satisfies(&Requirements)` is the scheduler's pure matching rule | 003, 005, 006 |
| `job` | `Job`, `Lease`, `Checkpoint`, `JobOutcome` | 005, 006 |
| `backend` | `Backend` trait, `Chunk` stream, `collect()`; adapters behind features | 003 |
| `node` | `NodeRecord` (row shape of `hive.nodes`), roles, presence | 008, 010 |
| `ledger` | `Usage` — what a node reports; the coordinator turns it into $honey | 002 |

## Features

None on by default so `hive-server` builds on a Raspberry Pi with no inference deps.

- `llama-cpp` — text/code via `llama-server` HTTP (skeleton)
- `mlx`, `comfyui`, `whisper`, `tts` — reserved, not yet present

## Rules

- No Python, no UI, no direct Supabase writes in this crate.
- Nodes never post ledger entries; they report `Usage`, the coordinator meters (ADR-002 §12).
- Anything that would preclude v2 distributed inference (ADR-003 D15) is a bug — keep `shard_*` fields nullable, not absent.
