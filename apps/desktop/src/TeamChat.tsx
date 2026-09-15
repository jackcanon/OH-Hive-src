import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// ADR-035 C1 team chat (2026-09-15): a Bots DM with this Mac's own local agent, through the
// same LocalHubStore the `hive bots` CLI and the Swift bridge both use. The app's own
// background drain loop (Rust side, `bots.rs`'s spawn_drain_loop) is what actually replies --
// no separate `hive bots work` process needed while this app is open.

type BotsAgent = { id: string; name: string; preferred_host: string | null; archived: boolean };
type BotsConversation = { id: string; coordinator: string | null; policy_revision: number };
type BotsMessage = { id: string; server_sequence: number; author: "you" | "agent"; body: string | null; created_at: string };

export function TeamChat() {
  const [agents, setAgents] = useState<BotsAgent[] | null>(null);
  const [agent, setAgent] = useState<BotsAgent | null>(null);
  const [conversation, setConversation] = useState<BotsConversation | null>(null);
  const [messages, setMessages] = useState<BotsMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const lastSeq = useRef(0);

  const loadAgents = useCallback(() => {
    invoke<BotsAgent[]>("bots_agents_list")
      .then((list) => {
        setAgents(list);
        setErr(null);
        setAgent((prev) => prev ?? list.find((a) => !a.archived) ?? list[0] ?? null);
      })
      .catch((e) => setErr(String(e)));
  }, []);

  useEffect(() => {
    loadAgents();
  }, [loadAgents]);

  useEffect(() => {
    if (!agent) {
      setConversation(null);
      return;
    }
    invoke<BotsConversation>("bots_dm_open", { agentId: agent.id })
      .then((c) => {
        setConversation(c);
        setMessages([]);
        lastSeq.current = 0;
      })
      .catch((e) => setErr(String(e)));
  }, [agent]);

  const poll = useCallback(() => {
    if (!conversation) return;
    invoke<BotsMessage[]>("bots_messages_list", {
      conversationId: conversation.id,
      after: lastSeq.current || null,
    })
      .then((fresh) => {
        if (fresh.length === 0) return;
        setMessages((prev) => [...prev, ...fresh]);
        lastSeq.current = fresh[fresh.length - 1].server_sequence;
      })
      .catch((e) => setErr(String(e)));
  }, [conversation]);

  useEffect(() => {
    if (!conversation) return;
    poll();
    const t = setInterval(poll, 2000);
    return () => clearInterval(t);
  }, [conversation, poll]);

  const register = async () => {
    setBusy(true);
    setErr(null);
    try {
      const a = await invoke<BotsAgent>("bots_agent_register", { name: null });
      setAgents((prev) => [...(prev ?? []), a]);
      setAgent(a);
    } catch (e) {
      setErr(String(e));
    }
    setBusy(false);
  };

  const send = async () => {
    if (!conversation || !agent || !draft.trim()) return;
    setBusy(true);
    setErr(null);
    const text = draft;
    setDraft("");
    try {
      await invoke("bots_dm_send", {
        conversationId: conversation.id,
        agentId: agent.id,
        expectedPolicyRevision: conversation.policy_revision,
        text,
      });
      poll();
    } catch (e) {
      setErr(String(e));
    }
    setBusy(false);
  };

  if (agents === null) return <p className="muted">…</p>;

  return (
    <>
      {err && <div className="banner">{err}</div>}
      {agents.length === 0 ? (
        <div className="card">
          <h2>No agents yet</h2>
          <p className="muted" style={{ fontSize: 12, marginTop: 0 }}>
            Register this Mac as a Bots agent to start a team chat with it.
          </p>
          <button className="primary" disabled={busy} onClick={register}>
            Register this Mac
          </button>
        </div>
      ) : (
        <>
          {agents.length > 1 && (
            <div className="card">
              <label className="field">Agent</label>
              <select
                value={agent?.id ?? ""}
                onChange={(e) => setAgent(agents.find((a) => a.id === e.target.value) ?? null)}
              >
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            </div>
          )}
          <div className="card" style={{ display: "flex", flexDirection: "column", minHeight: 320 }}>
            <div className="log" style={{ flex: 1, overflowY: "auto" }}>
              {messages.length === 0 && <div className="muted">No messages yet — say hello.</div>}
              {messages.map((m) => (
                <div key={m.id}>
                  <span className="at">{new Date(m.created_at).toLocaleTimeString()}</span>
                  <span style={{ flex: 1 }}>
                    <strong>{m.author === "you" ? "You" : agent?.name ?? "Agent"}:</strong>{" "}
                    {m.body ?? "(no body)"}
                  </span>
                </div>
              ))}
            </div>
            <div className="row" style={{ marginTop: 8 }}>
              <input
                style={{ flex: 1 }}
                value={draft}
                placeholder={`Message ${agent?.name ?? "agent"}…`}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !busy) send();
                }}
              />
              <button className="primary" disabled={busy || !draft.trim()} onClick={send}>
                Send
              </button>
            </div>
          </div>
        </>
      )}
    </>
  );
}
