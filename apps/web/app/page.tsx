import Link from "next/link";

// Surfaces per ADR-009: interview, kanban, wallet, Hive browser, settings.
// Each is a stub route until the hive schema and Edge Functions land.
const ROUTES = [
  ["/new", "Start a project (interview)"],
  ["/projects", "Browse the Hive"],
  ["/wallet", "$honey wallet"],
  ["/settings", "Settings"],
] as const;

export default function Home() {
  return (
    <main style={{ maxWidth: 720, margin: "64px auto", padding: "0 24px" }}>
      <h1 style={{ marginBottom: 4 }}>OH Hive</h1>
      <p style={{ color: "#666", marginTop: 0 }}>Invite-only. Sign in to see the Hive.</p>
      <ul>
        {ROUTES.map(([href, label]) => (
          <li key={href}>
            <Link href={href}>{label}</Link>
          </li>
        ))}
      </ul>
    </main>
  );
}
