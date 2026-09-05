import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AboutSection, type AboutInfo } from "@ohhive/ui";

// Preferences panes per ADR-010 decision 10. Everything but About is a stub
// until the coordinator client exists.
const PANES = ["Node", "Trust", "Backends & Models", "Server role", "Earnings", "About"] as const;
type Pane = (typeof PANES)[number];

export function App() {
  const [pane, setPane] = useState<Pane>("Node");
  const [about, setAbout] = useState<AboutInfo | null>(null);

  useEffect(() => {
    invoke<AboutInfo>("about").then(setAbout).catch(() => setAbout(null));
  }, []);

  return (
    <div style={{ display: "flex", height: "100vh", fontFamily: "system-ui, sans-serif" }}>
      <nav style={{ width: 200, borderRight: "1px solid #ddd", padding: 12 }}>
        <h1 style={{ fontSize: 16, margin: "4px 0 16px" }}>OH Hive</h1>
        {PANES.map((p) => (
          <button
            key={p}
            onClick={() => setPane(p)}
            style={{
              display: "block",
              width: "100%",
              textAlign: "left",
              padding: "6px 8px",
              margin: "2px 0",
              border: 0,
              background: p === pane ? "#eee" : "transparent",
              cursor: "pointer",
            }}
          >
            {p}
          </button>
        ))}
      </nav>
      <main style={{ flex: 1, padding: 24, overflow: "auto" }}>
        <h2 style={{ marginTop: 0 }}>{pane}</h2>
        {pane === "About" ? (
          about ? <AboutSection info={about} /> : <p>Loading…</p>
        ) : (
          <p style={{ color: "#666" }}>Not implemented yet — see ADR-010.</p>
        )}
      </main>
    </div>
  );
}
