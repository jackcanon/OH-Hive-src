//! ADR-035 C2 Track A: `@name` resolution. Pure -- no storage, no network, no clock.
//!
//! Turns a message body plus the room's agent roster into the recipient list that
//! `bots_message_send_with_cause` creates `agent_deliveries` rows from. Nothing else in the
//! Bots subsystem produced a recipient list before this: `message_send` has always *taken* one
//! (`recipient_ids: Vec<AgentId>`, already a Vec, already membership-checked), and C1's callers
//! simply passed the single DM counterpart.
//!
//! **This resolver proposes; it never authorizes.** `local_hub::bots_message_send`'s
//! `bots_is_member` check on every recipient stays exactly as it is. A name resolved here that
//! is not a conversation member is still refused at the storage boundary, which is why this
//! file is allowed to be liberal about parsing and has no trust responsibility.
//!
//! Resolution happens **once, at send time**, and the resulting list is persisted as delivery
//! rows. Readers never re-parse a body. So renaming an agent tomorrow does not retroactively
//! change who was addressed yesterday, and editing a message body cannot silently fan out again.

use super::{AgentId, AgentProfile, Principal};

/// Everything one parse of one body yields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MentionSet {
    /// Agents to create deliveries for, de-duplicated, roster order.
    pub recipients: Vec<AgentId>,
    /// `@name`s that matched no roster entry, or matched more than one. Reported so a UI can
    /// show them as plain unlinked text -- never an error, because a typo must not fail a send.
    pub unresolved: Vec<String>,
    /// `@everyone` appeared and resolved to the whole room.
    pub everyone: bool,
}

/// True for the characters a mention name may contain.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Byte ranges of the body that are inside a fenced block or inline code span, and therefore
/// must not be scanned for mentions.
///
/// This is not decoration. A room whose whole purpose is discussing code will contain `@` inside
/// pasted shell, Rust attributes and Swift property wrappers constantly; a resolver that
/// summoned a teammate for every `@State` or `@escaping` would be unusable in exactly the rooms
/// this feature exists for.
fn code_spans(body: &str) -> Vec<(usize, usize)> {
    let bytes = body.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        // Count the run of backticks -- three or more opens a fence, one or two an inline span.
        let start = i;
        let mut ticks = 0usize;
        while i < bytes.len() && bytes[i] == b'`' {
            ticks += 1;
            i += 1;
        }
        let fence: Vec<u8> = vec![b'`'; ticks];
        match find_from(bytes, &fence, i) {
            // Closing run found: everything from the opening run through it is code.
            Some(close) => {
                spans.push((start, close + ticks));
                i = close + ticks;
            }
            // Unterminated. Treat the rest of the body as code rather than reopening it to
            // scanning: a half-pasted snippet should fail closed (nobody summoned), not open.
            None => {
                spans.push((start, bytes.len()));
                break;
            }
        }
    }
    spans
}

fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn in_any_span(spans: &[(usize, usize)], at: usize) -> bool {
    spans.iter().any(|(a, b)| at >= *a && at < *b)
}

/// Every `@name` in `body`, in order of appearance, excluding code spans. Names are returned as
/// written; matching is the caller's job.
fn scan_names(body: &str) -> Vec<String> {
    let spans = code_spans(body);
    let bytes = body.as_bytes();
    let mut names = Vec::new();
    for (index, ch) in body.char_indices() {
        if ch != '@' || in_any_span(&spans, index) {
            continue;
        }
        // An `@` directly after a name character is an address or a handle inside a word
        // (jack@happyjack.media), not a mention.
        if index > 0 {
            let prev = body[..index].chars().next_back();
            if prev.is_some_and(is_name_char) {
                continue;
            }
        }
        let rest = &body[index + 1..];
        let end: usize = rest
            .char_indices()
            .find(|(_, c)| !is_name_char(*c))
            .map(|(offset, _)| offset)
            .unwrap_or(rest.len());
        if end == 0 {
            continue; // a bare '@'
        }
        // Trailing punctuation terminates the name naturally, because it isn't a name char:
        // "@Sif," and "@Sif." both yield "Sif".
        names.push(rest[..end].to_string());
        let _ = bytes; // scanning is char-based; bytes only used for span detection
    }
    names
}

/// Resolve `body`'s mentions against `roster` (the conversation's non-archived agent members).
///
/// `author` is dropped from the result: an agent that writes its own name does not wake itself.
/// That is the cheapest available infinite loop and it costs one comparison to close.
///
/// A name matching two roster agents resolves to **neither**, and is reported in `unresolved` --
/// guessing which teammate was meant is worse than saying the name is ambiguous.
pub fn resolve_mentions(body: &str, roster: &[AgentProfile], author: Principal) -> MentionSet {
    let mut set = MentionSet::default();
    let author_agent = match author {
        Principal::Agent(id) => Some(id),
        Principal::User(_) => None,
    };
    let mut push = |set: &mut MentionSet, id: AgentId| {
        if Some(id) != author_agent && !set.recipients.contains(&id) {
            set.recipients.push(id);
        }
    };
    for name in scan_names(body) {
        if name.eq_ignore_ascii_case("everyone") {
            set.everyone = true;
            for agent in roster {
                push(&mut set, agent.id);
            }
            continue;
        }
        let matches: Vec<&AgentProfile> = roster
            .iter()
            .filter(|a| a.name.eq_ignore_ascii_case(&name))
            .collect();
        match matches.as_slice() {
            [one] => push(&mut set, one.id),
            // Zero matches, or two or more: both are "we will not guess."
            _ => {
                if !set.unresolved.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
                    set.unresolved.push(name);
                }
            }
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::AgentRuntimeKind;
    use chrono::Utc;
    use uuid::Uuid;

    fn agent(name: &str) -> AgentProfile {
        AgentProfile {
            id: Uuid::new_v4(),
            owner: Uuid::nil(),
            name: name.into(),
            role_revision: 1,
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: name.into(),
            archived: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn resolves_a_simple_mention() {
        let sif = agent("Sif");
        let set = resolve_mentions("can @Sif take this?", &[sif.clone()], Principal::User(Uuid::nil()));
        assert_eq!(set.recipients, vec![sif.id]);
        assert!(set.unresolved.is_empty());
        assert!(!set.everyone);
    }

    #[test]
    fn match_is_case_insensitive_and_punctuation_terminates() {
        let sif = agent("Sif");
        let roster = [sif.clone()];
        for body in ["@sif,", "@SIF.", "@Sif!", "ping @sif; thanks"] {
            let set = resolve_mentions(body, &roster, Principal::User(Uuid::nil()));
            assert_eq!(set.recipients, vec![sif.id], "body: {body}");
        }
    }

    #[test]
    fn everyone_resolves_to_the_whole_roster() {
        let a = agent("Loki");
        let b = agent("Sif");
        let set = resolve_mentions("@everyone standup", &[a.clone(), b.clone()], Principal::User(Uuid::nil()));
        assert!(set.everyone);
        assert_eq!(set.recipients.len(), 2);
        assert!(set.recipients.contains(&a.id) && set.recipients.contains(&b.id));
    }

    #[test]
    fn self_mention_is_dropped() {
        let sif = agent("Sif");
        let other = agent("Loki");
        let set = resolve_mentions(
            "I, @Sif, will ask @Loki",
            &[sif.clone(), other.clone()],
            Principal::Agent(sif.id),
        );
        assert_eq!(set.recipients, vec![other.id], "an agent must not wake itself");
    }

    #[test]
    fn everyone_still_excludes_its_own_author() {
        let sif = agent("Sif");
        let other = agent("Loki");
        let set = resolve_mentions("@everyone", &[sif.clone(), other.clone()], Principal::Agent(sif.id));
        assert_eq!(set.recipients, vec![other.id]);
    }

    #[test]
    fn unknown_name_is_reported_not_dropped_and_never_errors() {
        let sif = agent("Sif");
        let set = resolve_mentions("@Sifff typo", &[sif], Principal::User(Uuid::nil()));
        assert!(set.recipients.is_empty());
        assert_eq!(set.unresolved, vec!["Sifff".to_string()]);
    }

    #[test]
    fn duplicate_names_resolve_to_neither() {
        let one = agent("Helper");
        let two = agent("helper");
        let set = resolve_mentions("@Helper please", &[one, two], Principal::User(Uuid::nil()));
        assert!(set.recipients.is_empty(), "ambiguity must not be guessed");
        assert_eq!(set.unresolved, vec!["Helper".to_string()]);
    }

    #[test]
    fn fenced_code_is_not_scanned() {
        let sif = agent("Sif");
        let body = "look at this:\n```swift\n@State var x = 1\n@escaping closure\n```\nthoughts?";
        let set = resolve_mentions(body, &[sif], Principal::User(Uuid::nil()));
        assert!(set.recipients.is_empty());
        assert!(set.unresolved.is_empty(), "code is not an unresolved mention either");
    }

    #[test]
    fn inline_code_is_not_scanned_but_text_around_it_is() {
        let sif = agent("Sif");
        let set = resolve_mentions(
            "the attribute is `@Sendable` -- @Sif can you confirm?",
            &[sif.clone()],
            Principal::User(Uuid::nil()),
        );
        assert_eq!(set.recipients, vec![sif.id]);
        assert!(set.unresolved.is_empty(), "`@Sendable` must not register at all");
    }

    #[test]
    fn unterminated_fence_fails_closed() {
        let sif = agent("Sif");
        let set = resolve_mentions("```\n@Sif", &[sif], Principal::User(Uuid::nil()));
        assert!(set.recipients.is_empty(), "a half-pasted snippet summons nobody");
    }

    #[test]
    fn email_address_is_not_a_mention() {
        let sif = agent("Sif");
        let set = resolve_mentions("mail jack@happyjack.media", &[sif], Principal::User(Uuid::nil()));
        assert!(set.recipients.is_empty());
        assert!(set.unresolved.is_empty());
    }

    #[test]
    fn repeated_mention_creates_one_recipient() {
        let sif = agent("Sif");
        let set = resolve_mentions("@Sif @Sif @sif", &[sif.clone()], Principal::User(Uuid::nil()));
        assert_eq!(set.recipients, vec![sif.id], "one delivery per recipient, not per mention");
    }

    #[test]
    fn bare_at_and_empty_body_are_harmless() {
        let sif = agent("Sif");
        for body in ["", "@", "@ Sif", "email @ me"] {
            let set = resolve_mentions(body, &[sif.clone()], Principal::User(Uuid::nil()));
            assert!(set.recipients.is_empty(), "body: {body:?}");
        }
    }

    #[test]
    fn unicode_body_does_not_panic_on_byte_boundaries() {
        let sif = agent("Sif");
        let set = resolve_mentions("héllo — @Sif ✅ done", &[sif.clone()], Principal::User(Uuid::nil()));
        assert_eq!(set.recipients, vec![sif.id]);
    }
}
