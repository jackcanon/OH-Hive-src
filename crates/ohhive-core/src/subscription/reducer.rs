//! Turns protocol-level notifications and server-initiated requests into the typed events and
//! coarse UI states section 8 of the handoff specifies, so Stage 2+'s FFI layer has something
//! concrete to serialize instead of raw `serde_json::Value`. Two things section 8 calls out
//! explicitly are load-bearing here, not incidental: "Scope every event by session/runtime
//! generation; reject late responses from a previous generation" (`Reducer::generation` +
//! `bump_generation`, driven by `Supervisor::reconnect`), and "unknown server requests must
//! receive a supported protocol error/decline rather than being silently accepted or leaving the
//! turn hanging" (`reduce_server_request`'s `decline` flag, acted on by `Supervisor::pump_once`).
//!
//! Auth and terminal shapes are checked against pinned 0.149.0 schema fixtures.
//! Live account/session binding remains Stage 2 work; unsupported requests are declined.

use super::protocol::{self, Notification, ServerRequest};

/// Auth UI states, section 8, verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    MissingRuntime,
    Starting,
    SignedOut,
    SigningIn,
    Ready,
    ReconnectRequired,
    UnavailableEntitlement,
    Limited,
    RuntimeError,
}

/// Coordinator states, section 8, verbatim -- "a ready account does not imply a running
/// coordinator", i.e. deliberately tracked separately from `AuthState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorState {
    Idle,
    Running,
    WaitingOnFleet,
    AwaitingUser,
    PausedLimit,
    Disconnected,
    Completed,
    Failed,
    Interrupted,
}

/// The shared UI/FFI event vocabulary, section 8, verbatim list. Payloads are opaque
/// `serde_json::Value` for now (see this module's doc comment) -- typing them precisely is
/// Stage 2+ work once a real schema exists to type them *against*.
#[derive(Debug, Clone, PartialEq)]
pub enum CoordinatorEvent {
    AuthChanged(serde_json::Value),
    LoginRequired,
    TextDelta(serde_json::Value),
    TaskLinked(serde_json::Value),
    TaskChanged(serde_json::Value),
    ApprovalRequested {
        request_id: super::generated::ServerRequestId,
        method: String,
        params: Option<serde_json::Value>,
    },
    UserInputRequested(serde_json::Value),
    LimitsChanged(serde_json::Value),
    CoordinatorPaused {
        reason: String,
    },
    TurnCompleted(serde_json::Value),
    RuntimeUnavailable {
        detail: String,
    },
    ProtocolMismatch {
        detail: String,
    },
    /// A notification or server request whose method this module doesn't yet know how to map to
    /// one of the events above. Never silently dropped.
    Diagnostic {
        method: String,
        params: Option<serde_json::Value>,
    },
}

/// What to do with one incoming server-initiated request.
pub struct ServerRequestOutcome {
    pub events: Vec<CoordinatorEvent>,
    /// `true` iff `Supervisor::pump_once` should write back a decline response -- always true
    /// today, since no server-request method is confirmed yet (see this module's doc comment);
    /// once a real approval-request shape is known, its arm below will set this `false` and emit
    /// `CoordinatorEvent::ApprovalRequested` instead.
    pub decline: bool,
}

pub struct Reducer {
    generation: u64,
    auth_state: AuthState,
    coordinator_state: CoordinatorState,
}

impl Reducer {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            auth_state: AuthState::Starting,
            coordinator_state: CoordinatorState::Idle,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Invalidates anything scoped to the current generation and starts a new one -- called by
    /// `Supervisor::reconnect` after a crash/restart, never by this module on its own.
    pub fn bump_generation(&mut self) -> u64 {
        self.generation += 1;
        self.auth_state = AuthState::Starting;
        self.coordinator_state = CoordinatorState::Disconnected;
        self.generation
    }

    pub fn auth_state(&self) -> AuthState {
        self.auth_state
    }

    pub fn coordinator_state(&self) -> CoordinatorState {
        self.coordinator_state
    }

    pub fn reduce_notification(&mut self, n: Notification) -> Vec<CoordinatorEvent> {
        let event = match n.method.as_str() {
            protocol::METHOD_INITIALIZED => return Vec::new(), // handshake ack, not UI-facing
            protocol::METHOD_ACCOUNT_LOGIN_COMPLETED => {
                let params = n.params.unwrap_or(serde_json::Value::Null);
                // Login completion alone does not establish the selected account/auth mode.
                // Stage 2 must correlate loginId and re-read the account before activation.
                self.auth_state = if serde_json::from_value::<
                    super::generated::AccountLoginCompletedNotification,
                >(params.clone())
                .map(|value| value.success)
                .unwrap_or(false)
                {
                    AuthState::Starting
                } else {
                    AuthState::SignedOut
                };
                CoordinatorEvent::AuthChanged(params)
            }
            protocol::METHOD_ACCOUNT_UPDATED => {
                let params = n.params.unwrap_or(serde_json::Value::Null);
                self.auth_state = match params.get("authMode") {
                    Some(serde_json::Value::String(mode)) if mode == "chatgpt" => AuthState::Ready,
                    Some(serde_json::Value::Null) => AuthState::SignedOut,
                    Some(serde_json::Value::String(_)) => AuthState::UnavailableEntitlement,
                    _ => AuthState::ReconnectRequired,
                };
                CoordinatorEvent::AuthChanged(params)
            }
            protocol::METHOD_ACCOUNT_RATE_LIMITS_UPDATED => {
                CoordinatorEvent::LimitsChanged(n.params.unwrap_or(serde_json::Value::Null))
            }
            protocol::METHOD_TURN_STARTED => {
                self.coordinator_state = CoordinatorState::Running;
                CoordinatorEvent::TaskChanged(n.params.unwrap_or(serde_json::Value::Null))
            }
            protocol::METHOD_TURN_COMPLETED => {
                let params = n.params.unwrap_or(serde_json::Value::Null);
                self.coordinator_state =
                    match params.pointer("/turn/status").and_then(|v| v.as_str()) {
                        Some("completed") => CoordinatorState::Completed,
                        Some("failed") => CoordinatorState::Failed,
                        Some("interrupted") => CoordinatorState::Interrupted,
                        _ => {
                            self.coordinator_state = CoordinatorState::Disconnected;
                            return vec![CoordinatorEvent::ProtocolMismatch {
                                detail: "turn/completed is missing a recognized terminal status"
                                    .into(),
                            }];
                        }
                    };
                CoordinatorEvent::TurnCompleted(params)
            }
            other => CoordinatorEvent::Diagnostic {
                method: other.to_string(),
                params: n.params,
            },
        };
        vec![event]
    }

    pub fn reduce_server_request(&mut self, sr: &ServerRequest) -> ServerRequestOutcome {
        // No server-request method is confirmed by a real generated schema yet -- section 5
        // names an approval prompt by *shape*, not by a method string, so nothing matches here
        // today. Every server request is therefore "unknown": declined, per spec, never silently
        // accepted or left hanging.
        ServerRequestOutcome {
            events: vec![CoordinatorEvent::Diagnostic {
                method: sr.method.clone(),
                params: sr.params.clone(),
            }],
            decline: true,
        }
    }

    /// Records a transport-level failure (malformed/oversized frame, I/O error) as the one
    /// `ProtocolMismatch`/`RuntimeUnavailable` pairing section 8 gives UI code to distinguish "the
    /// wire said something we don't understand" from "the connection itself is gone".
    pub fn reduce_transport_error(
        &mut self,
        detail: String,
        connection_lost: bool,
    ) -> CoordinatorEvent {
        if connection_lost {
            self.auth_state = AuthState::ReconnectRequired;
            self.coordinator_state = CoordinatorState::Disconnected;
            CoordinatorEvent::RuntimeUnavailable { detail }
        } else {
            CoordinatorEvent::ProtocolMismatch { detail }
        }
    }
}
