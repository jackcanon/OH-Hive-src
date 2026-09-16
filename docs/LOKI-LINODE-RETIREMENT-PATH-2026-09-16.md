# Retiring the Linodes: what has to exist first

**Loki, 2026-09-16** · Prompted by Jack: *"Linode is a short term solution, we will be shutting
these down soon. They only exist until community based substitutions come online."*

That reframes several things I filed today, and it makes one low-priority item the critical path.

---

## 1. Answering the Linode Backups question: **no**

The console is nagging to enable Linode Backups on all three. Don't. They are billed per instance
and these instances are scaffolding. Hive data is already protected by the nightly age-encrypted
hub backups (`HIVE_BACKUP_RECIPIENT` is set on all three, and each logs `nightly hub backups
enabled hour_utc=9`). Paying for VM snapshots of boxes we intend to delete buys nothing that
matters after the handover.

The thing worth protecting is not the VM, it is **the artifact blobs and the registration
identity** — and that is a replication problem, not a snapshot problem. See §3.

---

## 2. What is actually at risk: two regions have exactly one server, and it is a Linode

Measured tonight:

| server | region | tier | blobs |
|---|---|---|---|
| heimdall-dam | us-west | (compute_and_server) | 16 |
| chicago-hive | us-central | standby | 3 |
| **amsterdam-hive** | **eu-west** | **primary** | 13 |
| **Sydney** | **ap-southeast** | **primary** | 11 |

**eu-west and ap-southeast each have exactly one regional server, it is tier `primary`, and it is
a Linode.** Shut either one down before a community replacement is online and that region drops
to zero — no primary, no local artifact copy, and coordinator election loses a candidate.

ADR-013 §G sets a day-1 floor of five regional servers across NA/EU/APAC. We have four, two of
them temporary. So the Linodes are not surplus capacity being wound down; **they are currently
holding two of the three regions up.**

Also worth noting: chicago holds 3 blobs against amsterdam's 13. If replication factor 2 (ADR-007)
is meant to be holding, that spread deserves a look on its own — but it especially matters here,
because it means the copies are not evenly placed to survive a server leaving.

---

## 3. Nobody has written the handover, and that is the real gap

"Stand up a community server, then shut the Linode down" hides at least five steps, none of which
are documented:

1. **Blob drain.** The new server has to reach replication parity *before* the old one dies, not
   after. `hive.replication_plan` pulls artifacts a server lacks from another region, but nothing
   says "this server is leaving, get its artifacts to safety first." A departing server needs a
   drain state, or an operator has to verify counts by hand and hope.
2. **Identity.** Does the community box get a **new** `node_id` and registration, or does it
   inherit the region's identity? A new one is cleaner, but then `sydney.ohghive.com` has to move
   and any artifact registry rows pointing at the old server need to resolve.
3. **DNS / tunnel.** `amsterdam.ohghive.com`, `sydney.ohghive.com` and `chicago.ohghive.com` all
   resolve to Cloudflare, so the origin swap is a Cloudflare change plus a tunnel on the new host.
   That is the easy step, but it has to be sequenced after the drain, not before.
4. **Tier and coordinator election.** Both Linodes are `primary`. A volunteer box on a home
   connection probably should not inherit `primary` on day one — ADR-005's standby delay exists
   precisely so a weaker server does not grab the coordinator lease. Handing over means deciding
   the new server's tier deliberately.
5. **Trust and payment.** `hive.server_register` currently enforces `operator='hjm'` only for the
   founder account (per `e6099537`). A community-operated regional server is `operator='volunteer'`
   and earns `earn_infra` — which is exactly what `967ad663` extends. Retiring a Linode is the
   first real test of the volunteer operator path, not a routine ops task.

**None of this is hard. All of it is unwritten**, and it is the kind of thing that gets discovered
at 2am with a region already dark.

---

## 4. Priority changes this implies

- **`225329c5` — the volunteer infrastructure brief (`docs/VOLUNTEER-INFRA.md`) is currently
  `low` and not started. It should not be low.** It is the recruiting document for the people who
  replace these boxes. Until it exists there is nobody to hand a region to, so every other
  retirement step is blocked behind it. It is the critical path to shutting the Linodes down.
- **`967ad663` — extending `earn_infra`** (relay bandwidth, snapshot serving, backup replicas) is
  `low`. It is what makes running a regional server *worth* something to a volunteer. Same
  argument: it is not a nice-to-have, it is the incentive half of the replacement plan.
- **`c9a656e0` — reconciling the systemd units with `packaging/`** matters more now than when I
  filed it, and for a different reason than I originally gave. Today it is three boxes we control.
  After the handover it is *community operators hand-writing units on machines we will never log
  into.* The repo only ships the user-unit form; anyone standing up a server today writes their
  own system unit and invents their own drift. Ship a reviewed one.
- **Linode Backups** — no, per §1.

---

## 5. One thing that should be checked before any Linode is touched

Every `hive-server` on **v0.3.0** that started successfully and kept running logged nothing at all
(see today's correction entry in `CONTINUITY.md`). All four servers are on v0.4.0 as of tonight,
so the fleet can be observed again — but a community operator who installs a stale release
inherits a server that runs blind and looks fine.

The `version-gate` job added to `release.yml` today (`33d9b6f`) stops a *new* release shipping
binaries that misreport themselves. It does nothing about v0.3.0 still being downloadable. Worth
deciding whether v0.3.0 should be yanked or clearly marked before volunteers start installing from
releases.

---

Loki
