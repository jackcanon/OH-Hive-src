"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";

type Overview = {
  id: string; title: string; goal: string; license_kind: string; license_spdx: string | null;
  requires_internet: boolean; owner: string; my_role: string | null; fund_balance: number;
  cards: Record<string, number> | null;
};

function List() {
  const [rows, setRows] = useState<Overview[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    supabaseBrowser().rpc("hive_projects_overview").then(({ data, error }) => {
      if (error) setErr(error.message); else setRows(data as Overview[]);
    });
  }, []);
  if (err) return <p style={{ padding: 24, color: "#b00020" }}>{err}</p>;
  if (!rows) return <p style={{ padding: 24 }}>Loading the Hive…</p>;
  return (
    <main style={{ maxWidth: 900, margin: "0 auto", padding: 24 }}>
      <h1 style={{ marginTop: 8 }}>The Hive</h1>
      <p style={{ color: "#666" }}>Every project is open to every member. {rows.length} project{rows.length === 1 ? "" : "s"}.</p>
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
