import { AboutSection } from "@ohhive/ui";

export default function Settings() {
  return (
    <main style={{ maxWidth: 720, margin: "64px auto", padding: "0 24px" }}>
      <h1>Settings</h1>
      <p style={{ color: "#666" }}>Account, nodes, and notifications land with the hive schema.</p>
      <h2 style={{ fontSize: 16, marginTop: 32 }}>About</h2>
      <AboutSection
        info={{
          app_version: "0.1.0",
          core_version: "web",
          made_by: "Happy Jack Media",
          made_by_url: "https://happyjack.media",
          blog_name: "This Is Not A Draft",
          blog_url: "https://thisisnotadraft.com",
        }}
      />
    </main>
  );
}
