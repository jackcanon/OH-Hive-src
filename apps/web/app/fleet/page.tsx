"use client";

// Personal Hive fleet channel (ADR-022 S2, #183). Jack, 2026-09-13: "I want to be able to see the
// receipts, so I think we take Buzz's channel model and run with it." This is that channel: every
// paired machine's own activity (came online, went offline, a card completed or failed) posts here
// automatically (hive.personal_channel_posts, populated by DB triggers + a couple of directly-
// wired RPCs -- see the 2026-09-13 migrations), alongside anything the member types themselves.
// One fleet-wide feed, filterable to a single machine -- same table either way, just a query
// parameter, per the "can we get both" decision recorded in the ADR.
//
// Styled like the project forum's comment thread (hive.project_comments / this repo's other async
// discussion surface) rather than a live chat room -- polling, not websockets, matching this app's
// existing pattern (wallet/page.tsx polls hive_my_wallet the same way).

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember } from "@/components/RequireMember";
import { friendlyError } from "@/lib/errors";

type Post = {
  id: string;
  node_id: string | null;
  node_display: string | null;
  author_kind: "member" | "node" | "assistant";
  event_type: string;
  body: string;
  payload: Record<string, unknown>;
  created_at: string;
};

type NodeOption = { id: string; display_name: string };

function FleetView() {
  const [posts, setPosts] = useState<Post[] | null>(null);
  const [nodes, setNodes] = useState<NodeOption[]>([]);
  const [nodeFilter, setNodeFilter] = useState<string>("");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = () => {
    supabaseBrowser()
      .rpc("hive_personal_channel_list", { p_node_id: nodeFilter || null, p_limit: 200 })
      .then(({ data, error }) => {
        if (error) setErr(friendlyError(error.message)); else setPosts(data as Post[]);
      });
  };

  useEffect(() => {
    // The member's own node list comes along for free on hive_my_wallet -- no need for a second
    // RPC just to populate a filter dropdown.
    supabaseBrowser().rpc("hive_my_wallet", { p_limit: 1 }).then(({ data }) => {
      const w = data as { nodes?: NodeOption[] } | null;
      if (w?.nodes) setNodes(w.nodes.map((n) => ({ id: n.id, display_name: n.display_name })));
    });
  }, []);

  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodeFilter]);

  async function post() {
    const body = draft.trim();
    if (!body) return;
    setBusy(true);
    const { error } = await supabaseBrowser().rpc("hive_personal_channel_post", { p_body: body });
    setBusy(false);
    if (error) { setErr(friendlyError(error.message)); return; }
    setDraft("");
    load();
  }

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: "24px 16px" }}>
      <h1 style={{ marginBottom: 4 }}>Fleet</h1>
      <p style={{ color: "var(--muted-strong)", fontSize: 13, marginBottom: 16 }}>
        Everything your own paired machines are doing, and anything you want to tell them — the receipts for your
        Personal Hive. Nothing here is shared with the community.
      </p>

      {nodes.length > 0 && (
        <div style={{ marginBottom: 12 }}>
          <select value={nodeFilter} onChange={(e) => setNodeFilter(e.target.value)} style={{ padding: 6 }}>
            <option value="">All machines</option>
            {nodes.map((n) => <option key={n.id} value={n.id}>{n.display_name}</option>)}
          </select>
        </div>
      )}

      {err && <p style={{ color: "var(--danger, #c0392b)", fontSize: 13 }}>{err}</p>}

      <div style={{ display: "flex", gap: 8, marginBottom: 16 }}>
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") post(); }}
          placeholder="Post a message to your fleet…"
          style={{ flex: 1, padding: 8 }}
        />
        <button onClick={post} disabled={busy || !draft.trim()} style={{ padding: "8px 14px", cursor: "pointer" }}>Post</button>
      </div>

      {posts === null && <p style={{ color: "var(--muted)", fontSize: 13 }}>Loading…</p>}
      {posts !== null && posts.length === 0 && (
        <p style={{ color: "var(--muted)", fontSize: 13 }}>
          Nothing here yet — pair a machine and put it to work, or post a message above.
        </p>
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {posts?.map((p) => (
          <div key={p.id} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 10, background: "var(--surface)", fontSize: 13 }}>
            <div style={{ display: "flex", justifyContent: "space-between", color: "var(--muted-strong)", fontSize: 12, marginBottom: 4 }}>
              <span>
                {p.author_kind === "member" ? "You" : p.author_kind === "assistant" ? "Assistant" : (p.node_display ?? "A machine")}
                {p.author_kind === "node" && <span style={{ color: "var(--muted)" }}> · {p.event_type.replace(/_/g, " ")}</span>}
              </span>
              <span>{new Date(p.created_at).toLocaleString()}</span>
            </div>
            <div>{p.body}</div>
          </div>
        ))}
      </div>
    </main>
  );
}

export default function Fleet() {
  return <RequireMember next="/fleet">{() => (<><Nav /><FleetView /></>)}</RequireMember>;
}
