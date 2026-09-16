//! ADR-029 phase one: deterministic contracts and policy, with no native input implementation.
//! Authority, risk classification, observations and consent must come from a trusted local broker,
//! never from model arguments. This module does not advertise a runnable worker capability.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub mod journal;
pub mod limits;
#[cfg(feature = "desktop-provider")]
pub mod provider;
pub mod session;
use journal::{Receipt, ReceiptAction, ReceiptState};

pub const PROFILE: &str = "desktop_v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub bundle_id: String,
    pub process_id: u32,
    pub process_instance: Uuid,
    pub window_id: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Observe,
    Click { x: u32, y: u32 },
    Type { text: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub session: Uuid,
    pub call_id: Uuid,
    pub sequence: u64,
    pub observation: u64,
    pub policy_revision: u64,
    pub target: Target,
    pub action: Action,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    View,
    Control,
}
/// App classification is supplied by the trusted app resolver, not by the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppClass {
    Browser,
    TerminalOrIde,
    Ordinary,
}
pub fn default_access(class: AppClass) -> Access {
    match class {
        AppClass::Browser | AppClass::TerminalOrIde => Access::View,
        AppClass::Ordinary => Access::Control,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    Routine,
    Unknown,
    ExecuteCode,
    SendOrPublish,
    Purchase,
    Settings,
    PaymentCredentials,
    TradeOrTransfer,
    PermanentDelete,
    Captcha,
    ExternalConsent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Allow,
    Confirm,
    Deny,
}
#[derive(Clone, Debug)]
pub struct Policy {
    pub revision: u64,
    exceptions: BTreeMap<Risk, Rule>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            revision: 1,
            exceptions: BTreeMap::new(),
        }
    }
}
impl Policy {
    pub fn rule(&self, risk: Risk) -> Rule {
        self.exceptions.get(&risk).copied().unwrap_or(match risk {
            Risk::Routine => Rule::Allow,
            Risk::Unknown
            | Risk::ExecuteCode
            | Risk::SendOrPublish
            | Risk::Purchase
            | Risk::Settings => Rule::Confirm,
            _ => Rule::Deny,
        })
    }
    /// Trusted local consent path only. The policy is intentionally not a model-wire type.
    /// The caller must persist the reviewed explainer and consent record before invoking this.
    pub fn set_consented_exception(&mut self, risk: Risk, rule: Rule) {
        self.exceptions.insert(risk, rule);
        self.revision = self.revision.saturating_add(1);
    }
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub id: u64,
    pub target: Target,
    pub width: u32,
    pub height: u32,
    /// Bounds of the allowed window in the screenshot canvas; blank regions cannot be clicked.
    pub allowed_rect: [u32; 4],
    pub valid_until: u64,
}
#[derive(Clone, Debug)]
pub struct Authority {
    pub session: Uuid,
    pub owner: Uuid,
    pub project_owner: Uuid,
    pub node: Uuid,
    pub target_node: Uuid,
    pub private_local: bool,
    pub lease_deadline: u64,
    pub authority_deadline: u64,
    pub stopped: bool,
    pub grant_deadline: u64,
    pub access: Access,
    pub target: Target,
}
/// Opaque to the model transport. A native consent UI may create this only after a real grant.
#[derive(Clone, Debug)]
pub struct Approval {
    request: Request,
    risk: Risk,
    expires: u64,
}
impl Approval {
    pub fn from_local_consent(request: &Request, risk: Risk, expires: u64) -> Self {
        Self {
            request: request.clone(),
            risk,
            expires,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denial {
    Focus,
    GrantRevoked,
    Ownership,
    Expired,
    Stopped,
    Session,
    Target,
    StalePolicy,
    StaleObservation,
    Bounds,
    Access,
    Policy,
    Payload,
    Sequence,
    Uncertain,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Confirm,
    Deny(Denial),
}
/// `now` and deadlines share a broker-owned monotonic time domain, not model wall-clock strings.
/// `risk` is broker classification. Missing classification must use Unknown, not Routine.
pub fn evaluate(
    a: &Authority,
    p: &Policy,
    o: &Observation,
    r: &Request,
    risk: Risk,
    approval: Option<&Approval>,
    now: u64,
) -> Decision {
    use Decision::*;
    if !a.private_local || a.owner != a.project_owner || a.node != a.target_node {
        return Deny(Denial::Ownership);
    }
    if a.stopped {
        return Deny(Denial::Stopped);
    }
    if now >= a.lease_deadline || now >= a.authority_deadline || now >= a.grant_deadline {
        return Deny(Denial::Expired);
    }
    if r.session != a.session {
        return Deny(Denial::Session);
    }
    if r.target != a.target || r.target != o.target {
        return Deny(Denial::Target);
    }
    if r.policy_revision != p.revision {
        return Deny(Denial::StalePolicy);
    }
    if r.observation != o.id || now >= o.valid_until {
        return Deny(Denial::StaleObservation);
    }
    match &r.action {
        Action::Observe => return Allow,
        Action::Click { x, y } => {
            let [left, top, right, bottom] = o.allowed_rect;
            if left >= right
                || top >= bottom
                || right > o.width
                || bottom > o.height
                || *x < left
                || *y < top
                || *x >= right
                || *y >= bottom
            {
                return Deny(Denial::Bounds);
            }
        }
        Action::Type { text } => {
            if text.is_empty() || text.len() > 16_384 || text.contains('\0') {
                return Deny(Denial::Payload);
            }
        }
    }
    if a.access != Access::Control {
        return Deny(Denial::Access);
    }
    match p.rule(risk) {
        Rule::Allow => Allow,
        Rule::Deny => Deny(Denial::Policy),
        Rule::Confirm => {
            if approval.is_some_and(|g| g.request == *r && g.risk == risk && now < g.expires) {
                Allow
            } else {
                Confirm
            }
        }
    }
}

/// Moved to `crate::brain` (2026-09-15) -- this module's own doc comment on the old definition
/// ("the later shared session-loop integration") named exactly this move before it happened.
/// Re-exported so `desktop/tests.rs` and `desktop/provider/tests.rs` (both reference
/// `ContentBlock::Png{..}` / `ContentBlock::validate_envelope`) keep compiling unchanged.
pub use crate::brain::ContentBlock;

/// Only a fake executor exists in this milestone. Native dispatch will require fresh OS target
/// checks, durable dispatch intent and final gate evaluation in the broker immediately before input.
#[derive(Default)]
pub struct FakeDesktop {
    next_sequence: u64,
    focused_target: Option<Target>,
    focus_invalidated_through: Option<u64>,
    revoked_targets: Vec<Target>,
    pub effects: usize,
    uncertain: bool,
    interrupted: bool,
    journal: Vec<Receipt>,
    bound_session: Option<Uuid>,
    invalidated_observation: Option<u64>,
    consumed: std::collections::HashSet<Uuid>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Observed,
    Applied,
    NeedsConfirmation,
    Rejected(Denial),
    NeedsReview,
}
impl FakeDesktop {
    /// Trusted broker event, never a model tool. `observed_through` is the highest observation
    /// issued before this event (including observations used by queued actions).
    /// Even returning to the same window requires a new observation.
    pub fn report_focus_change(&mut self, target: Option<Target>, observed_through: u64) {
        self.focused_target = target;
        self.focus_invalidated_through = Some(
            self.focus_invalidated_through
                .map_or(observed_through, |old| old.max(observed_through)),
        );
    }
    /// Explicit, session-lifetime revocation; local review cannot resurrect this grant.
    pub fn revoke_grant(&mut self, target: &Target) {
        if !self.revoked_targets.contains(target) {
            self.revoked_targets.push(target.clone());
        }
    }
    fn check_broker_state(&self, target: &Target, observation: u64) -> Result<(), Denial> {
        if self.revoked_targets.contains(target) {
            return Err(Denial::GrantRevoked);
        }
        if self.focused_target.as_ref() != Some(target) {
            return Err(Denial::Focus);
        }
        if self
            .focus_invalidated_through
            .is_some_and(|last| observation <= last)
        {
            return Err(Denial::StaleObservation);
        }
        Ok(())
    }
    /// Simulates an action whose effect happened but whose receipt was lost. Never replay it.
    pub fn mark_uncertain(&mut self) {
        self.uncertain = true;
        if let Some(last) = self.journal.last_mut() {
            last.state = ReceiptState::DispatchedUnknown;
        }
    }
    // Same reasoning as `session::execute_step`: each capability arrives as its own argument so a
    // caller cannot hand over more authority than the action needs.
    #[allow(clippy::too_many_arguments)]
    pub fn execute(
        &mut self,
        a: &Authority,
        p: &Policy,
        o: &Observation,
        r: &Request,
        risk: Risk,
        approval: Option<&Approval>,
        now: u64,
    ) -> Outcome {
        if self.next_sequence >= 10_000 {
            return Outcome::Rejected(Denial::Payload);
        }
        if self.bound_session.is_some_and(|s| s != a.session) {
            return Outcome::Rejected(Denial::Session);
        }
        if self.interrupted {
            return Outcome::Rejected(Denial::Stopped);
        }
        if self.uncertain {
            return Outcome::Rejected(Denial::Uncertain);
        }
        if r.sequence != self.next_sequence || self.consumed.contains(&r.call_id) {
            return Outcome::Rejected(Denial::Sequence);
        }
        if !matches!(r.action, Action::Observe)
            && self.invalidated_observation == Some(r.observation)
        {
            return Outcome::Rejected(Denial::StaleObservation);
        }
        if let Err(reason) = self.check_broker_state(&r.target, r.observation) {
            return Outcome::Rejected(reason);
        }
        match evaluate(a, p, o, r, risk, approval, now) {
            Decision::Confirm => Outcome::NeedsConfirmation,
            Decision::Deny(reason) => Outcome::Rejected(reason),
            Decision::Allow => {
                self.journal.push(Receipt {
                    session: a.session,
                    call_id: r.call_id,
                    sequence: r.sequence,
                    observation: r.observation,
                    policy_revision: p.revision,
                    target: r.target.clone(),
                    state: if matches!(r.action, Action::Observe) {
                        ReceiptState::Observed
                    } else {
                        ReceiptState::DispatchedUnknown
                    },
                    action: match r.action {
                        Action::Observe => ReceiptAction::Observe,
                        Action::Click { .. } => ReceiptAction::Click,
                        Action::Type { .. } => ReceiptAction::Type,
                    },
                    at: now,
                });
                self.bound_session = Some(a.session);
                self.next_sequence += 1;
                self.consumed.insert(r.call_id);
                if matches!(r.action, Action::Observe) {
                    Outcome::Observed
                } else {
                    self.effects += 1;
                    self.journal.last_mut().unwrap().state = ReceiptState::Applied;
                    self.invalidated_observation = Some(r.observation);
                    Outcome::Applied
                }
            }
        }
    }
}
#[cfg(test)]
mod tests;
