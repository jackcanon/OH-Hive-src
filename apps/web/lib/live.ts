"use client";

// Live board updates from a regional server (ADR-013 §A.4) with a polling fallback.
//
// The server exposes `ws(s)://<public_url>/live/<project_id>?token=<supabase jwt>` and pushes a
// frame whenever the board changes. We pick an online server from hive.servers() (nearest region
// first — the RPC already orders by region), connect, and hand frames to the caller. If no server
// is reachable, the socket fails, or it goes quiet, the caller's `poll` runs every 15 s instead.

import { useEffect, useRef, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";

export type LiveState = "connecting" | "live" | "polling";

type Server = { public_url: string | null; status: string; region: string | null };

async function pickServer(): Promise<string | null> {
  const { data } = await supabaseBrowser().rpc("hive_servers");
  const servers = (data as Server[] | null) ?? [];
  const online = servers.filter((s) => s.status === "online" && s.public_url);
  return online[0]?.public_url ?? null;
}

export function useLive<T>(projectId: string | null, onFrame: (board: T) => void, poll: () => Promise<void>, pollMs = 15000): LiveState {
  const [state, setState] = useState<LiveState>("connecting");
  const onFrameRef = useRef(onFrame);
  const pollRef = useRef(poll);
  onFrameRef.current = onFrame;
  pollRef.current = poll;

  useEffect(() => {
    if (!projectId) return;
    let ws: WebSocket | null = null;
    let closed = false;
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let quiet: ReturnType<typeof setTimeout> | null = null;

    const startPolling = () => {
      if (pollTimer) return;
      setState("polling");
      pollRef.current();
      pollTimer = setInterval(() => pollRef.current(), pollMs);
    };
    const stopPolling = () => { if (pollTimer) { clearInterval(pollTimer); pollTimer = null; } };

    (async () => {
      // Always load once immediately so the page isn't empty while the socket connects.
      await pollRef.current();
      const base = await pickServer();
      const { data: { session } } = await supabaseBrowser().auth.getSession();
      if (!base || !session?.access_token || closed) { startPolling(); return; }
      const url = base.replace(/^http/, "ws").replace(/\/$/, "") + `/live/${projectId}?token=${encodeURIComponent(session.access_token)}`;
      try { ws = new WebSocket(url); } catch { startPolling(); return; }
      ws.onopen = () => { stopPolling(); setState("live"); };
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data as string);
          if (msg.type === "board") onFrameRef.current(msg.board as T);
          // if the server stops talking for 60 s, quietly poll as well
          if (quiet) clearTimeout(quiet);
          quiet = setTimeout(() => pollRef.current(), 60000);
        } catch { /* ignore */ }
      };
      ws.onerror = () => { startPolling(); };
      ws.onclose = () => { if (!closed) startPolling(); };
    })();

    return () => {
      closed = true;
      stopPolling();
      if (quiet) clearTimeout(quiet);
      try { ws?.close(); } catch { /* ignore */ }
    };
  }, [projectId, pollMs]);

  return state;
}
