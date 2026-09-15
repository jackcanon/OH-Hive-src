//! Trusted per-session accounting. Time is monotonic milliseconds; money is integer micro-USD.
//! Retain this ledger for a logical session; no restart/resume persistence is provided here.
//! Never recreate it with fresh counters for an existing session.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug)]
pub struct Limits {
    pub turns: u64,
    pub duration_ms: u64,
    pub bytes: u64,
    pub actions: u64,
    pub actions_per_second: usize,
    pub cost_micro_usd: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            turns: 100,
            duration_ms: 900_000,
            bytes: 128 * 1024 * 1024,
            actions: 1000,
            actions_per_second: 4,
            cost_micro_usd: 1_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LimitError {
    #[error("invalid limits or backwards clock")]
    Invalid,
    #[error("desktop session deadline reached")]
    Time,
    #[error("desktop turn limit reached")]
    Turns,
    #[error("desktop byte limit reached")]
    Bytes,
    #[error("desktop spending limit reached or quote exceeded")]
    Cost,
    #[error("desktop action limit reached")]
    Actions,
    #[error("desktop action rate exceeded")]
    Rate,
    #[error("a provider call is unresolved")]
    Pending,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    pub turns: u64,
    pub bytes: u64,
    pub actions: u64,
    pub cost_micro_usd: u64,
}
/// Not a model-wire type. Configuration is fixed for this ledger's lifetime.
pub struct Budget {
    limits: Limits,
    deadline: u64,
    last_now: u64,
    counters: Counters,
    action_times: std::collections::VecDeque<u64>,
    pending: Option<u64>,
    failed: bool,
}
impl Budget {
    pub fn new(limits: Limits, now_ms: u64) -> Result<Self, LimitError> {
        if limits.turns == 0
            || limits.duration_ms == 0
            || limits.bytes == 0
            || limits.actions == 0
            || limits.actions_per_second == 0
            || limits.actions_per_second > 1000
            || limits.cost_micro_usd == 0
        {
            return Err(LimitError::Invalid);
        }
        let deadline = now_ms
            .checked_add(limits.duration_ms)
            .ok_or(LimitError::Invalid)?;
        Ok(Self {
            limits,
            deadline,
            last_now: now_ms,
            counters: Counters::default(),
            action_times: Default::default(),
            pending: None,
            failed: false,
        })
    }
    pub fn counters(&self) -> Counters {
        self.counters
    }
    pub fn check_time(&mut self, now_ms: u64) -> Result<(), LimitError> {
        if now_ms < self.last_now {
            return Err(LimitError::Invalid);
        }
        self.last_now = now_ms;
        if now_ms >= self.deadline {
            return Err(LimitError::Time);
        }
        if self.failed {
            return Err(LimitError::Cost);
        }
        Ok(())
    }
    pub fn ready(&mut self, now_ms: u64) -> Result<(), LimitError> {
        self.check_time(now_ms)?;
        if self.pending.is_some() {
            return Err(LimitError::Pending);
        }
        Ok(())
    }
    /// Account serialized action-request bytes, including rejected attempts conservatively.
    pub fn request_bytes(&mut self, bytes: u64) -> Result<(), LimitError> {
        self.counters.bytes = self
            .counters
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.bytes)
            .ok_or(LimitError::Bytes)?;
        Ok(())
    }
    /// Reserve a trusted upper bound BEFORE network I/O. Bytes include the maximum response.
    /// Quote must cover all billable tokens/features; this module cannot verify provider pricing.
    pub fn begin_provider(
        &mut self,
        now_ms: u64,
        bytes: u64,
        cost_ceiling: u64,
    ) -> Result<(), LimitError> {
        self.check_time(now_ms)?;
        if self.pending.is_some() {
            return Err(LimitError::Pending);
        }
        if self.counters.turns >= self.limits.turns {
            return Err(LimitError::Turns);
        }
        let new_bytes = self
            .counters
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.bytes)
            .ok_or(LimitError::Bytes)?;
        let cost = self
            .counters
            .cost_micro_usd
            .checked_add(cost_ceiling)
            .filter(|n| *n <= self.limits.cost_micro_usd)
            .ok_or(LimitError::Cost)?;
        if cost_ceiling == 0 {
            return Err(LimitError::Cost);
        }
        self.counters.turns += 1;
        self.counters.bytes = new_bytes;
        self.counters.cost_micro_usd = cost;
        self.pending = Some(cost_ceiling);
        Ok(())
    }
    /// None means uncertain billing: retain full reservation. Quote overrun permanently stops ledger.
    pub fn finish_provider(&mut self, actual_cost: Option<u64>) -> Result<(), LimitError> {
        let reserved = self.pending.take().ok_or(LimitError::Invalid)?;
        if let Some(actual) = actual_cost {
            if actual > reserved {
                self.failed = true;
                self.counters.cost_micro_usd = self
                    .counters
                    .cost_micro_usd
                    .saturating_add(actual - reserved);
                return Err(LimitError::Cost);
            }
            self.counters.cost_micro_usd -= reserved - actual;
        }
        Ok(())
    }
    /// Reserve an input attempt before dispatch; rejected attempts remain counted conservatively.
    pub fn input_action(&mut self, now_ms: u64) -> Result<(), LimitError> {
        self.check_time(now_ms)?;
        if self.pending.is_some() {
            return Err(LimitError::Pending);
        }
        if self.counters.actions >= self.limits.actions {
            return Err(LimitError::Actions);
        }
        while self
            .action_times
            .front()
            .is_some_and(|t| now_ms - *t >= 1000)
        {
            self.action_times.pop_front();
        }
        if self.action_times.len() >= self.limits.actions_per_second {
            return Err(LimitError::Rate);
        }
        self.action_times.push_back(now_ms);
        self.counters.actions += 1;
        Ok(())
    }
}
