//! Simulated recovery records, not a native durable store. Callers must authenticate local review.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiptState {
    Observed,
    Applied,
    DispatchedUnknown,
    ReviewedApplied,
    ReviewedNotApplied,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiptAction {
    Observe,
    Click,
    Type,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub session: Uuid,
    pub call_id: Uuid,
    pub sequence: u64,
    pub observation: u64,
    pub policy_revision: u64,
    pub target: Target,
    pub state: ReceiptState,
    pub action: ReceiptAction,
    pub at: u64,
}
/// Versioned journal image for simulated process restart. Contains no typed text or screenshots.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalSnapshot {
    pub version: u32,
    pub receipts: Vec<Receipt>,
    pub revoked_targets: Vec<Target>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewFinding {
    EffectObserved,
    EffectNotObserved,
}
impl FakeDesktop {
    pub fn journal_snapshot(&self) -> JournalSnapshot {
        JournalSnapshot {
            version: 2,
            receipts: self.journal.clone(),
            revoked_targets: self.revoked_targets.clone(),
        }
    }
    /// Simulates reconstruction from trusted local storage. Restart always requires local review.
    pub fn recover(snapshot: JournalSnapshot) -> Result<Self, &'static str> {
        if snapshot.version != 2
            || snapshot.receipts.len() > 10_000
            || snapshot.revoked_targets.len() > 10_000
        {
            return Err("unsupported or oversized journal");
        }
        let mut d = Self::default();
        d.interrupted = true;
        d.revoked_targets = snapshot.revoked_targets;
        for receipt in snapshot.receipts {
            if receipt.sequence != d.next_sequence
                || d.consumed.contains(&receipt.call_id)
                || d.bound_session.is_some_and(|s| s != receipt.session)
            {
                return Err("inconsistent journal");
            }
            if d.uncertain {
                return Err("actions after unresolved dispatch");
            }
            d.bound_session = Some(receipt.session);
            d.next_sequence += 1;
            d.consumed.insert(receipt.call_id);
            if receipt.state == ReceiptState::DispatchedUnknown {
                d.uncertain = true;
            }
            if receipt.state != ReceiptState::Observed {
                d.invalidated_observation = Some(receipt.observation);
            }
            if matches!(
                receipt.state,
                ReceiptState::Applied | ReceiptState::ReviewedApplied
            ) {
                d.effects += 1;
            }
            d.journal.push(receipt);
        }
        Ok(d)
    }
    /// Trusted human interruption signal; queued actions must re-enter execute and cannot bypass it.
    pub fn interrupt(&mut self) {
        self.interrupted = true;
    }
    /// New observation and current authority are mandatory. Findings never authorize replay.
    pub fn resume_after_local_review(
        &mut self,
        a: &Authority,
        p: &Policy,
        o: &Observation,
        finding: Option<ReviewFinding>,
        now: u64,
    ) -> Result<(), Denial> {
        if self.bound_session.is_some_and(|s| s != a.session) {
            return Err(Denial::Session);
        }
        if self
            .journal
            .last()
            .is_some_and(|last| o.id <= last.observation)
        {
            return Err(Denial::StaleObservation);
        }
        self.check_broker_state(&o.target, o.id)?;
        let request = Request {
            session: a.session,
            call_id: Uuid::new_v4(),
            sequence: self.next_sequence,
            observation: o.id,
            policy_revision: p.revision,
            target: o.target.clone(),
            action: Action::Observe,
        };
        if let Decision::Deny(reason) = evaluate(a, p, o, &request, Risk::Unknown, None, now) {
            return Err(reason);
        }
        if self.uncertain {
            let last = self.journal.last_mut().ok_or(Denial::Uncertain)?;
            if last.state != ReceiptState::DispatchedUnknown {
                return Err(Denial::Uncertain);
            }
            last.state = match finding.ok_or(Denial::Uncertain)? {
                ReviewFinding::EffectObserved => ReceiptState::ReviewedApplied,
                ReviewFinding::EffectNotObserved => ReceiptState::ReviewedNotApplied,
            };
        }
        self.effects = self
            .journal
            .iter()
            .filter(|r| {
                matches!(
                    r.state,
                    ReceiptState::Applied | ReceiptState::ReviewedApplied
                )
            })
            .count();
        self.uncertain = false;
        self.interrupted = false;
        Ok(())
    }
}
