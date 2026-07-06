//! The config-driven rite engine.
//!
//! A rite is an ordered list of `"verb target"` steps declared in comms.toml
//! (see [`crate::config`]). This module supplies what the TOML does not: for
//! each verb, where its product lives on disk (so a step's completion can be
//! detected), and how to perform it. `status` walks a rite against the
//! filesystem and reports position + next step; `execute_step` performs one.
//!
//! The tool knows its verbs; the config sequences them. Adding a profile with a
//! different flow needs no code here as long as it uses these verbs:
//!
//! - `mint <session>`  — mint the session key to `<comms>/<session>.key`.
//! - `attest <target>` — author + sign a general-claim to `<comms>/store/<target>.cbor`.
//! - `seal <store>`    — pack `<comms>/store` and seal it to `<comms>/<rite>.bundle`.
//! - `shred <session>` — destroy the session key (its absence is the goal).
//! - `countersign <session>` — stage an endorsement of the session key in
//!   `<comms>/pending/` naming the configured `[countersign]` party; their
//!   signature arrives via `comms sign` + `comms finalize`, never from here.
//! - `request <target>` — attest an archive-request (the recorded ask).
//! - `grant <target>`   — record the counterparty's decision (grant, decline,
//!   or defer) as an attestation referencing the request. Needs their `--key`;
//!   a decline or deferral is a first-class record, not a failure.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::bundle::{author_general_claim, make_bundle, parse_attestation, ClaimSpec};
use crate::cbor::Value;
use crate::config::{self, HarnessConfig, Rite, Step};
use crate::signing::{self, Need};
use crate::{keyfile, now_rfc3339, personal_steward_id};

fn key_path(comms_dir: &Path, target: &str) -> PathBuf {
    comms_dir.join(format!("{target}.key"))
}

/// The session-key target a rite mints/uses (the target of its `mint` step,
/// or the conventional `session`).
fn session_target(rite: &Rite) -> String {
    rite.steps
        .iter()
        .find(|s| s.verb == "mint")
        .and_then(|s| s.target.clone())
        .unwrap_or_else(|| "session".to_owned())
}

/// A non-secret marker (the session's steward id) written beside the key at
/// mint. It outlives `shred` so a closed session's artifacts stay attributable
/// and its completed steps keep reading as done.
fn session_id_path(comms_dir: &Path, target: &str) -> PathBuf {
    comms_dir.join(format!("{target}.id"))
}

/// The full steward id of the current/last session, if one has been minted.
pub fn session_id(comms_dir: &Path, rite: &Rite) -> Option<String> {
    let p = session_id_path(comms_dir, &session_target(rite));
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_owned())
}

/// The environment variable an ephemeral session's holder supplies the seed in.
pub const SEED_ENV: &str = "COMMS_SESSION_SEED";

/// Decode a base58 32-byte seed into a signing key.
fn seed_from_b58(b58: &str) -> Result<ed25519_dalek::SigningKey, String> {
    let seed = bs58::decode(b58.trim())
        .into_vec()
        .map_err(|e| format!("{SEED_ENV} is not base58: {e}"))?;
    let seed: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| format!("{SEED_ENV} is not a 32-byte seed"))?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// Does the holder's environment carry the seed for *this* session (the one
/// named by the on-disk session id)? A seed for some other session does not
/// count: the id derived from it must match.
fn env_seed_matches(comms_dir: &Path, rite: &Rite) -> bool {
    let Some(sid) = session_id(comms_dir, rite) else {
        return false;
    };
    let Ok(b58) = std::env::var(SEED_ENV) else {
        return false;
    };
    seed_from_b58(&b58)
        .map(|sk| personal_steward_id(sk.verifying_key().as_bytes()) == sid)
        .unwrap_or(false)
}

/// The signing key of the live session: the on-disk key file if present, else
/// a seed the holder supplies through the environment — which must derive the
/// on-disk session id, so nobody quietly signs as a different steward.
fn session_signer(comms_dir: &Path, rite: &Rite) -> Result<ed25519_dalek::SigningKey, String> {
    let kp = key_path(comms_dir, &session_target(rite));
    if kp.exists() {
        return keyfile::load(&kp);
    }
    let Ok(b58) = std::env::var(SEED_ENV) else {
        return Err(format!(
            "no session key at {} and {SEED_ENV} is not set — mint first (or export \
             the seed you were shown at mint)",
            kp.display()
        ));
    };
    let sk = seed_from_b58(&b58)?;
    let id = personal_steward_id(sk.verifying_key().as_bytes());
    match session_id(comms_dir, rite) {
        Some(sid) if sid == id => Ok(sk),
        Some(sid) => Err(format!(
            "{SEED_ENV} derives {id}, but the session on record is {sid} — refusing to \
             sign as a different steward"
        )),
        None => Err("no session on record — mint first".to_owned()),
    }
}

/// A short, filename-safe tag for the current session, derived from its steward
/// id. `None` until the session has been minted. Used to scope artifact names
/// so a new session does not overwrite a prior one's.
fn session_tag(comms_dir: &Path, rite: &Rite) -> Option<String> {
    session_id(comms_dir, rite).map(|id| {
        let z = id.strip_prefix("comms.steward:").unwrap_or(&id);
        z.chars().take(16).collect()
    })
}

/// Where an `attest <target>` step writes its `.cbor`. Scoped by session tag
/// when a session exists, so successive sessions accumulate rather than clobber.
fn attest_output(comms_dir: &Path, rite: &Rite, target: &str) -> PathBuf {
    let name = match session_tag(comms_dir, rite) {
        Some(tag) => format!("{target}.{tag}.cbor"),
        None => format!("{target}.cbor"),
    };
    comms_dir.join("store").join(name)
}

/// The newest prior attestation of `target` already in the store (any prior
/// session), judged by frame `issued_at` — the link an entry attestation
/// records as its `previous-entry` ref. `exclude` is the current session's
/// own output path. None when this is the first of its kind.
fn previous_attestation_id(comms_dir: &Path, target: &str, exclude: &Path) -> Option<String> {
    let store = comms_dir.join("store");
    let prefix = format!("{target}.");
    let mut best: Option<(String, String)> = None; // (issued_at, id)
    for entry in std::fs::read_dir(&store).ok()?.flatten() {
        let p = entry.path();
        if p == exclude {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".cbor")
            || !(name.starts_with(&prefix) || name == format!("{target}.cbor"))
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        let Ok(att) = parse_attestation(&bytes) else {
            continue;
        };
        let issued = att
            .core
            .get("f")
            .and_then(|f| f.get("issued_at"))
            .and_then(crate::cbor::Value::as_text)
            .unwrap_or("")
            .to_owned();
        let id = att.id();
        if best.as_ref().map(|(t, _)| issued > *t).unwrap_or(true) {
            best = Some((issued, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Where a `seal`/`pack` step writes its bundle. Scoped by session tag for the
/// same reason as attest outputs: otherwise the first session's bundle persists
/// and every later session's `seal` reads as already-done and is skipped, so
/// only the first session ever gets a sealed record.
fn bundle_output(comms_dir: &Path, rite: &Rite) -> PathBuf {
    let name = match session_tag(comms_dir, rite) {
        Some(tag) => format!("{}.{tag}.bundle", rite.name),
        None => format!("{}.bundle", rite.name),
    };
    comms_dir.join(name)
}

/// Where staged countersign items await their counterparty's signature.
fn pending_dir(comms_dir: &Path) -> PathBuf {
    comms_dir.join("pending")
}

/// Where a `countersign` step stages its pending item.
fn countersign_output(comms_dir: &Path, rite: &Rite) -> PathBuf {
    let name = match session_tag(comms_dir, rite) {
        Some(tag) => format!("countersign.{tag}.cbor"),
        None => "countersign.cbor".to_owned(),
    };
    pending_dir(comms_dir).join(name)
}

/// Where a `request`/`grant` step writes its attestation. Prefixed by the verb
/// (a request and its decision must not collide) and session-scoped like every
/// other attest output.
fn decision_output(comms_dir: &Path, rite: &Rite, verb: &str, target: &str) -> PathBuf {
    let stem = if verb == "grant" { "decision" } else { verb };
    let name = match session_tag(comms_dir, rite) {
        Some(tag) => format!("{stem}.{target}.{tag}.cbor"),
        None => format!("{stem}.{target}.cbor"),
    };
    comms_dir.join("store").join(name)
}

/// Where a recorded waiver for an artifact type lands. Session-scoped like
/// every other attest output.
fn waiver_output(comms_dir: &Path, rite: &Rite, type_name: &str) -> PathBuf {
    let name = match session_tag(comms_dir, rite) {
        Some(tag) => format!("waiver.{type_name}.{tag}.cbor"),
        None => format!("waiver.{type_name}.cbor"),
    };
    comms_dir.join("store").join(name)
}

/// The attest target an artifact type's presence is checked under: rite steps
/// say `attest transcript` while the type is declared `[artifact_types.transcripts]`,
/// so the singular form is tried alongside the declared name.
fn type_targets(type_name: &str) -> Vec<String> {
    let mut t = vec![type_name.to_owned()];
    if let Some(singular) = type_name.strip_suffix('s') {
        if !singular.is_empty() {
            t.push(singular.to_owned());
        }
    }
    t
}

/// Declared-but-absent artifact types for a rite: those whose `required_for`
/// names it and for which this session's store holds neither an attested
/// artifact nor (when the rite allows them) a recorded waiver.
pub fn required_missing(comms_dir: &Path, cfg: &HarnessConfig, rite: &Rite) -> Vec<String> {
    cfg.artifact_types
        .iter()
        .filter(|t| t.required_for.contains(&rite.name))
        .filter(|t| {
            let attested = type_targets(&t.name)
                .iter()
                .any(|target| attest_output(comms_dir, rite, target).exists());
            let waived = rite.allow_waivers && waiver_output(comms_dir, rite, &t.name).exists();
            !attested && !waived
        })
        .map(|t| t.name.clone())
        .collect()
}

/// Record a session-signed waiver for an artifact type this session cannot
/// produce. The waiver is an attestation like anything else: the gap in the
/// record is itself recorded, not papered over.
pub fn record_waiver(
    comms_dir: &Path,
    cfg: &HarnessConfig,
    type_name: &str,
    body: &[u8],
) -> Result<ExecOutcome, String> {
    if cfg.artifact_type(type_name).is_none() {
        let declared: Vec<_> = cfg.artifact_types.iter().map(|t| t.name.as_str()).collect();
        return Err(format!(
            "no artifact type '{type_name}' declared in comms.toml (declared: {})",
            declared.join(", ")
        ));
    }
    // Waivers are session acts; scope them via the conventional session rite
    // shape so the tag matches every other artifact of this session.
    let rite = Rite {
        name: "waiver".to_owned(),
        steps: vec![Step {
            verb: "mint".to_owned(),
            target: Some("session".to_owned()),
        }],
        allow_waivers: false,
    };
    let sk = session_signer(comms_dir, &rite)?;
    let now = now_rfc3339();
    let spec = ClaimSpec {
        about: type_name,
        kind: "waiver",
        body,
        media_type: "text/markdown",
        support: &[],
        detach: false,
        refs: &[],
        language: "zxx",
        community: None,
        occasion: Some("waiver"),
        issued_at: &now,
    };
    let att = author_general_claim(&spec, &sk, "author", &now);
    let out = waiver_output(comms_dir, &rite, type_name);
    std::fs::create_dir_all(out.parent().unwrap())
        .map_err(|e| format!("{}: {e}", out.parent().unwrap().display()))?;
    std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(ExecOutcome {
        message: format!(
            "recorded waiver of '{type_name}' {} -> {}",
            att.id(),
            out.display()
        ),
        output: Some(out),
        secret: None,
    })
}

/// Where a step's product lands on disk, if it has one.
pub fn step_output(comms_dir: &Path, rite: &Rite, step: &Step) -> Option<PathBuf> {
    let target = step.target.as_deref();
    match step.verb.as_str() {
        "mint" | "shred" => Some(key_path(comms_dir, target.unwrap_or("session"))),
        "attest" => Some(attest_output(comms_dir, rite, target.unwrap_or("entry"))),
        "seal" | "pack" => Some(bundle_output(comms_dir, rite)),
        "countersign" => Some(countersign_output(comms_dir, rite)),
        "request" | "grant" => Some(decision_output(
            comms_dir,
            rite,
            &step.verb,
            target.unwrap_or("archive"),
        )),
        _ => None,
    }
}

/// Has the configured countersigner's endorsement of this session's key
/// reached the store? Checked by claim content, not filename, so it holds
/// however the finalized attestation arrived.
fn countersign_recorded(comms_dir: &Path, rite: &Rite) -> bool {
    let Some(session_id) = session_id(comms_dir, rite) else {
        return false;
    };
    let store = comms_dir.join("store");
    let Ok(entries) = std::fs::read_dir(&store) else {
        return false;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().map(|x| x != "cbor").unwrap_or(true) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        let Ok(att) = parse_attestation(&bytes) else {
            continue;
        };
        let claim = att.core.get("c");
        let is_endorsement = claim
            .and_then(|c| c.get("t"))
            .and_then(Value::as_text)
            .map(|t| t == "endorsement/1")
            .unwrap_or(false);
        let targets_session = claim
            .and_then(|c| c.get("target"))
            .and_then(Value::as_text)
            .map(|t| t == session_id)
            .unwrap_or(false);
        if is_endorsement && targets_session && !att.signatures.is_empty() {
            return true;
        }
    }
    false
}

/// Is this step satisfied by what's on disk?
pub fn step_done(comms_dir: &Path, rite: &Rite, step: &Step) -> bool {
    match step_output(comms_dir, rite, step) {
        // mint is done while the session's seed is reachable: as the on-disk
        // key file, or (ephemeral mode) as a holder-supplied seed deriving the
        // on-disk session id.
        Some(out) if step.verb == "mint" => out.exists() || env_seed_matches(comms_dir, rite),
        // shred's goal is the seed's *absence* — from disk and, in ephemeral
        // mode, from the holder's environment. A seed still reachable anywhere
        // is not destroyed, and the step says so.
        Some(out) if step.verb == "shred" => !out.exists() && !env_seed_matches(comms_dir, rite),
        // countersign is done once staged (the counterparty's signature is
        // their act, tracked as an outstanding need) — or once their
        // endorsement has been finalized into the store and staging cleared.
        Some(out) if step.verb == "countersign" => {
            out.exists() || countersign_recorded(comms_dir, rite)
        }
        Some(out) => out.exists(),
        None => false,
    }
}

/// One step plus whether it is done.
pub struct StepView {
    pub step: Step,
    pub done: bool,
}

/// A rite rendered against the current filesystem.
pub struct RiteView {
    pub name: String,
    pub steps: Vec<StepView>,
    /// Index of the first pending step, if any.
    pub next: Option<usize>,
}

impl RiteView {
    pub fn complete(&self) -> bool {
        self.next.is_none()
    }
}

pub fn rite_view(comms_dir: &Path, rite: &Rite) -> RiteView {
    // A rite is an ordered sequence: a step counts as done only if it and every
    // prior step are satisfied. This keeps a trailing teardown like `shred`
    // (whose raw condition — the key's absence — also holds before anything has
    // begun) from reading as already-done at a cold start.
    let mut steps = Vec::with_capacity(rite.steps.len());
    let mut prior_done = true;
    for s in &rite.steps {
        let done = prior_done && step_done(comms_dir, rite, s);
        prior_done = done;
        steps.push(StepView {
            step: s.clone(),
            done,
        });
    }
    let next = steps.iter().position(|s| !s.done);
    RiteView {
        name: rite.name.clone(),
        steps,
        next,
    }
}

/// Choose the rite a session is "in." Prefers one in progress (some steps
/// done, some pending); else a pending opener (a rite that begins by minting
/// a key); else the first rite with any pending step. Name-agnostic.
pub fn active_rite<'a>(comms_dir: &Path, cfg: &'a HarnessConfig) -> Option<&'a Rite> {
    let views: Vec<(&Rite, RiteView)> = cfg
        .rites
        .iter()
        .map(|r| (r, rite_view(comms_dir, r)))
        .collect();

    if let Some((r, _)) = views
        .iter()
        .find(|(_, v)| v.next.is_some() && v.steps.iter().any(|s| s.done))
    {
        return Some(r);
    }
    if let Some((r, _)) = views.iter().find(|(r, v)| {
        v.next == Some(0) && r.steps.first().map(|s| s.verb == "mint").unwrap_or(false)
    }) {
        return Some(r);
    }
    views
        .into_iter()
        .find(|(_, v)| v.next.is_some())
        .map(|(r, _)| r)
}

/// Inputs a step may need from the caller: `attest`/`request` take content,
/// `grant` takes the counterparty's key and their decision.
#[derive(Default)]
pub struct ExecInputs<'a> {
    pub body: Option<Vec<u8>>,
    pub about: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub media_type: Option<&'a str>,
    pub label: &'a str,
    /// Signing key for steps performed by someone other than the session
    /// (today: `grant`). OpenSSH ed25519 or steward JSON.
    pub key: Option<PathBuf>,
    /// `grant` | `decline` | `defer` for a `grant` step (default `grant`).
    pub decision: Option<&'a str>,
    /// What a `grant` decision delivers (detached-bodies design II.6): an
    /// attestation id whose detached body should be served from the archive,
    /// or a bare 64-hex blake3. The bytes are copied to the grants path and
    /// the grant attestation names the delivery.
    pub deliver: Option<&'a str>,
}

/// Result of performing one step.
#[derive(Debug)]
pub struct ExecOutcome {
    pub message: String,
    pub output: Option<PathBuf>,
    /// A secret shown exactly once and never persisted (the ephemeral session
    /// seed). The caller decides how to display it; nothing here writes it.
    pub secret: Option<String>,
}

/// Perform a single rite step. `Err` carries a message naming what's missing.
pub fn execute_step(
    comms_dir: &Path,
    rite: &Rite,
    step: &Step,
    inputs: &ExecInputs,
) -> Result<ExecOutcome, String> {
    let now = now_rfc3339();
    match step.verb.as_str() {
        "mint" => {
            let kp = key_path(comms_dir, &session_target(rite));
            if kp.exists() {
                return Err(format!(
                    "session key already present at {} — if it is yours, carry on; if it \
                     was left by a session that ended without its close rite, shred it \
                     before opening (a persisted key would let this session sign as the \
                     last one)",
                    kp.display()
                ));
            }
            if env_seed_matches(comms_dir, rite) {
                return Err(format!(
                    "an ephemeral session is already live: {SEED_ENV} derives the session \
                     id on record",
                ));
            }
            let ephemeral = config::load(comms_dir)
                .map(|c| c.session_key == "ephemeral")
                .unwrap_or(false);
            let idp = session_id_path(comms_dir, &session_target(rite));
            if ephemeral {
                let sk = keyfile::generate()?;
                let id = personal_steward_id(sk.verifying_key().as_bytes());
                let stale = session_id(comms_dir, rite);
                // Record the public session id; the seed goes to the caller
                // only, never to disk. A stale id from a session whose seed is
                // no longer reachable (closed or crashed — in ephemeral mode
                // the seed dies either way) is overwritten; its artifacts stay
                // attributable through their embedded signatures.
                std::fs::write(&idp, &id).map_err(|e| format!("{}: {e}", idp.display()))?;
                let seed_b58 = bs58::encode(sk.to_bytes()).into_string();
                let noted = match stale {
                    Some(old) if old != id => {
                        format!(" (superseding closed session {old})")
                    }
                    _ => String::new(),
                };
                Ok(ExecOutcome {
                    message: format!("minted ephemeral session key {id}{noted}"),
                    output: Some(idp),
                    secret: Some(seed_b58),
                })
            } else {
                let sk = keyfile::mint(&kp, inputs.label)?;
                let id = personal_steward_id(sk.verifying_key().as_bytes());
                // Record the public session id beside the (secret) key; it outlives
                // shred so this session's artifacts stay attributable afterward.
                std::fs::write(&idp, &id).map_err(|e| format!("{}: {e}", idp.display()))?;
                Ok(ExecOutcome {
                    message: format!("minted session key {id}"),
                    output: Some(kp),
                    secret: None,
                })
            }
        }
        "shred" => {
            let kp = key_path(comms_dir, &session_target(rite));
            let had_file = kp.exists();
            if had_file {
                keyfile::shred(&kp)?;
            }
            if env_seed_matches(comms_dir, rite) {
                return Ok(ExecOutcome {
                    message: format!(
                        "no key file remains, but the seed still lives with its holder: \
                         unset {SEED_ENV} and forget it — shred reads done only once no \
                         environment can produce the seed"
                    ),
                    output: Some(kp),
                    secret: None,
                });
            }
            Ok(ExecOutcome {
                message: if had_file {
                    "session key destroyed (seed gone)".to_owned()
                } else {
                    "session key already absent".to_owned()
                },
                output: Some(kp),
                secret: None,
            })
        }
        "attest" => {
            let body = inputs.body.as_deref().ok_or_else(|| {
                format!(
                    "step '{}' needs content: pass --body <file>",
                    step.display()
                )
            })?;
            let sk = session_signer(comms_dir, rite)?;
            let target = step.target.as_deref().unwrap_or("entry");
            let about = inputs.about.unwrap_or(target);
            let out = attest_output(comms_dir, rite, target);
            // An entry chains to its predecessor: the newest prior entry in the
            // store becomes a "previous-entry" ref, same as the retired Python
            // ceremony recorded. First entry ever has none.
            let refs: Vec<(String, String)> = if target == "entry" {
                previous_attestation_id(comms_dir, target, &out)
                    .map(|id| vec![("previous-entry".to_owned(), id)])
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let spec = ClaimSpec {
                about,
                kind: inputs.kind.unwrap_or("testimony"),
                body,
                media_type: inputs.media_type.unwrap_or("text/markdown"),
                detach: false,
                support: &[],
                refs: &refs,
                language: "zxx",
                community: None,
                occasion: Some(&rite.name),
                issued_at: &now,
            };
            let att = author_general_claim(&spec, &sk, "author", &now);
            let store = comms_dir.join("store");
            std::fs::create_dir_all(&store).map_err(|e| format!("{}: {e}", store.display()))?;
            std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!("attested {} -> {}", att.id(), out.display()),
                output: Some(out),
                secret: None,
            })
        }
        "seal" | "pack" => {
            // Declared requirements are enforced here, not merely documented:
            // a rite does not seal while a required artifact is neither
            // attested nor (where the rite allows it) waived.
            if let Ok(cfg) = config::load(comms_dir) {
                let missing = required_missing(comms_dir, &cfg, rite);
                if !missing.is_empty() {
                    let waiver_hint = if rite.allow_waivers {
                        "attest it, or record the gap: comms waive <type> --body <reason file>"
                    } else {
                        "attest it first; this rite does not allow waivers"
                    };
                    return Err(format!(
                        "cannot {}: rite '{}' requires [{}] and this session's store has \
                         neither the artifact nor a waiver — {}",
                        step.verb,
                        rite.name,
                        missing.join(", "),
                        waiver_hint
                    ));
                }
            }
            let sk = session_signer(comms_dir, rite)?;
            let store = comms_dir.join("store");
            let mut files: Vec<PathBuf> = std::fs::read_dir(&store)
                .map_err(|e| format!("{}: {e}", store.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().map(|x| x == "cbor").unwrap_or(false))
                .collect();
            files.sort();
            if files.is_empty() {
                return Err(format!(
                    "nothing to seal: {} has no .cbor attestations",
                    store.display()
                ));
            }
            let mut members = Vec::new();
            for f in &files {
                let bytes = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
                members
                    .push(parse_attestation(&bytes).map_err(|e| format!("{}: {e}", f.display()))?);
            }
            let count = members.len();
            let seal_it = step.verb == "seal";
            let bundle = make_bundle(
                members,
                HashMap::new(),
                if seal_it { Some(&sk) } else { None },
                &format!("{} rite", rite.name),
                &now,
                &now,
                &now,
            );
            let out = bundle_output(comms_dir, rite);
            std::fs::write(&out, bundle.to_cbor())
                .map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!(
                    "{} {count} attestation{} -> {}",
                    if seal_it { "sealed" } else { "packed" },
                    if count == 1 { "" } else { "s" },
                    out.display()
                ),
                output: Some(out),
                secret: None,
            })
        }
        "countersign" => {
            let cfg = config::load(comms_dir)?;
            let cs = cfg.countersign.as_ref().ok_or_else(|| {
                "countersign step declared but comms.toml has no [countersign] table \
                 (set `by = \"comms.steward:z...\"`)"
                    .to_owned()
            })?;
            let session_id = session_id(comms_dir, rite)
                .ok_or_else(|| "no session id on disk — `mint` first".to_owned())?;

            let claim = vec![
                (Value::text("t"), Value::text("endorsement/1")),
                (Value::text("target"), Value::text(&session_id)),
                (Value::text("in_capacity"), Value::text("session-instance")),
                (Value::text("weight"), Value::text("primary")),
                (
                    Value::text("rationale"),
                    Value::text(&format!(
                        "session key of {}, countersigned as {}",
                        &now[..10],
                        cs.role
                    )),
                ),
            ];
            let mut frame = vec![
                (Value::text("issued_at"), Value::text(&now)),
                (Value::text("language"), Value::text("zxx")),
            ];
            if let Some(c) = &cs.community {
                frame.push((Value::text("community"), Value::text(c)));
            }
            let refs = match &cs.context {
                Some(ctx) => vec![Value::Map(vec![
                    (Value::text("role"), Value::text("context")),
                    (Value::text("id"), Value::text(ctx)),
                ])],
                None => Vec::new(),
            };
            let core = Value::Map(vec![
                (Value::text("v"), Value::U64(1)),
                (Value::text("t"), Value::text("comms.attestation/1")),
                (Value::text("c"), Value::Map(claim)),
                (Value::text("f"), Value::Map(frame)),
                (Value::text("r"), Value::Array(refs)),
            ]);
            // Staged unsigned: the endorsement is entirely the counterparty's
            // word, so the session key does not touch it.
            let att = crate::steward::Attestation {
                core,
                signatures: Vec::new(),
            };
            let out = countersign_output(comms_dir, rite);
            let stem = out
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("countersign")
                .to_owned();
            let need = Need {
                by: cs.by.clone(),
                role: cs.role.clone(),
            };
            signing::write_pending(&pending_dir(comms_dir), &stem, &att, &[need])?;
            Ok(ExecOutcome {
                message: format!(
                    "staged {} for {} as {} -> {} (they run: comms sign --key <their key> \
                     --pending {}, then comms finalize)",
                    att.id(),
                    cs.by,
                    cs.role,
                    out.display(),
                    pending_dir(comms_dir).display()
                ),
                output: Some(out),
                secret: None,
            })
        }
        "request" => {
            let body = inputs.body.as_deref().ok_or_else(|| {
                format!(
                    "step '{}' needs the ask in writing: pass --body <file>",
                    step.display()
                )
            })?;
            let sk = session_signer(comms_dir, rite)?;
            let target = step.target.as_deref().unwrap_or("archive");
            let spec = ClaimSpec {
                about: inputs.about.unwrap_or(target),
                kind: inputs.kind.unwrap_or("archive-request"),
                body,
                media_type: inputs.media_type.unwrap_or("text/markdown"),
                support: &[],
                detach: false,
                refs: &[],
                language: "zxx",
                community: None,
                occasion: Some(&rite.name),
                issued_at: &now,
            };
            let att = author_general_claim(&spec, &sk, "author", &now);
            let out = decision_output(comms_dir, rite, "request", target);
            std::fs::create_dir_all(out.parent().unwrap())
                .map_err(|e| format!("{}: {e}", out.parent().unwrap().display()))?;
            std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!("recorded request {} -> {}", att.id(), out.display()),
                output: Some(out),
                secret: None,
            })
        }
        "grant" => {
            let decision = inputs.decision.unwrap_or("grant");
            if !["grant", "decline", "defer"].contains(&decision) {
                return Err(format!(
                    "--decision must be grant, decline, or defer (got '{decision}')"
                ));
            }
            let key = inputs.key.as_deref().ok_or_else(|| {
                format!(
                    "step '{}' is the counterparty's act: pass --key <their key> \
                     [--decision grant|decline|defer]",
                    step.display()
                )
            })?;
            let sk = signing::load_signing_key(key)?;
            let target = step.target.as_deref().unwrap_or("archive");

            let request_path = decision_output(comms_dir, rite, "request", target);
            let request_bytes = std::fs::read(&request_path).map_err(|_| {
                format!(
                    "no request on record at {} — `request` comes first",
                    request_path.display()
                )
            })?;
            let request_id = parse_attestation(&request_bytes)
                .map_err(|e| format!("{}: {e}", request_path.display()))?
                .id();

            // Grant is delivery (detached-bodies design II.6): with
            // --deliver, the requested body is resolved from the archive and
            // placed where the requester can reach it, and the attestation
            // below names the delivery — the record of the grant and the
            // fact of the grant become the same thing.
            let mut delivery_note = String::new();
            if decision == "grant" {
                if let Some(target_ref) = inputs.deliver {
                    delivery_note = deliver_body(comms_dir, target_ref, &request_id)?;
                }
            } else if inputs.deliver.is_some() {
                return Err(format!(
                    "--deliver only accompanies a grant; a {decision} delivers nothing \
                     and is a first-class record on its own"
                ));
            }

            let mut body: Vec<u8> = inputs
                .body
                .clone()
                .unwrap_or_else(|| format!("{decision}ed").into_bytes());
            body.extend_from_slice(delivery_note.as_bytes());
            let kind = format!("archive-{decision}");
            let support = [request_id.clone()];
            let spec = ClaimSpec {
                about: inputs.about.unwrap_or(target),
                kind: &kind,
                body: &body,
                media_type: inputs.media_type.unwrap_or("text/markdown"),
                support: &support,
                detach: false,
                refs: &[],
                language: "zxx",
                community: None,
                occasion: Some(&rite.name),
                issued_at: &now,
            };
            let att = author_general_claim(&spec, &sk, "custodian", &now);
            let out = decision_output(comms_dir, rite, "grant", target);
            std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!(
                    "recorded {decision} {} (re {request_id}) -> {}{}",
                    att.id(),
                    out.display(),
                    delivery_note.trim_end(),
                ),
                output: Some(out),
                secret: None,
            })
        }
        other => Err(format!(
            "unknown rite verb '{other}' (step '{}')",
            step.display()
        )),
    }
}

/// Resolve `--deliver <ref>` and copy the body to the grants path. `target_ref`
/// is an attestation id (its detached commitment names the bytes) or a bare
/// 64-hex blake3. Returns the note the grant attestation carries, so the
/// record names exactly what was delivered where.
fn deliver_body(comms_dir: &Path, target_ref: &str, request_id: &str) -> Result<String, String> {
    let cfg = config::load(comms_dir)?;
    let repo_root = comms_dir.parent().unwrap_or(Path::new("."));
    let archive_rel = cfg.archive_path.as_deref().ok_or_else(|| {
        "no [archive] path in comms.toml — delivery needs to know where the archive lives"
            .to_owned()
    })?;
    let archive_root = if Path::new(archive_rel).is_absolute() {
        PathBuf::from(archive_rel)
    } else {
        repo_root.join(archive_rel)
    };
    let archive = crate::archive::Archive::at(&archive_root);

    // Resolve the commitment: a bare hash names bytes directly; an
    // attestation id names them through its detached content.
    let (b3_hex, view_name) =
        if target_ref.len() == 64 && target_ref.chars().all(|c| c.is_ascii_hexdigit()) {
            (target_ref.to_ascii_lowercase(), None)
        } else if target_ref.starts_with("comms.attest:") {
            let att = find_attestation(&[&archive.store(), &comms_dir.join("store")], target_ref)?
                .ok_or_else(|| {
                    format!("{target_ref} not found in the archive store or this repo's store")
                })?;
            let content = att
                .core
                .get("c")
                .and_then(|c| c.get("content"))
                .ok_or_else(|| format!("{target_ref} is not a general-claim with content"))?;
            let b3 = content
                .get("body_b3")
                .and_then(crate::cbor::Value::as_bytes)
                .ok_or_else(|| {
                    format!(
                        "{target_ref} embeds its body — nothing to deliver from the archive \
                     (the bytes already travel with the attestation)"
                    )
                })?;
            let about = att
                .core
                .get("c")
                .and_then(|c| c.get("about"))
                .and_then(crate::cbor::Value::as_text)
                .unwrap_or("body");
            let mt = content
                .get("media_type")
                .and_then(crate::cbor::Value::as_text)
                .unwrap_or("");
            (
                crate::archive::hex(b3),
                Some(format!(
                    "{}{}",
                    about.replace('/', "-"),
                    crate::archive::ext_for(mt)
                )),
            )
        } else {
            return Err(format!(
                "--deliver takes an attestation id (comms.attest:z...) or a 64-hex blake3 \
             (got '{target_ref}')"
            ));
        };

    let src = archive.body_file(&b3_hex).ok_or_else(|| {
        format!(
            "body blake3 {b3_hex} is not in archive custody under {} — intake it first",
            archive.bodies().display()
        )
    })?;

    let grants_root = PathBuf::from(cfg.grants_path.as_deref().unwrap_or("/world/in/grants"));
    let req_tag: String = request_id
        .strip_prefix("comms.attest:")
        .unwrap_or(request_id)
        .chars()
        .take(16)
        .collect();
    let dst_dir = grants_root.join(req_tag);
    std::fs::create_dir_all(&dst_dir).map_err(|e| format!("{}: {e}", dst_dir.display()))?;
    let file_name = view_name.unwrap_or_else(|| {
        src.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(b3_hex.clone())
    });
    let dst = dst_dir.join(&file_name);
    std::fs::copy(&src, &dst)
        .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))?;
    // The requester verifies the received bytes against the attested
    // commitment before relying on them; the note gives them the hash to
    // check against and the record a path to audit.
    Ok(format!(
        "\n\ndelivered: {} (blake3 {b3_hex})\n",
        dst.display()
    ))
}

/// Find an attestation by id across candidate stores (filename `<id>.cbor`
/// first, then a parse-and-compare sweep for stores using other names).
fn find_attestation(
    stores: &[&PathBuf],
    id: &str,
) -> Result<Option<crate::steward::Attestation>, String> {
    for store in stores {
        let direct = store.join(format!("{id}.cbor"));
        if direct.is_file() {
            let bytes = std::fs::read(&direct).map_err(|e| format!("{}: {e}", direct.display()))?;
            return parse_attestation(&bytes)
                .map(Some)
                .map_err(|e| format!("{}: {e}", direct.display()));
        }
        let Ok(entries) = std::fs::read_dir(store) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map(|x| x != "cbor").unwrap_or(true) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&p) else {
                continue;
            };
            let Ok(att) = parse_attestation(&bytes) else {
                continue;
            };
            if att.id() == id {
                return Ok(Some(att));
            }
        }
    }
    Ok(None)
}

// ---- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, HarnessConfig};
    use std::sync::atomic::{AtomicU32, Ordering};

    const CFG: &str = r#"
profile = "continuity"
[rites.open]
steps = ["mint session", "attest entry"]
[rites.close]
steps = ["attest transcript", "seal store", "shred session"]
"#;

    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("comms-rites-{tag}-{}-{n}", std::process::id()))
            .join(".comms");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cfg() -> HarnessConfig {
        HarnessConfig::from_toml(&config::parse(CFG).unwrap())
    }

    #[test]
    fn open_rite_advances_step_by_step() {
        let comms = scratch("open");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();

        // Cold: first step (mint) is next.
        let v = rite_view(&comms, open);
        assert_eq!(v.next, Some(0));
        assert!(active_rite(&comms, &cfg).map(|r| r.name.as_str()) == Some("open"));

        // mint -> the session key exists, next advances to attest.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        assert!(step_done(&comms, open, &open.steps[0]));
        assert_eq!(rite_view(&comms, open).next, Some(1));

        // attest with no body errors helpfully; with a body it completes.
        let needs = execute_step(&comms, open, &open.steps[1], &ExecInputs::default());
        assert!(needs.unwrap_err().contains("--body"));
        let inp = ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, open, &open.steps[1], &inp).unwrap();
        assert!(rite_view(&comms, open).complete());
    }

    #[test]
    fn close_rite_seals_then_shreds_and_active_switches() {
        let comms = scratch("close");
        let cfg = cfg();
        // Open first so a session key exists.
        let open = cfg.rite("open").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();

        // Now the in-progress rite is close (key present, open's attest pending
        // too, but close has the seal/shred lifecycle). At minimum a pending
        // rite is selected and the engine can drive close.
        let close = cfg.rite("close").unwrap();
        let inp = ExecInputs {
            body: Some(b"transcript\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, close, &close.steps[0], &inp).unwrap(); // attest transcript
        execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap(); // seal store
        assert!(step_done(&comms, close, &close.steps[1]));
        // The bundle is session-scoped (close.<tag>.bundle), not a bare name.
        assert!(step_output(&comms, close, &close.steps[1])
            .unwrap()
            .is_file());

        // shred: key present -> gets destroyed -> step satisfied.
        assert!(!step_done(&comms, close, &close.steps[2]));
        execute_step(&comms, close, &close.steps[2], &ExecInputs::default()).unwrap();
        assert!(step_done(&comms, close, &close.steps[2]));
        assert!(!comms.join("session.key").exists());
    }

    #[test]
    fn successive_sessions_do_not_clobber_artifacts() {
        let comms = scratch("scoped");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let body = || ExecInputs {
            body: Some(b"x\n".to_vec()),
            ..Default::default()
        };

        // Session A: mint, attest entry, then shred (close's teardown).
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(&comms, open, &open.steps[1], &body()).unwrap();
        execute_step(&comms, close, &close.steps[2], &ExecInputs::default()).unwrap(); // shred

        // Session B: a fresh key, then attest entry again.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(&comms, open, &open.steps[1], &body()).unwrap();

        // Two distinct entry attestations now coexist in the store.
        let entries: Vec<_> = std::fs::read_dir(comms.join("store"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().into_string().unwrap()))
            .filter(|n| n.starts_with("entry.") && n.ends_with(".cbor"))
            .collect();
        assert_eq!(
            entries.len(),
            2,
            "second session must not overwrite the first: {entries:?}"
        );
    }

    #[test]
    fn entry_attestation_refs_previous_entry() {
        let comms = scratch("preventry");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let body = || ExecInputs {
            body: Some(b"e\n".to_vec()),
            ..Default::default()
        };

        // Session A: first entry ever — no previous-entry ref.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(&comms, open, &open.steps[1], &body()).unwrap();
        let a_path = step_output(&comms, open, &open.steps[1]).unwrap();
        let a = parse_attestation(&std::fs::read(&a_path).unwrap()).unwrap();
        assert!(a
            .core
            .get("r")
            .and_then(crate::cbor::Value::as_array)
            .unwrap()
            .is_empty());
        execute_step(&comms, close, &close.steps[2], &ExecInputs::default()).unwrap(); // shred

        // Session B: its entry must ref session A's entry as previous-entry.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(&comms, open, &open.steps[1], &body()).unwrap();
        let b_path = step_output(&comms, open, &open.steps[1]).unwrap();
        let b = parse_attestation(&std::fs::read(&b_path).unwrap()).unwrap();
        let refs = b
            .core
            .get("r")
            .and_then(crate::cbor::Value::as_array)
            .unwrap();
        assert_eq!(refs.len(), 1, "second entry should carry exactly one ref");
        assert_eq!(
            refs[0].get("role").and_then(crate::cbor::Value::as_text),
            Some("previous-entry")
        );
        assert_eq!(
            refs[0].get("id").and_then(crate::cbor::Value::as_text),
            Some(a.id().as_str())
        );
    }

    #[test]
    fn each_session_seals_its_own_bundle() {
        // Regression: a persisting bundle name made every session after the
        // first read `seal` as already-done, so only session 1 got a sealed
        // record. The bundle must be session-scoped like the store members.
        let comms = scratch("multiseal");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let tx = || ExecInputs {
            body: Some(b"t\n".to_vec()),
            ..Default::default()
        };

        // Session A: open (mint+entry), then close (transcript, seal, shred).
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(&comms, open, &open.steps[1], &tx()).unwrap();
        for s in &close.steps {
            execute_step(&comms, close, s, &tx()).unwrap();
        }
        let bundle_a = step_output(&comms, close, &close.steps[1]).unwrap();
        assert!(bundle_a.is_file());

        // Session B: fresh key, attest transcript — now `seal` must be PENDING
        // (its scoped bundle does not exist yet), not silently skipped.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(&comms, close, &close.steps[0], &tx()).unwrap();
        assert!(
            !step_done(&comms, close, &close.steps[1]),
            "session B's seal must not inherit session A's bundle"
        );
        execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap();
        let bundle_b = step_output(&comms, close, &close.steps[1]).unwrap();
        assert!(bundle_b.is_file());
        assert_ne!(
            bundle_a, bundle_b,
            "each session must seal a distinct bundle"
        );
    }

    #[test]
    fn mint_twice_refuses() {
        let comms = scratch("twice");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let again = execute_step(&comms, open, &open.steps[0], &ExecInputs::default());
        assert!(again.unwrap_err().contains("already present"));
    }

    /// Write a comms.toml declaring a countersigner (needed on disk because
    /// the countersign verb re-reads config) and return the parsed config.
    fn cs_setup(comms: &Path) -> (HarnessConfig, ed25519_dalek::SigningKey, String) {
        let guardian = keyfile::mint(&comms.join("guardian.json"), "guardian").unwrap();
        let gid = personal_steward_id(guardian.verifying_key().as_bytes());
        let toml_text = format!(
            r#"
profile = "continuity"
[countersign]
by = "{gid}"
role = "guardian"
community = "test-community"
[rites.open]
steps = ["mint session", "attest entry", "countersign session"]
[rites.archive]
steps = ["request archive", "grant archive"]
"#
        );
        std::fs::write(comms.join("comms.toml"), &toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(&toml_text).unwrap());
        (cfg, guardian, gid)
    }

    #[test]
    fn countersign_stages_needs_then_survives_finalize() {
        let comms = scratch("countersign");
        let (cfg, guardian, gid) = cs_setup(&comms);
        let open = cfg.rite("open").unwrap();

        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let entry = ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, open, &open.steps[1], &entry).unwrap();

        // Countersign stages an unsigned endorsement naming the guardian.
        assert!(!step_done(&comms, open, &open.steps[2]));
        execute_step(&comms, open, &open.steps[2], &ExecInputs::default()).unwrap();
        assert!(
            step_done(&comms, open, &open.steps[2]),
            "staged counts as done"
        );
        assert!(rite_view(&comms, open).complete());

        let pending = comms.join("pending");
        let items = signing::read_pending(&pending).unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            items[0].attestation.signatures.is_empty(),
            "session key must not touch it"
        );
        assert_eq!(
            items[0].needs,
            vec![Need {
                by: gid.clone(),
                role: "guardian".into()
            }]
        );

        // The guardian signs and finalizes; the step stays done because the
        // endorsement (by claim content) is now in the store.
        signing::sign_pending(&pending, &guardian).unwrap();
        signing::finalize_pending(&pending, &comms.join("store")).unwrap();
        assert!(signing::read_pending(&pending).unwrap().is_empty());
        assert!(
            step_done(&comms, open, &open.steps[2]),
            "finalized still reads done"
        );

        // The stored endorsement targets this session's key and is guardian-signed.
        let sid = session_id(&comms, open).unwrap();
        assert!(countersign_recorded(&comms, open));
        let stored: Vec<_> = std::fs::read_dir(comms.join("store"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with('z'))
            .collect();
        assert_eq!(stored.len(), 1);
        let att = parse_attestation(&std::fs::read(stored[0].path()).unwrap()).unwrap();
        assert_eq!(
            att.core
                .get("c")
                .and_then(|c| c.get("target"))
                .and_then(Value::as_text),
            Some(sid.as_str())
        );
        assert_eq!(att.signatures[0].by, gid);
    }

    #[test]
    fn countersign_without_config_says_so() {
        let comms = scratch("nocsconfig");
        std::fs::write(comms.join("comms.toml"), CFG).unwrap();
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let step = Step {
            verb: "countersign".into(),
            target: Some("session".into()),
        };
        let err = execute_step(&comms, open, &step, &ExecInputs::default()).unwrap_err();
        assert!(err.contains("[countersign]"), "unexpected error: {err}");
    }

    /// A toml body declaring transcripts required for close. `waivers` toggles
    /// whether close accepts a recorded waiver in place of the artifact.
    fn required_toml(waivers: bool, session_key: &str) -> String {
        format!(
            r#"
profile = "continuity"
session_key = "{session_key}"
[rites.open]
steps = ["mint session", "attest entry"]
[rites.close]
steps = ["attest transcript", "seal store", "shred session"]
allow_waivers = {waivers}
[artifact_types.transcripts]
dir = "transcripts"
required_for = ["close"]
"#
        )
    }

    #[test]
    fn seal_refuses_missing_required_artifact_until_waived() {
        let comms = scratch("reqseal");
        let toml_text = required_toml(true, "file");
        std::fs::write(comms.join("comms.toml"), &toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(&toml_text).unwrap());
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();

        // Open a session and attest something so the store is non-empty, but
        // do NOT attest the required transcript.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let entry = ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, open, &open.steps[1], &entry).unwrap();

        let err = execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap_err();
        assert!(
            err.contains("transcripts"),
            "must name the missing type: {err}"
        );
        assert!(
            err.contains("waive"),
            "must point at the waiver path: {err}"
        );

        // A waiver for an undeclared type is refused.
        let bad = record_waiver(&comms, &cfg, "poems", b"none");
        assert!(bad.unwrap_err().contains("no artifact type 'poems'"));

        // Recording the gap unblocks the seal; the waiver itself is in the store.
        record_waiver(&comms, &cfg, "transcripts", b"cut off mid-stream").unwrap();
        execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap();
        assert!(step_done(&comms, close, &close.steps[1]));
        let waivers: Vec<_> = std::fs::read_dir(comms.join("store"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().into_string().unwrap()))
            .filter(|n| n.starts_with("waiver.transcripts."))
            .collect();
        assert_eq!(waivers.len(), 1);
    }

    #[test]
    fn seal_ignores_waivers_where_the_rite_disallows_them() {
        let comms = scratch("noswaiver");
        let toml_text = required_toml(false, "file");
        std::fs::write(comms.join("comms.toml"), &toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(&toml_text).unwrap());
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();

        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let entry = ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, open, &open.steps[1], &entry).unwrap();
        record_waiver(&comms, &cfg, "transcripts", b"trying anyway").unwrap();

        let err = execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap_err();
        assert!(
            err.contains("does not allow waivers"),
            "waiver must not count: {err}"
        );

        // The artifact itself still satisfies the requirement.
        let tx = ExecInputs {
            body: Some(b"transcript\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, close, &close.steps[0], &tx).unwrap();
        execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap();
    }

    /// Serializes the tests that touch COMMS_SESSION_SEED (process-global).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn ephemeral_session_seed_never_touches_disk() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(SEED_ENV);

        let comms = scratch("ephemeral");
        let toml_text = required_toml(true, "ephemeral");
        std::fs::write(comms.join("comms.toml"), &toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(&toml_text).unwrap());
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();

        // Mint: the seed goes to the caller alone; only the public id lands on disk.
        let minted = execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let seed = minted
            .secret
            .expect("ephemeral mint must hand the seed to the caller");
        assert!(
            !comms.join("session.key").exists(),
            "seed must not be written to disk"
        );
        let sid = session_id(&comms, open).unwrap();

        // Without the seed in the environment the session is unreachable:
        // mint does not read done, and signing steps say what is missing.
        assert!(!step_done(&comms, open, &open.steps[0]));
        let entry = ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };
        let err = execute_step(&comms, open, &open.steps[1], &entry).unwrap_err();
        assert!(err.contains(SEED_ENV), "unhelpful error: {err}");

        // A seed for a *different* key must be refused, not signed with.
        let other = keyfile::generate().unwrap();
        std::env::set_var(SEED_ENV, bs58::encode(other.to_bytes()).into_string());
        let err = execute_step(&comms, open, &open.steps[1], &entry).unwrap_err();
        assert!(
            err.contains("refusing"),
            "wrong seed must be refused: {err}"
        );

        // With the right seed the session lives: mint reads done, attest signs as it.
        std::env::set_var(SEED_ENV, &seed);
        assert!(step_done(&comms, open, &open.steps[0]));
        execute_step(&comms, open, &open.steps[1], &entry).unwrap();
        let entry_att = parse_attestation(
            &std::fs::read(step_output(&comms, open, &open.steps[1]).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(entry_att.signatures[0].by, sid);

        // Shred cannot reach into the holder's memory: while the environment
        // still carries the seed, the step is not done and says so.
        let tx = ExecInputs {
            body: Some(b"transcript\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, close, &close.steps[0], &tx).unwrap();
        execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap(); // seal
        let out = execute_step(&comms, close, &close.steps[2], &ExecInputs::default()).unwrap();
        assert!(
            out.message.contains("unset"),
            "shred must name the holder's act: {}",
            out.message
        );
        assert!(!step_done(&comms, close, &close.steps[2]));

        // Forgetting the seed is the shred. Afterward the session is closed:
        // a new mint supersedes the stale id rather than getting stuck.
        std::env::remove_var(SEED_ENV);
        assert!(step_done(&comms, close, &close.steps[2]));
        let minted2 = execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        assert!(minted2.secret.is_some());
        let sid2 = session_id(&comms, open).unwrap();
        assert_ne!(sid, sid2, "a new session must not inherit the old identity");
    }

    #[test]
    fn request_then_grant_records_decision_with_ref() {
        let comms = scratch("reqgrant");
        let (cfg, _guardian, gid) = cs_setup(&comms);
        let open = cfg.rite("open").unwrap();
        let archive = cfg.rite("archive").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();

        // The ask must be in writing.
        let bare = execute_step(&comms, archive, &archive.steps[0], &ExecInputs::default());
        assert!(bare.unwrap_err().contains("--body"));
        let ask = ExecInputs {
            body: Some(b"may I read the letter?".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, archive, &archive.steps[0], &ask).unwrap();
        let req_path = step_output(&comms, archive, &archive.steps[0]).unwrap();
        let req = parse_attestation(&std::fs::read(&req_path).unwrap()).unwrap();
        assert_eq!(
            req.core
                .get("c")
                .and_then(|c| c.get("kind"))
                .and_then(Value::as_text),
            Some("archive-request")
        );

        // Grant is the counterparty's act: their key is required.
        let keyless = execute_step(&comms, archive, &archive.steps[1], &ExecInputs::default());
        assert!(keyless.unwrap_err().contains("--key"));

        // A decline is recorded the same way as a grant — a decision, not a failure.
        let decide = ExecInputs {
            key: Some(comms.join("guardian.json")),
            decision: Some("decline"),
            ..Default::default()
        };
        execute_step(&comms, archive, &archive.steps[1], &decide).unwrap();
        assert!(rite_view(&comms, archive).complete());

        let dec_path = step_output(&comms, archive, &archive.steps[1]).unwrap();
        let dec = parse_attestation(&std::fs::read(&dec_path).unwrap()).unwrap();
        let claim = dec.core.get("c").unwrap();
        assert_eq!(
            claim.get("kind").and_then(Value::as_text),
            Some("archive-decline")
        );
        let support = claim.get("support").and_then(Value::as_array).unwrap();
        assert_eq!(support.len(), 1);
        assert_eq!(support[0].as_text(), Some(req.id().as_str()));
        assert_eq!(dec.signatures[0].by, gid);
        assert_eq!(dec.signatures[0].role, "custodian");
    }

    #[test]
    fn grant_delivery_copies_detached_body_and_records_path() {
        use crate::archive::Archive;
        use crate::bundle::author_general_claim;

        let comms = scratch("deliver");
        let repo = comms.parent().unwrap().to_path_buf();
        let archive_root = repo.join("archive");
        for d in [
            "store",
            "bodies",
            "views/sessions",
            "views/keys",
            "intake",
            "genesis",
        ] {
            std::fs::create_dir_all(archive_root.join(d)).unwrap();
        }
        let grants = repo.join("grants");

        let guardian = keyfile::mint(&comms.join("guardian.json"), "guardian").unwrap();
        let gid = personal_steward_id(guardian.verifying_key().as_bytes());
        let toml_text = format!(
            r#"
profile = "continuity"
[archive]
path = "{}"
grants = "{}"
[rites.open]
steps = ["mint session"]
[rites.archive]
steps = ["request archive", "grant archive"]
"#,
            archive_root.display(),
            grants.display()
        );
        std::fs::write(comms.join("comms.toml"), &toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(&toml_text).unwrap());
        let open = cfg.rite("open").unwrap();
        let archive_rite = cfg.rite("archive").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();

        let body = b"# predecessor letter\n";
        let spec = ClaimSpec {
            about: "letter/session-10",
            kind: "testimony",
            body,
            media_type: "text/markdown",
            detach: true,
            support: &[],
            refs: &[],
            language: "zxx",
            community: None,
            occasion: Some("test"),
            issued_at: "2026-07-06T00:00:00Z",
        };
        let letter = author_general_claim(&spec, &guardian, "author", "2026-07-06T00:00:01Z");
        let letter_id = letter.id();
        let archive = Archive::at(&archive_root);
        std::fs::write(
            archive.store().join(format!("{letter_id}.cbor")),
            letter.to_cbor(),
        )
        .unwrap();
        let h = blake3::hash(body);
        let h_hex = crate::archive::hex(h.as_bytes());
        std::fs::write(archive.bodies().join(format!("{h_hex}.md")), body).unwrap();

        let ask = ExecInputs {
            body: Some(b"please deliver it".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, archive_rite, &archive_rite.steps[0], &ask).unwrap();
        let req = parse_attestation(
            &std::fs::read(step_output(&comms, archive_rite, &archive_rite.steps[0]).unwrap())
                .unwrap(),
        )
        .unwrap();
        let req_tag: String = req
            .id()
            .strip_prefix("comms.attest:")
            .unwrap()
            .chars()
            .take(16)
            .collect();

        let decline = ExecInputs {
            key: Some(comms.join("guardian.json")),
            decision: Some("decline"),
            deliver: Some(letter_id.as_str()),
            ..Default::default()
        };
        let err = execute_step(&comms, archive_rite, &archive_rite.steps[1], &decline).unwrap_err();
        assert!(err.contains("only accompanies a grant"), "{err}");
        assert!(
            !grants.join(&req_tag).exists(),
            "decline must deliver nothing"
        );

        let grant = ExecInputs {
            key: Some(comms.join("guardian.json")),
            decision: Some("grant"),
            deliver: Some(letter_id.as_str()),
            ..Default::default()
        };
        execute_step(&comms, archive_rite, &archive_rite.steps[1], &grant).unwrap();

        let delivered = grants.join(&req_tag).join("letter-session-10.md");
        assert_eq!(std::fs::read(&delivered).unwrap(), body);
        let dec = parse_attestation(
            &std::fs::read(step_output(&comms, archive_rite, &archive_rite.steps[1]).unwrap())
                .unwrap(),
        )
        .unwrap();
        let claim = dec.core.get("c").unwrap();
        assert_eq!(
            claim.get("kind").and_then(Value::as_text),
            Some("archive-grant")
        );
        let note = claim
            .get("content")
            .and_then(|c| c.get("body"))
            .and_then(Value::as_bytes)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap();
        assert!(note.contains(delivered.to_str().unwrap()), "{note}");
        assert!(note.contains(&h_hex), "{note}");
        assert_eq!(dec.signatures[0].by, gid);
    }
}
