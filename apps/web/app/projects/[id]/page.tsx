"use client";

import { useCallback, useEffect, useState } from "react";
import { useParams } from "next/navigation";
import { supabaseBrowser } from "@/lib/supabase";
import { useLive } from "@/lib/live";
import { Nav, RequireMember, honey } from "@/components/RequireMember";
import { HoneyMark, formatHoney } from "@/components/Honey";

type NodeRole = "compute" | "regional_server" | "compute_and_server";
type Card = {
  id: string; key: string; title: string; modality: string; status: string; inputs: string; acceptance: string;
  deps: string[]; requires_internet: boolean; required_capabilities: Record<string, unknown>;
  lease: { node: string; node_role: NodeRole; expires_at: string } | null;
  output: { content: string; model_id: string | null; usage: { tokens_out?: number; compute_seconds?: number };
            node: string | null; node_role: NodeRole | null; created_at: string } | null;
};

// hive.node_role: "compute" is a member's own machine; "regional_server"/"compute_and_server" is
// one of the Hive's own cloud servers. Jack's ask: never leave this ambiguous on the board.
function nodeKindBadge(role: NodeRole | null | undefined) {
  if (!role) return null;
  const cloud = role === "regional_server" || role === "compute_and_server";
  return (
    <span
      title={cloud ? "Ran on one of the Hive's own cloud servers" : "Ran on a member's own machine"}
      style={{
        fontSize: 10, fontWeight: 600, letterSpacing: "0.02em", padding: "1px 6px", borderRadius: 999,
        border: "1px solid var(--border)", color: "var(--muted-strong)", whiteSpace: "nowrap",
      }}
    >
      {cloud ? "☁ CLOUD" : "💻 LOCAL"}
    </span>
  );
}
type Board = {
  project: { id: string; title: string; goal: string; license_kind: string; license_spdx: string | null; requires_internet: boolean;
             owner: string; my_role: string | null; fund_balance: number } | null;
  cards: Card[];
};
type Contributors = {
  credited: { member_id: string; display_name: string; total_honey: number; last_at: string }[];
  anonymous_total: number;
  anonymous_count: number;
};

const COLUMNS: [string, string][] = [
  ["suggested", "Suggested"], ["ready", "Ready"], ["running", "Running"], ["blocked", "Blocked"], ["review", "Review"], ["done", "Done"],
];

function BoardView({ id }: { id: string }) {
  const [board, setBoard] = useState<Board | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [fundOpen, setFundOpen] = useState(false);
  const [fundAmount, setFundAmount] = useState("");
  const [fundAnonymous, setFundAnonymous] = useState(false);
  const [funding, setFunding] = useState(false);
  const [contributors, setContributors] = useState<Contributors | null>(null);

  const load = useCallback(async () => {
    const { data, error } = await supabaseBrowser().rpc("hive_project_board", { p_project_id: id });
    if (error) setErr(error.message); else setBoard(data as Board);
  }, [id]);

  const loadContributors = useCallback(async () => {
    const { data, error } = await supabaseBrowser().rpc("hive_project_contributors", { p_project_id: id });
    if (!error) setContributors(data as Contributors);
  }, [id]);
  useEffect(() => { loadContributors(); }, [loadContributors]);

  // Nodes won't claim a single card until the project's fund balance is above zero (ADR-002 D19) --
  // a new project starts unfunded, so this is the only thing standing between "planned" and "running".
  async function fundProject() {
    const amount = Number(fundAmount);
    if (!amount || amount <= 0) return;
    setFunding(true);
    const { error } = await supabaseBrowser().rpc("hive_fund_project", { p_project_id: id, p_amount: amount, p_anonymous: fundAnonymous });
    setFunding(false);
    if (error) setErr(error.message);
    else { setFundAmount(""); setFundAnonymous(false); setFundOpen(false); load(); loadContributors(); }
  }

  // Live frames from a regional server when one is online; otherwise 15 s polling (ADR-013 §A.4).
  const live = useLive<Board>(id, (b) => setBoard(b), load);

  async function act(fn: "hive_card_accept" | "hive_card_send_back" | "hive_card_promote", card: Card, note?: string) {
    setBusy(card.id);
    const args: Record<string, unknown> = { p_card_id: card.id };
    if (fn === "hive_card_send_back") args.p_note = note ?? "";
    const { error } = await supabaseBrowser().rpc(fn, args);
    setBusy(null);
    if (error) setErr(error.message); else load();
  }

  if (err) return <p style={{ padding: 24, color: "var(--danger)" }}>{err}</p>;
  if (!board) return <p style={{ padding: 24 }}>Loading board…</p>;
  if (!board.project) return <p style={{ padding: 24 }}>Project not found.</p>;
  const p = board.project;
  const admin = p.my_role === "owner" || p.my_role === "admin";

  return (
    <main style={{ padding: 24 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", flexWrap: "wrap", gap: 8 }}>
        <div>
          <h1 style={{ margin: "8px 0 4px" }}>{p.title}</h1>
          <div style={{ color: "var(--muted-strong)" }}>{p.goal}</div>
          <div style={{ fontSize: 12, color: "var(--muted)", marginTop: 6 }}>
            by {p.owner} · {p.license_kind === "open_source" ? `open source · ${p.license_spdx}` : "owner-only"}
            {p.requires_internet && " · 🌐 needs internet"} {p.my_role && ` · you: ${p.my_role}`}
            {" · "}<span title={live === "live" ? "pushed by a regional server as it changes" : "refreshing every 15 s"} style={{ color: live === "live" ? "var(--ok)" : "var(--muted)" }}>
              {live === "live" ? "● live" : live === "polling" ? "○ polling" : "○ connecting"}
            </span>
          </div>
        </div>
        <div style={{ textAlign: "right" }}>
          <button
            onClick={() => setFundOpen((v) => !v)}
            title="Add Honey from your wallet to this project's fund"
            style={{ ...btn, display: "inline-flex", alignItems: "center", gap: 6, fontSize: 14, background: "var(--surface)" }}
          >
            <HoneyMark height={16} /> <strong>{honey(p.fund_balance)}</strong> in fund
          </button>
          {p.fund_balance <= 0 && (
            <div style={{ color: "var(--danger)", fontSize: 12, marginTop: 4 }}>nothing will run until this is funded</div>
          )}
          {fundOpen && (
            <div style={{ marginTop: 8, display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 6,
                          border: "1px solid var(--border)", borderRadius: 8, padding: 10, background: "var(--surface)" }}>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <input
                  type="number" min="0" step="1" placeholder="Honey" value={fundAmount} autoFocus
                  onChange={(e) => setFundAmount(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && fundProject()}
                  style={{ width: 90, fontSize: 13, padding: "4px 6px" }}
                />
                <button disabled={funding || !fundAmount} onClick={fundProject} style={btn}>
                  {funding ? "Adding…" : "Add Honey"}
                </button>
              </div>
              <label style={{ fontSize: 12, color: "var(--muted-strong)", display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}>
                <input type="checkbox" checked={fundAnonymous} onChange={(e) => setFundAnonymous(e.target.checked)} />
                Contribute anonymously (won't be named on the board)
              </label>
            </div>
          )}
        </div>
      </div>

      {contributors && (contributors.credited.length > 0 || contributors.anonymous_count > 0) && (
        <div style={{ marginTop: 16, fontSize: 13, color: "var(--muted-strong)" }}>
          <strong style={{ color: "var(--fg)" }}>Funded by</strong>{" "}
          {contributors.credited.map((c, i) => (
            <span key={c.member_id}>
              {i > 0 && ", "}
              <span title={`${formatHoney(c.total_honey)} Honey`}>#{i + 1} {c.display_name} ({formatHoney(c.total_honey)})</span>
            </span>
          ))}
          {contributors.anonymous_count > 0 && (
            <span>
              {contributors.credited.length > 0 && ", "}
              {contributors.anonymous_count} anonymous supporter{contributors.anonymous_count === 1 ? "" : "s"} ({formatHoney(contributors.anonymous_total)})
            </span>
          )}
        </div>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 12, marginTop: 20, overflowX: "auto" }}>
        {COLUMNS.map(([status, label]) => {
          const cards = board.cards.filter((c) => c.status === status);
          return (
            <section key={status} style={{ background: "var(--surface-2)", borderRadius: 8, padding: 10, minHeight: 120 }}>
              <div style={{ fontSize: 12, fontWeight: 600, color: "var(--muted-strong)", marginBottom: 8 }}>{label} · {cards.length}</div>
              {cards.map((c) => (
                <div key={c.id} onClick={() => setOpen(open === c.id ? null : c.id)}
                     style={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 6, padding: 10, marginBottom: 8, cursor: "pointer" }}>
                  <div style={{ fontWeight: 600, fontSize: 14 }}>{c.title}</div>
                  <div style={{ fontSize: 11, color: "var(--muted)", marginTop: 4, display: "flex", alignItems: "center", gap: 5, flexWrap: "wrap" }}>
                    <span>
                      {c.modality}{c.requires_internet && " · 🌐"}{c.deps.length > 0 && ` · after ${c.deps.join(", ")}`}
                      {c.lease && ` · on ${c.lease.node}`}
                      {c.output?.node && c.status !== "running" && ` · by ${c.output.node}`}
                    </span>
                    {c.lease && nodeKindBadge(c.lease.node_role)}
                    {!c.lease && c.output?.node && c.status !== "running" && nodeKindBadge(c.output.node_role)}
                  </div>
                  {open === c.id && (
                    <div style={{ marginTop: 10, fontSize: 13 }} onClick={(e) => e.stopPropagation()}>
                      <div style={{ color: "var(--muted-strong)", whiteSpace: "pre-wrap" }}><strong>Task.</strong> {c.inputs}</div>
                      <div style={{ color: "var(--muted-strong)", marginTop: 6 }}><strong>Accept when.</strong> {c.acceptance}</div>
                      {c.output && (
                        <div style={{ marginTop: 10, padding: 10, background: "var(--bg)", border: "1px solid var(--border)", borderRadius: 6 }}>
                          <div style={{ fontSize: 11, color: "var(--muted)", marginBottom: 6, display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                            <span>
                              Output · {c.output.model_id ?? "?"} · {c.output.usage?.tokens_out ?? "?"} tokens
                              {typeof c.output.usage?.compute_seconds === "number" && ` · ${c.output.usage.compute_seconds.toFixed(1)}s`}
                            </span>
                            {nodeKindBadge(c.output.node_role)}
                          </div>
                          <div style={{ whiteSpace: "pre-wrap" }}>{c.output.content}</div>
                        </div>
                      )}
                      {admin && (
                        <div style={{ display: "flex", gap: 8, marginTop: 10, flexWrap: "wrap" }}>
                          {(c.status === "review" || c.status === "blocked") && (
                            <button disabled={busy === c.id} onClick={() => act("hive_card_accept", c)} style={btn}>Accept</button>
                          )}
                          {(c.status === "review" || c.status === "blocked" || c.status === "running") && (
                            <button disabled={busy === c.id}
                              onClick={() => act("hive_card_send_back", c, prompt("Note for the node (optional):") ?? "")} style={btn}>
                              Send back
                            </button>
                          )}
                          {c.status === "suggested" && (
                            <button disabled={busy === c.id} onClick={() => act("hive_card_promote", c)} style={btn}>Approve suggestion</button>
                          )}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </section>
          );
        })}
      </div>
    </main>
  );
}

const btn = { padding: "6px 12px", cursor: "pointer", fontSize: 13 } as const;

export default function ProjectPage() {
  const { id } = useParams<{ id: string }>();
  return (
    <RequireMember next={`/projects/${id}`}>{() => (<><Nav /><BoardView id={id} /></>)}</RequireMember>
  );
}
