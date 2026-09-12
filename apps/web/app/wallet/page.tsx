"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";
import { friendlyError } from "@/lib/errors";
import { NodeAvatar, NODE_PRESETS } from "@/components/NodeAvatar";

type Wallet = {
  balance: number;
  sources: { purchased: number; earned: number; grant: number } | null;
  provider: { spendable_honey: number; budget_usd_cap: number | null; budget_usd_spent: number | null } | null;
  rate: { honey_per_output_token: number; model_ref: string; since: string } | null;
  entries: { at: string; type: string; direction: string; amount: number; tokens_out: number | null; memo: string; card: string | null; node: string | null }[];
  nodes: { id: string; display_name: string; presence: string; region: string; role: string; avatar_choice: string; gpu: string | null; models: number; last_heartbeat: string | null; allow_internet: boolean; tools_level: string }[];
};

function WalletView() {
  const [w, setW] = useState<Wallet | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [editingNode, setEditingNode] = useState<string | null>(null);
  const [avatarBusy, setAvatarBusy] = useState<string | null>(null);
  const load = () => supabaseBrowser().rpc("hive_my_wallet", { p_limit: 50 }).then(({ data, error }) => {
    if (error) setErr(friendlyError(error.message)); else setW(data as Wallet);
  });
  useEffect(() => {
    load(); const t = setInterval(load, 15000); return () => clearInterval(t);
  }, []);

  // Node avatars (Jack, 2026-09-12): click a node's badge to pick its hardware picture --
  // Mac Mini/Studio/iMac/rack server, or Auto to go back to the role-based default.
  async function setNodeAvatar(nodeId: string, choice: string) {
    setAvatarBusy(nodeId);
    const { error } = await supabaseBrowser().rpc("hive_node_set_avatar", { p_node_id: nodeId, p_avatar_choice: choice });
    setAvatarBusy(null);
    if (error) { setErr(friendlyError(error.message)); return; }
    setEditingNode(null);
    load();
  }
  if (err) return <p style={{ padding: 24, color: "var(--danger)" }}>{err}</p>;
  if (!w) return <p style={{ padding: 24 }}>Loading wallet…</p>;
  return (
    <main style={{ maxWidth: 900, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>{honey(w.balance)}</h1>
      {w.sources && (
        <p style={{ color: "var(--muted-strong)", fontSize: 13, margin: "0 0 6px" }}>
          <strong>{Number(w.sources.earned).toFixed(4)}</strong> earned · <strong>{Number(w.sources.purchased).toFixed(4)}</strong> purchased
          {Number(w.sources.grant) > 0 && <> · <strong>{Number(w.sources.grant).toFixed(4)}</strong> grant</>}
          <span style={{ color: "var(--muted)" }}> — earned Honey buys Hive compute and storage; purchased (and grant) Honey can also pay for provider APIs like the project chat.</span>
        </p>
      )}
      {w.rate && (
        <p style={{ color: "var(--muted-strong)", fontSize: 13 }}>
          Earning {w.rate.honey_per_output_token} Honey per output token (pegged to {w.rate.model_ref}; 1 Honey = US$0.01).
        </p>
      )}

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Your nodes</h2>
      {w.nodes.length === 0 && <p style={{ color: "var(--muted-strong)" }}>No machines yet — <a href="/pair">pair one</a>.</p>}
      {w.nodes.map((n) => (
        <div key={n.id} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 12, marginBottom: 8, background: "var(--surface)", fontSize: 14 }}>
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <button
              onClick={() => setEditingNode(editingNode === n.id ? null : n.id)}
              title="Change this machine's picture"
              style={{ padding: 0, border: "none", background: "none", cursor: "pointer", lineHeight: 0 }}
            >
              <NodeAvatar choice={n.avatar_choice} role={n.role} size={40} />
            </button>
            <div style={{ flex: 1, minWidth: 0 }}>
              <strong>{n.display_name}</strong>{" "}
              <span style={{ color: n.presence === "checked_in" ? "var(--ok)" : "var(--muted)" }}>● {n.presence.replace("_", " ")}</span>
              <div style={{ fontSize: 12, color: "var(--muted)", marginTop: 4 }}>
                {n.region} · {n.gpu ?? "no GPU"} · {n.models} models · internet {n.allow_internet ? "on" : "off"} · {n.tools_level.replace("_", " ")}
                {n.last_heartbeat && ` · heartbeat ${new Date(n.last_heartbeat).toLocaleTimeString()}`}
              </div>
            </div>
          </div>
          {editingNode === n.id && (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 10, paddingTop: 10, borderTop: "1px solid var(--border)" }}>
              <button
                onClick={() => setNodeAvatar(n.id, "auto")}
                disabled={avatarBusy === n.id}
                title="Auto (based on role)"
                style={{
                  width: 36, height: 36, padding: 0, cursor: "pointer",
                  border: n.avatar_choice === "auto" ? "2px solid var(--gold)" : "1px solid var(--border)", borderRadius: 10,
                }}
              >
                <NodeAvatar choice="auto" role={n.role} size={32} />
              </button>
              {Object.entries(NODE_PRESETS).map(([key, p]) => (
                <button
                  key={key}
                  onClick={() => setNodeAvatar(n.id, key)}
                  disabled={avatarBusy === n.id}
                  title={p.label}
                  style={{
                    width: 36, height: 36, padding: 0, cursor: "pointer",
                    border: n.avatar_choice === key ? "2px solid var(--gold)" : "1px solid var(--border)", borderRadius: 10,
                  }}
                >
                  <NodeAvatar choice={key} size={32} />
                </button>
              ))}
              {avatarBusy === n.id && <span style={{ fontSize: 12, color: "var(--muted)", alignSelf: "center" }}>Saving…</span>}
            </div>
          )}
        </div>
      ))}

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Ledger</h2>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 13 }}>
        <tbody>
          {w.entries.map((e, i) => (
            <tr key={i} style={{ borderBottom: "1px solid var(--border)" }}>
              <td style={{ padding: "6px 4px", color: "var(--muted)", whiteSpace: "nowrap" }}>{new Date(e.at).toLocaleString()}</td>
              <td style={{ padding: "6px 4px" }}>{e.type.replace("_", " ")}{e.card && ` · ${e.card}`}{e.node && ` on ${e.node}`}{e.tokens_out ? ` · ${e.tokens_out} tok` : ""}</td>
              <td style={{ padding: "6px 4px", color: "var(--muted)" }}>{e.memo}</td>
              <td style={{ padding: "6px 4px", textAlign: "right", color: e.direction === "credit" ? "var(--ok)" : "var(--danger)", whiteSpace: "nowrap" }}>
                {e.direction === "credit" ? "+" : "−"}{Number(e.amount).toLocaleString(undefined, { maximumFractionDigits: 4 })}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </main>
  );
}

export default function WalletPage() {
  return <RequireMember next="/wallet">{() => (<><Nav /><WalletView /></>)}</RequireMember>;
}
