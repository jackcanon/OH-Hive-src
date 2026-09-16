"use client";
import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";

type Replica = { hash: string; node_id: string; node_name: string; bytes: number; size_matches: boolean;
  max_honey_per_day: number | null; approved_size_matches: boolean | null; unpaid_since: string | null; server_status: string | null };
type Page = { can_manage: boolean; replicas: Replica[] };
export function StorageAllowances({ projectId }: { projectId: string }) {
  const [expanded, setExpanded] = useState(false);
  const [page, setPage] = useState<Page | null>(null);
  const [offset, setOffset] = useState(0);
  const [refresh, setRefresh] = useState(0);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!expanded) return;
    let active = true;
    setPage(null); setError(null);
    Promise.resolve(supabaseBrowser().rpc("hive_project_storage_allowances", { p_project_id: projectId, p_offset: offset }))
      .then(({ data, error }) => { if (active) { if (error) setError(error.message); else setPage(data as Page); } })
      .catch(() => { if (active) setError("Could not load storage allowances. Please refresh."); });
    return () => { active = false; };
  }, [expanded, projectId, offset, refresh]);
  return <section aria-label="Storage payments" style={{ marginTop: 20 }}>
    <button onClick={() => setExpanded((v) => !v)} aria-expanded={expanded}>Storage payments</button>
    {expanded && <>
      <p>Each stored copy needs its own daily Honey limit. Payments come from this project’s fund. Stopping an allowance stops future payments; it does not delete the file or refund completed payments.</p>
      {error && <p role="alert">{error}</p>}
      {!page && !error && <p role="status">Loading storage…</p>}
      {page?.replicas.length === 0 && <p>No stored copies on this page.</p>}
      {page?.replicas.map((replica) => <StorageReplica key={`${replica.hash}:${replica.node_id}`} replica={replica} canManage={page.can_manage} projectId={projectId} onSaved={() => setRefresh((n) => n + 1)} />)}
      <button disabled={offset === 0} onClick={() => setOffset((n) => Math.max(0,n-100))}>Previous</button>{" "}
      <button disabled={!page || page.replicas.length < 100} onClick={() => setOffset((n) => n+100)}>Next</button>{" "}
      <button onClick={() => setRefresh((n) => n+1)}>Refresh storage</button>
    </>}
  </section>;
}
function StorageReplica({ replica, canManage, projectId, onSaved }: { replica: Replica; canManage: boolean; projectId: string; onSaved: () => void }) {
  const [limit, setLimit] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function change(revoke: boolean) {
    if (busy || !canManage) return;
    setBusy(true); setError(null);
    try {
      const { error } = revoke
        ? await supabaseBrowser().rpc("hive_storage_allowance_revoke", { p_hash: replica.hash, p_node: replica.node_id })
        : await supabaseBrowser().rpc("hive_storage_allowance_approve", { p_hash: replica.hash, p_node: replica.node_id, p_payer_project: projectId, p_max_honey_per_day: Number(limit) });
      if (error) setError(error.message); else onSaved();
    } catch { setError("Could not confirm the change. Refresh storage to check its current status."); }
    finally { setBusy(false); }
  }
  return <div style={{ border: "1px solid var(--border)", padding: 12, margin: "8px 0" }}>
    <strong>{replica.node_name}</strong> · {Number(replica.bytes).toLocaleString()} bytes · {replica.server_status ?? "not a storage server"}
    <div style={{ overflowWrap: "anywhere" }}>File: {replica.hash}</div>
    <p>{replica.max_honey_per_day == null ? "No allowance approved." : `Approved daily limit: ${replica.max_honey_per_day} Honey.`}</p>
    {(!replica.size_matches || replica.approved_size_matches === false) && <p>The stored size has changed or does not match. Payment is paused until sizes match and the owner approves again.</p>}
    {replica.unpaid_since && <p>A storage payment could not be funded. Check the project fund.</p>}
    {error && <p role="alert">{error}</p>}
    {canManage && <form onSubmit={(e) => { e.preventDefault(); void change(false); }}>
      <label>Maximum Honey per day for this copy <input type="number" required min="0.000001" step="0.000001" value={limit} disabled={busy} onChange={(e) => setLimit(e.target.value)} /></label>{" "}
      <button type="submit" disabled={busy || !replica.size_matches || !Number.isFinite(Number(limit)) || Number(limit)<=0}>Approve daily limit</button>{" "}
      {replica.max_honey_per_day != null && <button type="button" disabled={busy} onClick={() => void change(true)}>Stop allowance</button>}
    </form>}
  </div>;
}
