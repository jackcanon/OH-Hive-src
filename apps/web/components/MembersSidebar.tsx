"use client";

import { useEffect, useState } from "react";
import { supabaseBrowser } from "@/lib/supabase";
import { Avatar } from "@/components/Avatar";

// Persistent chrome (Jack, 2026-09-12): "the members page to show up on every screen, anchored to
// the right side of the screen, under the top header bar." This replaces navigating to /members as
// the primary way to see who's around -- /members itself still exists (linked at the bottom) for
// the full list with bios. Same hive_member_directory() RPC as that page, just a compact render.
//
// Mounted once in the root layout so it shows on every route without every page opting in. It
// quietly renders nothing for a signed-out visitor or a non-member (the RPC just errors for them).

type Member = {
  id: string;
  display_name: string;
  avatar_choice: string;
  google_avatar_url: string | null;
  custom_avatar_url: string | null;
  bio: string;
  is_admin: boolean;
  joined: string;
  online: boolean;
  working: boolean;
  hosting: boolean;
  regions: string[];
};

const REFRESH_MS = 20000;

export function MembersSidebar() {
  const [members, setMembers] = useState<Member[] | null>(null);
  const [open, setOpen] = useState(false);
  const [navHeight, setNavHeight] = useState(57);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      supabaseBrowser().rpc("hive_member_directory", {}).then(({ data, error }) => {
        if (cancelled) return;
        if (error) { setMembers(null); return; } // signed out / not a member -- render nothing
        setMembers((data ?? []) as Member[]);
      });
    };
    load();
    const t = setInterval(load, REFRESH_MS);
    return () => { cancelled = true; clearInterval(t); };
  }, []);

  useEffect(() => {
    function measure() {
      const nav = document.querySelector("nav.hive");
      if (nav) setNavHeight(nav.getBoundingClientRect().height);
    }
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  useEffect(() => {
    document.body.classList.toggle("has-hive-sidebar", !!members);
    return () => document.body.classList.remove("has-hive-sidebar");
  }, [members]);

  if (!members) return null;

  const sorted = [...members].sort((a, b) => Number(b.online) - Number(a.online));
  const onlineCount = members.filter((m) => m.online).length;

  return (
    <>
      <button
        type="button"
        className="hive-sidebar-toggle"
        aria-label="Toggle members"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {onlineCount > 0 ? onlineCount : "◷"}
      </button>
      <aside className={open ? "hive-sidebar open" : "hive-sidebar"} style={{ paddingTop: navHeight + 16 }}>
        <div className="hive-sidebar-header">
          <h2>Members · {members.length}</h2>
        </div>
        {sorted.map((m) => (
          <div key={m.id} className="hive-sidebar-row" title={m.bio || m.display_name}>
            <Avatar choice={m.avatar_choice} googleUrl={m.google_avatar_url} customUrl={m.custom_avatar_url} name={m.display_name} size={26} />
            <span
              className="hive-sidebar-dot"
              style={{ background: m.online ? "var(--ok)" : "var(--border)" }}
              aria-hidden
            />
            <span className="hive-sidebar-name">{m.display_name}</span>
            {m.hosting && (
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--muted-strong)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
                <title>Hosting a server</title>
                <rect x="3" y="4" width="18" height="16" rx="1.5" />
                <path d="M6 8h.01M6 16h.01M10 8h8M10 16h8" />
              </svg>
            )}
            {m.working && (
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--gold)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
                <title>Working in the Hive</title>
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
              </svg>
            )}
          </div>
        ))}
        <div className="hive-sidebar-footer">
          <a href="/members">View all &amp; bios →</a>
        </div>
      </aside>
    </>
  );
}
