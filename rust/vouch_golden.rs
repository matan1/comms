use std::collections::HashMap;

use comms_core::steward::{Attestation, SignatureObject};
use comms_core::vouch::{
    evaluate, judgment_receipt, parse_policy, store_view_id, Outcome, Query, ENGINE,
};
use comms_core::{personal_sign, personal_steward_id, Value};
use ed25519_dalek::SigningKey;

const T: &str = "2026-06-14T12:00:00Z";
const REFERENCE_POLICY: &str = include_str!("../data/vouch-reference-policy.json");

fn key(b: u8) -> SigningKey {
    SigningKey::from_bytes(&[b; 32])
}

fn map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(k, v)| (Value::text(k), v))
            .collect(),
    )
}

fn texts(xs: &[&str]) -> Value {
    Value::Array(xs.iter().map(|x| Value::text(x)).collect())
}

fn signed(claim: Value, refs: Vec<(&str, String)>, signer: &SigningKey) -> Attestation {
    signed_when(claim, refs, signer, T)
}

fn signed_when(
    claim: Value,
    refs: Vec<(&str, String)>,
    signer: &SigningKey,
    issued_at: &str,
) -> Attestation {
    let core = map(vec![
        ("v", Value::U64(1)),
        ("t", Value::text("comms.attestation/1")),
        ("c", claim),
        (
            "f",
            map(vec![
                ("issued_at", Value::text(issued_at)),
                ("language", Value::text("en")),
            ]),
        ),
        (
            "r",
            Value::Array(
                refs.into_iter()
                    .map(|(role, id)| {
                        map(vec![("role", Value::text(role)), ("id", Value::text(&id))])
                    })
                    .collect(),
            ),
        ),
    ]);
    let by = personal_steward_id(signer.verifying_key().as_bytes());
    Attestation {
        signatures: vec![SignatureObject {
            by,
            alg: "ed25519".to_owned(),
            role: "author".to_owned(),
            signed_at: issued_at.to_owned(),
            keyset: None,
            signature: personal_sign(&core, signer, "author", issued_at).to_vec(),
        }],
        core,
    }
}

fn policy_for(signer: &SigningKey, purpose_name: &str, propagation: bool) -> Attestation {
    signed(
        map(vec![
            ("t", Value::text("vouch-policy/1")),
            ("community", Value::text("comms.steward:zTEST")),
            ("name", Value::text("Vouch golden profile")),
            (
                "anchors",
                Value::Array(vec![Value::text(&personal_steward_id(
                    key(20).verifying_key().as_bytes(),
                ))]),
            ),
            (
                "purposes",
                Value::Array(vec![map(vec![
                    ("purpose", Value::text(purpose_name)),
                    (
                        "positive_types",
                        if purpose_name == "succession" {
                            texts(&["succession/1"])
                        } else {
                            texts(&["action-record/1"])
                        },
                    ),
                    ("negative_types", texts(&["action-record/1"])),
                    (
                        "min_positive_issuers",
                        Value::U64(if purpose_name == "succession" { 1 } else { 2 }),
                    ),
                    ("min_negative_issuers", Value::U64(2)),
                    ("min_endorsers", Value::U64(2)),
                    ("issuer_cap", Value::U64(1)),
                    ("require_direct", Value::U64(1)),
                    (
                        "propagation",
                        map(vec![
                            ("enabled", Value::U64(u64::from(propagation))),
                            ("max_depth", Value::U64(3)),
                            ("min_paths", Value::U64(2)),
                        ]),
                    ),
                    (
                        "positive",
                        map(vec![
                            ("op", Value::text("all")),
                            (
                                "of",
                                Value::Array(vec![
                                    map(vec![
                                        ("op", Value::text("distinct-issuer-count")),
                                        ("class", Value::text("positive")),
                                        (
                                            "min",
                                            Value::U64(if purpose_name == "succession" {
                                                1
                                            } else {
                                                2
                                            }),
                                        ),
                                    ]),
                                    map(vec![
                                        ("op", Value::text("distinct-issuer-count")),
                                        ("class", Value::text("endorsement")),
                                        ("min", Value::U64(2)),
                                    ]),
                                    map(vec![
                                        ("op", Value::text("unresolved-count")),
                                        ("max", Value::U64(0)),
                                    ]),
                                ]),
                            ),
                        ]),
                    ),
                    (
                        "negative",
                        map(vec![
                            ("op", Value::text("distinct-issuer-count")),
                            ("class", Value::text("negative")),
                            ("min", Value::U64(2)),
                        ]),
                    ),
                ])]),
            ),
        ]),
        vec![],
        signer,
    )
}

fn policy(signer: &SigningKey, propagation: bool) -> Attestation {
    policy_for(signer, "admission", propagation)
}

fn action(subject: &str, outcome: &str, signer: &SigningKey) -> Attestation {
    let by = personal_steward_id(signer.verifying_key().as_bytes());
    signed(
        map(vec![
            ("t", Value::text("action-record/1")),
            ("agent", Value::text(subject)),
            ("action", Value::text("admission")),
            ("outcome", Value::text(outcome)),
            ("detail", map(vec![("witness", Value::text(&by))])),
        ]),
        vec![],
        signer,
    )
}

fn action_with_missing_ref(subject: &str, signer: &SigningKey) -> Attestation {
    signed(
        map(vec![
            ("t", Value::text("action-record/1")),
            ("agent", Value::text(subject)),
            ("action", Value::text("admission")),
            ("outcome", Value::text("completed")),
            ("detail", map(vec![])),
        ]),
        vec![("supports", "comms.attest:zMISSING".to_owned())],
        signer,
    )
}

fn endorsement(target: &str, signer: &SigningKey) -> Attestation {
    let by = personal_steward_id(signer.verifying_key().as_bytes());
    signed(
        map(vec![
            ("t", Value::text("endorsement/1")),
            ("target", Value::text(target)),
            ("in_capacity", Value::text("admission")),
            ("weight", Value::text("primary")),
            (
                "rationale",
                Value::text(&format!("golden testimony by {by}")),
            ),
        ]),
        vec![],
        signer,
    )
}

fn disposition(
    target: &str,
    state: &str,
    signer: &SigningKey,
    prev: Option<String>,
) -> Attestation {
    signed(
        map(vec![
            ("t", Value::text("vouch-disposition/1")),
            ("target", Value::text(target)),
            ("state", Value::text(state)),
        ]),
        prev.into_iter().map(|id| ("supersedes", id)).collect(),
        signer,
    )
}

fn objection(target: &str, signer: &SigningKey) -> Attestation {
    signed(
        map(vec![
            ("t", Value::text("objection/1")),
            ("target", Value::text(target)),
            ("kind", Value::text("factual")),
            ("grounds", Value::text("conflicting witnessed account")),
            ("evidence", Value::Array(vec![])),
        ]),
        vec![("responds-to", target.to_owned())],
        signer,
    )
}

fn store(atts: Vec<Attestation>) -> HashMap<String, Attestation> {
    atts.into_iter().map(|a| (a.id(), a)).collect()
}

fn query(subject: &str) -> Query {
    Query {
        subject: subject.to_owned(),
        purpose: "admission".to_owned(),
        community: Some("comms.steward:zTEST".to_owned()),
        as_of: T.to_owned(),
    }
}

fn query_for(subject: &str, purpose: &str) -> Query {
    Query {
        purpose: purpose.to_owned(),
        ..query(subject)
    }
}

fn trusted_fixture(subject: &str) -> (HashMap<String, Attestation>, String) {
    let p = policy(&key(1), false);
    let pid = p.id();
    let atts = vec![
        p,
        action(subject, "completed", &key(2)),
        action(subject, "fulfilled", &key(3)),
        endorsement(subject, &key(4)),
        endorsement(subject, &key(5)),
    ];
    (store(atts), pid)
}

#[test]
fn rust_reads_the_signed_python_reference_policy() {
    let vector: serde_json::Value = serde_json::from_str(REFERENCE_POLICY).unwrap();
    let bytes = hex::decode(vector["canonical_cbor_hex"].as_str().unwrap()).unwrap();
    let att = comms_core::bundle::parse_attestation(&bytes).unwrap();
    assert_eq!(att.id(), vector["attestation_id"].as_str().unwrap());
    let policy = parse_policy(&att).unwrap();
    assert_eq!(policy.name, "Informative careful-admission profile");
    assert!(policy.purposes[0].positive_predicate.is_some());
    let id = att.id();
    let s = store(vec![att]);
    assert_eq!(
        evaluate(&s, &id, query("comms.steward:zUNKNOWN"))
            .unwrap()
            .outcome,
        Outcome::AwaitingContext
    );
}

#[test]
fn direct_trust_and_receipt_are_stable() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let (s, pid) = trusted_fixture(&subject);
    let got = evaluate(&s, &pid, query(&subject)).unwrap();
    assert_eq!(got.outcome, Outcome::Trusted);
    assert_eq!(got.positive_issuers.len(), 2);
    assert_eq!(got.endorsers.len(), 2);
    assert_eq!(got.store_view, store_view_id(&s));
    let receipt = judgment_receipt(&got, &key(30), T);
    assert_eq!(
        receipt.id(),
        "comms.attest:zBPaFoFSpecvHM2kaGGSDpJ4JzWBwGBh71VD5KTqGLeGR"
    );
    assert_eq!(ENGINE, "comms-core-vouch/0.1.0");
}

#[test]
fn independent_negative_reports_reject() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let p = policy(&key(1), false);
    let pid = p.id();
    let s = store(vec![
        p,
        action(&subject, "failed", &key(2)),
        action(&subject, "breach", &key(3)),
    ]);
    assert_eq!(
        evaluate(&s, &pid, query(&subject)).unwrap().outcome,
        Outcome::Rejected
    );
}

#[test]
fn missing_context_is_not_negative() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let p = policy(&key(1), false);
    let pid = p.id();
    let s = store(vec![p, action_with_missing_ref(&subject, &key(2))]);
    let got = evaluate(&s, &pid, query(&subject)).unwrap();
    assert_eq!(got.outcome, Outcome::AwaitingContext);
    assert_eq!(got.unresolved, vec!["comms.attest:zMISSING"]);
}

#[test]
fn future_and_expired_evidence_do_not_count() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let p = policy(&key(1), false);
    let pid = p.id();
    let future = signed_when(
        map(vec![
            ("t", Value::text("action-record/1")),
            ("agent", Value::text(&subject)),
            ("action", Value::text("admission")),
            ("outcome", Value::text("completed")),
            ("detail", map(vec![])),
        ]),
        vec![],
        &key(2),
        "2026-06-15T12:00:00Z",
    );
    let expired = signed(
        map(vec![
            ("t", Value::text("endorsement/1")),
            ("target", Value::text(&subject)),
            ("in_capacity", Value::text("admission")),
            ("weight", Value::text("primary")),
            ("expires_at", Value::text("2026-06-14T11:59:59Z")),
        ]),
        vec![],
        &key(4),
    );
    let s = store(vec![p, future, expired]);
    let got = evaluate(&s, &pid, query(&subject)).unwrap();
    assert_eq!(got.outcome, Outcome::AwaitingContext);
    assert!(got.evidence.iter().all(|e| !e.counted));
}

#[test]
fn challenged_decisive_evidence_is_contested() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let (mut s, pid) = trusted_fixture(&subject);
    let target = s
        .values()
        .find(|a| {
            a.core
                .get("c")
                .and_then(|c| c.get("t"))
                .and_then(Value::as_text)
                == Some("action-record/1")
        })
        .unwrap()
        .id();
    let obj = objection(&target, &key(8));
    s.insert(obj.id(), obj);
    assert_eq!(
        evaluate(&s, &pid, query(&subject)).unwrap().outcome,
        Outcome::Contested
    );
}

#[test]
fn withdrawal_and_reaffirmation_change_contribution_without_revocation() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let (mut s, pid) = trusted_fixture(&subject);
    let target = s
        .values()
        .find(|a| {
            a.signatures.first().map(|x| x.by.as_str())
                == Some(personal_steward_id(key(2).verifying_key().as_bytes()).as_str())
        })
        .unwrap()
        .id();
    let off = disposition(&target, "inactive", &key(2), None);
    let off_id = off.id();
    s.insert(off_id.clone(), off);
    assert_eq!(
        evaluate(&s, &pid, query(&subject)).unwrap().outcome,
        Outcome::AwaitingContext
    );
    let on = disposition(&target, "reaffirmed", &key(2), Some(off_id));
    s.insert(on.id(), on);
    assert_eq!(
        evaluate(&s, &pid, query(&subject)).unwrap().outcome,
        Outcome::Trusted
    );
}

#[test]
fn disposition_fork_is_contested() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let (mut s, pid) = trusted_fixture(&subject);
    let target = s
        .values()
        .find(|a| {
            a.signatures.first().map(|x| x.by.as_str())
                == Some(personal_steward_id(key(2).verifying_key().as_bytes()).as_str())
        })
        .unwrap()
        .id();
    let a = disposition(&target, "inactive", &key(2), None);
    let b = disposition(&target, "reaffirmed", &key(2), None);
    s.insert(a.id(), a);
    s.insert(b.id(), b);
    assert_eq!(
        evaluate(&s, &pid, query(&subject)).unwrap().outcome,
        Outcome::Contested
    );
}

#[test]
fn one_issuer_cannot_farm_a_quorum() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let p = policy(&key(1), false);
    let pid = p.id();
    let s = store(vec![
        p,
        action(&subject, "completed", &key(2)),
        action(&subject, "fulfilled", &key(2)),
        endorsement(&subject, &key(4)),
        endorsement(&subject, &key(5)),
    ]);
    let got = evaluate(&s, &pid, query(&subject)).unwrap();
    assert_eq!(got.outcome, Outcome::AwaitingContext);
    assert_eq!(got.positive_issuers.len(), 1);
}

#[test]
fn bounded_independent_paths_enable_issuers_and_cycles_do_not_help() {
    let subject = personal_steward_id(key(9).verifying_key().as_bytes());
    let anchor = personal_steward_id(key(20).verifying_key().as_bytes());
    let hop1 = personal_steward_id(key(21).verifying_key().as_bytes());
    let hop2 = personal_steward_id(key(22).verifying_key().as_bytes());
    let issuer1 = personal_steward_id(key(2).verifying_key().as_bytes());
    let issuer2 = personal_steward_id(key(3).verifying_key().as_bytes());
    let p = policy(&key(1), true);
    let pid = p.id();
    let s = store(vec![
        p,
        endorsement(&hop1, &key(20)),
        endorsement(&hop2, &key(20)),
        endorsement(&issuer1, &key(21)),
        endorsement(&issuer1, &key(22)),
        endorsement(&issuer2, &key(21)),
        endorsement(&issuer2, &key(22)),
        endorsement(&anchor, &key(21)), // cycle back to anchor
        action(&subject, "completed", &key(2)),
        action(&subject, "completed", &key(3)),
        endorsement(&subject, &key(2)),
        endorsement(&subject, &key(3)),
    ]);
    let got = evaluate(&s, &pid, query(&subject)).unwrap();
    assert_eq!(got.outcome, Outcome::Trusted);
    assert!(got.paths.iter().all(|p| {
        let unique: std::collections::HashSet<_> = p.iter().collect();
        unique.len() == p.len() && p.len() <= 4
    }));
}

#[test]
fn succession_subject_uses_same_generic_query() {
    let successor = personal_steward_id(key(9).verifying_key().as_bytes());
    let predecessor = personal_steward_id(key(8).verifying_key().as_bytes());
    let p = policy_for(&key(1), "succession", false);
    let pid = p.id();
    let succession = signed(
        map(vec![
            ("t", Value::text("succession/1")),
            ("predecessor", Value::text(&predecessor)),
            ("successor", Value::text(&successor)),
            (
                "account",
                Value::text("threshold keys lost; witnessed re-founding"),
            ),
        ]),
        vec![],
        &key(9),
    );
    let s = store(vec![
        p,
        succession,
        signed(
            map(vec![
                ("t", Value::text("endorsement/1")),
                ("target", Value::text(&successor)),
                ("in_capacity", Value::text("succession")),
                ("weight", Value::text("primary")),
                ("rationale", Value::text("neighbor one")),
            ]),
            vec![],
            &key(4),
        ),
        signed(
            map(vec![
                ("t", Value::text("endorsement/1")),
                ("target", Value::text(&successor)),
                ("in_capacity", Value::text("succession")),
                ("weight", Value::text("primary")),
                ("rationale", Value::text("neighbor two")),
            ]),
            vec![],
            &key(5),
        ),
    ]);
    assert_eq!(
        evaluate(&s, &pid, query_for(&successor, "succession"))
            .unwrap()
            .outcome,
        Outcome::Trusted
    );
}
