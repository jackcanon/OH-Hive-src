#!/usr/bin/env python3
"""Create one cloud code card, watch it, and verify the result on disk.

The first end-to-end cloud card (2026-09-16, card ec29f8e0) was run by hand: six SQL
queries, three SSH sessions, and a lot of squinting. This turns that into one command, so
testing "can the Den build?" is cheap enough to do repeatedly -- which is the only way it
gets done repeatedly.

    export DATABASE_URL=postgres://...           # the Hive hub
    python3 scripts/cloud_card.py --task "..." --workspace /Users/jack/scratch/thing
    python3 scripts/cloud_card.py --task "..." --workspace ... --expect src/math.rs --expect-text "pub fn add"

It creates the card via hive.code_session_create_for, polls hive.cards until it leaves
'doing', then prints the node's final report, the recorded usage, and -- if --expect was
given -- whether the files it was supposed to produce are actually there.

    --expect PATH            file that must exist under the workspace afterwards
    --expect-text STRING     string that must appear in one of the --expect files
    --should-fail            invert the verdict: the card is SUPPOSED to fail or fall short

`--should-fail` exists because a gate only ever observed passing has not been tested. Use it
for the negative case (a task the agent cannot satisfy) and it exits 0 when the card does
NOT produce the expected result.

Checking the filesystem matters more than it sounds: a code card's final text is written by
the model, about its own work. Believing it is how you end up with a green card and an empty
directory. This reads the disk.

Requires psycopg (pip install psycopg[binary]) and, for remote workspaces, ssh access.
"""
import argparse, json, os, subprocess, sys, time, uuid

POLL_SECONDS = 2.0
TERMINAL = {"review", "done", "blocked", "failed", "todo"}


def q(conn, sql, params=None, fetch=True):
    with conn.cursor() as cur:
        cur.execute(sql, params or ())
        return cur.fetchall() if fetch else None


def read_remote(host, path):
    """cat a file, locally or over ssh. Returns None if it isn't there."""
    cmd = (["ssh", "-n", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", host, f"cat {path!r}"]
           if host else ["cat", path])
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.stdout if p.returncode == 0 else None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--task", required=True)
    ap.add_argument("--workspace", required=True, help="absolute path on the node that claims it")
    ap.add_argument("--member", default=os.environ.get("HIVE_MEMBER_ID"))
    ap.add_argument("--project", default=os.environ.get("HIVE_PROJECT_ID"),
                    help="must be an execution_mode='local' project -- code cards are refused otherwise")
    ap.add_argument("--brain", default="anthropic")
    ap.add_argument("--model", default=None)
    ap.add_argument("--max-turns", type=int, default=6,
                    help="the ONLY spend bound today: nothing meters a cloud session yet, and each "
                         "turn resends a growing transcript, so cost per turn climbs. Keep it low.")
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--ssh", default=None, metavar="USER@HOST",
                    help="read the workspace over ssh instead of locally")
    ap.add_argument("--expect", action="append", default=[], metavar="PATH")
    ap.add_argument("--expect-text", action="append", default=[], metavar="STRING")
    ap.add_argument("--should-fail", action="store_true")
    a = ap.parse_args()

    dsn = os.environ.get("DATABASE_URL")
    if not dsn:
        print("DATABASE_URL is not set", file=sys.stderr); return 2
    if not a.member or not a.project:
        print("--member and --project are required (or HIVE_MEMBER_ID / HIVE_PROJECT_ID)", file=sys.stderr)
        return 2
    try:
        import psycopg
    except ImportError:
        print("needs psycopg:  pip install 'psycopg[binary]'", file=sys.stderr); return 2

    with psycopg.connect(dsn, autocommit=True) as conn:
        row = q(conn, """
            select hive.code_session_create_for(
              p_member := %s::uuid, p_project_id := %s::uuid, p_task := %s,
              p_workspace_path := %s, p_brain := %s, p_model_id := %s,
              p_max_turns := %s, p_cloud_consent := true)
        """, (a.member, a.project, a.task, a.workspace, a.brain, a.model, a.max_turns))[0][0]
        card_id = row["card_id"] if isinstance(row, dict) else json.loads(row)["card_id"]
        print(f"card      {card_id}")
        print(f"brain     {a.brain}  max_turns={a.max_turns}")
        print(f"workspace {a.workspace}{f'  (via {a.ssh})' if a.ssh else ''}")
        print("waiting…", flush=True)

        started, status, last = time.time(), None, None
        while time.time() - started < a.timeout:
            status = q(conn, "select status from hive.cards where id=%s", (card_id,))[0][0]
            if status != last:
                print(f"  {time.time()-started:6.1f}s  {status}", flush=True)
                last = status
            if status in TERMINAL and status != "todo":
                break
            time.sleep(POLL_SECONDS)
        else:
            print(f"\nTIMEOUT after {a.timeout}s, last status {status}")
            return 1

        out = q(conn, """
            select o.content, o.usage, n.display_name
            from hive.card_outputs o left join hive.nodes n on n.id = o.node_id
            where o.card_id = %s order by o.created_at desc limit 1
        """, (card_id,))
        elapsed = time.time() - started
        print(f"\nfinished in {elapsed:.1f}s with status '{status}'")
        if out:
            content, usage, node = out[0]
            print(f"node      {node}")
            print(f"usage     {json.dumps(usage)}")
            if isinstance(usage, dict) and not any(usage.get(k) for k in ("tokens_in", "tokens_out")):
                print("          ^ zeros are expected today: the Edge Function returns token counts and")
                print("            CloudBrain drops them. Nothing meters a cloud session yet.")
            print("\n--- the node's own report (written by the model, about itself) ---")
            print(content.strip()[:1500])
        else:
            print("no card_outputs row -- the session failed before reporting")

    if not a.expect:
        print("\nno --expect given, so nothing was verified on disk")
        return 0

    print("\n--- what is actually on disk ---")
    ok, blobs = True, []
    for rel in a.expect:
        path = f"{a.workspace.rstrip('/')}/{rel}"
        body = read_remote(a.ssh, path)
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
    verdict = "as expected" if a.should_fail else "verified"
    print(f"\n{'PASS' if passed else 'FAIL'}  ({'card fell short ' + verdict if a.should_fail else 'card produced what it claimed' if ok else 'card claimed more than it produced'})")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
