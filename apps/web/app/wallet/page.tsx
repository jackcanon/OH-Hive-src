"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";

type Wallet = {
  balance: number;
  rate: { honey_per_output_token: number; model_ref: string; since: string } | null;
  entries: { at: string; type: string; direction: string; amount: number; tokens_out: number | null; memo: string; card: string | null; node: string | null }[];
  nodes: { id: string; display_name: string; presence: string; region: string; gpu: string | null; models: number; last_heartbeat: string | null; allow_internet: boolean; tools_level: string }[];
};

function WalletView() {
  const [w, setW] = useState<Wallet | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    const load = () => supabaseBrowser().rpc("hive_my_wallet", { p_limit: 50 }).then(({ data, error }) => {
      if (error) setErr(error.message); else setW(data as Wallet);
    });
    load(); const t = setInterval(load, 15000); return () => clearInterval(t);
  }, []);
  if (err) return <p style={{ padding: 24, color: "#b00020" }}>{err}</p>;
  if (!w) return <p style={{ padding: 24 }}>Loading wallet…</p>;
  return (
    <main style={{ maxWidth: 900, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>{honey(w.balance)}</h1>
      {w.rate && (
        <p style={{ color: "#666", fontSize: 13 }}>
          Earning {w.rate.honey_per_output_token} $honey per output token (pegged to {w.rate.model_ref}; 1 $honey = US$0.01).
        </p>
      )}

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Your nodes</h2>
      {w.nodes.length === 0 && <p style={{ color: "#666" }}>No machines yet — <a href="/pair">pair one</a>.</p>}
      {w.nodes.map((n) => (
        <div key={n.id} style={{ border: "1px solid #e6e2d6", borderRadius: 8, padding: 12, marginBottom: 8, background: "#fff", fontSize: 14 }}>
          <strong>{n.display_name}</strong>{" "}
          <span style={{ color: n.presence === "checked_in" ? "#2a7" : "#999" }}>● {n.presence.replace("_", " ")}</span>
          <div style={{ fontSize: 12, color: "#777", marginTop: 4 }}>
            {n.region} · {n.gpu ?? "no GPU"} · {n.models} models · internet {n.allow_internet ? "on" : "off"} · {n.tools_level.replace("_", " ")}
            {n.last_heartbeat && ` · heartbeat ${new Date(n.last_heartbeat).toLocaleTimeString()}`}
          </div>
        </div>
      ))}

      <h2 style={{ fontSize: 16, marginTop: 28 }}>Ledger</h2>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 13 }}>
        <tbody>
          {w.entries.map((e, i) => (
            <tr key={i} style={{ borderBottom: "1px solid #eee" }}>
              <td style={{ padding: "6px 4px", color: "#777", whiteSpace: "nowrap" }}>{new Date(e.at).toLocaleString()}</td>
              <td style={{ padding: "6px 4px" }}>{e.type.replace("_", " ")}{e.card && ` · ${e.card}`}{e.node && ` on ${e.node}`}{e.tokens_out ? ` · ${e.tokens_out} tok` : ""}</td>
              <td style={{ padding: "6px 4px", color: "#777" }}>{e.memo}</td>
              <td style={{ padding: "6px 4px", textAlign: "right", color: e.direction === "credit" ? "#2a7" : "#b00020", whiteSpace: "nowrap" }}>
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
