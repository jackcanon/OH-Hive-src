"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";

type Overview = {
  id: string; title: string; goal: string; license_kind: string; license_spdx: string | null;
  requires_internet: boolean; owner: string; my_role: string | null; fund_balance: number;
  cards: Record<string, number> | null;
};

type Source = { kind: "snapshot"; server: string; age: number; coordinator: string } | { kind: "hub" };

// ADR-013 §A.5: the Hive browser reads the coordinator's snapshot from a regional server; the
// projects_overview RPC is the fallback when no server is reachable.
async function loadOverview(): Promise<{ rows: Overview[]; source: Source }> {
  const sb = supabaseBrowser();
  try {
    const [{ data: servers }, { data: { session } }] = await Promise.all([sb.rpc("hive_servers"), sb.auth.getSession()]);
    const online = ((servers as { public_url: string | null; status: string; name: string }[] | null) ?? []).filter((s) => s.status === "online" && s.public_url);
    if (online[0] && session?.access_token) {
      const r = await fetch(`${online[0].public_url!.replace(/\/$/, "")}/snapshot/latest?token=${encodeURIComponent(session.access_token)}`, { cache: "no-store" });
      if (r.ok) {
        const snap = await r.json() as { projects: Omit<Overview, "my_role">[]; coordinator: string };
        const { data: roles } = await sb.rpc("hive_my_roles");
        const mine = (roles as Record<string, string> | null) ?? {};
        return {
          rows: snap.projects.map((p) => ({ ...p, my_role: mine[p.id] ?? null })),
          source: { kind: "snapshot", server: online[0].name, age: Number(r.headers.get("x-hive-snapshot-age") ?? 0), coordinator: snap.coordinator },
        };
      }
    }
  } catch { /* fall through to the hub */ }
  const { data, error } = await sb.rpc("hive_projects_overview");
  if (error) throw new Error(error.message);
  return { rows: data as Overview[], source: { kind: "hub" } };
}

function List() {
  const [rows, setRows] = useState<Overview[] | null>(null);
  const [source, setSource] = useState<Source>({ kind: "hub" });
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    const load = () => loadOverview().then(({ rows, source }) => { setRows(rows); setSource(source); }).catch((e) => setErr(String(e.message ?? e)));
    load(); const t = setInterval(load, 15000); return () => clearInterval(t);
  }, []);
  if (err) return <p style={{ padding: 24, color: "#b00020" }}>{err}</p>;
  if (!rows) return <p style={{ padding: 24 }}>Loading the Hive…</p>;
  return (
    <main style={{ maxWidth: 900, margin: "0 auto", padding: 24 }}>
      <h1 style={{ marginTop: 8 }}>The Hive</h1>
      <p style={{ color: "#666" }}>
        Every project is open to every member. {rows.length} project{rows.length === 1 ? "" : "s"}.
        <span style={{ fontSize: 12, color: "#999", marginLeft: 8 }}>
          {source.kind === "snapshot" ? `snapshot from ${source.server} · ${source.age}s old · coordinator ${source.coordinator}` : "read from the hub"}
        </span>
      </p>
      {rows.map((p) => {
        const c = p.cards ?? {};
        const total = Object.values(c).reduce((a, b) => a + b, 0);
        return (
          <a key={p.id} href={`/projects/${p.id}`} style={{ display: "block", textDecoration: "none", color: "inherit",
              border: "1px solid #e6e2d6", borderRadius: 8, padding: 16, marginBottom: 12, background: "#fff" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
              <strong style={{ fontSize: 17 }}>{p.title}</strong>
              <span style={{ fontSize: 13, color: "#666" }}>{honey(p.fund_balance)} in fund</span>
            </div>
            <div style={{ color: "#555", marginTop: 4 }}>{p.goal}</div>
            <div style={{ fontSize: 12, color: "#777", marginTop: 8, display: "flex", gap: 12, flexWrap: "wrap" }}>
              <span>by {p.owner}</span>
              <span>{p.license_kind === "open_source" ? `open source · ${p.license_spdx}` : "owner-only"}</span>
              {p.requires_internet && <span>🌐 needs internet</span>}
              {p.my_role && <span>you: {p.my_role}</span>}
              <span>{total} cards{c.done ? ` · ${c.done} done` : ""}{c.review ? ` · ${c.review} to review` : ""}{c.running ? ` · ${c.running} running` : ""}</span>
            </div>
          </a>
        );
      })}
    </main>
  );
}

export default function Projects() {
  return (
    <RequireMember next="/projects">{() => (<><Nav /><List /></>)}</RequireMember>
  );
}
