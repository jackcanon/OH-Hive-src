//! ADR-035 C2 Track A slice 4: the termination argument, as a test.
//!
//! These are the tests that make the one-line change from `Vec::new()` to a real recipient list
//! safe to ship. Two agents that each reply by naming the other is an infinite loop in any
//! system without a bound; every test here gives the executor exactly that input and asserts it
//! stops, and stops where the budgets say it should.
//!
//! Each loop runs under an explicit iteration cap so a regression fails the test rather than
//! hanging CI.

#![cfg(all(feature = "bots", feature = "local-hub"))]
use std::sync::Arc;

use async_trait::async_trait;
use hive_core::bots::{
    AgentProfile, AgentRuntimeKind, ConversationKind, DeliveryStatus, HandoffBudgets,
    LocalBotsTurnRunner, LocalTurnError, LocalTurnOutcome, LocalTurnRequest, MessageId,
    MessageKind, NewAgentProfile, NewConversation, NewMessage, Principal, StorageScope,
};
use hive_core::bots::executor::DeliveryExecutor;
use hive_core::local_hub::LocalHubStore;
use uuid::Uuid;

/// Replies with whatever the fixture was told to say, keyed by the replying agent's name, so a
/// test can build a deliberate mention cycle. No model, no capacity concerns.
pub struct ScriptedRunner {
    script: Vec<(String, String)>,
}

#[async_trait]
impl LocalBotsTurnRunner for ScriptedRunner {
    async fn run_turn(
        &self,
        agent: &AgentProfile,
        _request: LocalTurnRequest,
    ) -> Result<LocalTurnOutcome, LocalTurnError> {
        let body = self
            .script
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&agent.name))
            .map(|(_, reply)| reply.clone())
            .unwrap_or_else(|| "acknowledged".to_string());
        Ok(LocalTurnOutcome {
            reply_body: body,
            usage: None,
        })
    }
}

struct Fixture {
    store: Arc<LocalHubStore>,
    owner: Uuid,
    host: Uuid,
    conversation: Uuid,
    agents: Vec<AgentProfile>,
}

/// A Team room owned by one user with `names` as agent members, all locally hosted.
fn room(names: &[&str]) -> Fixture {
    let store = Arc::new(LocalHubStore::in_memory().expect("store"));
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let agents: Vec<AgentProfile> = names
        .iter()
        .map(|name| {
            store
                .bots_agents_create(NewAgentProfile {
                    owner,
                    name: (*name).to_string(),
                    runtime_kind: AgentRuntimeKind::Local,
                    preferred_host: Some(host),
                    capability_policy_ref: "default".into(),
                    provider_account_ref: None,
                    memory_namespace: (*name).to_string(),
                })
                .expect("create agent")
        })
        .collect();
    let conversation = store
        .bots_conversations_create(NewConversation {
            // Rooms gained a persistent name in schema v10 (Sif). The fixture names its room so
            // these tests exercise the same shape the app actually creates.
            title: Some("loop safety".into()),
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        })
        .expect("create room")
        .id;
    for agent in &agents {
        store
            .bots_conversations_join(Principal::Agent(agent.id), conversation)
            .expect("join room");
    }
    Fixture {
        store,
        owner,
        host,
        conversation,
        agents,
    }
}

impl Fixture {
    fn agent(&self, name: &str) -> &AgentProfile {
        self.agents
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .expect("agent in room")
    }

    /// The human opens the chain. Returns the root message id.
    fn human_says(&self, body: &str, to: &[&str]) -> MessageId {
        let recipients: Vec<Uuid> = to.iter().map(|n| self.agent(n).id).collect();
        self.store
            .bots_message_send(
                Principal::User(self.owner),
                self.conversation,
                format!("human:{}", Uuid::new_v4()),
                1,
                recipients,
                NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some(body.into()),
                    attachment_refs: Vec::new(),
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .expect("human send")
            .id
    }

    fn executor(&self, script: Vec<(&str, &str)>, budgets: HandoffBudgets) -> DeliveryExecutor {
        let runner = Arc::new(ScriptedRunner {
            script: script
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        });
        DeliveryExecutor::new(self.store.clone(), runner, self.host, self.owner)
            .with_budgets(budgets)
    }

    fn turns_for_root(&self, root: MessageId) -> u32 {
        self.store.bots_turns_for_root(root).expect("count")
    }

    fn held(&self) -> usize {
        self.store.bots_deliveries_held(200).expect("held").len()
    }

    fn system_notices(&self) -> Vec<String> {
        use hive_core::bots::{BotsService, MessagePage};
        let messages = futures::executor::block_on(self.store.messages_list(
            Principal::User(self.owner),
            self.conversation,
            MessagePage {
                before: None,
                after: None,
                limit: 500,
            },
        ))
        .expect("list");
        messages
            .into_iter()
            .filter(|m| m.kind == MessageKind::System)
            .filter_map(|m| m.body)
            .collect()
    }
}

/// Drain until quiet, with a hard cap so a loop bug fails instead of hanging.
async fn drain_to_quiet(executor: &DeliveryExecutor, cap: usize) -> usize {
    for pass in 1..=cap {
        let summary = executor.drain_once().await;
        if summary.delivered == 0 && summary.failed == 0 && summary.requeued == 0 {
            return pass;
        }
    }
    panic!("executor never went quiet in {cap} passes -- the chain is not terminating");
}

/// The headline test. A and B each reply by naming the other: an unbounded loop without depth
/// enforcement. It must terminate, and it must terminate exactly at max_depth.
#[tokio::test]
async fn mutual_mention_cycle_terminates_at_max_depth() {
    let f = room(&["Alpha", "Beta"]);
    let budgets = HandoffBudgets {
        max_depth: 3,
        max_turns_per_root: 1000, // deliberately not the binding constraint here
        ..HandoffBudgets::default()
    };
    let executor = f.executor(
        vec![("Alpha", "@Beta your turn"), ("Beta", "@Alpha your turn")],
        budgets,
    );
    let root = f.human_says("@Alpha kick off", &["Alpha"]);

    drain_to_quiet(&executor, 50).await;

    // depth 0 (the human's delivery to Alpha) through depth 3 inclusive = 4 deliveries. Nothing
    // is created at depth 4, which is what stops the cycle.
    assert_eq!(
        f.turns_for_root(root),
        4,
        "expected one delivery per depth 0..=3 and nothing beyond"
    );
    assert!(
        f.system_notices().iter().any(|n| n.contains("Depth limit")),
        "hitting the depth limit must be visible, not silent: {:?}",
        f.system_notices()
    );
}

/// Same cycle at the agreed enabled numbers (depth 6, 30 turns per root). Note this is NOT what
/// ships by default -- `DeliveryExecutor::new` disables fan-out, and `HandoffBudgets::default()`
/// passed to `with_budgets` is what turns it on.
#[tokio::test]
async fn mutual_mention_cycle_terminates_at_the_agreed_enabled_numbers() {
    let f = room(&["Alpha", "Beta"]);
    let executor = f.executor(
        vec![("Alpha", "@Beta your turn"), ("Beta", "@Alpha your turn")],
        HandoffBudgets::default(),
    );
    let root = f.human_says("@Alpha kick off", &["Alpha"]);

    drain_to_quiet(&executor, 100).await;

    assert_eq!(f.turns_for_root(root), 7, "depths 0..=6 under max_depth 6");
    assert_eq!(f.held(), 0, "30-turn gate must not fire on a 7-turn chain");
}

/// The gate: reaching max_turns_per_root holds the next deliveries for a person instead of
/// killing the chain, and a release resumes it.
#[tokio::test]
async fn per_root_budget_holds_for_a_human_then_releases() {
    let f = room(&["Alpha", "Beta"]);
    let budgets = HandoffBudgets {
        max_depth: 20,        // let the turn budget be the binding constraint
        max_turns_per_root: 3,
        ..HandoffBudgets::default()
    };
    let executor = f.executor(
        vec![("Alpha", "@Beta your turn"), ("Beta", "@Alpha your turn")],
        budgets,
    );
    let root = f.human_says("@Alpha kick off", &["Alpha"]);

    drain_to_quiet(&executor, 50).await;

    assert_eq!(f.held(), 1, "the turn that would exceed the budget is held, not dropped");
    assert!(
        f.system_notices().iter().any(|n| n.contains("Release to continue")),
        "a held chain must say so: {:?}",
        f.system_notices()
    );
    let before = f.turns_for_root(root);

    // A person lets it continue.
    assert_eq!(executor.release_root(root), 1, "one held delivery released");
    assert_eq!(f.held(), 0, "release returns held deliveries to pending");
    drain_to_quiet(&executor, 50).await;
    assert!(
        f.turns_for_root(root) > before,
        "the released chain must actually make progress"
    );
}

/// A held delivery is not work: it must never be handed to a drain pass.
#[tokio::test]
async fn held_deliveries_are_never_drained() {
    let f = room(&["Alpha", "Beta"]);
    let budgets = HandoffBudgets {
        max_depth: 20,
        max_turns_per_root: 2,
        ..HandoffBudgets::default()
    };
    let executor = f.executor(
        vec![("Alpha", "@Beta your turn"), ("Beta", "@Alpha your turn")],
        budgets,
    );
    f.human_says("@Alpha kick off", &["Alpha"]);
    drain_to_quiet(&executor, 50).await;

    let held_before = f.held();
    assert!(held_before > 0, "fixture should have produced a hold");
    // Many further passes must change nothing at all while the hold stands.
    for _ in 0..5 {
        let summary = executor.drain_once().await;
        assert_eq!(summary.delivered, 0, "a held chain must not advance on its own");
    }
    assert_eq!(f.held(), held_before, "holds neither expire nor multiply");
}

/// An agent naming itself is the cheapest infinite loop. It must create no delivery at all.
#[tokio::test]
async fn self_mention_creates_no_delivery() {
    let f = room(&["Alpha"]);
    let executor = f.executor(
        vec![("Alpha", "@Alpha I will keep thinking")],
        HandoffBudgets::default(),
    );
    let root = f.human_says("@Alpha begin", &["Alpha"]);

    drain_to_quiet(&executor, 20).await;

    assert_eq!(
        f.turns_for_root(root),
        1,
        "only the human's own delivery; a self-mention must not wake the agent again"
    );
}

/// Fan-out width is asymmetric: a human may address a room, an agent's reply may not.
#[tokio::test]
async fn agent_fan_out_is_capped_and_says_so() {
    let f = room(&["Alpha", "Beta", "Gamma", "Delta"]);
    let budgets = HandoffBudgets {
        max_depth: 1, // one hop, so we measure the first fan-out only
        max_active_specialist_handoffs_per_run: 2,
        max_turns_per_root: 1000,
        ..HandoffBudgets::default()
    };
    let executor = f.executor(
        vec![("Alpha", "@Beta @Gamma @Delta all of you please")],
        budgets,
    );
    let root = f.human_says("@Alpha delegate", &["Alpha"]);

    drain_to_quiet(&executor, 30).await;

    // 1 human delivery + 2 (not 3) from Alpha's reply.
    assert_eq!(f.turns_for_root(root), 3, "an agent reply may wake at most 2");
    assert!(
        f.system_notices().iter().any(|n| n.contains("at most 2")),
        "the suppressed fan-out must be visible: {:?}",
        f.system_notices()
    );
}

/// A System message wakes nobody, whatever recipients are handed to it. This is what makes the
/// notices above free, and it is what stops a six-agent room turning every status post into six
/// billable turns.
#[tokio::test]
async fn system_messages_create_no_deliveries() {
    let f = room(&["Alpha"]);
    let alpha = f.agent("Alpha").id;
    let sent = f
        .store
        .bots_message_send(
            Principal::User(f.owner),
            f.conversation,
            "sys:1".into(),
            1,
            vec![alpha],
            NewMessage {
                thread_root: None,
                kind: MessageKind::System,
                body: Some("still working".into()),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .expect("system send");
    assert_eq!(
        f.turns_for_root(sent.id),
        0,
        "a System message must create no deliveries even with a recipient list"
    );
}

/// A mention inside a code fence must not summon anyone -- the failure mode that would make a
/// room about code unusable.
#[tokio::test]
async fn code_fenced_mention_wakes_no_one() {
    let f = room(&["Alpha", "Beta"]);
    let executor = f.executor(
        vec![("Alpha", "try this:\n```swift\n@Beta var x = 1\n```\ndone")],
        HandoffBudgets::default(),
    );
    let root = f.human_says("@Alpha show me", &["Alpha"]);

    drain_to_quiet(&executor, 20).await;

    assert_eq!(f.turns_for_root(root), 1, "code is not a mention");
}

/// Pre-v10 rows migrate as depth-0 roots. A delivery created through the unchanged
/// `bots_message_send` path must behave exactly like a human-originated one.
#[tokio::test]
async fn human_send_is_its_own_root_at_depth_zero() {
    let f = room(&["Alpha"]);
    let alpha = f.agent("Alpha").id;
    let root = f.human_says("@Alpha hello", &["Alpha"]);
    let pending = f
        .store
        .bots_deliveries_pending_for_agent(alpha, 10)
        .expect("pending");
    let delivery = pending.first().expect("one pending delivery");
    assert_eq!(delivery.turn_depth, 0);
    assert_eq!(delivery.root_message_id, Some(root));
    assert_eq!(delivery.cause_message_id, None);
    assert_eq!(delivery.status, DeliveryStatus::Pending);
}

/// What actually ships: an executor built the way every production call site builds one -- no
/// `with_budgets` -- must not fan out at all, however enthusiastically an agent names people.
#[tokio::test]
async fn a_default_executor_does_not_fan_out() {
    let f = room(&["Alpha", "Beta", "Gamma"]);
    let runner = Arc::new(ScriptedRunner {
        script: vec![("Alpha".into(), "@Beta @Gamma @everyone all of you".into())],
    });
    let executor = DeliveryExecutor::new(f.store.clone(), runner, f.host, f.owner);
    let root = f.human_says("@Alpha begin", &["Alpha"]);

    drain_to_quiet(&executor, 20).await;

    assert_eq!(
        f.turns_for_root(root),
        1,
        "the shipped default is fan-out off; enabling it must be a deliberate with_budgets call"
    );
    assert!(
        f.system_notices().is_empty(),
        "fan-out off is a feature switched off, not a budget being hit -- no notice: {:?}",
        f.system_notices()
    );
}

/// Audit §3.4: `before` must return the page immediately preceding it -- the NEWEST messages
/// below that point. It returned the oldest, so an agent in any conversation longer than the
/// history window replied to the opening of the conversation and never saw the live thread.
#[tokio::test]
async fn before_paging_returns_the_most_recent_page_not_the_oldest() {
    use hive_core::bots::{BotsService, MessagePage};

    let f = room(&["Alpha"]);
    let alpha = f.agent("Alpha").id;
    for n in 1..=40 {
        f.store
            .bots_message_send(
                Principal::User(f.owner),
                f.conversation,
                format!("m{n}"),
                1,
                Vec::new(),
                NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some(format!("message {n}")),
                    attachment_refs: Vec::new(),
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .expect("send");
    }

    let page = f
        .store
        .messages_list(
            Principal::Agent(alpha),
            f.conversation,
            MessagePage { before: Some(41), after: None, limit: 32 },
        )
        .await
        .expect("list");

    let sequences: Vec<u64> = page.iter().map(|m| m.server_sequence).collect();
    assert_eq!(sequences.len(), 32);
    assert_eq!(
        (*sequences.first().unwrap(), *sequences.last().unwrap()),
        (9, 40),
        "must be sequences 9..=40 (the newest 32 below 41), not 1..=32"
    );
    assert!(
        sequences.windows(2).all(|w| w[0] < w[1]),
        "and still oldest-first for the caller"
    );

    // Forward paging is unchanged.
    let forward = f
        .store
        .messages_list(
            Principal::Agent(alpha),
            f.conversation,
            MessagePage { before: None, after: Some(0), limit: 5 },
        )
        .await
        .expect("list");
    assert_eq!(
        forward.iter().map(|m| m.server_sequence).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5],
        "after: paging still walks forward from the start"
    );
}

/// Routing for BYOK provider agents. Before this, `drain_once` filtered to
/// `runtime_kind == Local && preferred_host == this host`, so a Claude or Nous agent's delivery was
/// created and then claimed by nothing, ever -- the room simply looked like Claude ignored you.
#[tokio::test]
async fn a_byok_agent_replies_when_a_cloud_runner_is_attached() {
    let f = room(&["Local"]);
    let claude = f
        .store
        .bots_agents_create(hive_core::bots::NewAgentProfile {
            owner: f.owner,
            name: "Claude".into(),
            runtime_kind: AgentRuntimeKind::AnthropicByok,
            // As ensure_provider_agents creates it: no host, because the turn happens hub-side.
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "claude".into(),
        })
        .expect("byok agent");
    f.store
        .bots_conversations_join(Principal::Agent(claude.id), f.conversation)
        .expect("join");
    f.store
        .bots_message_send(
            Principal::User(f.owner),
            f.conversation,
            "ask".into(),
            1,
            vec![claude.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("@Claude are you there?".into()),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .expect("send");

    // Stand-in for CloudTurnRunner: the executor's contract with it is the trait, nothing more.
    let cloud = Arc::new(ScriptedRunner {
        script: vec![("Claude".into(), "Yes — reading now.".into())],
    });
    let executor = f
        .executor(vec![], HandoffBudgets::default())
        .with_cloud_runner(cloud);

    let summary = executor.drain_once().await;
    assert_eq!(summary.delivered, 1, "the BYOK delivery must actually run");
    assert_eq!(summary.failed, 0);

    let notices = f.system_notices();
    assert!(
        !notices.iter().any(|n| n.to_lowercase().contains("not implemented")
            || n.to_lowercase().contains("has not started a reply")),
        "a supported cloud agent must not also be told it is unsupported: {notices:?}"
    );

    let pending = f
        .store
        .bots_deliveries_pending_for_agent(claude.id, 10)
        .expect("pending");
    assert!(pending.is_empty(), "the delivery is resolved, not left hanging");
}

/// Without a cloud runner the same delivery must stay claimable -- left for another host, or for
/// this one once a runner is configured -- and never be marked failed.
#[tokio::test]
async fn a_byok_agent_without_a_cloud_runner_is_left_claimable() {
    let f = room(&["Local"]);
    let nous = f
        .store
        .bots_agents_create(hive_core::bots::NewAgentProfile {
            owner: f.owner,
            name: "Nous".into(),
            runtime_kind: AgentRuntimeKind::NousByok,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "nous".into(),
        })
        .expect("byok agent");
    f.store
        .bots_conversations_join(Principal::Agent(nous.id), f.conversation)
        .expect("join");
    f.store
        .bots_message_send(
            Principal::User(f.owner),
            f.conversation,
            "ask".into(),
            1,
            vec![nous.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("@Nous thoughts?".into()),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .expect("send");

    let executor = f.executor(vec![], HandoffBudgets::default());
    let summary = executor.drain_once().await;
    assert_eq!(summary.delivered, 0);
    assert_eq!(summary.failed, 0, "no runner is not a failed turn");

    let pending = f
        .store
        .bots_deliveries_pending_for_agent(nous.id, 10)
        .expect("pending");
    assert_eq!(pending.len(), 1, "must stay claimable for a host that can run it");
    assert!(
        !f.system_notices().is_empty(),
        "and the room must be told why nothing happened"
    );
}

/// A subscription-coordinator runtime has no runner at all yet (ADR-034: only Codex has any
/// scaffold, and Claude is excluded from that path permanently). Attaching a cloud runner must not
/// accidentally route those to it -- the BYOK and subscription concepts are different credentials.
#[tokio::test]
async fn a_subscription_agent_is_not_routed_to_the_cloud_runner() {
    let f = room(&["Local"]);
    let gpt = f
        .store
        .bots_agents_create(hive_core::bots::NewAgentProfile {
            owner: f.owner,
            name: "ChatGPT".into(),
            runtime_kind: AgentRuntimeKind::ChatgptSubscription,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "gpt".into(),
        })
        .expect("subscription agent");
    f.store
        .bots_conversations_join(Principal::Agent(gpt.id), f.conversation)
        .expect("join");
    f.store
        .bots_message_send(
            Principal::User(f.owner),
            f.conversation,
            "ask".into(),
            1,
            vec![gpt.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("@ChatGPT hello".into()),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .expect("send");

    let cloud = Arc::new(ScriptedRunner {
        script: vec![("ChatGPT".into(), "I should not have been asked".into())],
    });
    let executor = f
        .executor(vec![], HandoffBudgets::default())
        .with_cloud_runner(cloud);

    let summary = executor.drain_once().await;
    assert_eq!(summary.delivered, 0, "a subscription runtime has no runner yet");
    let pending = f
        .store
        .bots_deliveries_pending_for_agent(gpt.id, 10)
        .expect("pending");
    assert_eq!(pending.len(), 1);
}
