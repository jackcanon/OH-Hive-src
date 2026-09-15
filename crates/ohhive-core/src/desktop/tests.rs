use super::*;
fn fixture() -> (Authority, Policy, Observation, Request) {
    let owner = Uuid::new_v4();
    let node = Uuid::new_v4();
    let session = Uuid::new_v4();
    let target = Target {
        bundle_id: "fixture.editor".into(),
        process_id: 42,
        process_instance: Uuid::new_v4(),
        window_id: 3,
    };
    let a = Authority {
        session,
        owner,
        project_owner: owner,
        node,
        target_node: node,
        private_local: true,
        lease_deadline: 100,
        authority_deadline: 90,
        grant_deadline: 80,
        stopped: false,
        access: Access::Control,
        target: target.clone(),
    };
    let o = Observation {
        id: 1,
        target: target.clone(),
        width: 100,
        height: 100,
        allowed_rect: [10, 10, 90, 90],
        valid_until: 70,
    };
    let r = Request {
        session,
        call_id: Uuid::new_v4(),
        sequence: 0,
        observation: 1,
        policy_revision: 1,
        target,
        action: Action::Click { x: 20, y: 20 },
    };
    (a, Policy::default(), o, r)
}
#[test]
fn authority_and_revocation_override_all_permissions() {
    let (a, p, o, r) = fixture();
    for mutate in [
        |a: &mut Authority| a.private_local = false,
        |a: &mut Authority| a.owner = Uuid::new_v4(),
        |a: &mut Authority| a.target_node = Uuid::new_v4(),
    ] {
        let mut bad = a.clone();
        mutate(&mut bad);
        assert_eq!(
            evaluate(&bad, &p, &o, &r, Risk::Routine, None, 0),
            Decision::Deny(Denial::Ownership)
        );
    }
    for mutate in [
        |a: &mut Authority| a.lease_deadline = 0,
        |a: &mut Authority| a.authority_deadline = 0,
        |a: &mut Authority| a.grant_deadline = 0,
    ] {
        let mut bad = a.clone();
        mutate(&mut bad);
        assert_eq!(
            evaluate(&bad, &p, &o, &r, Risk::Routine, None, 0),
            Decision::Deny(Denial::Expired)
        );
    }
    let mut stopped = a.clone();
    stopped.stopped = true;
    assert_eq!(
        evaluate(&stopped, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Deny(Denial::Stopped)
    );
}
#[test]
fn view_grants_never_permit_clicks_or_typing() {
    let (mut a, p, o, mut r) = fixture();
    a.access = Access::View;
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Deny(Denial::Access)
    );
    r.action = Action::Type {
        text: "run anything".into(),
    };
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Deny(Denial::Access)
    );
    r.action = Action::Observe;
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Allow
    );
}
#[test]
fn confirmation_is_bound_to_exact_action_risk_target_and_expiry() {
    let (a, p, o, r) = fixture();
    let approval = Approval::from_local_consent(&r, Risk::SendOrPublish, 20);
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::SendOrPublish, None, 0),
        Decision::Confirm
    );
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::SendOrPublish, Some(&approval), 0),
        Decision::Allow
    );
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::SendOrPublish, Some(&approval), 20),
        Decision::Confirm
    );
    let mut changed = r.clone();
    changed.action = Action::Type {
        text: "different message".into(),
    };
    assert_eq!(
        evaluate(
            &a,
            &p,
            &o,
            &changed,
            Risk::SendOrPublish,
            Some(&approval),
            0
        ),
        Decision::Confirm
    );
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Purchase, Some(&approval), 0),
        Decision::Confirm
    );
}
#[test]
fn exceptions_are_independent_and_invalidate_old_requests() {
    let (a, mut p, o, mut r) = fixture();
    let risks = [
        Risk::PaymentCredentials,
        Risk::TradeOrTransfer,
        Risk::PermanentDelete,
        Risk::Captcha,
        Risk::ExternalConsent,
    ];
    for risk in risks {
        assert_eq!(p.rule(risk), Rule::Deny);
    }
    p.set_consented_exception(Risk::PermanentDelete, Rule::Allow);
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::PermanentDelete, None, 0),
        Decision::Deny(Denial::StalePolicy)
    );
    r.policy_revision = p.revision;
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::PermanentDelete, None, 0),
        Decision::Allow
    );
    assert_eq!(p.rule(Risk::TradeOrTransfer), Rule::Deny);
    assert_eq!(p.rule(Risk::Unknown), Rule::Confirm);
}
#[test]
fn stale_focus_geometry_and_process_identity_fail_closed() {
    let (a, p, o, mut r) = fixture();
    r.target.process_instance = Uuid::new_v4();
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Deny(Denial::Target)
    );
    r.target = a.target.clone();
    r.action = Action::Click { x: 0, y: 0 };
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Deny(Denial::Bounds)
    );
    r.action = Action::Click { x: 90, y: 20 };
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Deny(Denial::Bounds)
    );
    r.observation = 99;
    assert_eq!(
        evaluate(&a, &p, &o, &r, Risk::Routine, None, 0),
        Decision::Deny(Denial::StaleObservation)
    );
}
#[test]
fn simulated_effects_need_fresh_observations_and_never_replay() {
    let (a, p, mut o, mut r) = fixture();
    let mut d = focused_desktop(&a);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Sequence)
    );
    r.sequence = 1;
    r.call_id = Uuid::new_v4();
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::StaleObservation)
    );
    o.id = 2;
    r.observation = 2;
    d.mark_uncertain();
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Uncertain)
    );
    assert_eq!(d.effects, 1);
}
#[test]
fn model_wire_cannot_inject_policy_or_shell_tools() {
    let (_, _, _, r) = fixture();
    let mut wire = serde_json::to_value(r).unwrap();
    wire["approved"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Request>(wire).is_err());
    assert!(serde_json::from_value::<Action>(
        serde_json::json!({"kind":"run_command","command":"whoami"})
    )
    .is_err());
    assert!(!ContentBlock::Png {
        bytes: vec![],
        width: 1,
        height: 1
    }
    .validate_envelope());
}
#[test]
fn denied_or_unconfirmed_actions_have_no_simulated_effect() {
    let (a, p, o, r) = fixture();
    let mut d = focused_desktop(&a);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Unknown, None, 0),
        Outcome::NeedsConfirmation
    );
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::TradeOrTransfer, None, 0),
        Outcome::Rejected(Denial::Policy)
    );
    assert_eq!(d.effects, 0);
}

#[test]
fn default_app_tiers_and_simulator_session_isolation() {
    assert_eq!(default_access(AppClass::TerminalOrIde), Access::View);
    assert_eq!(default_access(AppClass::Browser), Access::View);
    let (a, p, o, r) = fixture();
    let mut d = focused_desktop(&a);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    let (other, p, o, mut r) = fixture();
    r.sequence = 1;
    assert_eq!(
        d.execute(&other, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Session)
    );
}

#[test]
fn two_actions_work_with_a_fresh_observation_between_them() {
    let (a, p, mut o, mut r) = fixture();
    let mut d = focused_desktop(&a);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    r.sequence = 1;
    r.call_id = Uuid::new_v4();
    r.action = Action::Observe;
    o.id += 1;
    r.observation = o.id;
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Observed
    );
    r.sequence = 2;
    r.call_id = Uuid::new_v4();
    r.action = Action::Type {
        text: "second action".into(),
    };
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    assert_eq!(d.effects, 2);
}
#[test]
fn image_envelope_rejects_oversized_dimensions_and_bytes() {
    let magic = b"\x89PNG\r\n\x1a\n".to_vec();
    for (width, height) in [(0, 1), (1, 0), (4097, 1), (1, 4097)] {
        assert!(!ContentBlock::Png {
            bytes: magic.clone(),
            width,
            height
        }
        .validate_envelope());
    }
    assert!(ContentBlock::Png {
        bytes: magic.clone(),
        width: 4096,
        height: 4096
    }
    .validate_envelope());
    let mut bytes = vec![0; 8 * 1024 * 1024];
    bytes[..8].copy_from_slice(&magic);
    assert!(ContentBlock::Png {
        bytes: bytes.clone(),
        width: 1,
        height: 1
    }
    .validate_envelope());
    bytes.push(0);
    assert!(!ContentBlock::Png {
        bytes,
        width: 1,
        height: 1
    }
    .validate_envelope());
}

#[test]
fn interruption_and_mid_sequence_grant_revocation_stop_effects() {
    let (mut a, p, mut o, mut r) = fixture();
    let mut d = focused_desktop(&a);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    r.sequence = 1;
    r.call_id = Uuid::new_v4();
    o.id += 1;
    r.observation = o.id;
    a.grant_deadline = 0;
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Expired)
    );
    a.grant_deadline = 80;
    d.interrupt();
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Stopped)
    );
    d.resume_after_local_review(&a, &p, &o, None, 0).unwrap();
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    assert_eq!(d.journal_snapshot().receipts.len(), 2);
}
#[test]
fn restart_requires_review_and_never_replays_uncertain_action() {
    use super::journal::*;
    let (a, p, mut o, mut r) = fixture();
    let mut d = focused_desktop(&a);
    r.action = Action::Type {
        text: "private text not for receipts".into(),
    };
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    d.mark_uncertain();
    let saved = serde_json::to_string(&d.journal_snapshot()).unwrap();
    assert!(!saved.contains("private text"));
    let mut recovered = FakeDesktop::recover(serde_json::from_str(&saved).unwrap()).unwrap();
    assert!(recovered
        .resume_after_local_review(&a, &p, &o, Some(ReviewFinding::EffectObserved), 0)
        .is_err());
    recovered.report_focus_change(Some(a.target.clone()), o.id);
    o.id += 1;
    assert!(recovered
        .resume_after_local_review(&a, &p, &o, None, 0)
        .is_err());
    recovered
        .resume_after_local_review(&a, &p, &o, Some(ReviewFinding::EffectObserved), 0)
        .unwrap();
    assert_eq!(
        recovered.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Sequence)
    );
    r.sequence = 1;
    r.call_id = Uuid::new_v4();
    r.observation = o.id;
    assert_eq!(
        recovered.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    let mut bad = recovered.journal_snapshot();
    bad.receipts[1].sequence = 0;
    assert!(FakeDesktop::recover(bad).is_err());
}

fn focused_desktop(a: &Authority) -> FakeDesktop {
    let mut d = FakeDesktop::default();
    d.report_focus_change(Some(a.target.clone()), 0);
    d
}
#[test]
fn focus_loss_rejects_queued_action_and_return_requires_new_observation() {
    let (a, p, mut o, mut r) = fixture();
    let mut d = focused_desktop(&a);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    o.id = 2;
    r.observation = 2;
    r.sequence = 1;
    r.call_id = Uuid::new_v4();
    let queued = r.clone();
    let mut other = a.target.clone();
    other.window_id += 1;
    d.report_focus_change(Some(other), 2);
    assert_eq!(
        d.execute(&a, &p, &o, &queued, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Focus)
    );
    d.report_focus_change(Some(a.target.clone()), 2);
    assert_eq!(
        d.execute(&a, &p, &o, &queued, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::StaleObservation)
    );
    o.id = 3;
    r.observation = 3;
    r.call_id = Uuid::new_v4();
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    assert_eq!(d.effects, 2);
    assert_eq!(d.journal_snapshot().receipts.len(), 2);
}
#[test]
fn revoked_grant_defeats_queued_approval_review_and_restart() {
    use super::journal::*;
    let (a, p, mut o, mut r) = fixture();
    let mut d = focused_desktop(&a);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Applied
    );
    o.id = 2;
    r.observation = 2;
    r.sequence = 1;
    r.call_id = Uuid::new_v4();
    let approval = Approval::from_local_consent(&r, Risk::SendOrPublish, 60);
    d.revoke_grant(&a.target);
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::SendOrPublish, Some(&approval), 0),
        Outcome::Rejected(Denial::GrantRevoked)
    );
    assert_eq!(
        d.resume_after_local_review(&a, &p, &o, None, 0),
        Err(Denial::GrantRevoked)
    );
    let wire = serde_json::to_string(&d.journal_snapshot()).unwrap();
    let mut restored = FakeDesktop::recover(serde_json::from_str(&wire).unwrap()).unwrap();
    restored.report_focus_change(Some(a.target.clone()), 2);
    o.id = 3;
    assert_eq!(
        restored.resume_after_local_review(&a, &p, &o, Some(ReviewFinding::EffectObserved), 0),
        Err(Denial::GrantRevoked)
    );
    assert_eq!(d.effects, 1);
    assert_eq!(d.journal_snapshot().receipts.len(), 1);
}
#[test]
fn unknown_focus_blocks_capture_and_review_and_epoch_never_moves_back() {
    let (a, p, mut o, mut r) = fixture();
    let mut d = FakeDesktop::default();
    r.action = Action::Observe;
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::Focus)
    );
    d.report_focus_change(None, 10);
    d.report_focus_change(Some(a.target.clone()), 2);
    o.id = 3;
    r.observation = 3;
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Rejected(Denial::StaleObservation)
    );
    o.id = 11;
    r.observation = 11;
    assert_eq!(
        d.execute(&a, &p, &o, &r, Risk::Routine, None, 0),
        Outcome::Observed
    );
    d.report_focus_change(None, 11);
    o.id = 12;
    assert_eq!(
        d.resume_after_local_review(&a, &p, &o, None, 0),
        Err(Denial::Focus)
    );
}
