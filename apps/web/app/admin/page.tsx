"use client";

import { useEffect, useState, type CSSProperties } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember } from "@/components/RequireMember";
import { Honey } from "@/components/Honey";

// Admin-only (Jack, 2026-09-12): "a list of hive members ... a server section so we can see the
// names of the servers and show the regions ... our available storage." Gated server-side by
// hive.is_admin() (migration 20260912210000) -- the RPCs below raise `not_admin` for anyone who
// isn't, so this page never trusts the client-side `isAdmin` state for anything but what to show;
// a non-admin who somehow lands here just sees the same message a regular member would.

type Member = {
  id: string;
  display_name: string;
  email: string;
  status: "invited" | "active" | "suspended";
  onramp: string | null;
  is_admin: boolean;
  invited_by: string | null;
  created_at: string;
  node_count: number;
  wallet_honey: number;
};

type Server = {
  node_id: string;
  name: string;
  region: string;
  operator: "volunteer" | "hjm";
  tier: "primary" | "standby";
  status: string;
  public_url: string | null;
  storage_gb_offered: number | null;
  storage_used_bytes: number;
  connections: number;
  last_heartbeat: string | null;
  version: string | null;
  owner: string;
};

type StorageSummary = {
  offered_bytes: number;
  used_bytes: number;
  available_bytes: number;
  server_count: number;
  online_count: number;
  pct_used: number;
};

function fmtBytes(n: number | null | undefined) {
  const bytes = Number(n ?? 0);
  if (bytes <= 0) return "0 GB";
  const gb = bytes / 1073741824;
  if (gb >= 1024) return `${(gb / 1024).toFixed(2)} TB`;
  return `${gb.toFixed(gb >= 10 ? 0 : 1)} GB`;
}

const cardStyle: CSSProperties = {
  border: "1px solid var(--border)", borderRadius: 10, padding: 16, marginBottom: 20, background: "var(--surface)",
};
const th: CSSProperties = { textAlign: "left", fontSize: 11, color: "var(--muted)", fontWeight: 500, padding: "0 10px 6px 0", borderBottom: "1px solid var(--border)" };
const td: CSSProperties = { fontSize: 13, padding: "8px 10px 8px 0", borderBottom: "1px solid var(--border)", verticalAlign: "top" };

function AdminView() {
  const [isAdmin, setIsAdmin] = useState<boolean | null>(null);
  const [members, setMembers] = useState<Member[] | null>(null);
  const [servers, setServers] = useState<Server[] | null>(null);
  const [storage, setStorage] = useState<StorageSummary | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const [confirmSuspend, setConfirmSuspend] = useState<string | null>(null);

  const refreshMembers = () => {
    supabaseBrowser().rpc("hive_admin_members", {}).then(({ data, error }) => {
      if (!error) setMembers((data ?? []) as Member[]);
    });
  };

  useEffect(() => {
    const sb = supabaseBrowser();
    sb.rpc("hive_am_i_admin", {}).then(({ data, error }) => {
      if (error) { setErr("Couldn't check admin access. Try refreshing."); return; }
      setIsAdmin(!!data);
      if (!data) return;
      Promise.all([
        sb.rpc("hive_admin_members", {}),
        sb.rpc("hive_admin_servers", {}),
        sb.rpc("hive_admin_storage_summary", {}),
      ]).then(([m, s, st]) => {
        if (m.error || s.error || st.error) { setErr("Something went wrong loading the admin section. Try refreshing."); return; }
        setMembers((m.data ?? []) as Member[]);
        setServers((s.data ?? []) as Server[]);
        setStorage(st.data as StorageSummary);
      });
    });
  }, []);

  async function suspend(id: string) {
    setActionBusy(id);
    setConfirmSuspend(null);
    const { error } = await supabaseBrowser().rpc("hive_admin_suspend_member", { p_member_id: id });
    setActionBusy(null);
    if (error) { setErr(error.message === "cannot_suspend_admin" ? "Demote them from admin first — can't suspend another admin." : "Couldn't suspend that member."); return; }
    refreshMembers();
  }
  async function reinstate(id: string) {
    setActionBusy(id);
    const { error } = await supabaseBrowser().rpc("hive_admin_reinstate_member", { p_member_id: id });
    setActionBusy(null);
    if (error) { setErr("Couldn't reinstate that member."); return; }
    refreshMembers();
  }

  if (isAdmin === null && !err) return <main style={{ maxWidth: 960, margin: "0 auto", padding: 24 }}><p style={{ color: "var(--muted-strong)" }}>Loading…</p></main>;
  if (err) return <main style={{ maxWidth: 960, margin: "0 auto", padding: 24 }}><p style={{ color: "var(--danger)" }}>{err}</p></main>;
  if (!isAdmin) {
    return (
      <main style={{ maxWidth: 960, margin: "0 auto", padding: 24 }}>
        <h1>Admin</h1>
        <p style={{ color: "var(--muted-strong)" }}>This section is for Hive admins only.</p>
      </main>
    );
  }

  return (
    <main style={{ maxWidth: 960, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>Admin</h1>
      <p style={{ color: "var(--muted-strong)", fontSize: 14, margin: "0 0 24px" }}>Members, servers, and storage across the Hive.</p>

      <section style={cardStyle}>
        <h2 style={{ fontSize: 15, margin: "0 0 12px" }}>Storage</h2>
        {storage && (
          <>
            <div style={{ height: 8, borderRadius: 4, background: "var(--bg)", border: "1px solid var(--border)", overflow: "hidden", marginBottom: 10 }}>
              <div style={{ height: "100%", width: `${Math.min(storage.pct_used, 100)}%`, background: "var(--accent)" }} />
            </div>
            <p style={{ fontSize: 13, color: "var(--muted-strong)", margin: 0 }}>
              {fmtBytes(storage.used_bytes)} used of {fmtBytes(storage.offered_bytes)} offered
              ({storage.pct_used}%) · {fmtBytes(storage.available_bytes)} available
            </p>
            <p style={{ fontSize: 13, color: "var(--muted-strong)", margin: "4px 0 0" }}>
              {storage.online_count} of {storage.server_count} servers online
            </p>
          </>
        )}
      </section>

      <section style={cardStyle}>
        <h2 style={{ fontSize: 15, margin: "0 0 12px" }}>Servers ({servers?.length ?? 0})</h2>
        <div style={{ overflowX: "auto" }}>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead><tr>
              <th style={th}>Name</th><th style={th}>Region</th><th style={th}>Operator</th>
              <th style={th}>Status</th><th style={th}>Storage</th><th style={th}>Owner</th><th style={th}>Last seen</th>
            </tr></thead>
            <tbody>
              {servers?.map((s) => (
                <tr key={s.node_id}>
                  <td style={td}>{s.name}</td>
                  <td style={td}>{s.region}</td>
                  <td style={td}>{s.operator === "hjm" ? "HJM" : "Volunteer"} · {s.tier}</td>
                  <td style={td}>
                    <span style={{ color: s.status === "online" ? "var(--ok)" : "var(--muted)" }}>●</span> {s.status}
                  </td>
                  <td style={td}>{fmtBytes(s.storage_used_bytes)} / {s.storage_gb_offered ?? 0} GB</td>
                  <td style={td}>{s.owner}</td>
                  <td style={td}>{s.last_heartbeat ? new Date(s.last_heartbeat).toLocaleString() : "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section style={cardStyle}>
        <h2 style={{ fontSize: 15, margin: "0 0 12px" }}>Members ({members?.length ?? 0})</h2>
        <div style={{ overflowX: "auto" }}>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead><tr>
              <th style={th}>Name</th><th style={th}>Email</th><th style={th}>Status</th>
              <th style={th}>Onramp</th><th style={th}>Nodes</th><th style={th}>Wallet</th><th style={th}>Joined</th><th style={th}>Action</th>
            </tr></thead>
            <tbody>
              {members?.map((m) => (
                <tr key={m.id}>
                  <td style={td}>{m.display_name}{m.is_admin && <span style={{ marginLeft: 6, fontSize: 10, color: "var(--accent)" }}>ADMIN</span>}</td>
                  <td style={td}>{m.email}</td>
                  <td style={td}>
                    <span style={{ color: m.status === "active" ? "var(--ok)" : m.status === "suspended" ? "var(--danger)" : "var(--muted)" }}>●</span> {m.status}
                  </td>
                  <td style={td}>{m.onramp ?? "—"}</td>
                  <td style={td}>{m.node_count}</td>
                  <td style={td}><Honey n={m.wallet_honey} digits={2} /></td>
                  <td style={td}>{new Date(m.created_at).toLocaleDateString()}</td>
                  <td style={td}>
                    {m.is_admin ? (
                      <span style={{ color: "var(--muted)" }}>—</span>
                    ) : m.status === "suspended" ? (
                      <a href="#" onClick={(e) => { e.preventDefault(); reinstate(m.id); }} style={{ opacity: actionBusy === m.id ? 0.5 : 1 }}>
                        {actionBusy === m.id ? "…" : "Reinstate"}
                      </a>
                    ) : confirmSuspend === m.id ? (
                      <a href="#" onClick={(e) => { e.preventDefault(); suspend(m.id); }} style={{ color: "var(--danger)" }}>
                        Confirm?
                      </a>
                    ) : (
                      <a href="#" onClick={(e) => { e.preventDefault(); setConfirmSuspend(m.id); }} style={{ opacity: actionBusy === m.id ? 0.5 : 1 }}>
                        {actionBusy === m.id ? "…" : "Suspend"}
                      </a>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </main>
  );
}

export default function AdminPage() {
  return <RequireMember next="/admin">{() => (<><Nav /><AdminView /></>)}</RequireMember>;
}
