"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { friendlyError } from "@/lib/errors";

// Release notes on login (#178). Jack, mid-session: "we also need to make sure when we are
// updating now that we have a couple of users we need to make sure when users login after an
// update there should be release notes." Mounted once in the root layout (same idea as
// MembersSidebar) so it shows on every route without every page opting in -- it quietly renders
// nothing until there's an actual signed-in session, then fetches whatever this member hasn't
// seen yet via hive_release_notes_unseen. Dismissing calls hive_release_notes_mark_seen, which
// moves the member's watermark server-side -- so this is "once, ever" per note across every
// device/browser this member signs into, not a per-browser localStorage flag.

type ReleaseNote = {
  seq: number;
  version: string;
  title: string;
  body_md: string;
  published_at: string;
};

export function ReleaseNotes() {
  const [notes, setNotes] = useState<ReleaseNote[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    const sb = supabaseBrowser();
    let fetched = false;
    const maybeLoad = (signedIn: boolean) => {
      if (!signedIn || fetched) return;
      fetched = true;
      sb.rpc("hive_release_notes_unseen", {}).then(({ data, error }) => {
        if (error) return; // signed out / not a member yet -- nothing to show
        const unseen = (data ?? []) as ReleaseNote[];
        if (unseen.length > 0) setNotes(unseen);
      });
    };
    sb.auth.getSession().then(({ data }) => maybeLoad(!!data.session));
    const { data: sub } = sb.auth.onAuthStateChange((_e, s) => maybeLoad(!!s));
    return () => sub.subscription.unsubscribe();
  }, []);

  async function dismiss() {
    setBusy(true);
    setErr(null);
    const { error } = await supabaseBrowser().rpc("hive_release_notes_mark_seen", {});
    setBusy(false);
    if (error) { setErr(friendlyError(error.message)); return; }
    setNotes(null);
  }

  if (!notes || notes.length === 0) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="What's new"
      style={{
        position: "fixed", inset: 0, background: "rgba(0, 0, 0, 0.55)", zIndex: 1000,
        display: "flex", alignItems: "center", justifyContent: "center", padding: 24,
      }}
    >
      <div
        style={{
          background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 10,
          padding: 24, maxWidth: 520, width: "100%", maxHeight: "80vh", overflowY: "auto",
        }}
      >
        <h2 style={{ margin: "0 0 4px" }}>What&rsquo;s new</h2>
        <p style={{ color: "var(--muted-strong)", fontSize: 13, margin: "0 0 18px" }}>
          Here&rsquo;s what&rsquo;s changed in the Hive since you last signed in.
        </p>
        {notes.map((n) => (
          <div key={n.seq} style={{ marginBottom: 18, paddingBottom: 18, borderBottom: "1px solid var(--border)" }}>
            <h3 style={{ margin: "0 0 6px", fontSize: 15 }}>
              {n.title} <span style={{ fontWeight: 400, fontSize: 12, color: "var(--muted)" }}>v{n.version}</span>
            </h3>
            <p style={{ margin: 0, fontSize: 13, color: "var(--muted-strong)", whiteSpace: "pre-wrap" }}>{n.body_md}</p>
          </div>
        ))}
        {err && <p style={{ color: "var(--danger)", fontSize: 13 }}>{err}</p>}
        <button onClick={dismiss} disabled={busy} style={{ padding: "8px 16px", cursor: "pointer" }}>
          {busy ? "…" : "Got it"}
        </button>
      </div>
    </div>
  );
}
