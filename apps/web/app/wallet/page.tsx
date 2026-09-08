"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";
import { friendlyError } from "@/lib/errors";

type Wallet = {
  balance: number;
  sources: { purchased: number; earned: number; grant: number } | null;
  provider: { spendable_honey: number; budget_usd_cap: number | null; budget_usd_spent: number | null } | null;
  rate: { honey_per_output_token: number; model_ref: string; since: string } | null;
  entries: { at: string; type: string; direction: string; amount: number; tokens_out: number | null; memo: string; card: string | null; node: string | null }[];
  nodes: { id: string; display_name: string; presence: string; region: string; gpu: string | null; models: number; last_heartbeat: string | null; allow_internet: boolean; tools_level: string }[];
};

function WalletView() {
  const [w, setW] = useState<Wallet | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    const load = () => supabaseBrowser().rpc("hive_my_wallet", { p_limit: 50 }).then(({ data, error }) => {
      if (error) setErr(friendlyError(error.message)); else setW(data as Wallet);
    });
    load(); const t = setInterval(load, 15000); return () => clearInterval(t);
  }, []);
  if (err) return <p style={{ padding: 24, color: "var(--danger)" }}>{err}</p>;
  if (!w) return <p style={{ padding: 24 }}>Loading wallet…</p>;
  return (
    <main style={{ maxWidth: 900, margin: "0 auto", padding: 24 }}>
      <h1 style={{ margin: "8px 0" }}>{honey(w.balance)}</h1>
      {w.sources && (
        <p style={{ color: "var(--muted-strong)", fontSize: 13, margin: "0 0 6px" }}>
          <strong>{Number(w.sources.earned).toFixed(4)}</strong> earned · <strong>{Number(w.sources.purchased).toFixed(4)}</strong> purchased
          {Number(w.sources.grant) > 0 && <> · <strong>{Number(w.sources.grant).toFixed(4)}</strong> grant</>}
          <span style={{ color: "var(--muted)" }}> — earned Honey buys Hive compute and storage; purchased (and grant) Honey can also pay for provider APIs like the interviewer.</span>
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
          <strong>{n.display_name}</strong>{" "}
          <span style={{ color: n.presence === "checked_in" ? "var(--ok)" : "var(--muted)" }}>● {n.presence.replace("_", " ")}</span>
          <div style={{ fontSize: 12, color: "var(--muted)", marginTop: 4 }}>
            {n.region} · {n.gpu ?? "no GPU"} · {n.models} models · internet {n.allow_internet ? "on" : "off"} · {n.tools_level.replace("_", " ")}
            {n.last_heartbeat && ` · heartbeat ${new Date(n.last_heartbeat).toLocaleTimeString()}`}
          </div>
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
