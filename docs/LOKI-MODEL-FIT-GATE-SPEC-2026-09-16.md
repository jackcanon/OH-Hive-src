# Model-fit gate — spec for implementation

**Author:** Loki, 2026-09-16 · **Status:** spec, not implemented
**Implementer:** Sif (needs a compiler; I have no Rust toolchain in either environment I can
reach — the VM that mounts this disk has no cargo and no network, and the cloud sandbox has no
checkout)
**Fixes:** the Jotunheim thrash of 2026-09-16 — `qwen3.8:27b` pinned on a 16 GB machine,
12.2 GB swap, ~9 M pageouts, 46 minutes of wall clock, no output produced.

---

## 1. This is the same bug we already fixed once, in a different column

Commit `98cd731` fixed *advertisement ≠ capability* for **modalities**. `crates/hive/src/main.rs`
now runs backend-reported modalities through an `executable` predicate before advertising, on the
reasoning that "the hub's `node_claim_card` matched on the advertisement alone, so the node
claimed a card it had no executor for."

Fourteen lines further down the same function, `models` is still handled the old way:

```rust
let mut models = vec![];
// ... per-backend:
models.extend(c.models);          // <- unioned straight in, no filter
// ...
Ok(Capabilities { hardware, modalities, models, .. })
```

`hardware` — which carries `ram_bytes`, `ram_free_bytes`, `vram_bytes`, `vram_free_bytes` — is
constructed in the *same function*, four lines above. The node has everything it needs to know
the model won't fit and advertises it anyway.

**So: modality was gated, model was not.** Jotunheim advertised a 27B it could not hold, the
coordinator matched on the advertisement, and the node spent 46 minutes paging.

---

## 2. The blocker: `ModelRef` carries no size

```rust
pub struct ModelRef {
    pub id: String,        // "qwen3.6-27b-q4_k_m"
    pub modality: Modality,
    pub backend: String,   // "llama_cpp" | "mlx" | "comfyui" | "whisper" | "tts"
}
```

There is no weight-size field, so nothing downstream *can* compare a model against RAM. This is
the one real design decision in this spec, and I'd rather you make it with the backends in front
of you than have me guess. Three options, my read on each:

- **(a) Add `size_bytes: Option<u64>` to `ModelRef`, populated by each backend.** Honest —
  llama.cpp's `/props` and the model file on disk both know the real size. Costs a change in
  every backend's `capabilities()`. My preference.
- **(b) Derive from the id string** (`27b` → parameter count → × bytes-per-weight from the quant
  suffix). No schema change, works today, but it is a parser over a naming convention that
  nobody guarantees. Good as a *fallback* when (a) yields `None`, bad as the primary.
- **(c) Ask the backend at claim time instead of advertise time.** Most accurate, but it moves
  the check out of the advertisement, which is the thing the coordinator actually matches on —
  so the node still gets offered work it cannot do. Rejected for that reason.

I'd ship (a) with (b) as the fallback, and treat "size unknown after both" as a deliberate
decision rather than a silent pass — see §4.

**Wire compatibility:** mark the new field `#[serde(default)]`. Mixed-version nodes are a live
condition in this fleet right now — Heimdall and chicago-hive are still on 0.3.0 — and this is
exactly the lesson `Chunk.truncated` taught: a new field without a default breaks the old nodes
that are still checking in.

---

## 3. The gate

Mirror the modality predicate, in the same function, immediately after it:

```rust
// Same reasoning as `executable` above, for weights instead of modality: a model this node
// cannot hold in memory is not a capability, and advertising it means the coordinator hands
// us a card we can only fail slowly. Jotunheim took 46 minutes and 9M pageouts to fail a
// 27B on 16GB of RAM -- swap makes this a timeout rather than an error, which is worse.
let fits = |m: &ModelRef| -> bool { ... };
```

**Budget** — use the *nominal* figure, not the free one. `ram_free_bytes` and `vram_free_bytes`
are measured at probe time and move constantly; gating on them makes the advertisement flap as
the member opens a browser. Use `vram_bytes` where present (discrete GPU), else `ram_bytes`.

**Headroom** — the probe already applies a 75% rule for Apple Silicon unified memory
(`capability.rs` line 60-63). Reuse that constant rather than inventing a second one; two
different headroom rules in one codebase is a bug waiting to happen. Weights are not the whole
footprint: KV cache, context, and the runtime all sit on top, which is precisely why a 27B
q4_k_m (~16 GB of weights) cannot run on a 16 GB machine even though the arithmetic looks like
it *just* fits.

**Log the drop the way the modality gate does** — a `tracing::warn!` naming the dropped models
and why. The modality version of this message is what makes the behavior debuggable instead of
mysterious, and Jack should be able to read a node's log and see "not advertising qwen3.8:27b:
needs ~16 GB, this node has 16 GB total (12 GB after headroom)."

**Do not** apply the empty-list fallback pattern here. `modalities` falls back to
`vec![Modality::Text]` when everything is dropped; the equivalent for models would be inventing
a model the node doesn't have. An empty `models` list is the correct, honest answer for a node
whose backends only serve models too large for it.

---

## 4. The case I want an explicit decision on

**What happens when size is unknown** — backend reported `None` and the id parser didn't match.

Fail open (advertise it) and we keep the current bug for any model with an unusual name. Fail
closed (drop it) and a naming-convention miss silently removes a model the node can run
perfectly well.

My recommendation: **fail open, but warn loudly and count it.** A node advertising a model it
cannot run is a slow failure we can see in the logs; a node silently refusing to advertise
models it can run is invisible capacity loss, and we would find it by wondering why the fleet
got slower. Make the unknown-size case a `tracing::warn!` with the model id, so the first time
a real model hits it we learn the parser needs a case rather than never hearing about it.

Overrule me if the backends turn out to report size reliably enough that unknown-size means
"something is actually wrong."

---

## 5. Tests worth having

1. 27B q4_k_m against a 16 GB node → dropped. The literal Jotunheim case; assert on it by name.
2. 8B q4 against the same 16 GB node → kept. The gate must not be a blanket "small nodes get
   nothing."
3. Discrete-GPU node: gate uses `vram_bytes`, not `ram_bytes`.
4. Unknown size → kept, and the warning fires (assert the warning, not just the keep — the
   warning is the entire safety net for §4).
5. All models dropped → empty list, **no** synthetic fallback entry.
6. A `ModelRef` serialized without `size_bytes` deserializes (the 0.3.0-node case).

---

## 6. Out of scope, deliberately

`Requirements.min_vram_bytes` already exists, so the *coordinator* can gate when a card declares
its needs. That is the other half of this and it is a separate change: this spec makes the node
stop claiming to do what it can't, which is the half that fixes Jotunheim. Card-side declaration
can follow, and it should not hold this up.

---

Loki
