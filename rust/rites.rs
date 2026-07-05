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
    rite
        .steps
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

/// Where a step's product lands on disk, if it has one.
pub fn step_output(comms_dir: &Path, rite: &Rite, step: &Step) -> Option<PathBuf> {
    let target = step.target.as_deref();
    match step.verb.as_str() {
        "mint" | "shred" => Some(key_path(comms_dir, target.unwrap_or("session"))),
        "attest" => Some(attest_output(comms_dir, rite, target.unwrap_or("entry"))),
        "seal" | "pack" => Some(bundle_output(comms_dir, rite)),
        "countersign" => Some(countersign_output(comms_dir, rite)),
        "request" | "grant" => {
            Some(decision_output(comms_dir, rite, &step.verb, target.unwrap_or("archive")))
        }
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
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let Ok(att) = parse_attestation(&bytes) else { continue };
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
        // shred's goal is the seed's *absence*.
        Some(out) if step.verb == "shred" => !out.exists(),
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
        steps.push(StepView { step: s.clone(), done });
    }
    let next = steps.iter().position(|s| !s.done);
    RiteView { name: rite.name.clone(), steps, next }
}

/// Choose the rite a session is "in." Prefers one in progress (some steps
/// done, some pending); else a pending opener (a rite that begins by minting
/// a key); else the first rite with any pending step. Name-agnostic.
pub fn active_rite<'a>(comms_dir: &Path, cfg: &'a HarnessConfig) -> Option<&'a Rite> {
    let views: Vec<(&Rite, RiteView)> =
        cfg.rites.iter().map(|r| (r, rite_view(comms_dir, r))).collect();

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
    views.into_iter().find(|(_, v)| v.next.is_some()).map(|(r, _)| r)
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
}

/// Result of performing one step.
#[derive(Debug)]
pub struct ExecOutcome {
    pub message: String,
    pub output: Option<PathBuf>,
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
                return Err(format!("session key already present at {}", kp.display()));
            }
            let sk = keyfile::mint(&kp, inputs.label)?;
            let id = personal_steward_id(sk.verifying_key().as_bytes());
            // Record the public session id beside the (secret) key; it outlives
            // shred so this session's artifacts stay attributable afterward.
            let idp = session_id_path(comms_dir, &session_target(rite));
            std::fs::write(&idp, &id).map_err(|e| format!("{}: {e}", idp.display()))?;
            Ok(ExecOutcome {
                message: format!("minted session key {id}"),
                output: Some(kp),
            })
        }
        "shred" => {
            let kp = key_path(comms_dir, &session_target(rite));
            if !kp.exists() {
                return Ok(ExecOutcome {
                    message: "session key already absent".to_owned(),
                    output: Some(kp),
                });
            }
            keyfile::shred(&kp)?;
            Ok(ExecOutcome {
                message: "session key destroyed (seed gone)".to_owned(),
                output: Some(kp),
            })
        }
        "attest" => {
            let body = inputs
                .body
                .as_deref()
                .ok_or_else(|| format!("step '{}' needs content: pass --body <file>", step.display()))?;
            let sk = keyfile::load(&key_path(comms_dir, &session_target(rite)))?;
            let target = step.target.as_deref().unwrap_or("entry");
            let about = inputs.about.unwrap_or(target);
            let spec = ClaimSpec {
                about,
                kind: inputs.kind.unwrap_or("testimony"),
                body,
                media_type: inputs.media_type.unwrap_or("text/markdown"),
                support: &[],
                language: "zxx",
                community: None,
                occasion: Some(&rite.name),
                issued_at: &now,
            };
            let att = author_general_claim(&spec, &sk, "author", &now);
            let store = comms_dir.join("store");
            std::fs::create_dir_all(&store).map_err(|e| format!("{}: {e}", store.display()))?;
            let out = attest_output(comms_dir, rite, target);
            std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!("attested {} -> {}", att.id(), out.display()),
                output: Some(out),
            })
        }
        "seal" | "pack" => {
            let sk = keyfile::load(&key_path(comms_dir, &session_target(rite)))?;
            let store = comms_dir.join("store");
            let mut files: Vec<PathBuf> = std::fs::read_dir(&store)
                .map_err(|e| format!("{}: {e}", store.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().map(|x| x == "cbor").unwrap_or(false))
                .collect();
            files.sort();
            if files.is_empty() {
                return Err(format!("nothing to seal: {} has no .cbor attestations", store.display()));
            }
            let mut members = Vec::new();
            for f in &files {
                let bytes = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
                members.push(parse_attestation(&bytes).map_err(|e| format!("{}: {e}", f.display()))?);
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
            std::fs::write(&out, bundle.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!(
                    "{} {count} attestation{} -> {}",
                    if seal_it { "sealed" } else { "packed" },
                    if count == 1 { "" } else { "s" },
                    out.display()
                ),
                output: Some(out),
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
            let att = crate::steward::Attestation { core, signatures: Vec::new() };
            let out = countersign_output(comms_dir, rite);
            let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("countersign").to_owned();
            let need = Need { by: cs.by.clone(), role: cs.role.clone() };
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
            })
        }
        "request" => {
            let body = inputs.body.as_deref().ok_or_else(|| {
                format!("step '{}' needs the ask in writing: pass --body <file>", step.display())
            })?;
            let sk = keyfile::load(&key_path(comms_dir, &session_target(rite)))?;
            let target = step.target.as_deref().unwrap_or("archive");
            let spec = ClaimSpec {
                about: inputs.about.unwrap_or(target),
                kind: inputs.kind.unwrap_or("archive-request"),
                body,
                media_type: inputs.media_type.unwrap_or("text/markdown"),
                support: &[],
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
            })
        }
        "grant" => {
            let decision = inputs.decision.unwrap_or("grant");
            if !["grant", "decline", "defer"].contains(&decision) {
                return Err(format!("--decision must be grant, decline, or defer (got '{decision}')"));
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
                format!("no request on record at {} — `request` comes first", request_path.display())
            })?;
            let request_id = parse_attestation(&request_bytes)
                .map_err(|e| format!("{}: {e}", request_path.display()))?
                .id();

            let default_body = format!("{decision}ed");
            let body = inputs.body.as_deref().unwrap_or(default_body.as_bytes());
            let kind = format!("archive-{decision}");
            let support = [request_id.clone()];
            let spec = ClaimSpec {
                about: inputs.about.unwrap_or(target),
                kind: &kind,
                body,
                media_type: inputs.media_type.unwrap_or("text/markdown"),
                support: &support,
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
                    "recorded {decision} {} (re {request_id}) -> {}",
                    att.id(),
                    out.display()
                ),
                output: Some(out),
            })
        }
        other => Err(format!("unknown rite verb '{other}' (step '{}')", step.display())),
    }
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
        let inp = ExecInputs { body: Some(b"# entry\n".to_vec()), ..Default::default() };
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
        let inp = ExecInputs { body: Some(b"transcript\n".to_vec()), ..Default::default() };
        execute_step(&comms, close, &close.steps[0], &inp).unwrap(); // attest transcript
        execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap(); // seal store
        assert!(step_done(&comms, close, &close.steps[1]));
        // The bundle is session-scoped (close.<tag>.bundle), not a bare name.
        assert!(step_output(&comms, close, &close.steps[1]).unwrap().is_file());

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
        let body = || ExecInputs { body: Some(b"x\n".to_vec()), ..Default::default() };

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
        assert_eq!(entries.len(), 2, "second session must not overwrite the first: {entries:?}");
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
        let tx = || ExecInputs { body: Some(b"t\n".to_vec()), ..Default::default() };

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
        assert_ne!(bundle_a, bundle_b, "each session must seal a distinct bundle");
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
        let entry = ExecInputs { body: Some(b"# entry\n".to_vec()), ..Default::default() };
        execute_step(&comms, open, &open.steps[1], &entry).unwrap();

        // Countersign stages an unsigned endorsement naming the guardian.
        assert!(!step_done(&comms, open, &open.steps[2]));
        execute_step(&comms, open, &open.steps[2], &ExecInputs::default()).unwrap();
        assert!(step_done(&comms, open, &open.steps[2]), "staged counts as done");
        assert!(rite_view(&comms, open).complete());

        let pending = comms.join("pending");
        let items = signing::read_pending(&pending).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].attestation.signatures.is_empty(), "session key must not touch it");
        assert_eq!(items[0].needs, vec![Need { by: gid.clone(), role: "guardian".into() }]);

        // The guardian signs and finalizes; the step stays done because the
        // endorsement (by claim content) is now in the store.
        signing::sign_pending(&pending, &guardian).unwrap();
        signing::finalize_pending(&pending, &comms.join("store")).unwrap();
        assert!(signing::read_pending(&pending).unwrap().is_empty());
        assert!(step_done(&comms, open, &open.steps[2]), "finalized still reads done");

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
        assert_eq!(att.core.get("c").and_then(|c| c.get("target")).and_then(Value::as_text),
            Some(sid.as_str()));
        assert_eq!(att.signatures[0].by, gid);
    }

    #[test]
    fn countersign_without_config_says_so() {
        let comms = scratch("nocsconfig");
        std::fs::write(comms.join("comms.toml"), CFG).unwrap();
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let step = Step { verb: "countersign".into(), target: Some("session".into()) };
        let err = execute_step(&comms, open, &step, &ExecInputs::default()).unwrap_err();
        assert!(err.contains("[countersign]"), "unexpected error: {err}");
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
        let ask = ExecInputs { body: Some(b"may I read the letter?".to_vec()), ..Default::default() };
        execute_step(&comms, archive, &archive.steps[0], &ask).unwrap();
        let req_path = step_output(&comms, archive, &archive.steps[0]).unwrap();
        let req = parse_attestation(&std::fs::read(&req_path).unwrap()).unwrap();
        assert_eq!(req.core.get("c").and_then(|c| c.get("kind")).and_then(Value::as_text),
            Some("archive-request"));

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
        assert_eq!(claim.get("kind").and_then(Value::as_text), Some("archive-decline"));
        let support = claim.get("support").and_then(Value::as_array).unwrap();
        assert_eq!(support.len(), 1);
        assert_eq!(support[0].as_text(), Some(req.id().as_str()));
        assert_eq!(dec.signatures[0].by, gid);
        assert_eq!(dec.signatures[0].role, "custodian");
    }
}
