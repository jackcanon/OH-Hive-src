"use client";

import { FormEvent, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { Nav, RequireMember } from "@/components/RequireMember";
import { supabaseBrowser } from "@/lib/supabase";
import { friendlyError } from "@/lib/errors";

const providerLabel = (p: string) => p === "anthropic" ? "Anthropic" : p === "nous" ? "Nous Portal" : "OpenAI";

type Project = { id: string; title: string };
type Keys = Record<string, { preferred_model?: string | null }>;
function NewSession() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [keys, setKeys] = useState<Keys>({});
  const [loading, setLoading] = useState(true);
  const [project, setProject] = useState("");
  const [task, setTask] = useState("");
  const [source, setSource] = useState("path");
  const [path, setPath] = useState("");
  const [repo, setRepo] = useState("");
  const [ref, setRef] = useState("");
  const [brain, setBrain] = useState("local");
  const [model, setModel] = useState("");
  const [turns, setTurns] = useState(40);
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<{ card_id: string; project_id: string } | null>(null);
  const submission = useRef<{ payload: string; id: string } | null>(null);
  const submitting = useRef(false);
  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [p, k] = await Promise.all([supabaseBrowser().rpc("hive_code_session_projects"), supabaseBrowser().rpc("hive_member_keys_status")]);
        if (p.error || k.error) throw new Error(p.error?.message ?? k.error?.message);
        if (active) { setProjects(p.data ?? []); setProject(p.data?.[0]?.id ?? ""); setKeys(k.data ?? {}); }
      } catch (e) { if (active) setError(friendlyError(e instanceof Error ? e.message : "Unable to load your projects.")); }
      finally { if (active) setLoading(false); }
    }
    load(); return () => { active = false; };
  }, []);

  async function submit(e: FormEvent) {
    e.preventDefault(); if (submitting.current || created) return;
    submitting.current = true; setBusy(true); setError(null);
    const args = { p_project_id: project, p_task: task.trim(), p_workspace_path: source === "path" ? path.trim() : null,
      p_repo_url: source === "repo" ? repo.trim() : null, p_repo_ref: source === "repo" ? ref.trim() || null : null,
      p_brain: brain, p_model_id: brain === "local" ? model.trim() || null : null,
      p_max_turns: turns, p_cloud_consent: brain !== "local" && consent };
    const payload = JSON.stringify(args);
    if (submission.current?.payload !== payload) submission.current = { payload, id: crypto.randomUUID() };
    try {
      const { data, error } = await supabaseBrowser().rpc("hive_code_session_create", { ...args, p_request_id: submission.current.id });
      if (error) throw new Error(error.message);
      if (!data?.card_id) throw new Error("Session creation returned no card. Please retry.");
      setCreated(data);
    } catch (e) { setError(friendlyError(e instanceof Error ? e.message : "Unable to create the session.")); }
    finally { submitting.current = false; setBusy(false); }
  }
  const field = { display: "grid", gap: 6, marginBottom: 18 };
  return <main style={{ maxWidth: 720, margin: "0 auto", padding: "24px 16px" }}>
    <Link href="/fleet">← Private Fleet</Link>
    <h1>Start a coding session</h1>
    <p>Give your own computers a task. A local model or your chosen cloud provider guides the work; your computer reads files and runs commands.</p>
    {error && <p role="alert" style={{ color: "var(--danger, #c0392b)" }}>{error}</p>}
    {loading ? <p>Loading your projects and connected providers…</p> : created ? <div role="status">
      <h2>Session queued</h2><p>Your task is ready for an eligible computer in your fleet.</p>
      <Link href={`/projects/${created.project_id}`}>View the task on your project board</Link>
    </div> : !projects.length ? <p>You need a project you own with local execution enabled. <Link href="/projects">Open your projects</Link> to choose one, or <Link href="/new">create a project</Link>.</p> : <form onSubmit={submit}>
      <fieldset disabled={busy} style={{ border: 0, padding: 0, margin: "24px 0" }}>
        <label style={field}>Project<select required value={project} onChange={e => setProject(e.target.value)}>{projects.map(p => <option key={p.id} value={p.id}>{p.title}</option>)}</select></label>
        <label style={field}>What should the agent do?<textarea required rows={5} maxLength={20000} value={task} onChange={e => setTask(e.target.value)} placeholder="Describe the change and how to check that it works." /></label>
        <label style={field}>Where is the code?<select value={source} onChange={e => setSource(e.target.value)}><option value="path">Existing folder on my fleet computer</option><option value="repo">Clone a repository</option></select></label>
        {source === "path" ? <label style={field}>Full folder path<input required maxLength={4096} value={path} onChange={e => setPath(e.target.value)} placeholder="/home/me/project or C:\Users\me\project" /><small>This path must exist on the computer that claims the task. This first version does not select a specific computer.</small></label> : <>
          <label style={field}>Repository URL<input required maxLength={2048} value={repo} onChange={e => setRepo(e.target.value)} placeholder="https://github.com/you/project.git" /><small>Use HTTPS or git@host:repository. Private repositories need access configured on the computer. Do not include passwords or tokens.</small></label>
          <label style={field}>Branch, tag, or commit (optional)<input maxLength={255} value={ref} onChange={e => setRef(e.target.value)} /></label>
        </>}
        <label style={field}>Who guides the work?<select value={brain} onChange={e => { setBrain(e.target.value); setConsent(false); }}><option value="local">Local — the computer’s model</option>{["anthropic", "openai", "nous"].filter(p => keys[p]).map(p => <option key={p} value={p}>{providerLabel(p)} — my API key</option>)}</select></label>
        {brain === "local" ? <label style={field}>Installed model ID (optional)<input maxLength={200} value={model} onChange={e => setModel(e.target.value)} /><small>Leave blank to use the computer’s configured model.</small></label> : <>
          <p>Model: {keys[brain]?.preferred_model || "provider default"}. Manage models and API keys in <Link href="/settings">Settings</Link>.</p>
          <label style={{ ...field, display: "block" }}><input type="checkbox" required checked={consent} onChange={e => setConsent(e.target.checked)} /> I allow this session’s task, conversation, and tool results (including file contents) to be sent to {providerLabel(brain)} using my API key. Provider charges apply.</label>
        </>}
        <label style={field}>Maximum agent turns<input type="number" required min={1} max={100} value={turns} onChange={e => setTurns(Number(e.target.value))} /><small>A turn limit bounds the session length; it is not a spending limit.</small></label>
        <p>Your computer must be checked in with coding tools enabled. Repository cloning and cloud guidance also require internet access. Commands run with the worker’s permissions.</p>
        <button type="submit" disabled={!project || !task.trim() || (brain !== "local" && !consent)}>{busy ? "Queuing session…" : "Start coding session"}</button>
      </fieldset>
      <p><Link href="/settings">Connect or manage a cloud provider</Link></p>
    </form>}
  </main>;
}
export default function Page() { return <RequireMember next="/code/new">{() => <><Nav /><NewSession /></>}</RequireMember>; }
