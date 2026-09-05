//! Usage metering types. The **coordinator**, not the node, converts these into
//! $honey ledger entries (ADR-002 decision 12): a node reports usage, the
//! coordinator counts tokens from the output stream it actually received.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Usage {
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// Wall-clock compute seconds, the basis for non-token modalities
    /// (ADR-002 open item: compute-seconds × hardware class).
    pub compute_seconds: f64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.tokens_in += other.tokens_in;
        self.tokens_out += other.tokens_out;
        self.compute_seconds += other.compute_seconds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_accumulates() {
        let mut u = Usage {
            tokens_in: 1,
            tokens_out: 2,
            compute_seconds: 0.5,
        };
        u.add(Usage {
            tokens_in: 10,
            tokens_out: 20,
            compute_seconds: 1.0,
        });
        assert_eq!((u.tokens_in, u.tokens_out), (11, 22));
        assert!((u.compute_seconds - 1.5).abs() < f64::EPSILON);
    }
}
