"use client";

// Regional servers receive a five-minute project/server-scoped ticket, never an account JWT.
// Permission checks and the overview fallback remain at the hub.
import { useEffect, useRef, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { isTrustedRegionalServer } from "@/lib/trusted-regional-server";

export type LiveState = "connecting" | "live" | "polling";
type Server = { node_id: string; operator?: string; public_url: string | null; status: string };

export function useLive<T>(projectId: string | null, onFrame: (board: T) => void, poll: () => Promise<void>, pollMs = 15000): LiveState {
  const [state, setState] = useState<LiveState>("connecting");
  const onFrameRef = useRef(onFrame); const pollRef = useRef(poll);
  onFrameRef.current = onFrame; pollRef.current = poll;
  useEffect(() => {
    if (!projectId) return;
    let closed = false;
    let ws: WebSocket | null = null;
    let polling: ReturnType<typeof setInterval> | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let renewal: ReturnType<typeof setTimeout> | undefined;
    let quiet: ReturnType<typeof setTimeout> | undefined;
    const refresh = () => { if (!closed) void pollRef.current().catch(() => {}); };
    const stopPolling = () => { clearInterval(polling); polling = undefined; };
    const startPolling = () => {
      if (closed || polling) return;
      setState("polling"); refresh(); polling = setInterval(refresh, pollMs);
    };
    const later = () => {
      if (closed || retry) return;
      startPolling(); retry = setTimeout(() => { retry = undefined; void connect(); }, 15000);
    };
    const connect = async () => {
      if (closed) return;
      try {
        const sb = supabaseBrowser();
        const { data, error } = await sb.rpc("hive_servers");
        if (error || closed) { later(); return; }
        const server = ((data as Server[] | null) ?? []).filter(isTrustedRegionalServer)[0];
        if (!server?.node_id) { later(); return; }
        const { data: ticket, error: denied } = await sb.rpc("hive_live_token_mint", { p_project_id: projectId, p_server_id: server.node_id });
        if (closed) return;
        if (denied || typeof ticket?.token !== "string" || !ticket.token.startsWith("hive_live_v1.") || !Number.isFinite(ticket.expires_at)) { later(); return; }
        const remaining = ticket.expires_at * 1000 - Date.now();
        if (remaining <= 1000 || remaining > 301000) { later(); return; }
        const base = new URL(server.public_url!);
        base.protocol = "wss:"; base.pathname = `/live/${projectId}`; base.search = ""; base.hash = "";
        base.searchParams.set("token", ticket.token);
        const socket = new WebSocket(base.toString()); ws = socket;
        // Refresh before expiry; a refused refresh falls back to authorized hub reads.
        renewal = setTimeout(() => socket.close(), Math.max(1000, remaining - 30000));
        socket.onopen = () => { if (closed || ws !== socket) return; stopPolling(); setState("live"); quiet = setTimeout(startPolling, 60000); };
        socket.onmessage = (event) => {
          if (closed || ws !== socket) return;
          try { const message = JSON.parse(event.data as string); if (message.type === "board" && message.project_id === projectId) { onFrameRef.current(message.board as T); clearTimeout(quiet); quiet = setTimeout(startPolling, 60000); } } catch { /* ignore malformed frame */ }
        };
        socket.onerror = () => { if (!closed && ws === socket) startPolling(); };
        socket.onclose = () => { if (closed || ws !== socket) return; ws = null; clearTimeout(renewal); clearTimeout(quiet); later(); };
      } catch { later(); }
    };
    startPolling(); void connect();
    return () => { closed = true; stopPolling(); clearTimeout(retry); clearTimeout(renewal); clearTimeout(quiet); ws?.close(); };
  }, [projectId, pollMs]);
  return state;
}
