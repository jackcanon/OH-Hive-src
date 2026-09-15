//! Pure version negotiation and fail-stop simulated batch coordinator; no native effects.
use super::limits::{Budget, LimitError, Limits};
use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopProfile {
    pub name: String,
    pub version: u32,
}
impl DesktopProfile {
    pub fn current() -> Self {
        Self {
            name: "desktop".into(),
            version: 1,
        }
    }
}
pub fn validate_profile(profile: &DesktopProfile) -> Result<(), &'static str> {
    if profile == &DesktopProfile::current() {
        Ok(())
    } else {
        Err("unsupported desktop profile")
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum SessionError {
    Profile,
    Session,
    BatchStopped,
    Limits(LimitError),
}
/// A batch contains request identities, never shared consent or cached authority.
/// Execute one step at a time so trusted focus/revoke/stop events can interleave.
pub struct Batch {
    calls: Vec<Uuid>,
    next: usize,
    stopped: bool,
}
impl Batch {
    pub fn new(calls: Vec<Uuid>) -> Result<Self, SessionError> {
        let unique: std::collections::HashSet<_> = calls.iter().copied().collect();
        if calls.is_empty()
            || calls.len() > 100
            || unique.len() != calls.len()
            || calls.iter().any(Uuid::is_nil)
        {
            return Err(SessionError::BatchStopped);
        }
        Ok(Self {
            calls,
            next: 0,
            stopped: false,
        })
    }
    pub fn stopped(&self) -> bool {
        self.stopped
    }
}
pub struct DesktopSession {
    id: Uuid,
    desktop: FakeDesktop,
    budget: Budget,
    stopped: bool,
}
impl DesktopSession {
    pub fn new(
        id: Uuid,
        profile: &DesktopProfile,
        limits: Limits,
        now_ms: u64,
    ) -> Result<Self, SessionError> {
        validate_profile(profile).map_err(|_| SessionError::Profile)?;
        if id.is_nil() {
            return Err(SessionError::Session);
        }
        Ok(Self {
            id,
            desktop: FakeDesktop::default(),
            budget: Budget::new(limits, now_ms).map_err(SessionError::Limits)?,
            stopped: false,
        })
    }
    pub fn counters(&self) -> super::limits::Counters {
        self.budget.counters()
    }
    pub fn effects(&self) -> usize {
        self.desktop.effects
    }
    pub fn report_focus_change(&mut self, target: Option<Target>, through: u64) {
        self.desktop.report_focus_change(target, through);
    }
    pub fn revoke_grant(&mut self, target: &Target) {
        self.desktop.revoke_grant(target);
    }
    pub fn interrupt(&mut self) {
        self.stopped = true;
        self.desktop.interrupt();
    }
    pub fn mark_uncertain(&mut self) {
        self.stopped = true;
        self.desktop.mark_uncertain();
    }
    pub fn execute_step(
        &mut self,
        batch: &mut Batch,
        a: &Authority,
        p: &Policy,
        o: &Observation,
        r: &Request,
        risk: Risk,
        approval: Option<&Approval>,
        now: u64,
        now_ms: u64,
    ) -> Result<Outcome, SessionError> {
        if batch.stopped || self.stopped || batch.calls.get(batch.next) != Some(&r.call_id) {
            batch.stopped = true;
            return Err(SessionError::BatchStopped);
        }
        if a.session != self.id || r.session != self.id {
            batch.stopped = true;
            return Err(SessionError::Session);
        }
        let bytes = serde_json::to_vec(r)
            .map_err(|_| SessionError::Limits(LimitError::Bytes))?
            .len() as u64;
        if let Err(e) = self
            .budget
            .ready(now_ms)
            .and_then(|_| self.budget.request_bytes(bytes))
        {
            batch.stopped = true;
            return Err(SessionError::Limits(e));
        }
        let check = if matches!(r.action, Action::Observe) {
            self.budget.check_time(now_ms)
        } else {
            self.budget.input_action(now_ms)
        };
        if let Err(e) = check {
            batch.stopped = true;
            return Err(SessionError::Limits(e));
        }
        let outcome = self.desktop.execute(a, p, o, r, risk, approval, now);
        if matches!(outcome, Outcome::Applied | Outcome::Observed) {
            batch.next += 1;
        } else {
            batch.stopped = true;
        }
        Ok(outcome)
    }
    #[cfg(feature = "desktop-provider")]
    pub(super) fn provider_budget(&mut self, id: Uuid) -> Result<&mut Budget, SessionError> {
        if id != self.id {
            return Err(SessionError::Session);
        }
        if self.stopped {
            return Err(SessionError::BatchStopped);
        }
        Ok(&mut self.budget)
    }
}
#[cfg(test)]
mod tests;
