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
  const [top, setTop] = useState(57);

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
      if (nav) setTop(nav.getBoundingClientRect().height);
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
      <aside className={open ? "hive-sidebar open" : "hive-sidebar"} style={{ top }}>
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
            {m.hosting && <span title="Hosting a server" style={{ fontSize: 11 }}>🖥️</span>}
            {m.working && <span title="Working in the Hive" style={{ fontSize: 11 }}>⚙️</span>}
          </div>
        ))}
        <div className="hive-sidebar-footer">
          <a href="/members">View all &amp; bios →</a>
        </div>
      </aside>
    </>
  );
}
