use super::*;
use crate::desktop::limits::*;
fn limits() -> Limits {
    Limits {
        turns: 2,
        duration_ms: 5000,
        bytes: 100,
        actions: 3,
        actions_per_second: 2,
        cost_micro_usd: 100,
    }
}
#[test]
fn budget_reservations_are_atomic_bounded_and_conservative() {
    let mut b = Budget::new(limits(), 0).unwrap();
    assert_eq!(b.begin_provider(0, 101, 10), Err(LimitError::Bytes));
    assert_eq!(b.counters(), Counters::default());
    assert_eq!(b.begin_provider(0, 1, u64::MAX), Err(LimitError::Cost));
    assert_eq!(b.counters(), Counters::default());
    b.begin_provider(0, 50, 60).unwrap();
    assert_eq!(b.begin_provider(0, 1, 1), Err(LimitError::Pending));
    b.finish_provider(Some(40)).unwrap();
    assert_eq!(b.counters().cost_micro_usd, 40);
    b.begin_provider(1, 50, 60).unwrap();
    b.finish_provider(None).unwrap();
    assert_eq!(
        b.counters(),
        Counters {
            turns: 2,
            bytes: 100,
            actions: 0,
            cost_micro_usd: 100
        }
    );
    assert_eq!(b.begin_provider(2, 1, 1), Err(LimitError::Turns));
}
#[test]
fn overquote_clock_deadline_and_rate_fail_closed() {
    let mut b = Budget::new(limits(), 0).unwrap();
    b.begin_provider(0, 1, 10).unwrap();
    assert_eq!(b.finish_provider(Some(11)), Err(LimitError::Cost));
    assert_eq!(b.input_action(0), Err(LimitError::Cost));
    let mut b = Budget::new(limits(), 0).unwrap();
    b.input_action(0).unwrap();
    b.input_action(999).unwrap();
    assert_eq!(b.input_action(999), Err(LimitError::Rate));
    b.input_action(1000).unwrap();
    assert_eq!(b.input_action(2000), Err(LimitError::Actions));
    assert_eq!(b.check_time(1999), Err(LimitError::Invalid));
    assert_eq!(b.check_time(5000), Err(LimitError::Time));
    assert!(Budget::new(limits(), u64::MAX).is_err());
}
#[test]
fn profiles_reject_unknown_missing_and_legacy_shapes() {
    assert!(validate_profile(&DesktopProfile::current()).is_ok());
    for fixture in [
        r#"{"name":"desktop","version":2}"#,
        r#"{"name":"coding","version":1}"#,
        r#"{"name":"desktop","version":0}"#,
    ] {
        assert!(validate_profile(&serde_json::from_str(fixture).unwrap()).is_err());
    }
    for fixture in [
        r#"{"name":"desktop"}"#,
        r#"{"name":"desktop","version":1,"shell":true}"#,
        r#""desktop_v1""#,
    ] {
        assert!(serde_json::from_str::<DesktopProfile>(fixture).is_err());
    }
}
fn fixture() -> (
    DesktopSession,
    Authority,
    Policy,
    Observation,
    Request,
    Request,
) {
    let target = Target {
        bundle_id: "test".into(),
        process_id: 1,
        process_instance: Uuid::new_v4(),
        window_id: 2,
    };
    let owner = Uuid::new_v4();
    let node = Uuid::new_v4();
    let id = Uuid::new_v4();
    let a = Authority {
        session: id,
        owner,
        project_owner: owner,
        node,
        target_node: node,
        private_local: true,
        lease_deadline: 100,
        authority_deadline: 100,
        stopped: false,
        grant_deadline: 100,
        access: Access::Control,
        target: target.clone(),
    };
    let o = Observation {
        id: 1,
        target: target.clone(),
        width: 10,
        height: 10,
        allowed_rect: [0, 0, 10, 10],
        valid_until: 100,
    };
    let r = Request {
        session: id,
        call_id: Uuid::new_v4(),
        sequence: 0,
        observation: 1,
        policy_revision: 1,
        target: target.clone(),
        action: Action::Click { x: 1, y: 1 },
    };
    let mut next = r.clone();
    next.call_id = Uuid::new_v4();
    next.sequence = 1;
    next.observation = 2;
    let mut s = DesktopSession::new(id, &DesktopProfile::current(), Limits::default(), 0).unwrap();
    s.report_focus_change(Some(target), 0);
    (s, a, Policy::default(), o, r, next)
}
#[test]
fn each_action_needs_its_own_consent_and_failed_batch_cannot_resume() {
    let (mut s, a, p, mut o, r, next) = fixture();
    let mut b = Batch::new(vec![r.call_id, next.call_id]).unwrap();
    let consent = Approval::from_local_consent(&r, Risk::SendOrPublish, 50);
    assert_eq!(
        s.execute_step(
            &mut b,
            &a,
            &p,
            &o,
            &r,
            Risk::SendOrPublish,
            Some(&consent),
            1,
            1
        )
        .unwrap(),
        Outcome::Applied
    );
    o.id = 2;
    assert_eq!(
        s.execute_step(
            &mut b,
            &a,
            &p,
            &o,
            &next,
            Risk::SendOrPublish,
            Some(&consent),
            2,
            2
        )
        .unwrap(),
        Outcome::NeedsConfirmation
    );
    let new_consent = Approval::from_local_consent(&next, Risk::SendOrPublish, 50);
    assert_eq!(
        s.execute_step(
            &mut b,
            &a,
            &p,
            &o,
            &next,
            Risk::SendOrPublish,
            Some(&new_consent),
            2,
            2
        ),
        Err(SessionError::BatchStopped)
    );
    assert_eq!(s.effects(), 1);
}
#[test]
fn revocation_and_interruption_stop_remaining_individually_approved_actions() {
    for mode in 0..3 {
        let (mut s, a, p, mut o, r, next) = fixture();
        let mut b = Batch::new(vec![r.call_id, next.call_id]).unwrap();
        let first = Approval::from_local_consent(&r, Risk::SendOrPublish, 50);
        let second = Approval::from_local_consent(&next, Risk::SendOrPublish, 50);
        assert_eq!(
            s.execute_step(
                &mut b,
                &a,
                &p,
                &o,
                &r,
                Risk::SendOrPublish,
                Some(&first),
                1,
                1
            )
            .unwrap(),
            Outcome::Applied
        );
        match mode {
            0 => s.revoke_grant(&a.target),
            1 => s.interrupt(),
            _ => s.mark_uncertain(),
        };
        o.id = 2;
        let result = s.execute_step(
            &mut b,
            &a,
            &p,
            &o,
            &next,
            Risk::SendOrPublish,
            Some(&second),
            2,
            2,
        );
        assert!(!matches!(result, Ok(Outcome::Applied)));
        assert!(b.stopped());
        assert_eq!(s.effects(), 1);
    }
}
#[test]
fn fresh_observation_and_correct_order_are_required_even_with_consent() {
    let (mut s, a, p, o, r, next) = fixture();
    let mut b = Batch::new(vec![r.call_id, next.call_id]).unwrap();
    assert_eq!(
        s.execute_step(&mut b, &a, &p, &o, &next, Risk::Routine, None, 1, 1),
        Err(SessionError::BatchStopped)
    );
    assert_eq!(s.effects(), 0);
    let mut b = Batch::new(vec![r.call_id, next.call_id]).unwrap();
    assert_eq!(
        s.execute_step(&mut b, &a, &p, &o, &r, Risk::Routine, None, 1, 1)
            .unwrap(),
        Outcome::Applied
    );
    let stale = s
        .execute_step(&mut b, &a, &p, &o, &next, Risk::Routine, None, 2, 2)
        .unwrap();
    assert!(matches!(stale, Outcome::Rejected(_)));
    assert!(b.stopped());
    assert_eq!(s.effects(), 1);
}

#[test]
fn separate_consents_allow_fresh_actions_and_deadline_blocks_further_effects() {
    let (mut s, a, p, mut o, r, next) = fixture();
    let mut b = Batch::new(vec![r.call_id, next.call_id]).unwrap();
    let c1 = Approval::from_local_consent(&r, Risk::SendOrPublish, 50);
    let c2 = Approval::from_local_consent(&next, Risk::SendOrPublish, 50);
    assert_eq!(
        s.execute_step(&mut b, &a, &p, &o, &r, Risk::SendOrPublish, Some(&c1), 1, 1)
            .unwrap(),
        Outcome::Applied
    );
    o.id = 2;
    assert_eq!(
        s.execute_step(
            &mut b,
            &a,
            &p,
            &o,
            &next,
            Risk::SendOrPublish,
            Some(&c2),
            2,
            2
        )
        .unwrap(),
        Outcome::Applied
    );
    assert_eq!(s.effects(), 2);
    let mut third = next.clone();
    third.call_id = Uuid::new_v4();
    third.sequence = 2;
    third.observation = 3;
    o.id = 3;
    let mut b = Batch::new(vec![third.call_id]).unwrap();
    assert_eq!(
        s.execute_step(&mut b, &a, &p, &o, &third, Risk::Routine, None, 3, 900_000),
        Err(SessionError::Limits(LimitError::Time))
    );
    assert_eq!(s.effects(), 2);
}
#[test]
fn byte_limits_and_unresolved_provider_calls_block_all_batch_dispatch() {
    let (mut s, a, p, o, r, _) = fixture();
    s.budget = Budget::new(
        Limits {
            bytes: 1,
            ..Limits::default()
        },
        0,
    )
    .unwrap();
    let mut b = Batch::new(vec![r.call_id]).unwrap();
    assert_eq!(
        s.execute_step(&mut b, &a, &p, &o, &r, Risk::Routine, None, 1, 1),
        Err(SessionError::Limits(LimitError::Bytes))
    );
    assert_eq!(s.effects(), 0);
    let (mut s, a, p, o, mut r, _) = fixture();
    r.action = Action::Observe;
    s.budget.begin_provider(0, 1, 1).unwrap(); // models cancellation before settlement
    let mut b = Batch::new(vec![r.call_id]).unwrap();
    assert_eq!(
        s.execute_step(&mut b, &a, &p, &o, &r, Risk::Routine, None, 1, 1),
        Err(SessionError::Limits(LimitError::Pending))
    );
    assert_eq!(s.effects(), 0);
}
