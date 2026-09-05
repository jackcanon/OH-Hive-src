# Joining the Hive with a machine

Three commands. Works on macOS, Linux (including Raspberry Pi), and Windows.

## 1. Install

```sh
curl -fsSL https://ohghive.com/install.sh | sh
```

Windows: download `ohhive-<version>-x86_64-pc-windows-msvc.zip` from the releases page and put `hive.exe` somewhere on your PATH.

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

macOS and Windows get this from the OH Hive desktop app (in progress); until then `nohup hive work &` works.

## Useful

```sh
hive status                  # what the hub thinks of this node
hive probe                   # hardware + models it would advertise
hive models                  # models the backend can see
hive set HIVE_REGION us-west # optional; region is guessed from IP otherwise
hive check-out               # stop taking cards without stopping the process
```

Config lives in `~/.config/ohhive/node.env` (mode 0600).

## What you're agreeing to

You provide compute and earn $honey per token generated. You claim no rights in project outputs, and you agree not to redistribute owner-only material you can see inside the Hive. Full text is on the pairing page.
