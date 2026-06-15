//! Vouch 1.0 candidate reference evaluator.
//!
//! Unlike the Attest and Steward modules, this is Layer 4: every answer is
//! relative to an explicit policy, query, and store view. The evaluator emits
//! an explanation rather than a global reputation score.

use std::collections::{HashMap, HashSet};

use ed25519_dalek::SigningKey;

use crate::steward::{verify_community_attestation, Attestation, SignatureObject};
use crate::{cbor, dsh, multibase_z, personal_sign, personal_steward_id, personal_verify, Value};

pub const ENGINE: &str = "comms-core-vouch/0.1.0";
pub const CTX_VIEW: &[u8] = b"comms.vouch.view/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub subject: String,
    pub purpose: String,
    pub community: Option<String>,
    pub as_of: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Propagation {
    pub enabled: bool,
    pub max_depth: u64,
    pub min_paths: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurposePolicy {
    pub purpose: String,
    pub positive_types: Vec<String>,
    pub negative_types: Vec<String>,
    pub min_positive_issuers: u64,
    pub min_negative_issuers: u64,
    pub min_endorsers: u64,
    pub issuer_cap: u64,
    pub require_direct: bool,
    pub propagation: Propagation,
    pub positive_predicate: Option<Predicate>,
    pub negative_predicate: Option<Predicate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Not(Box<Predicate>),
    EvidenceCount { class: String, min: u64 },
    DistinctIssuerCount { class: String, min: u64 },
    IndependentPathCount { min: u64 },
    UnresolvedCount { max: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub id: String,
    pub community: String,
    pub name: String,
    pub anchors: Vec<String>,
    pub purposes: Vec<PurposePolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Trusted,
    Rejected,
    Contested,
    AwaitingContext,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Trusted => "trusted",
            Outcome::Rejected => "rejected",
            Outcome::Contested => "contested",
            Outcome::AwaitingContext => "awaiting-context",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceTrace {
    pub id: String,
    pub claim_type: String,
    pub issuer: Option<String>,
    pub class: String,
    pub counted: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub query: Query,
    pub policy_id: String,
    pub store_view: String,
    pub outcome: Outcome,
    pub evidence: Vec<EvidenceTrace>,
    pub unresolved: Vec<String>,
    pub paths: Vec<Vec<String>>,
    pub positive_issuers: Vec<String>,
    pub negative_issuers: Vec<String>,
    pub endorsers: Vec<String>,
    pub contested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VouchError {
    PolicyNotFound(String),
    MalformedPolicy(String),
    PurposeNotFound(String),
}

impl std::fmt::Display for VouchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VouchError::PolicyNotFound(id) => write!(f, "policy not found in store: {id}"),
            VouchError::MalformedPolicy(why) => write!(f, "malformed Vouch policy: {why}"),
            VouchError::PurposeNotFound(p) => write!(f, "policy has no purpose named {p:?}"),
        }
    }
}

impl std::error::Error for VouchError {}

fn text(v: &Value, field: &str) -> Result<String, VouchError> {
    v.get(field)
        .and_then(Value::as_text)
        .map(str::to_owned)
        .ok_or_else(|| VouchError::MalformedPolicy(format!("missing text field {field}")))
}

fn uint(v: &Value, field: &str) -> Result<u64, VouchError> {
    v.get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| VouchError::MalformedPolicy(format!("missing uint field {field}")))
}

fn texts(v: &Value, field: &str) -> Result<Vec<String>, VouchError> {
    v.get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| VouchError::MalformedPolicy(format!("missing array field {field}")))?
        .iter()
        .map(|x| {
            x.as_text()
                .map(str::to_owned)
                .ok_or_else(|| VouchError::MalformedPolicy(format!("{field} must contain text")))
        })
        .collect()
}

fn parse_predicate(v: &Value) -> Result<Predicate, VouchError> {
    let op = text(v, "op")?;
    match op.as_str() {
        "all" | "any" => {
            let items = v
                .get("of")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    VouchError::MalformedPolicy(format!("{op} requires array field of"))
                })?
                .iter()
                .map(parse_predicate)
                .collect::<Result<Vec<_>, _>>()?;
            if op == "all" {
                Ok(Predicate::All(items))
            } else {
                Ok(Predicate::Any(items))
            }
        }
        "not" => Ok(Predicate::Not(Box::new(parse_predicate(
            v.get("of")
                .ok_or_else(|| VouchError::MalformedPolicy("not requires field of".to_owned()))?,
        )?))),
        "evidence-count" => Ok(Predicate::EvidenceCount {
            class: text(v, "class")?,
            min: uint(v, "min")?,
        }),
        "distinct-issuer-count" => Ok(Predicate::DistinctIssuerCount {
            class: text(v, "class")?,
            min: uint(v, "min")?,
        }),
        "independent-path-count" => Ok(Predicate::IndependentPathCount {
            min: uint(v, "min")?,
        }),
        "unresolved-count" => Ok(Predicate::UnresolvedCount {
            max: uint(v, "max")?,
        }),
        _ => Err(VouchError::MalformedPolicy(format!(
            "unknown predicate operator {op:?}"
        ))),
    }
}

pub fn parse_policy(att: &Attestation) -> Result<Policy, VouchError> {
    let claim = att
        .core
        .get("c")
        .ok_or_else(|| VouchError::MalformedPolicy("missing claim".to_owned()))?;
    if claim.get("t").and_then(Value::as_text) != Some("vouch-policy/1") {
        return Err(VouchError::MalformedPolicy(
            "selected attestation is not vouch-policy/1".to_owned(),
        ));
    }
    let raw_purposes = claim
        .get("purposes")
        .and_then(Value::as_array)
        .ok_or_else(|| VouchError::MalformedPolicy("missing purposes".to_owned()))?;
    let mut purposes = Vec::new();
    let mut seen = HashSet::new();
    for p in raw_purposes {
        let purpose = text(p, "purpose")?;
        if !seen.insert(purpose.clone()) {
            return Err(VouchError::MalformedPolicy(format!(
                "duplicate purpose {purpose:?}"
            )));
        }
        let propagation = p
            .get("propagation")
            .ok_or_else(|| VouchError::MalformedPolicy("missing propagation".to_owned()))?;
        let enabled = uint(propagation, "enabled")?;
        let max_depth = uint(propagation, "max_depth")?;
        let min_paths = uint(propagation, "min_paths")?;
        let issuer_cap = uint(p, "issuer_cap")?;
        let require_direct = uint(p, "require_direct")?;
        if enabled > 1
            || require_direct > 1
            || issuer_cap == 0
            || max_depth == 0
            || max_depth > 4
            || min_paths == 0
        {
            return Err(VouchError::MalformedPolicy(
                "invalid boolean, cap, or propagation bound".to_owned(),
            ));
        }
        purposes.push(PurposePolicy {
            purpose,
            positive_types: texts(p, "positive_types")?,
            negative_types: texts(p, "negative_types")?,
            min_positive_issuers: uint(p, "min_positive_issuers")?,
            min_negative_issuers: uint(p, "min_negative_issuers")?,
            min_endorsers: uint(p, "min_endorsers")?,
            issuer_cap,
            require_direct: require_direct == 1,
            propagation: Propagation {
                enabled: enabled == 1,
                max_depth,
                min_paths,
            },
            positive_predicate: p.get("positive").map(parse_predicate).transpose()?,
            negative_predicate: p.get("negative").map(parse_predicate).transpose()?,
        });
    }
    Ok(Policy {
        id: att.id(),
        community: text(claim, "community")?,
        name: text(claim, "name")?,
        anchors: texts(claim, "anchors")?,
        purposes,
    })
}

pub fn store_view_id(store: &HashMap<String, Attestation>) -> String {
    let mut ids: Vec<Value> = store.keys().map(|x| Value::text(x)).collect();
    ids.sort_by(|a, b| a.as_text().cmp(&b.as_text()));
    let hash = dsh(CTX_VIEW, &cbor::encode(&Value::Array(ids)));
    format!("comms.vouch.view:{}", multibase_z(&hash))
}

fn claim_type(att: &Attestation) -> &str {
    att.core
        .get("c")
        .and_then(|c| c.get("t"))
        .and_then(Value::as_text)
        .unwrap_or("")
}

fn refs(att: &Attestation) -> Vec<(String, String)> {
    att.core
        .get("r")
        .and_then(Value::as_array)
        .map(|rs| {
            rs.iter()
                .filter_map(|r| {
                    Some((
                        r.get("role")?.as_text()?.to_owned(),
                        r.get("id")?.as_text()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn issuer(att: &Attestation) -> Option<String> {
    att.signatures.first().map(|s| s.by.clone())
}

fn effective_at(att: &Attestation, as_of: &str) -> bool {
    let issued_at = att
        .core
        .get("f")
        .and_then(|f| f.get("issued_at"))
        .and_then(Value::as_text);
    if issued_at.map(|t| t > as_of).unwrap_or(true) {
        return false;
    }
    let expires_at = att
        .core
        .get("c")
        .and_then(|c| c.get("expires_at"))
        .and_then(Value::as_text);
    !expires_at.map(|t| t <= as_of).unwrap_or(false)
}

fn personal_key(by: &str) -> Option<[u8; 32]> {
    let encoded = by.strip_prefix("comms.steward:z")?;
    let bytes = bs58::decode(encoded).into_vec().ok()?;
    bytes.as_slice().try_into().ok()
}

fn verified(att: &Attestation, store: &HashMap<String, Attestation>) -> bool {
    !att.signatures.is_empty()
        && att.signatures.iter().all(|sig| match sig.alg.as_str() {
            "ed25519" => {
                let (Some(key), Ok(signature)) = (
                    personal_key(&sig.by),
                    <[u8; 64]>::try_from(sig.signature.as_slice()),
                ) else {
                    return false;
                };
                personal_verify(
                    &att.core,
                    &sig.by,
                    &sig.role,
                    &sig.signed_at,
                    &key,
                    &signature,
                )
            }
            "ed25519-set/1" => verify_community_attestation(att, &sig.by, store).is_ok(),
            _ => false,
        })
}

fn target_matches(claim: &Value, subject: &str) -> bool {
    ["target", "agent", "steward", "successor"]
        .iter()
        .any(|field| claim.get(field).and_then(Value::as_text) == Some(subject))
}

fn outcome_class(claim: &Value, purpose: &str) -> Option<&'static str> {
    let t = claim.get("t")?.as_text()?;
    if t == "endorsement/1" {
        return (claim.get("in_capacity")?.as_text()? == purpose).then_some("endorsement");
    }
    if t == "succession/1" && purpose == "succession" {
        return Some("positive");
    }
    if t == "membership-binding/1" && purpose == "admission" {
        return Some("positive");
    }
    if t == "action-record/1" && claim.get("action")?.as_text()? != purpose {
        return None;
    }
    let outcome = claim
        .get("outcome")
        .and_then(Value::as_text)
        .or_else(|| claim.get("detail")?.get("outcome")?.as_text())?;
    match outcome {
        "completed" | "success" | "fulfilled" => Some("positive"),
        "failed" | "harm" | "breach" => Some("negative"),
        _ => None,
    }
}

fn disposition_state(
    target: &Attestation,
    store: &HashMap<String, Attestation>,
    as_of: &str,
) -> (Option<String>, bool) {
    let target_id = target.id();
    let target_issuers: HashSet<&str> = target.signatures.iter().map(|s| s.by.as_str()).collect();
    let dispositions: Vec<&Attestation> = store
        .values()
        .filter(|a| {
            let c = a.core.get("c");
            claim_type(a) == "vouch-disposition/1"
                && c.and_then(|x| x.get("target")).and_then(Value::as_text)
                    == Some(target_id.as_str())
                && verified(a, store)
                && effective_at(a, as_of)
                && issuer(a)
                    .as_deref()
                    .map(|i| target_issuers.contains(i))
                    .unwrap_or(false)
        })
        .collect();
    if dispositions.is_empty() {
        return (None, false);
    }
    let superseded: HashSet<String> = dispositions
        .iter()
        .flat_map(|a| refs(a))
        .filter(|(role, _)| role == "supersedes")
        .map(|(_, id)| id)
        .collect();
    let heads: Vec<&Attestation> = dispositions
        .into_iter()
        .filter(|a| !superseded.contains(&a.id()))
        .collect();
    if heads.len() != 1 {
        return (None, true);
    }
    (
        heads[0]
            .core
            .get("c")
            .and_then(|c| c.get("state"))
            .and_then(Value::as_text)
            .map(str::to_owned),
        false,
    )
}

fn endorsement_edges(
    store: &HashMap<String, Attestation>,
    purpose: &str,
    as_of: &str,
) -> HashMap<String, Vec<String>> {
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for att in store.values() {
        let Some(claim) = att.core.get("c") else {
            continue;
        };
        if claim_type(att) != "endorsement/1"
            || claim.get("in_capacity").and_then(Value::as_text) != Some(purpose)
            || !verified(att, store)
            || !effective_at(att, as_of)
        {
            continue;
        }
        let (Some(from), Some(to)) = (issuer(att), claim.get("target").and_then(Value::as_text))
        else {
            continue;
        };
        edges.entry(from).or_default().push(to.to_owned());
    }
    for tos in edges.values_mut() {
        tos.sort();
        tos.dedup();
    }
    edges
}

fn find_paths(
    anchors: &[String],
    target: &str,
    max_depth: u64,
    edges: &HashMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    fn walk(
        node: &str,
        target: &str,
        max_depth: usize,
        edges: &HashMap<String, Vec<String>>,
        path: &mut Vec<String>,
        out: &mut Vec<Vec<String>>,
    ) {
        if node == target {
            out.push(path.clone());
            return;
        }
        if path.len().saturating_sub(1) >= max_depth {
            return;
        }
        if let Some(next) = edges.get(node) {
            for n in next {
                if path.contains(n) {
                    continue;
                }
                path.push(n.clone());
                walk(n, target, max_depth, edges, path, out);
                path.pop();
            }
        }
    }
    let mut out = Vec::new();
    for anchor in anchors {
        let mut path = vec![anchor.clone()];
        walk(
            anchor,
            target,
            max_depth as usize,
            edges,
            &mut path,
            &mut out,
        );
    }
    out.sort();
    out.dedup();
    out
}

fn independent_path_count(paths: &[Vec<String>]) -> usize {
    let mut roots = HashSet::new();
    for path in paths {
        let key = if path.len() > 1 {
            format!("{}|{}", path[0], path[1])
        } else {
            path[0].clone()
        };
        roots.insert(key);
    }
    roots.len()
}

struct Facts<'a> {
    evidence: &'a [EvidenceTrace],
    positive_issuers: &'a [String],
    negative_issuers: &'a [String],
    endorsers: &'a [String],
    paths: &'a [Vec<String>],
    unresolved: &'a [String],
}

fn predicate_value(predicate: &Predicate, facts: &Facts<'_>) -> bool {
    match predicate {
        Predicate::All(items) => items.iter().all(|p| predicate_value(p, facts)),
        Predicate::Any(items) => items.iter().any(|p| predicate_value(p, facts)),
        Predicate::Not(item) => !predicate_value(item, facts),
        Predicate::EvidenceCount { class, min } => {
            facts
                .evidence
                .iter()
                .filter(|e| e.counted && e.class == *class)
                .count() as u64
                >= *min
        }
        Predicate::DistinctIssuerCount { class, min } => {
            let count = match class.as_str() {
                "positive" => facts.positive_issuers.len(),
                "negative" => facts.negative_issuers.len(),
                "endorsement" => facts.endorsers.len(),
                _ => 0,
            };
            count as u64 >= *min
        }
        Predicate::IndependentPathCount { min } => {
            independent_path_count(facts.paths) as u64 >= *min
        }
        Predicate::UnresolvedCount { max } => facts.unresolved.len() as u64 <= *max,
    }
}

pub fn evaluate(
    store: &HashMap<String, Attestation>,
    policy_id: &str,
    query: Query,
) -> Result<Evaluation, VouchError> {
    let policy_att = store
        .get(policy_id)
        .ok_or_else(|| VouchError::PolicyNotFound(policy_id.to_owned()))?;
    if !verified(policy_att, store) {
        return Err(VouchError::MalformedPolicy(
            "policy signatures do not verify in this view".to_owned(),
        ));
    }
    if !effective_at(policy_att, &query.as_of) {
        return Err(VouchError::MalformedPolicy(
            "policy is not effective at query as_of".to_owned(),
        ));
    }
    let policy = parse_policy(policy_att)?;
    let pp = policy
        .purposes
        .iter()
        .find(|p| p.purpose == query.purpose)
        .ok_or_else(|| VouchError::PurposeNotFound(query.purpose.clone()))?;
    let edges = endorsement_edges(store, &query.purpose, &query.as_of);
    let objections: HashSet<String> = store
        .values()
        .filter(|a| {
            claim_type(a) == "objection/1" && verified(a, store) && effective_at(a, &query.as_of)
        })
        .filter_map(|a| {
            a.core
                .get("c")
                .and_then(|c| c.get("target"))
                .and_then(Value::as_text)
                .map(str::to_owned)
        })
        .collect();

    let mut evidence = Vec::new();
    let mut unresolved = HashSet::new();
    let mut positive_counts: HashMap<String, u64> = HashMap::new();
    let mut negative_counts: HashMap<String, u64> = HashMap::new();
    let mut endorsers = HashSet::new();
    let mut all_paths = Vec::new();
    let mut contested = false;

    let mut ordered: Vec<&Attestation> = store.values().collect();
    ordered.sort_by_key(|a| a.id());
    for att in ordered {
        let Some(claim) = att.core.get("c") else {
            continue;
        };
        let t = claim_type(att).to_owned();
        if !target_matches(claim, &query.subject) {
            continue;
        }
        let Some(class) = outcome_class(claim, &query.purpose) else {
            continue;
        };
        let id = att.id();
        let who = issuer(att);
        let missing: Vec<String> = refs(att)
            .into_iter()
            .map(|(_, id)| id)
            .filter(|id| !store.contains_key(id))
            .collect();
        unresolved.extend(missing.iter().cloned());
        let (disposition, disposition_fork) = disposition_state(att, store, &query.as_of);
        contested |= disposition_fork;

        let mut counted = true;
        let mut reason = "counted".to_owned();
        if !effective_at(att, &query.as_of) {
            counted = false;
            reason = "not effective at query as_of".to_owned();
        } else if !verified(att, store) {
            counted = false;
            reason = "signature or community chain does not verify".to_owned();
        } else if !missing.is_empty() {
            counted = false;
            reason = "awaiting referenced context".to_owned();
        } else if disposition.as_deref() == Some("inactive") {
            counted = false;
            reason = "withdrawn by issuer disposition".to_owned();
        } else if disposition_fork {
            counted = false;
            reason = "competing disposition heads".to_owned();
        } else if class == "positive" && !pp.positive_types.contains(&t) {
            counted = false;
            reason = "claim type not allowed as positive evidence".to_owned();
        } else if class == "negative" && !pp.negative_types.contains(&t) {
            counted = false;
            reason = "claim type not allowed as negative evidence".to_owned();
        }

        let paths = who
            .as_deref()
            .map(|i| find_paths(&policy.anchors, i, pp.propagation.max_depth, &edges))
            .unwrap_or_default();
        if pp.propagation.enabled && counted {
            let eligible = who
                .as_deref()
                .map(|i| policy.anchors.iter().any(|a| a == i))
                .unwrap_or(false)
                || independent_path_count(&paths) >= pp.propagation.min_paths as usize;
            if !eligible {
                counted = false;
                reason = "issuer lacks required independent anchor paths".to_owned();
            }
        }
        all_paths.extend(paths);

        if counted {
            let Some(who) = who.clone() else {
                counted = false;
                reason = "evidence has no issuer signature".to_owned();
                evidence.push(EvidenceTrace {
                    id,
                    claim_type: t,
                    issuer: who,
                    class: class.to_owned(),
                    counted,
                    reason,
                });
                continue;
            };
            let map = match class {
                "positive" => &mut positive_counts,
                "negative" => &mut negative_counts,
                "endorsement" => {
                    endorsers.insert(who.clone());
                    evidence.push(EvidenceTrace {
                        id: id.clone(),
                        claim_type: t.clone(),
                        issuer: Some(who),
                        class: class.to_owned(),
                        counted: true,
                        reason: "counted as one distinct endorser".to_owned(),
                    });
                    if objections.contains(&id) {
                        contested = true;
                    }
                    continue;
                }
                _ => unreachable!(),
            };
            let count = map.entry(who).or_insert(0);
            if *count >= pp.issuer_cap {
                counted = false;
                reason = "issuer contribution cap reached".to_owned();
            } else {
                *count += 1;
            }
            if counted && objections.contains(&id) {
                contested = true;
                reason = "counted but challenged by a verified objection".to_owned();
            }
        }
        evidence.push(EvidenceTrace {
            id,
            claim_type: t,
            issuer: who,
            class: class.to_owned(),
            counted,
            reason,
        });
    }

    let mut positive_issuers: Vec<String> = positive_counts.keys().cloned().collect();
    let mut negative_issuers: Vec<String> = negative_counts.keys().cloned().collect();
    let mut endorsers: Vec<String> = endorsers.into_iter().collect();
    positive_issuers.sort();
    negative_issuers.sort();
    endorsers.sort();
    all_paths.sort();
    all_paths.dedup();
    let mut unresolved: Vec<String> = unresolved.into_iter().collect();
    unresolved.sort();
    let facts = Facts {
        evidence: &evidence,
        positive_issuers: &positive_issuers,
        negative_issuers: &negative_issuers,
        endorsers: &endorsers,
        paths: &all_paths,
        unresolved: &unresolved,
    };
    let positive_pass = pp
        .positive_predicate
        .as_ref()
        .map(|p| predicate_value(p, &facts))
        .unwrap_or_else(|| {
            positive_issuers.len() as u64 >= pp.min_positive_issuers
                && endorsers.len() as u64 >= pp.min_endorsers
        });
    let negative_pass = pp
        .negative_predicate
        .as_ref()
        .map(|p| predicate_value(p, &facts))
        .unwrap_or(negative_issuers.len() as u64 >= pp.min_negative_issuers);
    let outcome = if contested || (positive_pass && negative_pass) {
        Outcome::Contested
    } else if positive_pass {
        Outcome::Trusted
    } else if negative_pass {
        Outcome::Rejected
    } else {
        Outcome::AwaitingContext
    };
    Ok(Evaluation {
        query,
        policy_id: policy.id,
        store_view: store_view_id(store),
        outcome,
        evidence,
        unresolved,
        paths: all_paths,
        positive_issuers,
        negative_issuers,
        endorsers,
        contested,
    })
}

pub fn judgment_receipt(
    evaluation: &Evaluation,
    signer: &SigningKey,
    issued_at: &str,
) -> Attestation {
    let evidence = evaluation
        .evidence
        .iter()
        .filter(|e| e.counted)
        .map(|e| Value::text(&e.id))
        .collect();
    let mut claim = vec![
        (Value::text("t"), Value::text("vouch-judgment/1")),
        (
            Value::text("subject"),
            Value::text(&evaluation.query.subject),
        ),
        (
            Value::text("purpose"),
            Value::text(&evaluation.query.purpose),
        ),
        (Value::text("policy"), Value::text(&evaluation.policy_id)),
        (Value::text("as_of"), Value::text(&evaluation.query.as_of)),
        (
            Value::text("outcome"),
            Value::text(evaluation.outcome.as_str()),
        ),
        (
            Value::text("store_view"),
            Value::text(&evaluation.store_view),
        ),
        (Value::text("engine"), Value::text(ENGINE)),
        (Value::text("evidence"), Value::Array(evidence)),
        (
            Value::text("unresolved"),
            Value::Array(
                evaluation
                    .unresolved
                    .iter()
                    .map(|x| Value::text(x))
                    .collect(),
            ),
        ),
    ];
    if let Some(community) = &evaluation.query.community {
        claim.push((Value::text("community"), Value::text(community)));
    }
    let core = Value::Map(vec![
        (Value::text("v"), Value::U64(1)),
        (Value::text("t"), Value::text("comms.attestation/1")),
        (Value::text("c"), Value::Map(claim)),
        (
            Value::text("f"),
            Value::Map(vec![
                (Value::text("issued_at"), Value::text(issued_at)),
                (Value::text("language"), Value::text("zxx")),
                (
                    Value::text("occasion"),
                    Value::text("Vouch judgment receipt"),
                ),
            ]),
        ),
        (
            Value::text("r"),
            Value::Array(vec![Value::Map(vec![
                (Value::text("role"), Value::text("vouch-policy")),
                (Value::text("id"), Value::text(&evaluation.policy_id)),
            ])]),
        ),
    ]);
    let by = personal_steward_id(signer.verifying_key().as_bytes());
    let signature = personal_sign(&core, signer, "author", issued_at);
    Attestation {
        core,
        signatures: vec![SignatureObject {
            by,
            alg: "ed25519".to_owned(),
            role: "author".to_owned(),
            signed_at: issued_at.to_owned(),
            keyset: None,
            signature: signature.to_vec(),
        }],
    }
}
