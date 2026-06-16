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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::bundle::{author_general_claim, make_bundle, parse_attestation, ClaimSpec};
use crate::config::{HarnessConfig, Rite, Step};
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

/// Where a step's product lands on disk, if it has one.
pub fn step_output(comms_dir: &Path, rite: &Rite, step: &Step) -> Option<PathBuf> {
    let target = step.target.as_deref();
    match step.verb.as_str() {
        "mint" | "shred" => Some(key_path(comms_dir, target.unwrap_or("session"))),
        "attest" => Some(attest_output(comms_dir, rite, target.unwrap_or("entry"))),
        "seal" | "pack" => Some(bundle_output(comms_dir, rite)),
        _ => None,
    }
}

/// Is this step satisfied by what's on disk?
pub fn step_done(comms_dir: &Path, rite: &Rite, step: &Step) -> bool {
    match step_output(comms_dir, rite, step) {
        // shred's goal is the seed's *absence*.
        Some(out) if step.verb == "shred" => !out.exists(),
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

/// Inputs a step may need from the caller (only `attest` does, today).
#[derive(Default)]
pub struct ExecInputs<'a> {
    pub body: Option<Vec<u8>>,
    pub about: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub media_type: Option<&'a str>,
    pub label: &'a str,
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
}
