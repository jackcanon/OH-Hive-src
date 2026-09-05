"use client";

import { useEffect, useRef, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Nav, RequireMember, honey } from "@/components/RequireMember";

type Msg = { role: "user" | "assistant"; content: string };
type Reply = { reply: string; plan?: unknown; project_id?: string; cards?: number; charged: number; balance: number | null; error?: string; detail?: string };

function Interview() {
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [spent, setSpent] = useState(0);
  const [balance, setBalance] = useState<number | null>(null);
  const [done, setDone] = useState<{ project_id: string; cards: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => { bottom.current?.scrollIntoView({ behavior: "smooth" }); }, [msgs]);

  async function send() {
    const text = input.trim();
    if (!text || busy) return;
    const next = [...msgs, { role: "user" as const, content: text }];
    setMsgs(next); setInput(""); setBusy(true); setErr(null);
    const sb = supabaseBrowser();
    const { data, error } = await sb.functions.invoke<Reply>("interview", { body: { messages: next } });
    setBusy(false);
    if (error || !data) { setErr(error?.message ?? "no response"); return; }
    if (data.error) { setErr(`${data.error}${data.detail ? `: ${data.detail}` : ""}`); }
    if (data.reply) setMsgs([...next, { role: "assistant", content: data.reply }]);
    setSpent((s) => s + (data.charged ?? 0));
    if (data.balance != null) setBalance(data.balance);
    if (data.project_id) setDone({ project_id: data.project_id, cards: data.cards ?? 0 });
  }

  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: 24, display: "flex", flexDirection: "column", minHeight: "calc(100vh - 48px)" }}>
      <h1 style={{ margin: "8px 0 4px" }}>Start a project</h1>
      <p style={{ color: "#666", marginTop: 0 }}>
        Tell the interviewer what you want to make. It will ask a few questions, then build your kanban.
        {balance != null && <> · Wallet {honey(balance)}</>}{spent > 0 && <> · this interview {honey(spent)}</>}
      </p>

      <div style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 10, padding: "8px 0" }}>
        {msgs.length === 0 && (
          <div style={{ color: "#888", fontSize: 14 }}>
            Try: “A 90-second radio spot for our film festival: script, three taglines, and a voiceover.”
          </div>
        )}
        {msgs.map((m, i) => (
          <div key={i} style={{ alignSelf: m.role === "user" ? "flex-end" : "flex-start", maxWidth: "85%",
              background: m.role === "user" ? "#f5c542" : "#fff", border: "1px solid #e6e2d6", borderRadius: 10, padding: "10px 14px", whiteSpace: "pre-wrap" }}>
            {m.content}
          </div>
        ))}
        {busy && <div style={{ color: "#888", fontSize: 13 }}>thinking…</div>}
        {done && (
          <div style={{ border: "1px solid #2a7", borderRadius: 10, padding: 14, background: "#f3fbf6" }}>
            <strong>Project created</strong> with {done.cards} cards.{" "}
            <a href={`/projects/${done.project_id}`}>Open the board</a> — fund it from your wallet and nodes will start picking up cards.
          </div>
        )}
        {err && <div style={{ color: "#b00020", fontSize: 13 }}>{err}</div>}
        <div ref={bottom} />
      </div>

      {!done && (
        <form onSubmit={(e) => { e.preventDefault(); send(); }} style={{ display: "flex", gap: 8, paddingTop: 12, borderTop: "1px solid #e6e2d6" }}>
          <input value={input} onChange={(e) => setInput(e.target.value)} placeholder="What do you want to make?" disabled={busy}
                 style={{ flex: 1, padding: 10, fontSize: 15 }} autoFocus />
          <button type="submit" disabled={busy || !input.trim()} style={{ padding: "10px 16px", cursor: "pointer" }}>Send</button>
        </form>
      )}
    </main>
  );
}

export default function NewProject() {
  return <RequireMember next="/new">{() => (<><Nav /><Interview /></>)}</RequireMember>;
}
