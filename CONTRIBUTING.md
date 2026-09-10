# Contributing to Hive

Thanks for looking at this. A few things to know before sending a PR.

## Start here

- Read [`ADR/README.md`](ADR/README.md) first, then ADR-000. Architecture
  decisions are numbered (D1, D2, ...) and every other doc/PR should cite the
  decision it's implementing or changing.
- Work is tracked in Cmd Work (project: Hive), not GitHub Issues/Projects.
  If you want to pick something up, ask first so it doesn't collide with
  in-flight work.

## Building

```sh
cargo test --workspace --exclude ohhive-desktop
cargo clippy --workspace --exclude ohhive-desktop -- -D warnings
cargo fmt --all -- --check

pnpm install
pnpm --filter @ohhive/web build
```

`ohhive-desktop` needs Tauri's webkit prerequisites and isn't built in Linux
CI — build it locally if you're touching the desktop app.

## Rules CI enforces (and PRs should too)

- `hive-server` must cross-compile for `aarch64-unknown-linux-musl` with zero
  inference or Python dependencies (ADR-003 D12) — it has to run on a
  Raspberry Pi.
- Nodes never write ledger rows directly; they report `Usage` and the
  coordinator meters it (ADR-002 §12).
- Every Hive table lives in the Postgres schema `hive`, never `public`.
  `scripts/check-migrations.sh` checks this statically.
- No `hive.*` table goes into Supabase Realtime (ADR-013 D70) — it doesn't
  scale past a couple hundred nodes. Live UI state comes from the regional
  server's broadcast WebSocket instead.

## Security-sensitive areas

If your change touches any of the following, call it out explicitly in the
PR description so it gets a closer look:

- Row-level security policies in `supabase/migrations/`.
- Node/server pairing, key issuance, or anything in `hive.pairings` /
  `hive.node_keys`.
- The ledger (`hive.ledger_entries` and friends) or anything that can mint,
  move, or archive $honey.
- Whatever enforces `tools_level` / sandboxing for code or tools a compute
  node runs on behalf of a card — this is the boundary that keeps one
  member's task from touching another member's machine.

See [`SECURITY.md`](SECURITY.md) for how to report a vulnerability privately
instead of opening a public PR or issue for it.

## License

By submitting a contribution, you agree it's dual-licensed under
[Apache-2.0](LICENSE-APACHE) and [MIT](LICENSE-MIT), same as the rest of the
project, without any additional terms or conditions.
