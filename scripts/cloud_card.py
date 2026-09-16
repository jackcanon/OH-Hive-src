#!/usr/bin/env python3
"""Create one cloud code card, watch it, and verify the result on disk.

The first end-to-end cloud card (2026-09-16, card ec29f8e0) was run by hand: six SQL queries,
three ssh sessions, and a lot of squinting. This turns that into one command, so asking "can the
Den build?" is cheap enough to ask repeatedly -- which is the only way it gets asked repeatedly.

    python3 scripts/cloud_card.py --task "..." --workspace /Users/jack/scratch/thing \
        --ssh jack@100.80.147.109 --expect src/math.rs --expect-text "pub fn add"

Authentication is a NODE KEY, the same credential `hive` already uses -- read from
~/.config/ohhive/node.env by default. No DATABASE_URL, no service-role key, no pip install:
stdlib urllib against the hub's public RPCs (hive_code_session_create_node /
hive_code_session_status_node). The first version of this script needed psycopg and a DSN,
neither of which exists on the machine it was written for, so it could not have run at all.

    --expect PATH         file that must exist under the workspace afterwards
    --expect-text STRING  string that must appear in one of the --expect files
    --should-fail         invert the verdict: the card is SUPPOSED to fall short

`--should-fail` exists because a gate only ever observed passing has not been tested. The failing
run has to be as easy to launch as the passing one, or nobody launches it.

Reading the filesystem matters more than it sounds: a code card's final report is written by the
model, about its own work. Believing it is how you get a green card and an empty directory.
"""
import argparse, json, os, subprocess, sys, time, urllib.error, urllib.request
from pathlib import Path

# Same defaults the node CLI compiles in (crates/ohhive-core/src/nodeconfig.rs). The anon key is a
# publishable key -- it identifies the project, it does not authorise anything; the node key does.
DEFAULT_HUB = "https://pxfbnuxcnerulbvbmowz.supabase.co"
DEFAULT_ANON = "sb_publishable_VjfocwhBAykEFEllo6U3RQ_e-BdMcme"
# The node config dir is platform-specific, because the Rust side uses the OS convention rather
# than a single hardcoded path: macOS nodes keep it under ~/Library/Application Support/ohhive/,
# Linux nodes under ~/.config/ohhive/. Hardcoding the Linux path (the first version of this
# script) makes it fail on exactly the Macs that run most of the fleet's code cards.
NODE_ENV_CANDIDATES = [
    Path.home() / "Library" / "Application Support" / "ohhive" / "node.env",   # macOS
    Path.home() / ".config" / "ohhive" / "node.env",                            # Linux / XDG
]
POLL_SECONDS = 2.0
DONE_STATES = {"review", "done", "blocked", "failed"}


def node_key(explicit):
    if explicit:
        return explicit
    if os.environ.get("HIVE_NODE_KEY"):
        return os.environ["HIVE_NODE_KEY"]
    for env in NODE_ENV_CANDIDATES:
        if not env.exists():
            continue
        for line in env.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("export "):
                line = line[len("export "):]
            if line.startswith("HIVE_NODE_KEY="):
                return line.split("=", 1)[1].strip().strip("\"'")
    return None


def rpc(hub, anon, name, payload):
    req = urllib.request.Request(
        f"{hub}/rest/v1/rpc/{name}",
        data=json.dumps(payload).encode(),
        headers={"content-type": "application/json", "apikey": anon,
                 "Authorization": f"Bearer {anon}"},
        method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            body = r.read().decode()
            return json.loads(body) if body.strip() else None
    except urllib.error.HTTPError as e:
        detail = e.read().decode()[:400]
        raise SystemExit(f"{name} failed: HTTP {e.code}\n{detail}")


def read_file(host, path):
    """cat a file, locally or over ssh. None when it isn't there."""
    cmd = (["ssh", "-n", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", host, f"cat {path!r}"]
           if host else ["cat", path])
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.stdout if p.returncode == 0 else None


def main() -> int:
    ap = argparse.ArgumentParser()
    # Not `required=True`: --list-projects has to work on its own, and argparse would
    # reject it before we ever reach that branch. Checked by hand below instead.
    ap.add_argument("--task")
    ap.add_argument("--workspace", help="absolute path on the node that claims it")
    ap.add_argument("--project", default=os.environ.get("HIVE_PROJECT_ID"),
                    help="must be an execution_mode='local' project; code cards are refused otherwise")
    ap.add_argument("--brain", default="anthropic")
    ap.add_argument("--model", default=None)
    ap.add_argument("--max-turns", type=int, default=6,
                    help="the ONLY spend bound today: nothing meters a cloud session yet, and each "
                         "turn resends a growing transcript, so cost per turn climbs. Keep it low.")
    ap.add_argument("--node-key", default=None)
    ap.add_argument("--hub", default=os.environ.get("HIVE_HUB_URL", DEFAULT_HUB))
    ap.add_argument("--anon", default=os.environ.get("HIVE_ANON_KEY", DEFAULT_ANON))
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--ssh", default=None, metavar="USER@HOST")
    ap.add_argument("--expect", action="append", default=[], metavar="PATH")
    ap.add_argument("--expect-text", action="append", default=[], metavar="STRING")
    ap.add_argument("--should-fail", action="store_true")
    ap.add_argument("--list-projects", action="store_true",
                    help="print the projects this key may create code sessions in, and exit")
    a = ap.parse_args()

    key = node_key(a.node_key)
    if not key:
        looked = "\n  ".join(str(p) for p in NODE_ENV_CANDIDATES)
        print("no node key: pass --node-key, set HIVE_NODE_KEY, or run this on a paired node.\n"
              f"  looked in:\n  {looked}", file=sys.stderr)
        return 2

    if a.list_projects:
        for p in rpc(a.hub, a.anon, "hive_code_session_projects_node", {"p_raw_key": key}) or []:
            print(f"{p.get('id')}  {p.get('title','')[:70]}")
        return 0
    missing = [f"--{n}" for n, v in (("task", a.task), ("workspace", a.workspace),
                                      ("project", a.project)) if not v]
    if missing:
        print(f"missing {', '.join(missing)}"
              + ("  (--list-projects shows the eligible projects)" if "--project" in missing else ""),
              file=sys.stderr)
        return 2

    created = rpc(a.hub, a.anon, "hive_code_session_create_node", {
        "p_raw_key": key, "p_project_id": a.project, "p_task": a.task,
        "p_workspace_path": a.workspace, "p_brain": a.brain, "p_model_id": a.model,
        "p_max_turns": a.max_turns, "p_cloud_consent": True})
    card_id = (created or {}).get("card_id") if isinstance(created, dict) else created
    if not card_id:
        print(f"no card id came back: {created!r}", file=sys.stderr); return 1

    print(f"card       {card_id}")
    print(f"brain      {a.brain}   max_turns={a.max_turns}")
    print(f"workspace  {a.workspace}{f'   (reading via {a.ssh})' if a.ssh else ''}")
    print("waiting…", flush=True)

    started, last, state = time.time(), None, None
    while time.time() - started < a.timeout:
        st = rpc(a.hub, a.anon, "hive_code_session_status_node",
                 {"p_raw_key": key, "p_card_id": card_id}) or {}
        state = st.get("status") if isinstance(st, dict) else st
        if state != last:
            print(f"  {time.time()-started:6.1f}s  {state}", flush=True)
            last = state
        if state in DONE_STATES:
            break
        time.sleep(POLL_SECONDS)
    else:
        print(f"\nTIMEOUT after {a.timeout}s, last status {state}")
        return 1

    elapsed = time.time() - started
    print(f"\nfinished in {elapsed:.1f}s with status '{state}'")
    st = rpc(a.hub, a.anon, "hive_code_session_status_node",
             {"p_raw_key": key, "p_card_id": card_id}) or {}
    if isinstance(st, dict):
        # `hive_code_session_status_node` returns card_id/project_id/status/title/key/created_at
        # and `latest_output` -- the newest card_outputs.content. It does NOT return usage, so a
        # node-key caller cannot see what a session cost even once CloudBrain stops dropping the
        # token counts. Worth fixing on the same pass; noted rather than faked here.
        report = st.get("latest_output") or ""
        if report:
            print("\n--- the node's own report (written by the model, about itself) ---")
            print(str(report).strip()[:1500])
        else:
            print("(no report recorded -- the session failed before reporting)")

    if not a.expect:
        print("\nno --expect given, so nothing was verified on disk")
        return 0

    print("\n--- what is actually on disk ---")
    ok, blobs = True, []
    for rel in a.expect:
        body = read_file(a.ssh, f"{a.workspace.rstrip('/')}/{rel}")
        if body is None:
            print(f"  MISSING  {rel}"); ok = False
        else:
            print(f"  present  {rel}  ({len(body)} bytes)"); blobs.append(body)
    for needle in a.expect_text:
        if any(needle in b for b in blobs):
            print(f"  found    {needle!r}")
        else:
            print(f"  ABSENT   {needle!r}"); ok = False

    passed = (not ok) if a.should_fail else ok
    if a.should_fail:
        note = "card fell short, as the negative case expects" if not ok else "card SUCCEEDED but was expected to fall short"
    else:
        note = "card produced what it claimed" if ok else "card claimed more than it produced"
    print(f"\n{'PASS' if passed else 'FAIL'}  ({note})")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
