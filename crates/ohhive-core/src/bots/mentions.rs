//! Mention resolution is convenience, not authorization: callers pass only the room roster,
//! and storage still validates every recipient. No agent execution is activated here.
use super::{AgentId, AgentProfile, Principal};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MentionSet {
    pub recipients: Vec<AgentId>,
    pub unresolved: Vec<String>,
    pub everyone: bool,
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
    #[test]
    fn everyone_excludes_archived_and_self() {
        let a = agent("Sif");
        let b = agent("Nous");
        let mut c = agent("Loki");
        c.archived = true;
        let got = resolve_mentions(
            "@everyone @Sif @Nous",
            &[a.clone(), b.clone(), c],
            Principal::Agent(a.id),
        );
        assert!(got.everyone);
        assert_eq!(got.recipients, vec![b.id]);
    }
}
