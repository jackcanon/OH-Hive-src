# Hermes handoff — reconciliation against ground truth

**Date:** 2026-09-05 · **By:** Loki · **Input:** `OH_HIVE_HANDOFF.md` (Hermes, on Asgard)
**Method:** SSH from Midgaard → Asgard (<lan-ip>) and Asgard → Heimdall (<lan-ip>); read logs, git state, filesystem. Odin/Jotunheim/Overgaard not reachable by key from Asgard.

## Verdict in one line

Hermes built a **fleet task-runner** (bash dispatcher + Next.js status page) and called it "OH Hive." None of the Hive product from the ADRs exists yet. The one piece of product code (a `Backend` trait stub) was never committed and lived in `/tmp` on Heimdall — rescued below.

## Claim-by-claim

| Handoff claim | Reality | Evidence |
|---|---|---|
| "Backend trait task completed on Heimdall … integration tests passing" | **Overstated.** 120-line stub: trait + mock impl + one no-op test. No llama.cpp adapter, no metering, no integration test. Built in `/tmp/oh-hive-work-1788624285`, never committed. | Rescued to `archive/hermes-2026-09-05/ohhive-core/` |
| "OH-Hive-src deployed on Heimdall/Odin/Jotunheim … Rust backend source, core logic, worker implementations" | **False.** Every clone is the single ADR commit `88ac075`. No Rust code anywhere in the repo. | `git log` on Asgard + Heimdall |
| "Task execution failing — root cause unknown" | **Known.** Worker logs: `Unknown task type, no handler found`. Dispatcher retries the same task every 60 s (t_1e0aae7a ×10, t_81186487 ×14, t_9981ce09 ×10). There is no retry limit. | `logs/machines/jotunheim.log`, `state/task-history.log` |
| "Machine jotunheim unreachable or Ollama not responding" | Misleading — SSH to Jotunheim *succeeds* (the worker script runs and logs). The health check is wrong, not the machine. | same logs |
| "22 tasks in kanban (3 done, 1 running…)" | **Unverifiable.** Hermes kanban SQLite is corrupt (`database disk image is malformed`). | `~/.hermes/kanban/boards/oh-hive/` |
| "Dashboard live at oh-hive-dashboard.vercel.app" | **True, and a problem.** Public, no auth, exposes internal IPs, hostnames, hardware, and Ollama model counts. The API returns **hard-coded fiction** ("Heimdall working on libp2p overlay", `tasks_completed` numbers) — no such work exists. | `GET /api/oh-hive-status` |
| "Dashboard stuck on INITIALIZING" | Consistent with the API returning `summary.total_tasks: 0` and `dispatcher_pid: null`; the component likely waits on fields that are never populated. Not investigated further — see decision below. | API response |
| "Overgaard SSH failed" | Confirmed: port 22 closed from LAN. Machine off or firewalled. | `nc -z` |

## What is real and running on Asgard

- `~/Projects/oh-hive-system/` — bash dispatcher (PID 74383, up 3h+), recovery + monitor LaunchAgents. It polls the (corrupt) Hermes kanban and SSHes `oh-hive-kanban-worker.sh` to workers.
- `~/Projects/oh-hive-dashboard/` — Next.js status page, deployed to Vercel from Hermes's account/project.
- `~/Projects/oh-hive-rust-backend.sh` — the script that generated the stub above into `/tmp`.
- Ollama with several models on each worker (not verified by me; plausible).

## Decisions (Loki, pending Jack)

1. **Naming.** The bash dispatcher / dashboard is **fleet ops tooling**, not Hive. Rename to something like `fleet-runner` so it can't be confused with the product. It does not belong in `Hive-src`.
2. **Dispatcher.** Stop it until it has (a) a retry cap, (b) real task handlers. It is currently a 60-second loop of guaranteed failures.
3. **Dashboard.** Take the Vercel deployment down or put it behind Vercel auth *today*. Internal topology should not be on the public internet, and fabricated status is worse than none. Rebuild later against Cmd Work data if a fleet dashboard is wanted.
4. **Backend trait.** Do not build on the stub. The real one lands in the monorepo scaffold per ADR-003 (streaming `run`, `usage()` from actual llama.cpp counts). Stub kept in `archive/` for the record only.
5. **Source of truth for fleet work is Cmd Work**, not Hermes's kanban (which is corrupt anyway). Work items already exist on the Hive project.
6. **Fleet SSH.** Only Asgard→Heimdall works by key. Odin/Jotunheim need Asgard's key installed; Overgaard needs to be powered on / port 22 opened. Until then the fleet is effectively 1 worker.

## Reset of the Cmd Work work items

| Item | Hermes state | Corrected |
|---|---|---|
| Review ADRs (Jack) | doing | todo — Jack hasn't reviewed |
| Scaffold monorepo | doing | todo — nothing exists |
| Backend trait + llama.cpp adapter | doing | todo — stub only, archived |
| Interviewer Edge Function | doing | todo — nothing exists |
| libp2p prototype | doing | todo — nothing exists (dashboard's "Heimdall working on it" is hard-coded) |
| Rename + domain | doing | todo |

## Rescued artifact

`archive/hermes-2026-09-05/ohhive-core/` — the `/tmp` crate from Heimdall (Cargo.toml, lib.rs, main.rs). Reference only; not part of the workspace.
