import { redirect } from "next/navigation";

// Moved into Settings as a collapsible section (Jack, 2026-09-12) -- old bookmarks/links to
// /admin still land somewhere useful instead of 404ing. See app/settings/page.tsx's AdminSection.
export default function AdminPage() {
  redirect("/settings#admin");
}
