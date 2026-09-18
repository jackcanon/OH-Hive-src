//! Mention resolution is convenience, not authorization: callers pass only the room roster,
//! and storage still validates every recipient. No agent execution is activated here.
use super::{AgentId, AgentProfile, Principal};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MentionSet {
    pub recipients: Vec<AgentId>,
    pub unresolved: Vec<String>,
    pub everyone: bool,
}

/// Resolve mentions knowing the room's non-agent participants by name (today: the human owner).
///
/// A person in a room is a participant but never a delivery target -- they are already reading.
/// Without this, an agent writing `@Owner` produces an "unrecognized name" notice, which is noise
/// the model generates readily: a live three-agent run against llama3.1 had an agent reply
/// "@the person ..." unprompted. Recognized-and-wakes-nobody is the correct outcome.
pub fn resolve_mentions_with_participants(
    body: &str,
    roster: &[AgentProfile],
    author: Principal,
    non_agent_names: &[String],
) -> MentionSet {
    let mut set = resolve_mentions(body, roster, author);
    set.unresolved
        .retain(|u| !non_agent_names.iter().any(|n| n.eq_ignore_ascii_case(u)));
    set
}

pub fn resolve_mentions(body: &str, roster: &[AgentProfile], author: Principal) -> MentionSet {
    let mut result = MentionSet::default();
    let mut fence: Option<(char, usize)> = None;
    let mut inline = 0;
    let mut names = Vec::new();
    for original_line in body.lines() {
        // Markdown quotes and list-contained fences have container prefixes.
        let mut line = original_line;
        while let Some(rest) = line.trim_start_matches(' ').strip_prefix('>') {
            line = rest.strip_prefix(' ').unwrap_or(rest);
        }
        let list = line.trim_start_matches(' ');
        if let Some(rest) = list
            .strip_prefix("- ")
            .or_else(|| list.strip_prefix("* "))
            .or_else(|| list.strip_prefix("+ "))
        {
            line = rest;
        }
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let run = trimmed.chars().take_while(|c| *c == marker).count();
        if let Some((ch, n)) = fence {
            if indent <= 3 && marker == ch && run >= n && trimmed[run..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if inline == 0 && indent <= 3 && (marker == '`' || marker == '~') && run >= 3 {
            fence = Some((marker, run));
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && inline == 0 {
                i += 2;
                continue;
            }
            if chars[i] == '`' {
                let n = chars[i..].iter().take_while(|c| **c == '`').count();
                if inline == n {
                    inline = 0;
                } else if inline == 0 {
                    inline = n;
                }
                i += n;
                continue;
            }
            if inline == 0
                && chars[i] == '@'
                && (i == 0 || !(chars[i - 1].is_alphanumeric() || "._-+@/".contains(chars[i - 1])))
            {
                let start = i + 1;
                let mut end = start;
                while end < chars.len()
                    && (chars[end].is_ascii_alphanumeric() || "_-".contains(chars[end]))
                {
                    end += 1;
                }
                if end > start {
                    names.push(chars[start..end].iter().collect::<String>());
                }
                i = end;
            } else {
                i += 1;
            }
        }
    }
    let self_id = if let Principal::Agent(id) = author {
        Some(id)
    } else {
        None
    };
    for name in names {
        if name.eq_ignore_ascii_case("everyone") {
            // Audit §3.9: broadcast is asymmetric on purpose. A person spending their own budget
            // to wake a whole room is a choice; an agent doing it is a multiplier, and the
            // executor's fan-out cap would then pick an arbitrary two by roster order, which is
            // worse than refusing -- it looks like the agent chose them.
            if self_id.is_some() {
                continue;
            }
            result.everyone = true;
            for agent in roster
                .iter()
                .filter(|a| !a.archived && Some(a.id) != self_id)
            {
                if !result.recipients.contains(&agent.id) {
                    result.recipients.push(agent.id);
                }
            }
        } else {
            let matches: Vec<_> = roster
                .iter()
                .filter(|a| !a.archived && a.name.eq_ignore_ascii_case(&name))
                .collect();
            match matches.as_slice() {
                [agent] => {
                    if Some(agent.id) != self_id && !result.recipients.contains(&agent.id) {
                        result.recipients.push(agent.id);
                    }
                }
                _ => {
                    if !result
                        .unresolved
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(&name))
                    {
                        result.unresolved.push(name);
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::AgentRuntimeKind;
    use uuid::Uuid;
    fn agent(name: &str) -> AgentProfile {
        AgentProfile {
            id: Uuid::new_v4(),
            owner: Uuid::new_v4(),
            name: name.into(),
            role_revision: 1,
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: None,
            host_name: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "test".into(),
            archived: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
    #[test]
    fn names_punctuation_unknown_ambiguity_and_dedup() {
        let a = agent("Sif");
        let b = agent("Nous");
        let roster = [a.clone(), b.clone(), agent("NOUS")];
        let got = resolve_mentions(
            "@sIF, @Sif. @Nous @ghost @GHOST",
            &roster,
            Principal::User(Uuid::new_v4()),
        );
        assert_eq!(got.recipients, vec![a.id]);
        assert_eq!(got.unresolved, vec!["Nous", "ghost"]);
    }
    #[test]
    fn code_email_escape_and_fences_are_quiet() {
        let a = agent("Sif");
        for body in [
            "`@Sif`",
            "``a ` @Sif``",
            "```rust\n@Sif\n```",
            "~~~~\n@Sif\n~~~\n@Sif\n~~~~",
            "person@Sif.com",
            "name+tag@Sif.com",
            "\\@Sif",
            "α@Sif",
            "`multi\n@Sif\nline`",
            "```\n@Sif",
            "> ~~~\n> @Sif\n> ~~~",
            "- ~~~\n  @Sif\n  ~~~",
        ] {
            assert!(
                resolve_mentions(body, &[a.clone()], Principal::User(Uuid::new_v4()))
                    .recipients
                    .is_empty(),
                "{body}"
            );
        }
        assert_eq!(
            resolve_mentions(
                "```\n@Sif\n```\n(@Sif)",
                &[a.clone()],
                Principal::User(Uuid::new_v4())
            )
            .recipients,
            vec![a.id]
        );
    }
    // Three cases added by Loki when the automation branch merged. The rest of that branch's
    // 14-test suite duplicated what the three tests above already cover -- these are only the
    // gaps, not a second suite.

    /// Plain `@everyone` from a human: the whole roster, nothing excluded.
    #[test]
    fn everyone_from_a_human_reaches_the_whole_roster() {
        let a = agent("Sif");
        let b = agent("Nous");
        let got = resolve_mentions(
            "@everyone standup",
            &[a.clone(), b.clone()],
            Principal::User(Uuid::new_v4()),
        );
        assert!(got.everyone);
        assert_eq!(got.recipients.len(), 2);
        assert!(got.recipients.contains(&a.id) && got.recipients.contains(&b.id));
    }

    /// Degenerate bodies must resolve to nothing rather than panicking or inventing a name.
    #[test]
    fn bare_at_and_empty_bodies_are_harmless() {
        let a = agent("Sif");
        for body in ["", "@", "@ Sif", "email @ me", "@@", "@!"] {
            let got = resolve_mentions(body, &[a.clone()], Principal::User(Uuid::new_v4()));
            assert!(got.recipients.is_empty(), "body: {body:?}");
            assert!(got.unresolved.is_empty(), "body: {body:?}");
        }
    }

    /// A mention *after* multibyte text. The scanner indexes chars, but a regression to byte
    /// indexing would panic here rather than merely misbehave, so it is worth pinning.
    #[test]
    fn mention_after_multibyte_text_is_found() {
        let a = agent("Sif");
        let got = resolve_mentions(
            "héllo — ✅ @Sif please review",
            &[a.clone()],
            Principal::User(Uuid::new_v4()),
        );
        assert_eq!(got.recipients, vec![a.id]);
    }

    /// A mention of the human is recognized and wakes nobody, rather than reading as a typo.
    #[test]
    fn a_named_non_agent_participant_is_not_reported_unresolved() {
        let a = agent("Sif");
        let plain = resolve_mentions(
            "@Owner thanks, and @Sif please look",
            &[a.clone()],
            Principal::Agent(a.id),
        );
        assert_eq!(
            plain.unresolved,
            vec!["Owner".to_string()],
            "without the label it reads as a typo"
        );

        let known = resolve_mentions_with_participants(
            "@Owner thanks, and @Sif please look",
            &[a.clone()],
            Principal::Agent(a.id),
            &["Owner".to_string()],
        );
        assert!(
            known.unresolved.is_empty(),
            "a known participant is not unresolved: {:?}",
            known.unresolved
        );
        assert!(
            known.recipients.is_empty(),
            "and a person is never a delivery target"
        );
    }

    /// Audit §3.9: an agent may not broadcast. The design has said so since Track A; only the
    /// executor's width cap was enforcing it, and that picks an arbitrary two by roster order,
    /// which reads as the agent having chosen them.
    #[test]
    fn an_agent_cannot_broadcast_with_everyone() {
        let a = agent("Sif");
        let b = agent("Nous");
        let c = agent("Loki");
        let roster = [a.clone(), b.clone(), c.clone()];

        let from_agent = resolve_mentions(
            "@everyone drop what you're doing",
            &roster,
            Principal::Agent(a.id),
        );
        assert!(
            from_agent.recipients.is_empty(),
            "an agent's @everyone wakes nobody"
        );
        assert!(
            !from_agent.everyone,
            "and must not report a broadcast the caller would act on"
        );

        // A named mention in the same breath still works -- only the broadcast is refused.
        let mixed = resolve_mentions(
            "@everyone — and @Nous specifically",
            &roster,
            Principal::Agent(a.id),
        );
        assert_eq!(
            mixed.recipients,
            vec![b.id],
            "explicit names are unaffected"
        );

        let from_human = resolve_mentions(
            "@everyone standup",
            &roster,
            Principal::User(Uuid::new_v4()),
        );
        assert_eq!(
            from_human.recipients.len(),
            3,
            "a person may still address the room"
        );
        assert!(from_human.everyone);
    }

    #[test]
    fn everyone_excludes_archived_and_self() {
        let a = agent("Sif");
        let b = agent("Nous");
        let mut c = agent("Loki");
        c.archived = true;
        let got = resolve_mentions(
            "@everyone @Sif @Nous @Loki",
            &[a.clone(), b.clone(), c],
            Principal::Agent(a.id),
        );
        // Corrected 2026-09-16 (audit §3.9): an agent's @everyone is refused outright rather than
        // broadcasting, so the flag stays false. The rest of this test's point is unchanged --
        // @Sif is the author and excluded, @Loki is archived and unaddressable, @Nous lands.
        assert!(!got.everyone, "an agent may not broadcast");
        assert_eq!(got.recipients, vec![b.id]);
    }
}
