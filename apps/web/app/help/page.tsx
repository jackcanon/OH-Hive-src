"use client";

import type { ReactNode } from "react";
import { Nav } from "@/components/RequireMember";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section style={{ marginTop: 32 }}>
      <h2 style={{ fontSize: 18, marginBottom: 10 }}>{title}</h2>
      <div style={{ color: "var(--muted-strong)", lineHeight: 1.6 }}>{children}</div>
    </section>
  );
}

function Term({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div style={{ marginTop: 10 }}>
      <strong style={{ color: "var(--fg)" }}>{term}</strong> — {children}
    </div>
  );
}

export default function HelpPage() {
  return (
    <>
      <Nav />
      <main style={{ maxWidth: 720, margin: "0 auto", padding: "24px 24px 64px" }}>
        <h1 style={{ marginTop: 8 }}>How Hive works</h1>
        <p style={{ color: "var(--muted-strong)" }}>
          A quick glossary for the terms and board states you&apos;ll see around the Hive.
        </p>

        <Section title="Honey">
          <p>
            Honey is the Hive&apos;s currency — a closed-loop credit that only exists inside the Hive, it can&apos;t be
            cashed out. You get it two ways: <strong>earn</strong> it by letting your own machine run cards for other
            members&apos; projects, or <strong>buy</strong> it, which also feeds the Hive&apos;s cloud compute pool.
            You spend Honey by funding projects, which is what pays whoever&apos;s machine does the work.
          </p>
        </Section>

        <Section title="Projects, cards, and the board">
          <p>
            A <strong>project</strong> is a goal broken down into <strong>cards</strong> — individual tasks, like one
            scene of a script or one function of a program. Every card moves through the same columns on a project&apos;s
            board:
          </p>
          <Term term="Suggested">a card the interviewer or a node proposed mid-project; an admin has to approve it before it can run.</Term>
          <Term term="Ready">approved and waiting for a machine to pick it up.</Term>
          <Term term="Running">a node is actively working on it right now.</Term>
          <Term term="Blocked">waiting on something else first — usually another card it depends on.</Term>
          <Term term="Review">finished — a node produced output, and an admin needs to accept it or send it back.</Term>
          <Term term="Done">accepted. This is the finished output.</Term>
        </Section>

        <Section title="Local fleet vs. the Hive">
          <p>Every project runs in one of two modes, shown near its Honey balance:</p>
          <Term term="🏠 Local fleet">free, and only your own paired machines can claim its cards. No Honey moves at all — good for private work or testing on your own hardware.</Term>
          <Term term="Hive (default)">open to the whole community — any member&apos;s machine can claim a card, and each one is paid in Honey from the project&apos;s fund. Nothing runs until the project has Honey in it.</Term>
          <p>A project&apos;s owner or an admin can switch between the two at any time from the project page.</p>
        </Section>

        <Section title="Nodes and machines">
          <p>A <strong>node</strong> is a machine you&apos;ve paired with your account. Each one has a role:</p>
          <Term term="💻 Local">a member&apos;s own machine, doing the actual model work.</Term>
          <Term term="🖥 Server">one of the Hive&apos;s own backbone servers — relaying traffic and storing artifacts, not a member&apos;s personal computer.</Term>
          <p>
            When you pair a machine you also set its trust level: whether it&apos;s allowed to reach the internet
            (off by default), and whether it gets sandboxed tool access or inference only (model in, tokens out,
            nothing else).
          </p>
        </Section>

        <Section title="Funding a project">
          <p>
            Anyone can add Honey to any project — not just its owner — from the project page. You can contribute
            credited (your name shows next to the amount) or anonymously. Nodes only get paid, and cards only get
            picked up, once a project actually has Honey in its fund (Local fleet projects are the exception — they
            never need funding).
          </p>
        </Section>
      </main>
    </>
  );
}
