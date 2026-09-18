# Joining the Hive with a machine

Three commands. Works on macOS, Linux (including Raspberry Pi), and Windows.

## The app, if you'd rather not use a terminal

Everything is on **[ohghive.com/download](https://ohghive.com/download)**, which always lists the
current release — prefer it over a hardcoded link here, which is how the three dead ones this
section used to carry got there.

- **macOS (Apple Silicon)** — `Hive-Swift-<version>-macos-aarch64.dmg`. Drag it to Applications and
  open it. Until the app is notarized macOS asks you to confirm the first launch (right-click →
  Open). On an **Intel Mac** there is no app build; use the CLI below, which is the same node.
- **Linux (x86_64)** — `Hive-<version>-linux-x86_64.deb` if you're on Debian or Ubuntu and want
  your package manager to track it, or `Hive-<version>-linux-x86_64.AppImage` anywhere else
  (`chmod +x` it and run it — nothing to install). New in v0.4.1.
- **Windows** — CLI only for now. See below.

The app pairs with a code, starts and stops the worker, picks the model, and shows what the node
earns. Everything below works alongside it.

## 1. Install

```sh
curl -fsSL https://ohghive.com/install.sh | sh
```

This is the whole node — the app is the same thing with a window on it. Works on macOS (Intel and
Apple Silicon), Linux, and Raspberry Pi.

Binaries come from **[github.com/jackcanon/ohhive-releases/releases](https://github.com/jackcanon/ohhive-releases/releases)**
(checksummed in `SHA256SUMS`; no account or token needed).

Windows: download `hive-<version>-x86_64-pc-windows-msvc.zip` from that same releases page and put
`hive.exe` somewhere on your PATH.

## 2. Pair

```sh
hive pair
```

Your machine shows a code like `HK7-3PQ`. Open **ohghive.com/pair**, sign in, enter the code. You'll name the machine and set two trust switches:

- **Internet access** — off by default. Leave it off unless you're happy for projects to reach the web from this machine.
- **Tools** — *sandboxed tools* (default) lets agents use a scratch folder and sandboxed code execution; *inference only* is model-in, tokens-out, nothing else.

The key lands on your machine automatically. You never copy anything.

## 3. Work

```sh
hive work
```

The node checks in, advertises what it can run (it needs Ollama or a `llama-server` on `127.0.0.1:11434` / `:8080` — set `HIVE_LLAMA_URL` otherwise), and starts taking cards. Ctrl-C checks it out cleanly. Earnings show up in your wallet at **ohghive.com/wallet** as cards complete.

Run it as a service so it survives reboots:

```sh
# Linux (systemd user unit)
mkdir -p ~/.config/systemd/user && cp packaging/hive-worker.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now hive-worker
loginctl enable-linger $USER
```

```sh
# macOS (launchd agent — survives logout/reboot, releases its card cleanly on stop)
scripts/service-mac.sh worker            # or: curl -fsSL https://ohghive.com/service-mac.sh | sh -s worker
```

Windows gets this from the Hive desktop app (in progress); until then `hive work` in a terminal works.

## Useful

```sh
hive status                  # what the hub thinks of this node
hive probe                   # hardware + models it would advertise
hive models                  # models the backend can see
hive set HIVE_REGION us-west # optional; region is guessed from IP otherwise
hive check-out               # stop taking cards without stopping the process
```

Config lives in `~/.config/hive/node.env` on Linux, `~/Library/Application Support/hive/node.env` on macOS (mode 0600). `hive set KEY value` edits it wherever it is.

## What you're agreeing to

You provide compute and earn $honey per token generated. You claim no rights in project outputs, and you agree not to redistribute owner-only material you can see inside the Hive. Full text is on the pairing page.


---

# Running a regional server

Regional servers are the Hive's plumbing: they hold artifacts (the outputs projects make), push
live board updates to the web app, and one of them is elected **coordinator**. They need disk and
uptime, not a GPU — a Raspberry Pi 5, an N100 mini PC, or a retired laptop is ideal. The ask
(ADR-013 §G): 2–4 TB of disk, ≥ 50 Mbps upload, ≥ 95 % uptime, and a way to be reached from the
internet (Cloudflare Tunnel or a public IP). Servers earn `$honey` for bytes stored and served.

## 1. Install and pair

Same installer as a node. Then:

```sh
hive pair
```

On the pairing page choose **Regional server** (or *Compute and server* if the machine will also
run models). Pick a region.

## 2. Make it reachable

**Always prefer a wired (Ethernet) connection over Wi-Fi.** This is the rule for anything other
machines depend on — a regional server, and your own vault hub (`hive hub serve`) alike.

Two reasons, and the second one costs hours if you meet it without knowing it:

1. Wired has no roaming, no power-saving sleep, and no shared airtime. A hub on Wi-Fi drops out
   in ways that look like software faults.
2. If a machine has Ethernet **and** Wi-Fi on the same subnet, it has two addresses but only one
   default route. Bind the address that is *not* on the default route and requests arrive on one
   interface while replies leave by another. Switches and firewalls drop that asymmetry
   unpredictably, so peers see connections that work, then fail, then work — while a one-shot
   command from the very same machine succeeds every time. Nothing in any log names the cause.

So: bind the wired address, and make sure it is the one carrying the default route.

```sh
# macOS — which interface carries the default route, and what address it has
route -n get default | grep interface
ifconfig <that-interface> | grep 'inet '

# Linux
ip route show default
```

On macOS, put Ethernet above Wi-Fi in System Settings → Network → (⋯) → Set Service Order. On a
machine that serves, the simplest reliable answer is to turn Wi-Fi off entirely.

The server listens on `:8790`. Members' browsers and other nodes need to reach it, so give it a
public HTTPS hostname. The standard way is a free Cloudflare Tunnel (no port forwarding, no
certificates):

```sh
# one-time, on the server — the printed URL can be opened in a browser on any machine
cloudflared tunnel login
cloudflared tunnel create <yourname>-hive
cloudflared tunnel route dns <yourname>-hive <yourname>.ohghive.com   # or a hostname on your own zone
```

Write `~/.cloudflared/config.yml` (tunnel id + credentials file from the `create` step, ingress
`<yourname>.ohghive.com → http://localhost:8790`), then run `cloudflared tunnel run` as a service —
`packaging/cloudflared-hive.service` (Linux) has the exact commands in its header. Hostnames under
`ohghive.com` are handed out by a founder (the zone is Happy Jack Media's); your own domain works
just as well.

Then tell the Hive where you are:

```sh
hive set HIVE_PUBLIC_URL https://<yourname>.ohghive.com
```

A machine with a real public IP can skip the tunnel and set `http://<ip>:8790` (HTTPS is strongly
preferred for anything members will open in a browser).

## 3. Run it

```sh
hive-server serve            # foreground; Ctrl-C steps down cleanly
```

As a service:

```sh
# Linux
mkdir -p ~/.config/systemd/user && cp packaging/hive-server.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now hive-server
loginctl enable-linger $USER
# macOS
scripts/service-mac.sh server            # or: curl -fsSL https://ohghive.com/service-mac.sh | sh -s server
```

Options (flags or `hive set HIVE_…`): `--data-dir` (default `~/.local/share/hive/blobs`),
`--storage-gb`, `--region`, `--listen`, `--max-upload-mb`. `--operator hjm` / `--tier standby`
are reserved for Happy Jack Media's standby boxes (ADR-013 §F).

## What it does today

- Registers and heartbeats; shows up in the Hive pulse and `hive.servers()`.
- Stores artifacts: any paired node can `PUT /a` with its node key; anyone can `GET /a/<sha256>`.
  Every blob is announced to the hub so members can locate it.
- Pushes live board updates: the web app opens `wss://<your server>/live/<project>` instead of
  polling the database.
- Competes for the coordinator lease; exactly one server is coordinator at a time, and if it
  disappears another takes over within about two minutes.
- Pulls replicas: every pinned artifact is copied to at least two servers (three for backups).
- HJM-operated coordinators also take the nightly encrypted hub backup — see `docs/BACKUPS.md`.

Coming to the same binary: replication between servers, relay for nodes behind NAT, model-weight
cache, the read-all snapshot, backups.
