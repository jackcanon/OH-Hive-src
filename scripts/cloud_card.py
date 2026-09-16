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
    --check 'NAME=PROG ARG...'   an acceptance check the HOST runs after the model finishes
    --check-json '{...}'         the same thing with every field available
    --check-advisory 'NAME=...'  recorded in the receipt, never fails the card
    --expect-acceptance STATUS   require the receipt to say passed/failed/errored/unverified

`--should-fail` exists because a gate only ever observed passing has not been tested. The failing
run has to be as easy to launch as the passing one, or nobody launches it.

ACCEPTANCE CHECKS are the point of `--check`, and they are a different kind of evidence from
`--expect`. `--expect` is this script looking at the disk afterwards; a check is a command the NODE
runs, inside the lease, whose failure fails the card through `fail_card` (ADR-019 rules layer,
crates/ohhive-core/src/coder/acceptance.rs). Before this existed the harness could only submit cards
with no checks, which are `Unverified` by design and fail nothing -- so the gate shipped
unexercisable. Checks are declared in the card's `required_capabilities.acceptance` at creation and
also shown to the model up front, so it knows what it will be judged by.

Commands run directly, with NO SHELL. `--check 'tests=cargo test --quiet'` splits on whitespace into
a program and its arguments; pipes, redirects, globs and `&&` are not interpreted and an argument
containing spaces needs `--check-json`. That is the host's rule, not this script's: `shell` is a
rejected field on a check.

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
# The host appends this line to the card report because complete_card/fail_card persist report TEXT,
# not ToolOutcome.data (crates/ohhive-core/src/tools.rs:357 via AcceptanceOutcome::receipt). So the
# only way a node-key caller can see what the checks did is to find this line and parse it.
RECEIPT_PREFIX = "Acceptance checks:"
ACCEPTANCE_STATES = {"passed", "failed", "errored", "unverified", "skipped"}


def parse_check(spec, required=True):
    """'NAME=PROG ARG ARG' -> a check dict. No shell: the split is whitespace, nothing else."""
    name, sep, rest = spec.partition("=")
    if not sep or not name.strip() or not rest.split():
        raise argparse.ArgumentTypeError(
            f"--check wants 'NAME=PROGRAM [ARGS...]', got {spec!r}"
            " (use --check-json for an argument containing spaces)")
    prog, *args = rest.split()
    check = {"name": name.strip(), "command": prog}
    if args:
        check["args"] = args
    if not required:
        check["required"] = False
    return check


def parse_check_json(spec):
    try:
        check = json.loads(spec)
    except json.JSONDecodeError as e:
        raise argparse.ArgumentTypeError(f"--check-json is not JSON: {e}") from e
    if not isinstance(check, dict):
        raise argparse.ArgumentTypeError("--check-json wants one object per flag, repeated")
    return check


def receipt(report):
    """Pull the acceptance receipt out of a card report. Returns None when there isn't one."""
    for line in str(report).splitlines():
        if line.startswith(RECEIPT_PREFIX):
            try:
                return json.loads(line[len(RECEIPT_PREFIX):].strip())
            except json.JSONDecodeError:
                return None
    return None


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


def compute_verdict(expectations_met, should_fail):
    """Determine pass/fail verdict and explanatory note.
    
    Args:
        expectations_met: bool, whether all --expect checks passed
        should_fail: bool, whether --should-fail was passed
    
    Returns:
        tuple of (passed: bool, note: str)
    """
    passed = (not expectations_met) if should_fail else expectations_met
    if should_fail:
        note = "card fell short, as the negative case expects" if not expectations_met else "card SUCCEEDED but was expected to fall short"
    else:
        note = "card produced what it claimed" if expectations_met else "card claimed more than it produced"
    return passed, note


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
                    help="the per-session bound. Cloud turns are metered and capped per month as of "
                         "20260916060000, but that ceiling is monthly, not per card -- and each turn "
                         "resends a growing transcript, so cost per turn climbs. Keep it low.")
    ap.add_argument("--node-key", default=None)
    ap.add_argument("--hub", default=os.environ.get("HIVE_HUB_URL", DEFAULT_HUB))
    ap.add_argument("--anon", default=os.environ.get("HIVE_ANON_KEY", DEFAULT_ANON))
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--ssh", default=None, metavar="USER@HOST")
    ap.add_argument("--expect", action="append", default=[], metavar="PATH")
    ap.add_argument("--expect-text", action="append", default=[], metavar="STRING")
    ap.add_argument("--should-fail", action="store_true")
    ap.add_argument("--check", action="append", default=[], metavar="NAME=PROG ARG...",
                    help="required acceptance check; repeatable. No shell: whitespace-split.")
    ap.add_argument("--check-advisory", action="append", default=[], metavar="NAME=PROG ARG...",
                    help="acceptance check recorded in the receipt but never failing the card")
    ap.add_argument("--check-json", action="append", default=[], metavar="JSON",
                    help="one check as a JSON object -- for cwd, expect_exit, or args with spaces")
    ap.add_argument("--expect-acceptance", choices=sorted(ACCEPTANCE_STATES), default=None,
                    help="require the card's acceptance receipt to report this status")
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

    checks = ([parse_check(s) for s in a.check]
              + [parse_check(s, required=False) for s in a.check_advisory]
              + [parse_check_json(s) for s in a.check_json])

    payload = {
        "p_raw_key": key, "p_project_id": a.project, "p_task": a.task,
        "p_workspace_path": a.workspace, "p_brain": a.brain, "p_model_id": a.model,
        "p_max_turns": a.max_turns, "p_cloud_consent": True}
    if checks:
        # PostgREST picks between the 12- and 13-argument overloads of
        # `hive_code_session_create_node` by the exact set of argument NAMES in the body, so sending
        # `p_acceptance` only when there are checks keeps a no-check submission on the original
        # function -- and keeps its `required_capabilities` byte-identical to every card created
        # before 20260916070000, which is what `p_request_id` idempotency compares.
        payload["p_acceptance"] = checks
        payload.setdefault("p_repo_url", None)
        payload.setdefault("p_repo_ref", None)
        payload.setdefault("p_request_id", None)
        payload.setdefault("p_coordinator", False)
    created = rpc(a.hub, a.anon, "hive_code_session_create_node", payload)
    card_id = (created or {}).get("card_id") if isinstance(created, dict) else created
    if not card_id:
        print(f"no card id came back: {created!r}", file=sys.stderr); return 1

    print(f"card       {card_id}")
    print(f"brain      {a.brain}   max_turns={a.max_turns}")
    if checks:
        for c in checks:
            line = " ".join([c.get("command", "?")] + list(c.get("args") or []))
            where = f"  cwd={c['cwd']}" if c.get("cwd") else ""
            exit_note = f"  expect_exit={c['expect_exit']}" if c.get("expect_exit") else ""
            kind = "required" if c.get("required", True) else "advisory"
            print(f"check      {c.get('name','?')}: {line}{where}{exit_note}  ({kind})")
    else:
        print("check      none -- the card will report 'unverified', which fails nothing. "
              "Pass --check to exercise the gate.")
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
    acceptance = None
    if isinstance(st, dict):
        report = st.get("latest_output") or ""
        # `usage` and `model_id` arrived with 20260916060000. For a CODE card they read zero until
        # BrainTurn carries the counts (Cmd Work f0c2b6d8), so this prints what it was given without
        # dressing it up -- `hive_code_usage_node` is where a real spend figure lives today.
        used = st.get("usage") or {}
        if st.get("model_id") or used:
            tin, tout = used.get("tokens_in"), used.get("tokens_out")
            zero = "" if (tin or tout) else "   (zeros: the node reports Usage::default() for code cards)"
            print(f"model      {st.get('model_id') or '?'}   tokens_in={tin} tokens_out={tout}{zero}")
        acceptance = receipt(report)
        if acceptance:
            status = acceptance.get("status", "?")
            print(f"\n--- acceptance receipt: {status.upper()} "
                  "(the HOST ran these, not the model) ---")
            for r in acceptance.get("results") or []:
                verdict = "pass" if r.get("passed") else ("TIMEOUT" if r.get("timed_out") else "FAIL")
                tag = "" if r.get("required", True) else " advisory"
                print(f"  {verdict:>7}{tag}  {r.get('name')}: {r.get('command_line')}"
                      f"  exit={r.get('exit_status')}")
                if not r.get("passed"):
                    for stream in ("stderr_tail", "stdout_tail"):
                        tail = (r.get(stream) or "").strip()
                        if tail:
                            print(f"            {stream}: {tail.splitlines()[-1][:160]}")
                    if r.get("error"):
                        print(f"            error: {r['error']}")
        elif checks:
            print("\nNO ACCEPTANCE RECEIPT, though checks were submitted -- the session stopped "
                  "before checks could run (turn limit, expired lease, or waiting on a child), "
                  "or this node predates the acceptance build.")
        if report:
            print("\n--- the node's own report (written by the model, about itself) ---")
            print(str(report).strip()[:1500])
        else:
            print("(no report recorded -- the session failed before reporting)")

    # The receipt assertion comes first and stands on its own: it is the only evidence here that
    # does not depend on this script's own view of the filesystem.
    if a.expect_acceptance:
        got = (acceptance or {}).get("status")
        if got != a.expect_acceptance:
            print(f"\nFAIL  (expected acceptance '{a.expect_acceptance}', "
                  f"receipt says {got or 'nothing -- no receipt'})")
            return 1
        print(f"\nacceptance receipt says '{got}', as expected")

    if not a.expect:
        if a.expect_acceptance:
            return 0
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

    passed, note = compute_verdict(ok, a.should_fail)
    print(f"\n{'PASS' if passed else 'FAIL'}  ({note})")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
