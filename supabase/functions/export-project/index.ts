// OH Hive — project export (download-as-zip).
//
// GET /export-project?project_id=<uuid>   Authorization: Bearer <member's Supabase JWT>
// → application/zip (Content-Disposition: attachment)
//
// Jack, 2026-09-08: "once an artifact is delivered, how does the project owner get it?" Today a
// card's deliverable is a single LLM text completion sitting in hive.card_outputs.content -- not a
// real file tree, not a Hive artifact, nothing downloadable. This function is the stopgap: it
// reads the same board data the project page already renders (via hive_project_board, so it's
// gated by the same hive.is_member() check and RLS as everything else a member can already see --
// no new permission surface), best-effort-splits each card's output back into the individual files
// the model wrote inline (cards prompt the model with a "### filename" + fenced-code-block
// convention -- see card outputs like "### main.py\npython\n...\n"), and zips the result.
//
// This is explicitly NOT a build/compile/test pipeline -- see the 2026-09-08 discussion on why the
// sandbox can't run real toolchains today. It just gets the generated text out of the database and
// onto the owner's disk as separate files instead of one long scroll in the browser.
//
// CORS: same reasoning as interview/index.ts -- the web app calls this cross-origin, so every
// response (including errors) needs Access-Control-Allow-Origin or the browser swallows it.

import { createClient } from "npm:@supabase/supabase-js@2";
import JSZip from "npm:jszip@3.10.1";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "GET, OPTIONS",
};

function json(body: unknown, init?: ResponseInit) {
  return Response.json(body, { ...init, headers: { ...corsHeaders, ...(init?.headers ?? {}) } });
}

type Card = {
  key: string;
  title: string;
  status: string;
  modality: string;
  output: { content: string | null } | null;
};
type Board = {
  project: { id: string; title: string; goal: string } | null;
  cards: Card[];
};

// Cards are prompted to lay out multi-file output as repeated "### <filename>" headers each
// followed by a fenced code block. Split back into (name, body) pairs; if the model's run ended
// mid-file (seen in practice on a small local model with no continuation step) the trailing fence
// is just missing -- keep the partial content rather than dropping the file, and say so.
function splitFiles(content: string): { name: string; body: string }[] {
  const lines = content.split("\n");
  const files: { name: string; body: string }[] = [];
  let currentName: string | null = null;
  let buf: string[] = [];
  const flush = () => {
    if (currentName) {
      let body = buf.join("\n");
      body = body.replace(/^```[a-zA-Z0-9_+-]*\r?\n/, "");
      if (body.trimEnd().endsWith("```")) {
        body = body.trimEnd().slice(0, -3);
      } else {
        body += "\n\n[truncated -- the model's output ended before this file was finished]\n";
      }
      const safe = currentName.replace(/\.\.+/g, "").replace(/^[/\\]+/, "").trim();
      if (safe) files.push({ name: safe, body });
    }
    buf = [];
  };
  for (const line of lines) {
    const m = /^###\s+(\S+)\s*$/.exec(line);
    if (m) {
      flush();
      currentName = m[1];
    } else if (currentName) {
      buf.push(line);
    }
  }
  flush();
  return files;
}

function slugify(title: string): string {
  return (
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "oh-hive-project"
  );
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response(null, { headers: corsHeaders });
  if (req.method !== "GET") return json({ error: "method_not_allowed" }, { status: 405 });

  const projectId = new URL(req.url).searchParams.get("project_id");
  if (!projectId) return json({ error: "missing_project_id" }, { status: 400 });

  const auth = req.headers.get("Authorization") ?? "";
  if (!auth) return json({ error: "unauthenticated" }, { status: 401 });

  const url = Deno.env.get("SUPABASE_URL")!;
  const userClient = createClient(url, Deno.env.get("SUPABASE_ANON_KEY")!, {
    global: { headers: { Authorization: auth } },
  });

  const { data, error } = await userClient.rpc("hive_project_board", { p_project_id: projectId });
  if (error) {
    console.error("export-project: hive_project_board failed", error);
    return json({ error: "board_fetch_failed", detail: error.message }, { status: 500 });
  }
  const board = data as Board;
  if (!board?.project) return json({ error: "project_not_found_or_not_visible" }, { status: 404 });

  const zip = new JSZip();
  const p = board.project;
  const cardsWithOutput = board.cards.filter((c) => c.output?.content);

  const manifestLines = [
    `# ${p.title}`,
    "",
    p.goal,
    "",
    "Exported from OH Hive (ohghive.com). This is a snapshot of what the Hive has produced so far --",
    "not a build, and not guaranteed to compile or run as-is. See each card folder's own files.",
    "",
    "## Cards",
    "",
    ...board.cards.map((c) => `- **${c.key}** (${c.status}) -- ${c.title}`),
  ];
  zip.file("README.md", manifestLines.join("\n"));

  for (const card of cardsWithOutput) {
    const content = card.output!.content!;
    const files = splitFiles(content);
    const folder = zip.folder(card.key)!;
    if (files.length > 0) {
      for (const f of files) folder.file(f.name, f.body);
    } else {
      // No "### filename" convention detected -- ship the raw output so nothing is lost.
      folder.file("output.md", content);
    }
  }

  const bytes = await zip.generateAsync({ type: "uint8array" });
  const filename = `${slugify(p.title)}.zip`;
  return new Response(bytes, {
    headers: {
      ...corsHeaders,
      "Content-Type": "application/zip",
      "Content-Disposition": `attachment; filename="${filename}"`,
    },
  });
});
