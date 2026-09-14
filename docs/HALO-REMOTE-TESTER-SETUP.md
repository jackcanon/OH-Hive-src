# Project Halo — remote tester setup (Test 04: real WAN worker)

*Written 2026-09-10. For a volunteer outside the lab lending a machine as a shard-holder.*

## What you're signing up for

Your computer holds a slice of a large language model (a few gigabytes) while a machine in the
lab runs the model across the network. You'll download that slice once per test (minutes), then
your machine does a small amount of GPU work per generated token. Nothing on your machine is
read or changed beyond the folder you set up; the lab sees only what your worker process prints.
One honest caveat: a machine holding a slice of the model can technically observe intermediate
data from the prompt, so test prompts are deliberately public and boring ("write a history of
Sydney"). We are not sending anything private through your machine.

## What you need

- **Hardware**: Apple Silicon Mac with 16 GB+ (M1 Pro or better), or Linux/Windows with an
  NVIDIA card (8 GB+ VRAM). A 2021 M1 Pro laptop works — it was in the 70B pool on 2026-09-10.
- **Network**: wired Ethernet if at all possible. Wi-Fi added 17–150 ms of jitter in the lab and
  silently wrecked runs. Any home internet is fine for latency; download speed matters for how
  long the slice takes to arrive (8 GB at 50 Mbit/s ≈ 21 min, at 200 Mbit/s ≈ 5 min).
- **Tailscale**: free account, join the `halo` tailnet from the invite link Jack sends. This is
  the only way the lab reaches your machine — nothing is opened on your router.
- **The worker binary**: `ggml-rpc-server` from llama.cpp tag `b10883`, built with RPC enabled.
  Jack will send a prebuilt one for your platform (macOS arm64 from the lab's `~/halo/bin`;
  Linux CUDA from Heimdall's `build-cuda`), or build it yourself:
  ```bash
  git clone --branch b10883 --depth 1 https://github.com/ggml-org/llama.cpp
  cd llama.cpp
  # macOS:
  cmake -B build -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON -DGGML_RPC=ON -DGGML_RPC_RDMA=OFF -DLLAMA_CURL=OFF -DBUILD_SHARED_LIBS=OFF
  # Linux + NVIDIA:
  cmake -B build -DGGML_CUDA=ON -DGGML_RPC=ON -DLLAMA_CURL=OFF
  cmake --build build --config Release -j --target ggml-rpc-server
  ```

## Running the worker (the only command you run per test)

Find your Tailscale IP (`tailscale ip -4`, looks like `100.x.y.z`), then:

```bash
# macOS
./ggml-rpc-server -H <your-tailscale-ip> -p 50052 -d MTL0
# Linux + NVIDIA
./ggml-rpc-server -H <your-tailscale-ip> -p 50052 -d CUDA0
```

Leave it running in a terminal you can see. It prints a warning about not exposing the server to
an open network — that's why it's bound to the Tailscale address only. It accepts one client at
a time and doesn't notice if the lab's side crashes, so **restart it before each test** when Jack
says go. When you're done, Ctrl-C and you're out; it holds nothing on disk.

Things that bit us in the lab, so you don't repeat them: the `-d` device flag is required (without
it the worker silently runs on CPU and crashes); bind to the Tailscale IP, not `0.0.0.0`; turn
Wi-Fi off if you're wired; on a Mac, close anything memory-hungry first — a "24 GB" machine with
a browser and Photos open is a ~10 GB machine.

## What Jack sends you back

Your machine's share size, the model, and the two numbers (prompt tokens/s and generation
tokens/s), plus what those look like against the lab's LAN baseline and the latency prediction:
each token costs about 1.1× the one-way delay between you and the lab, so at 40 ms you'll see
roughly +45 ms per token versus LAN. The interesting result is whether that holds on a real link.

---

## Lab side (Loki / Jack)

**Test 04a — cloud box as CPU worker (no volunteer needed).** Chicago (us-central, ~30 ms),
then Sydney (~180 ms). 4 GB RAM, no GPU: install Tailscale on the box and on Overgaard; build
`ggml-rpc-server` CPU-only on the Linode (`cmake -B build -DGGML_RPC=ON -DLLAMA_CURL=OFF`);
run it with `-d CPU` bound to the Tailscale IP; from Overgaard run the **14B Q4** with a
**1.5 GB** share on the remote (`--rpc <ts-ip>:50052 --tensor-split 1.5/7 -lm none`). Compare
the measured ms/token delta against `1.1 × one-way RTT`. Also time the 1.5 GB upload — that's
the WAN shard-distribution number on a real path.

**Test 04b — volunteer GPU worker.** Same 14B first with a 2–3 GB share (fast to ship), then
scale the share to what their machine holds, then the 32B if their download speed makes a 10 GB
shard bearable. Overgaard hosts; Odin stays in as a LAN worker so the pool looks like the real
fleet (mixed LAN + WAN).

**HaloBench changes needed** (small): a per-host `managed: false` flag so the app skips the SSH
restart for machines it can't reach by SSH and just waits for the port; `wiredIP` becomes
"reachable IP" (a Tailscale address is fine). Report placement string should say `(WAN)` for
those workers. Add Test 04a/04b presets.

**Tailscale on Overgaard**: install once (`brew install tailscale` or the App Store app), join the
same tailnet. Nothing else in the lab needs it; workers on the LAN keep their wired IPs.

**Prediction to grade** (write it down first, per the doc's habit): 14B two-way with a 1.5 GB
share on a ~30 ms path lands near 26–30 tok/s decode on Overgaard (vs 39 solo: small share, but
one ~33 ms round trip per token); the 1.5 GB upload takes 1–3 min depending on the Linode's
inbound bandwidth. Sydney at ~180 ms: ~5 tok/s — the "far member" floor.
