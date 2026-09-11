"use client";

import type { ReactNode } from "react";
import { Nav } from "@/components/RequireMember";
import { TOS_VERSION } from "@/lib/tos";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section style={{ marginTop: 32 }}>
      <h2 style={{ fontSize: 18, marginBottom: 10 }}>{title}</h2>
      <div style={{ color: "var(--muted-strong)", lineHeight: 1.6 }}>{children}</div>
    </section>
  );
}

export default function TermsPage() {
  return (
    <>
      <Nav />
      <main style={{ maxWidth: 720, margin: "0 auto", padding: "24px 24px 64px" }}>
        <h1 style={{ marginTop: 8 }}>Terms of Service</h1>
        <p style={{ color: "var(--muted-strong)" }}>
          Version {TOS_VERSION} · Hive is operated by Happy Jack Media. This page governs your use of Hive
          (the web app, node app, and bot integrations) and your membership in the Hive community.
        </p>

        <Section title="1. What Hive is">
          <p>
            Hive is an invite-only community where members pool idle computers into a shared compute network.
            Members contribute compute (by running the node app on their own machine), storage, or purchased
            credit, earn <strong>Honey</strong> for doing so, and spend Honey funding AI-agent-driven projects
            that run on the network. Hive is experimental software, offered as-is to invited members — see
            Section 7.
          </p>
        </Section>

        <Section title="2. Membership">
          <p>
            Hive is invite-only. You must be invited by an existing member or an admin, and you must be at
            least 18 years old (or the age of majority where you live) to join. You&apos;re responsible for
            everything that happens under your account, including invite codes you hand out — every member
            your codes bring in is associated with you.
          </p>
          <p>
            We can suspend or terminate your membership at any time, for any reason, including suspected abuse
            of the network, other members, or these terms. You can leave at any time by checking out your nodes
            and asking us to close your account.
          </p>
        </Section>

        <Section title="3. Honey">
          <p>
            Honey is a closed-loop credit that exists only inside Hive. It has no cash value, cannot be
            exchanged for real-world currency, cannot be transferred outside Hive, and is not a security,
            deposit, or investment of any kind. You earn Honey by contributing compute or storage that other
            members&apos; projects use; you spend Honey by funding projects, which pays the Honey forward to
            whoever&apos;s machine did the work.
          </p>
          <p>
            Honey balances, exchange rates, and earning rates may change as Hive evolves, and we may reset,
            adjust, or revoke Honey in cases of abuse, fraud, or system error. There is currently no way to
            convert Honey back into money.
          </p>
        </Section>

        <Section title="4. Contributing your own compute">
          <p>
            If you register a machine as a Hive node, other members&apos; AI agent workloads may run on it
            inside a sandbox, subject to the trust level and internet-access setting you choose for that node.
            The sandbox limits what a card&apos;s agent loop can do, but no sandbox is a perfect guarantee —
            don&apos;t register a machine you can&apos;t afford to have run untrusted, automated workloads on,
            and don&apos;t leave sensitive personal data on a machine you&apos;ve opted into inference-only or
            tool-enabled work. You can check a node out of the network at any time to stop it from taking new
            work.
          </p>
        </Section>

        <Section title="5. Projects, ownership, and licensing">
          <p>
            Whoever creates a project owns what it produces. At creation, you choose whether the output stays
            owner-only or is released open source under a license you pick (we default to a permissive license
            like MIT for code or CC-BY-4.0 for other media). Once you release something open source, you
            can&apos;t revoke that release for copies already made. Other members funding a project doesn&apos;t
            give them ownership of its output — funding is a Honey contribution, not a purchase of rights,
            unless the project&apos;s own listing says otherwise.
          </p>
        </Section>

        <Section title="6. Acceptable use">
          <p>
            Don&apos;t use Hive to generate illegal content, to attempt to break out of the agent sandbox or
            access another member&apos;s machine or data without permission, to abuse or overload shared
            infrastructure, or to harass other members. Don&apos;t submit projects whose acceptance criteria
            require a node to violate these terms to satisfy them. We can remove content, revoke Honey, or
            terminate membership for violations.
          </p>
        </Section>

        <Section title="7. No warranty">
          <p>
            Hive is provided &quot;as is,&quot; without warranty of any kind. It&apos;s early, invite-only
            software: nodes can drop mid-job, projects can fail, and Honey balances or project data could be
            lost to a bug. We&apos;ll do our best to keep the ledger and your work intact, but we don&apos;t
            promise uptime, data durability, or that any given project will complete successfully.
          </p>
        </Section>

        <Section title="8. Limitation of liability">
          <p>
            To the maximum extent the law allows, Happy Jack Media isn&apos;t liable for indirect, incidental,
            or consequential damages arising from your use of Hive, including lost Honey, lost work, or damage
            to a machine you&apos;ve registered as a node. Nothing here limits liability that can&apos;t be
            limited by law.
          </p>
        </Section>

        <Section title="9. Changes to these terms">
          <p>
            We may update these terms as Hive evolves. Material changes will be dated with a new version at
            the top of this page. Continuing to use Hive after a change means you accept the new terms; if you
            don&apos;t, you can stop using Hive and ask us to close your account.
          </p>
        </Section>

        <Section title="10. Contact">
          <p>
            Hive is a Happy Jack Media project. Questions about these terms or your account go to the same
            place you got your invite — ask whoever invited you, or reach us through the community channels
            Hive links to.
          </p>
        </Section>
      </main>
    </>
  );
}
