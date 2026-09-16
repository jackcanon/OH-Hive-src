import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type BotsAgent = { id: string; name: string; preferred_host: string | null; archived: boolean };
type BotsConversation = { id: string; title: string | null; kind: string; project_id: string | null; coordinator: string | null; policy_revision: number };
type BotsMessage = { id: string; server_sequence: number; author: "you" | "agent" | "system"; author_id: string; body: string | null; created_at: string };
type Mentions = { recipient_ids: string[]; unresolved: string[] };
type Project = { id: string; title: string };
type Pending = { text: string; conversationId: string; recipientIds: string[]; expectedPolicyRevision: number; requestId: string };

export function TeamChat() {
  const [agents, setAgents] = useState<BotsAgent[]>([]);
  const [rooms, setRooms] = useState<BotsConversation[]>([]);
  const [selection, setSelection] = useState("");
  const selected = useRef(selection); selected.current = selection;
  const [conversation, setConversation] = useState<BotsConversation | null>(null);
  const [messages, setMessages] = useState<BotsMessage[]>([]);
  const [draft, setDraft] = useState("");
  const drafts = useRef<Record<string, string>>({});
  const pending = useRef<Record<string, Pending>>({});
  const [err, setErr] = useState<string | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const sending = useRef(false);
  const [creating, setCreating] = useState(false);
  const [title, setTitle] = useState("");
  const [members, setMembers] = useState<string[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [project, setProject] = useState("");

  const refresh = useCallback(async () => {
    try {
      const [list, roomList] = await Promise.all([
        invoke<BotsAgent[]>("bots_agents_list"), invoke<BotsConversation[]>("bots_rooms_list"),
      ]);
      const active = list.filter(a => !a.archived);
      setAgents(active); setRooms(roomList);
      setSelection(prev => prev || active[0]?.id || (roomList[0] ? `room:${roomList[0].id}` : ""));
    } catch (e) { setErr(String(e)); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let cursor: number | null = null;
    setConversation(null); setMessages([]); setErr(null); setSendError(null); setNote(null);
    setDraft(drafts.current[selection] ?? "");
    const run = async () => {
      if (!selection) return;
      try {
        const c = selection.startsWith("room:")
          ? (await invoke<BotsConversation[]>("bots_rooms_list")).find(r => `room:${r.id}` === selection)
          : await invoke<BotsConversation>("bots_dm_open", { agentId: selection });
        if (cancelled) return;
        if (!c) throw new Error("Room no longer available");
        setConversation(c);
        const poll = async () => {
          try {
            const batch = await invoke<BotsMessage[]>("bots_messages_list", { conversationId: c.id, after: cursor });
            if (cancelled) return;
            if (batch.length) {
              cursor = batch[batch.length - 1].server_sequence;
              setMessages(prev => { const ids = new Set(prev.map(m => m.id)); return [...prev, ...batch.filter(m => !ids.has(m.id))]; });
            }
            setErr(null);
            timer = setTimeout(poll, batch.length === 200 ? 0 : 2000);
          } catch (e) {
            if (!cancelled) { setErr(String(e)); timer = setTimeout(poll, 2000); }
          }
        };
        await poll();
      } catch (e) { if (!cancelled) setErr(String(e)); }
    };
    void run();
    return () => { cancelled = true; if (timer) clearTimeout(timer); };
  }, [selection]);

  const send = async () => {
    if (busy || sending.current || !conversation || !draft.trim()) return;
    sending.current = true;
    const key = selection; const text = draft.trim(); const c = conversation;
    setBusy(true); setSendError(null);
    try {
      let request = pending.current[key];
      if (!request || request.text !== text || request.conversationId !== c.id) {
        const mentions = c.kind === "agent_dm"
          ? { recipient_ids: c.coordinator ? [c.coordinator] : [], unresolved: [] }
          : await invoke<Mentions>("bots_mentions_resolve", { conversationId: c.id, text });
        if (selected.current !== key) return;
        setNote(mentions.unresolved.length ? `Not notified: ${mentions.unresolved.map(n => `@${n}`).join(", ")}` : null);
        request = { text, conversationId: c.id, recipientIds: mentions.recipient_ids, expectedPolicyRevision: c.policy_revision, requestId: crypto.randomUUID() };
        pending.current[key] = request;
      }
      await invoke("bots_chat_send", request);
      delete pending.current[key];
      if (drafts.current[key]?.trim() === text) drafts.current[key] = "";
      if (selected.current === key) setDraft(current => current.trim() === text ? "" : current);
    } catch (e) { if (selected.current === key) setSendError(`Message not confirmed. Retry the same text safely. ${String(e)}`); }
    finally { sending.current = false; setBusy(false); }
  };
  const register = async () => {
    setBusy(true);
    try {
      const a = await invoke<BotsAgent>("bots_agent_register", { name: title.trim() || null });
      await refresh(); setSelection(a.id); setTitle("");
    } catch (e) { setErr(String(e)); } finally { setBusy(false); }
  };
  const roomRetry = useRef<{ details: string; requestId: string } | null>(null);
  const creatingRoom = useRef(false);
  const create = async () => {
    if (creatingRoom.current) return;
    creatingRoom.current = true;
    setBusy(true);
    try {
      const details = JSON.stringify([title.trim(), [...new Set(members)].sort(), project || null]);
      if (roomRetry.current?.details !== details) roomRetry.current = { details, requestId: crypto.randomUUID() };
      const room = await invoke<BotsConversation>("bots_room_create", { requestId: roomRetry.current.requestId, title, agentIds: members, projectId: project || null });
      await refresh(); setSelection(`room:${room.id}`); setCreating(false); setTitle(""); setMembers([]); setProject(""); roomRetry.current = null;
    } catch (e) { setErr(String(e)); } finally { creatingRoom.current = false; setBusy(false); }
  };
  return <>
    {(sendError ?? err) && <div className="banner" role="alert">{sendError ?? err}</div>}
    <div className="card">
      <label className="field" htmlFor="bots-conversation">Conversation</label>
      <select id="bots-conversation" value={selection} onChange={e => setSelection(e.target.value)}>
        <option value="">Choose an agent or room</option>
        <optgroup label="Rooms">{rooms.map(r => <option key={r.id} value={`room:${r.id}`}>{r.title ?? "Room"}{r.kind === "project" ? " · Project" : ""}</option>)}</optgroup>
        <optgroup label="Agents">{agents.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</optgroup>
      </select>
      <div className="row" style={{ marginTop: 12 }}>
        <button onClick={() => void refresh()}>Refresh</button>
        <button disabled={!agents.length || busy} onClick={() => setCreating(!creating)}>New room</button>
        <input aria-label="New local agent name" placeholder="New local agent name" value={title} onChange={e => setTitle(e.target.value)} disabled={creating || busy} />
        <button disabled={busy || creating} onClick={register}>Add local agent</button>
      </div>
    </div>
    {creating && <section className="card" aria-label="New room">
      <h2>New room</h2>
      <label className="field">Room name<input value={title} onChange={e => setTitle(e.target.value)} /></label>
      <label className="field">Project<select value={project} onChange={e => setProject(e.target.value)}>
        <option value="">None — team room</option>{projects.map(p => <option key={p.id} value={p.id}>{p.title}</option>)}
      </select></label>
      <button onClick={() => { invoke<Project[]>("bots_projects_list").then(setProjects).catch(e => setErr(String(e))); }}>Load my Hive projects</button>
      <p className="muted">Choose up to 16 agents. Only local agents hosted on this computer can reply in this demo.</p>
      {agents.map(a => <label key={a.id} style={{ display: "block" }}><input type="checkbox" checked={members.includes(a.id)} disabled={!members.includes(a.id) && members.length >= 16} onChange={e => setMembers(prev => e.target.checked ? [...prev, a.id] : prev.filter(id => id !== a.id))} /> {a.name}</label>)}
      <button onClick={() => setCreating(false)} disabled={busy}>Cancel</button>
      <button className="primary" onClick={create} disabled={busy || !title.trim() || new TextEncoder().encode(title).length > 200 || !members.length}>Create room</button>
    </section>}
    <div className="card" style={{ display: "flex", flexDirection: "column", minHeight: 320 }}>
      <h2>{conversation?.title ?? agents.find(a => a.id === selection)?.name ?? "Bots"}</h2>
      <p className="muted">Private history on this computer. In rooms, use @names or @everyone for replies; agents do not trigger one another.</p>
      {note && <p role="status">{note}</p>}
      <div className="log" style={{ flex: 1, overflowY: "auto" }}>
        {!messages.length && <p className="muted">No messages yet.</p>}
        {messages.map(m => <div key={m.id}>
          <span className="at">{new Date(m.created_at).toLocaleTimeString()}</span>
          <span style={{ flex: 1, whiteSpace: "pre-wrap" }}><strong>{m.author === "system" ? "System" : m.author === "you" ? "You" : agents.find(a => a.id === m.author_id)?.name ?? `Agent ${m.author_id.slice(0, 8)}`}:</strong> {m.body ?? "(no body)"}</span>
        </div>)}
      </div>
      <div className="row" style={{ marginTop: 8 }}>
        <input style={{ flex: 1 }} aria-label="Message" value={draft} placeholder="Message…" onChange={e => { setDraft(e.target.value); drafts.current[selection] = e.target.value; }} onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) void send(); }} />
        <button className="primary" disabled={busy || !conversation || !draft.trim() || new TextEncoder().encode(draft).length > 65536} onClick={send}>Send</button>
      </div>
    </div>
  </>;
}
