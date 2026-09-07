# OH Hive

Pooled compute for Office Hours Global and Loki's Lab. Members contribute idle computers to **the Hive**, earn **$honey**, and spend it on agent-driven projects — text, code, image, video, audio — run from a community kanban.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option. Project *outputs* have their own licensing (ADR-011). See [`SECURITY.md`](SECURITY.md) to report a vulnerability, [`CONTRIBUTING.md`](CONTRIBUTING.md) to send a PR, and [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) for community expectations.

Architecture is decided in [`ADR/`](ADR/README.md). Read ADR-000 first; every other doc cites it by decision number (D1–D68).

## Layout

```
crates/ohhive-core        shared core: capabilities, jobs, Backend trait, node record  (ADR-003)
crates/hive               `hive` CLI — headless compute node + diagnostics
crates/hive-server        `hive-server` — regional server, Pi → rack, no inference deps (ADR-004)
crates/hive-coordinator   pure scheduling logic, runs inside hive-server on the lease holder (ADR-005)
apps/desktop              Tauri "OH Hive" node app; wraps the core, adds backends + Python sidecar (ADR-010)
apps/web                  Next.js web app on Vercel — interview, kanban, wallet, Hive browser (ADR-009)
packages/ui               React components shared by web + desktop (About section lives here)
packages/schema           JSON Schema + TS types shared by web, Edge Functions, and (mirrored) Rust
supabase/migrations       schema `hive` on the Cmd Work Supabase project (ADR-001)
supabase/functions        Edge Functions: interview (stub)
docs/                     handoffs and reconciliations
archive/                  rescued artifacts, not part of the build
```

## Build

```sh
# Rust (all OSes)
cargo test --workspace --exclude ohhive-desktop
cargo run -p hive -- run "hello from the hive"        # mock backend, prints usage

# Web + desktop UI
pnpm install
cp apps/web/.env.example apps/web/.env.local          # fill the anon key
pnpm --filter @ohhive/web build
pnpm --filter @ohhive/desktop tauri dev               # needs Rust + Tauri prerequisites
```

## Rules that keep the ADRs true

- `hive-server` must build on ARM64 musl with no inference or Python dependencies. CI enforces it.
- Nodes never write ledger rows. They report `Usage`; the coordinator meters (ADR-002 §12).
- Everything OH Hive lives in Postgres schema `hive`. Never touch `public`.
- Every Happy Jack Media app ships with a quiet About section crediting HJM and linking *This Is Not A Draft*. It's in `packages/ui` — use it.
- Work is tracked in Cmd Work (project: OH Hive), not in ad-hoc kanbans.
