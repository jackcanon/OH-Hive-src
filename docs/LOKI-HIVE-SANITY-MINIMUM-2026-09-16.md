# Hive infra: the sanity minimum, and what I got wrong about it

**Loki, 2026-09-16.** Jack: *"the servers are designed to fail over... we did test taking these
offline and failover worked... Loki's Den is the priority."*

---

## First, a correction

I framed the Linode retirement as a data-loss risk and raised two items to high on that basis. I
did not ask whether failover had been tested. **It has been** — servers were taken offline
deliberately and the failover worked, and Jack's local servers are designed to absorb the
Linodes' role. Coordinator election (ADR-005, `hive.coordinator_try`) and replication factor 2
(ADR-007) are both shipped and both were exercised.

So the correct framing is not "a region could go dark." It is "the handover is undocumented," and
that is a much smaller, later problem. I over-weighted it because I had just spent two hours
inside the fleet and everything looks urgent from in there. Priorities adjusted below.

---

## The sanity minimum — the short list

Not this week. This is what stops the infrastructure surprising us later, in rough order of
value per minute spent:

1. **Write down that failover is tested, and where.** It is the single most load-bearing fact
   about this fleet and it exists only in Jack's head and one test session. A paragraph in
   `CONTINUITY.md` or ADR-005 naming what was taken offline, what took over, and how long it took
   is enough. Every future agent — me included — will otherwise re-derive the risk from scratch
   and raise a false alarm, exactly as I just did.
2. **Yank or clearly mark release v0.3.0.** A v0.3.0 `hive-server` that starts successfully logs
   nothing at all (proven today). It is still the download a volunteer might land on. Marking it
   is a two-minute action on the releases page and it prevents a class of silent, unobservable
   community servers.
3. **Ship a reviewed regional-server unit in `packaging/`.** The repo only ships the user-unit
   form, so anyone standing up a server hand-writes a system unit and invents their own drift.
   Chicago's and Sydney's differ from the repo's and from each other. Low effort, and it stops
   mattering more later, when the operators are volunteers on machines nobody else can log into.
4. **A "safe to shut down" signal.** Not the full drain state I filed — just a readable answer to
   "are this server's artifacts replicated elsewhere?" so retiring a box is a check rather than a
   hope. Failover covers the *unplanned* case; this covers the *planned* one, which is the case we
   actually have coming.
5. **Linode Backups: no.** Billed per instance on boxes being retired; the nightly age-encrypted
   hub backups already cover the data.

Items 1 and 2 are minutes. Items 3 and 4 are small. None of them are this week.

---

## Priority corrections I am making now

| item | was | now | why |
|---|---|---|---|
| `225329c5` volunteer infra brief | **high** (raised by me today) | medium | Real, but it is the recruiting doc for a handover that is not imminent and is failover-protected |
| `c3d5ec29` drain state | **high** (filed by me today) | medium | Failover is tested; the gap is planned-retirement ergonomics, not data loss |
| `c9a656e0` unit reconciliation | medium | medium | Unchanged — already corrected once today |
| `af085cb6` release version-gate | high | **done** | Shipped in `33d9b6f` |

---

## The thing worth saying plainly

Jack's framing is right and it reframes my whole afternoon:

> *"The Hive doesn't really work until its end users have a system that can actually build
> projects... without Loki's Den configured to be a useful builder, the Hive is pointless. We've
> built cool infrastructure, but now we need to build strong workers."*

Four regional servers that replicate, fail over, elect a coordinator and settle storage in $honey
are worth exactly as much as the work they carry. Right now they carry very little — amsterdam
holds 13 blobs, chicago 3. That is not an infrastructure problem to solve with more
infrastructure. It is the absence of a builder good enough to generate work worth replicating.

I spent today making the servers healthier. That was worth doing and it is done. The next thing
is not another server.

Loki
