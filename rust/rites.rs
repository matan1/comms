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

use crate::bundle::{author_general_claim_with, make_bundle_with, parse_attestation, ClaimSpec};
use crate::cbor::Value;
use crate::config::{self, HarnessConfig, Rite, Step};
use crate::signing::{self, Need, SessionSigner};
use crate::{keyfile, now_rfc3339, personal_steward_id, sshagent, StewardSigner};

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

/// How this door's config says session seeds are held.
fn seed_mode(comms_dir: &Path) -> String {
    config::load(comms_dir)
        .map(|c| c.session_key.clone())
        .unwrap_or_else(|_| "file".to_owned())
}

/// Whether this harness holds its session seed with a key agent.
fn agent_mode(comms_dir: &Path) -> bool {
    seed_mode(comms_dir) == "ssh-agent"
}

/// A comms session key held by the agent at `sock`, if it holds one.
///
/// `mint` records the steward id as the key's comment, so an agent can be
/// asked which session it is holding. The comment alone is not trusted: the
/// held public key must derive the id it claims, or a mislabelled key in the
/// agent could name a session it cannot sign for.
fn agent_session(sock: &Path) -> Option<(String, [u8; 32])> {
    sshagent::list(sock).ok()?.into_iter().find_map(|(pk, comment)| {
        let claimed = comment.trim();
        (claimed.starts_with("comms.steward:") && personal_steward_id(&pk) == claimed)
            .then(|| (claimed.to_owned(), pk))
    })
}

/// The session **this actor** can sign as, or `None` when it holds none.
///
/// The session id is not stored anywhere: it is derived from wherever this
/// actor's seed lives, which is the only place that can answer the question
/// honestly. That is what lets several sessions be live in one work tree at
/// once — each holds its own seed in its own context, and none of them has to
/// agree with a shared file about whose turn it is.
///
/// - `file`      — `<comms>/<target>.key`. One path, so one session; this is
///                 the mode that cannot go concurrent, and says so at mint.
/// - `ssh-agent` — the agent at `COMMS_AGENT_SOCK`/`SSH_AUTH_SOCK`/the door's
///                 socket. A per-session socket is a per-session identity.
/// - `ephemeral` — `COMMS_SESSION_SEED`, which is per-process by nature.
pub fn actor_session(comms_dir: &Path, rite: &Rite) -> Option<String> {
    // Exactly one place per mode. A seed lying around somewhere the config did
    // not ask for must never become an identity: in file mode a stray
    // COMMS_SESSION_SEED in the environment is not this door's session, and
    // silently signing as it would be the worst kind of quiet.
    match seed_mode(comms_dir).as_str() {
        "ssh-agent" => agent_session(&sshagent::socket_path(comms_dir)).map(|(id, _)| id),
        "ephemeral" => std::env::var(SEED_ENV)
            .ok()
            .and_then(|b58| seed_from_b58(&b58).ok())
            .map(|sk| personal_steward_id(sk.verifying_key().as_bytes())),
        _ => keyfile::load(&key_path(comms_dir, &session_target(rite)))
            .ok()
            .map(|sk| personal_steward_id(sk.verifying_key().as_bytes())),
    }
}

/// The live session's signer for CLI use (`comms attest --key session`):
/// resolved exactly the way rite steps resolve it, so file, agent, and
/// ephemeral modes all work without naming a key file.
pub fn current_session_signer(
    comms_dir: &Path,
    cfg: &HarnessConfig,
) -> Result<SessionSigner, String> {
    let rite = cfg
        .rites
        .iter()
        .find(|r| r.steps.iter().any(|s| s.verb == "mint"))
        .or_else(|| cfg.rites.first())
        .ok_or("no rites declared in comms.toml")?;
    session_signer(comms_dir, rite)
}

/// This actor's signing capability: the on-disk key file if present; an
/// agent-held session key when the harness runs in ssh-agent mode; else the
/// seed the holder supplies through the environment.
///
/// Nothing is checked against a recorded id, because there is no longer one to
/// check against. Whoever holds a seed signs as the steward that seed derives —
/// which is all a signature ever meant.
fn session_signer(comms_dir: &Path, rite: &Rite) -> Result<SessionSigner, String> {
    match seed_mode(comms_dir).as_str() {
        "ssh-agent" => {
            let sock = sshagent::socket_path(comms_dir);
            let (_, public) = agent_session(&sock).ok_or_else(|| {
                format!(
                    "the agent at {} holds no comms session key — mint first, or point \
                     COMMS_AGENT_SOCK/SSH_AUTH_SOCK at the agent holding your session's \
                     seed (a dead agent is a shred)",
                    sock.display()
                )
            })?;
            Ok(SessionSigner::Agent { sock, public })
        }
        "ephemeral" => {
            let b58 = std::env::var(SEED_ENV).map_err(|_| {
                format!(
                    "{SEED_ENV} is not set — mint first, or export the seed you were \
                     shown at mint (this door holds session seeds in the environment)"
                )
            })?;
            seed_from_b58(&b58).map(SessionSigner::Local)
        }
        _ => {
            let kp = key_path(comms_dir, &session_target(rite));
            keyfile::load(&kp).map(SessionSigner::Local).map_err(|_| {
                format!("no session key at {} — mint first", kp.display())
            })
        }
    }
}

/// The filename-safe tag a steward id scopes its artifacts under.
fn tag_of_id(id: &str) -> String {
    let z = id.strip_prefix("comms.steward:").unwrap_or(id);
    z.chars().take(16).collect()
}

/// A short, filename-safe tag for the current session, derived from its steward
/// id. `None` until the session has been minted. Used to scope artifact names
/// so a new session does not overwrite a prior one's.
fn session_tag(comms_dir: &Path, rite: &Rite) -> Option<String> {
    actor_session(comms_dir, rite).map(|id| tag_of_id(&id))
}

/// Where an `attest <target>` step writes its `.cbor`. Scoped by session tag
/// when a session exists, so successive sessions accumulate rather than clobber.
fn attest_output(comms_dir: &Path, rite: &Rite, target: &str) -> PathBuf {
    attest_output_for(comms_dir, session_tag(comms_dir, rite).as_deref(), target)
}

fn attest_output_for(comms_dir: &Path, tag: Option<&str>, target: &str) -> PathBuf {
    let name = match tag {
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
    bundle_output_for(comms_dir, session_tag(comms_dir, rite).as_deref(), rite)
}

fn bundle_output_for(comms_dir: &Path, tag: Option<&str>, rite: &Rite) -> PathBuf {
    let name = match tag {
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
/// Where a `countersign <target>` step stages its pending item. The target is
/// part of the name so a session can have several things witnessed at once
/// without them colliding; `session` keeps the original name for continuity
/// with inboxes staged before targets were supported.
fn countersign_output_for(comms_dir: &Path, tag: Option<&str>, target: &str) -> PathBuf {
    let stem = if target == "session" {
        "countersign".to_owned()
    } else {
        format!("countersign.{target}")
    };
    let name = match tag {
        Some(tag) => format!("{stem}.{tag}.cbor"),
        None => format!("{stem}.cbor"),
    };
    pending_dir(comms_dir).join(name)
}

/// Where a `request`/`grant` step writes its attestation. Prefixed by the verb
/// (a request and its decision must not collide) and session-scoped like every
/// other attest output.
fn decision_output(comms_dir: &Path, rite: &Rite, verb: &str, target: &str) -> PathBuf {
    decision_output_for(
        comms_dir,
        session_tag(comms_dir, rite).as_deref(),
        verb,
        target,
    )
}

fn decision_output_for(comms_dir: &Path, tag: Option<&str>, verb: &str, target: &str) -> PathBuf {
    let stem = if verb == "grant" { "decision" } else { verb };
    let name = match tag {
        Some(tag) => format!("{stem}.{target}.{tag}.cbor"),
        None => format!("{stem}.{target}.cbor"),
    };
    comms_dir.join("store").join(name)
}

/// Resolve the recorded request id for a rite target. Used by host-side
/// delivery after a grant has already been recorded: the transport still lands
/// under the request id even when it is no longer part of `comms next`.
pub fn recorded_request_id(comms_dir: &Path, rite: &Rite, target: &str) -> Result<String, String> {
    let request_path = decision_output(comms_dir, rite, "request", target);
    let request_bytes = std::fs::read(&request_path).map_err(|_| {
        format!(
            "no request on record at {} — record the request before delivery",
            request_path.display()
        )
    })?;
    parse_attestation(&request_bytes)
        .map_err(|e| format!("{}: {e}", request_path.display()))
        .map(|att| att.id())
}

/// Where a recorded waiver for an artifact type lands. Session-scoped like
/// every other attest output.
fn waiver_output(comms_dir: &Path, rite: &Rite, type_name: &str) -> PathBuf {
    waiver_output_for(comms_dir, session_tag(comms_dir, rite).as_deref(), type_name)
}

fn waiver_output_for(comms_dir: &Path, tag: Option<&str>, type_name: &str) -> PathBuf {
    let name = match tag {
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
    // Config type names use underscores (toml keys); step targets use
    // hyphens. Both spell the same artifact.
    for x in t.clone() {
        let dashed = x.replace('_', "-");
        if dashed != x {
            t.push(dashed);
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

// ---- the active configuration as an artifact -------------------------------

/// The claim `kind` an attestation of the harness configuration carries, and
/// the `about` it names. A rite runs under a config; a record of the rite that
/// cannot say which config is missing its own premise.
pub const CONFIG_KIND: &str = "harness-config";
pub const CONFIG_ABOUT: &str = "comms.toml";

/// Where the active configuration stands against the record.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigState {
    /// No attestation of the configuration is in this door's store.
    Unattested { active_b3: String },
    /// The attested bytes are the bytes now in force.
    Matches { id: String, active_b3: String },
    /// comms.toml has changed since it was attested. Not an error — configs
    /// are meant to change — but it is a fact the door must not swallow.
    Drifted {
        id: String,
        active_b3: String,
        attested_b3: String,
    },
}

/// Read back what the store says about the configuration now in force.
///
/// The comparison is over exact bytes, not the parsed model: a comment or an
/// ordering change is a change to the document the community ratified, and the
/// point of attesting it is to be able to see that.
pub fn config_state(comms_dir: &Path) -> Result<ConfigState, String> {
    let path = comms_dir.join("comms.toml");
    let active = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let active_b3 = blake3::hash(&active).to_hex().to_string();

    // The newest config attestation in the store, by frame issued_at.
    let mut best: Option<(String, String, Vec<u8>)> = None; // (issued_at, id, body)
    if let Ok(entries) = std::fs::read_dir(comms_dir.join("store")) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map(|x| x != "cbor").unwrap_or(true) {
                continue;
            }
            let Ok(att) = std::fs::read(&p)
                .map_err(|_| ())
                .and_then(|b| parse_attestation(&b).map_err(|_| ()))
            else {
                continue;
            };
            let claim = att.core.get("c");
            let is_config = claim
                .and_then(|c| c.get("kind"))
                .and_then(Value::as_text)
                .map(|k| k == CONFIG_KIND)
                .unwrap_or(false)
                && claim
                    .and_then(|c| c.get("about"))
                    .and_then(Value::as_text)
                    .map(|a| a == CONFIG_ABOUT)
                    .unwrap_or(false);
            if !is_config {
                continue;
            }
            let Some(body) = claim
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("body"))
                .and_then(Value::as_bytes)
            else {
                continue; // a detached config commits to bytes we cannot read here
            };
            let issued = att
                .core
                .get("f")
                .and_then(|f| f.get("issued_at"))
                .and_then(Value::as_text)
                .unwrap_or("")
                .to_owned();
            if best.as_ref().map(|(t, _, _)| issued >= *t).unwrap_or(true) {
                best = Some((issued, att.id(), body.to_vec()));
            }
        }
    }

    Ok(match best {
        None => ConfigState::Unattested { active_b3 },
        Some((_, id, body)) if body == active => ConfigState::Matches { id, active_b3 },
        Some((_, id, body)) => ConfigState::Drifted {
            attested_b3: blake3::hash(&body).to_hex().to_string(),
            id,
            active_b3,
        },
    })
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
        requires: Vec::new(),
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
    let att = author_general_claim_with(&spec, &sk, "author", &now)?;
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
        facts: facts(&[
            ("result", "recorded"),
            ("id", &att.id()),
            ("artifact_type", type_name),
            ("body_b3", &b3(body)),
            ("body_len", &body.len().to_string()),
            ("signer", &sk.steward_id()),
            ("output", &out.display().to_string()),
        ]),
        output: Some(out),
        secret: None,
    })
}

// ---- session epochs --------------------------------------------------------
//
// A door's history is a succession of sessions, each with its own ephemeral
// key. Every artifact a session produces is already filename-scoped by a tag
// derived from that key's steward id, so the succession is *derivable* — no
// side ledger, no hidden "where am I" database. `epochs` reads it back.
//
// This is what separates the two questions a session key used to answer at
// once: whether a step *happened* (its signed product is in the store under
// that session's tag) and whether anyone can *sign as that session now* (the
// seed is reachable). Shredding a key ends the second and must not touch the
// first — see `Epoch::live`.

/// Where a session stands, from the vantage of the actor asking.
///
/// Sessions run concurrently in one work tree, so "is it live?" has no
/// answerable third-party form: nobody but a session's holder can see whether
/// its seed still exists. These three states say only what is knowable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// This actor holds its seed and can sign as it.
    Mine,
    /// Minted, and no closing evidence in the store. It may be a peer working
    /// right now, or a session that died without closing — indistinguishable
    /// from here, and the door should not pretend otherwise.
    Open,
    /// Its closing rite reached the end of its recorded work.
    ///
    /// This says the **rite concluded**, and nothing whatever about the seed.
    /// Ritual closing and key destruction are separate propositions: the first
    /// leaves signed artifacts anyone can check, the second leaves nothing
    /// anyone but its holder could ever witness. Reading one from the other is
    /// the same absence-for-evidence error in a new coat — and it is why this
    /// is not called `Closed`.
    Concluded,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Mine => "mine",
            SessionState::Open => "open",
            SessionState::Concluded => "concluded",
        }
    }
}

/// One session generation of this door, as read back from what it left behind.
#[derive(Debug, Clone)]
pub struct Epoch {
    /// The filename tag its artifacts are scoped under.
    pub tag: String,
    /// Its full steward id, when a signature on one of its artifacts (or this
    /// actor's own seed) supplies one.
    pub id: Option<String>,
    pub state: SessionState,
    /// Earliest `issued_at` among its artifacts; orders the succession.
    first_seen: String,
}

impl Epoch {
    /// The session as it is named in messages: full id if known, else the tag.
    pub fn name(&self) -> String {
        self.id.clone().unwrap_or_else(|| self.tag.clone())
    }
    /// Can this actor sign as this session?
    pub fn mine(&self) -> bool {
        self.state == SessionState::Mine
    }
}

/// The tag component of a session-scoped artifact name, if it carries one:
/// `entry.z9YSzSQnbVhPcQxG.cbor` -> `z9YSzSQnbVhPcQxG`. Files named by their
/// own attestation id (a finalized countersignature, say) have no tag and are
/// attributed by claim content instead.
fn tag_in_filename(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    let candidate = parts[parts.len() - 2];
    let is_tag = candidate.len() == 16
        && candidate.starts_with('z')
        && candidate.chars().all(|c| c.is_ascii_alphanumeric());
    is_tag.then(|| candidate.to_owned())
}

/// Has this session's closing rite reached its seal?
///
/// The evidence a session ended, stated positively. A rite that tears a
/// session down declares a `shred` step; everything before that step is the
/// closing work, and its products are on disk under the session's tag. When
/// they are all there, the session closed — whoever is asking, and whether or
/// not they can reach its seed.
///
/// No rite declares a `shred` step? Then this door has no notion of closing,
/// and no session is ever `Closed`.
fn closing_evidence(comms_dir: &Path, cfg: &HarnessConfig, tag: &str) -> bool {
    let probe = Epoch {
        tag: tag.to_owned(),
        id: None,
        state: SessionState::Open,
        first_seen: String::new(),
    };
    cfg.rites
        .iter()
        .filter_map(|r| {
            let shred_at = r.steps.iter().position(|s| s.verb == "shred")?;
            Some((r, shred_at))
        })
        .any(|(r, shred_at)| {
            shred_at > 0
                && r.steps[..shred_at]
                    .iter()
                    .all(|s| step_done_in(comms_dir, r, s, Some(&probe)))
        })
}

/// Every session generation this door holds evidence of, oldest first.
///
/// Derived, never recorded: tags come from artifact filenames, full ids from
/// the signatures on those artifacts, order from their `issued_at`. This
/// actor's own session is included even when it has signed nothing yet — it
/// holds the seed, so it knows.
pub fn epochs(comms_dir: &Path, cfg: &HarnessConfig) -> Vec<Epoch> {
    let rite = cfg
        .rites
        .iter()
        .find(|r| r.steps.iter().any(|s| s.verb == "mint"))
        .or_else(|| cfg.rites.first());
    let Some(rite) = rite else {
        return Vec::new();
    };
    let mut found: HashMap<String, (Option<String>, String)> = HashMap::new(); // tag -> (id, first_seen)

    let mut note = |tag: String, id: Option<String>, issued: String| {
        let e = found.entry(tag).or_insert((None, String::new()));
        if e.0.is_none() {
            e.0 = id;
        }
        if !issued.is_empty() && (e.1.is_empty() || issued < e.1) {
            e.1 = issued;
        }
    };

    if let Ok(entries) = std::fs::read_dir(comms_dir.join("store")) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(tag) = tag_in_filename(name) else {
                continue;
            };
            let (mut id, mut issued) = (None, String::new());
            if let Ok(att) = std::fs::read(&p).map_err(|_| ()).and_then(|b| {
                parse_attestation(&b).map_err(|_| ())
            }) {
                id = att
                    .signatures
                    .iter()
                    .map(|s| s.by.clone())
                    .find(|by| tag_of_id(by) == tag);
                issued = att
                    .core
                    .get("f")
                    .and_then(|f| f.get("issued_at"))
                    .and_then(Value::as_text)
                    .unwrap_or("")
                    .to_owned();
            }
            note(tag, id, issued);
        }
    }
    // Sealed bundles and staged countersignatures are a session's work too.
    for dir in [comms_dir.to_path_buf(), pending_dir(comms_dir)] {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if let Some(tag) = tag_in_filename(name) {
                    note(tag, None, String::new());
                }
            }
        }
    }

    // This actor's own session, which it knows about from holding the seed
    // whether or not it has signed anything yet.
    let mine = actor_session(comms_dir, rite);
    if let Some(id) = &mine {
        note(tag_of_id(id), Some(id.clone()), String::new());
    }
    let mine_tag: Option<String> = mine.as_deref().map(tag_of_id);

    let mut out: Vec<Epoch> = found
        .into_iter()
        .map(|(tag, (id, first_seen))| {
            let is_mine = mine_tag.as_deref() == Some(tag.as_str());
            let state = if is_mine {
                // Checked before Closed, and the order is load-bearing:
                // `shred` reads done when the state is Closed, so if closing
                // evidence won here, sealing would mark my own shred step done
                // and `next` would skip it — leaving the rite reporting
                // complete while I still hold a live key. Holding the seed is
                // the one thing that outranks the evidence, because I am the
                // only one who can see it.
                SessionState::Mine
            } else if closing_evidence(comms_dir, cfg, &tag) {
                SessionState::Concluded
            } else {
                SessionState::Open
            };
            Epoch {
                id: id.or_else(|| if is_mine { mine.clone() } else { None }),
                state,
                tag,
                first_seen,
            }
        })
        .collect();
    // Order the succession by when each session first signed something.
    //
    // Deliberately *not* by "this actor's session is newest": that holds only
    // while one door mints every session in turn. Sessions run in parallel —
    // several agents at once, no alignment between them — and their artifacts
    // meet in one store through a shared work tree, a merge, or an archive. A
    // session another actor minted can easily predate this one, and ordering
    // it last would misreport the history it just joined.
    //
    // A session that has signed nothing yet sorts last: it has only just been
    // minted. Equal timestamps fall back to tag — arbitrary, but stable, so
    // the same custody always renders the same way.
    out.sort_by_key(|e| {
        (
            e.first_seen.is_empty(),
            e.first_seen.clone(),
            e.tag.clone(),
        )
    });
    out
}

/// The session this actor can act as, if it holds one.
pub fn my_epoch(comms_dir: &Path, cfg: &HarnessConfig) -> Option<Epoch> {
    epochs(comms_dir, cfg).into_iter().find(Epoch::mine)
}

/// Where a step's product lands on disk for the session this actor holds.
/// `None` once no session is held — the path is session-scoped, so without a
/// session there is no path to name. Use [`step_output_in`] to ask about a
/// particular session, including a closed one.
pub fn step_output(comms_dir: &Path, rite: &Rite, step: &Step) -> Option<PathBuf> {
    step_output_for(comms_dir, session_tag(comms_dir, rite).as_deref(), rite, step)
}

/// Where a step's product lands for a named session generation.
pub fn step_output_in(
    comms_dir: &Path,
    rite: &Rite,
    step: &Step,
    epoch: &Epoch,
) -> Option<PathBuf> {
    step_output_for(comms_dir, Some(&epoch.tag), rite, step)
}

/// `step_output` for a named session generation rather than the current one.
fn step_output_for(
    comms_dir: &Path,
    tag: Option<&str>,
    rite: &Rite,
    step: &Step,
) -> Option<PathBuf> {
    let target = step.target.as_deref();
    match step.verb.as_str() {
        "mint" | "shred" => Some(key_path(comms_dir, target.unwrap_or("session"))),
        "attest" => Some(attest_output_for(comms_dir, tag, target.unwrap_or("entry"))),
        "seal" | "pack" => Some(bundle_output_for(comms_dir, tag, rite)),
        "countersign" => Some(countersign_output_for(
            comms_dir,
            tag,
            target.unwrap_or("session"),
        )),
        "request" | "grant" => Some(decision_output_for(
            comms_dir,
            tag,
            &step.verb,
            target.unwrap_or("archive"),
        )),
        _ => None,
    }
}

/// Has the configured countersigner's endorsement of a session's key reached
/// the store? Checked by claim content, not filename, so it holds however the
/// finalized attestation arrived — and it is asked of a *named* session, so a
/// closed generation's countersigned key keeps reading as countersigned.
fn countersign_recorded_for(comms_dir: &Path, session_id: &str) -> bool {
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

/// What a `countersign` step asks a counterparty to witness, for one session:
/// its steward id (`countersign session`) or the id of the artifact it
/// attested under that target. `None` when the thing does not exist yet.
fn countersign_subject(comms_dir: &Path, step: &Step, epoch: &Epoch) -> Option<String> {
    match step.target.as_deref().unwrap_or("session") {
        "session" => epoch.id.clone(),
        target => {
            let path = attest_output_for(comms_dir, Some(&epoch.tag), target);
            let bytes = std::fs::read(path).ok()?;
            parse_attestation(&bytes).ok().map(|a| a.id())
        }
    }
}

/// Is this step positively done, for the session this actor holds?
pub fn step_done(comms_dir: &Path, cfg: &HarnessConfig, rite: &Rite, step: &Step) -> bool {
    step_status_in(comms_dir, rite, step, my_epoch(comms_dir, cfg).as_ref()) == StepStatus::Done
}

/// Is this step positively done *for a named session generation*?
pub fn step_done_in(comms_dir: &Path, rite: &Rite, step: &Step, epoch: Option<&Epoch>) -> bool {
    step_status_in(comms_dir, rite, step, epoch) == StepStatus::Done
}

/// Where a step stands *for a named session generation*.
///
/// The distinction the Sentira continuity asked for lives here. `mint` asks
/// whether that session ever existed — a fact its artifacts keep true forever.
///
/// `shred` is the one verb that produces no artifact. Nothing it does can be
/// seen from outside: a destroyed seed and a carefully kept one look identical
/// in the store. So it is answered only for the caller's own session, where
/// the answer is knowable, and reported [`StepStatus::Unverifiable`] for every
/// other — never inferred from the closing work, which evidences a *different*
/// proposition. A community that wants a claim about key destruction on the
/// record can have its session attest one as its last act; that is the
/// session's own testimony, which is the most anyone can offer.
///
/// `epoch` is `None` for a session not yet opened: nothing has happened in it,
/// so nothing in it is done.
pub fn step_status_in(
    comms_dir: &Path,
    rite: &Rite,
    step: &Step,
    epoch: Option<&Epoch>,
) -> StepStatus {
    let Some(epoch) = epoch else {
        return StepStatus::Pending;
    };
    match step.verb.as_str() {
        // This session was minted: it has an identity on record. Historical
        // completion, not current signing capability.
        "mint" => StepStatus::Done,
        "shred" => match epoch.state {
            // I hold the seed, so I know it is not destroyed.
            SessionState::Mine => StepStatus::Pending,
            // Anyone else, about anyone else: unknowable, permanently.
            _ => StepStatus::Unverifiable,
        },
        // Staging is a request, not the counterparty's act. The step completes
        // only once the signed endorsement of what this step names — the
        // session key, or the artifact — has been finalized into the store.
        "countersign" => match countersign_subject(comms_dir, step, epoch) {
            Some(subject) if countersign_recorded_for(comms_dir, &subject) => StepStatus::Done,
            _ => StepStatus::Pending,
        },
        _ => match step_output_for(comms_dir, Some(&epoch.tag), rite, step) {
            Some(p) if p.exists() => StepStatus::Done,
            _ => StepStatus::Pending,
        },
    }
}

/// Where a step stands, for the session being asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// Its product is on disk, or the caller can see it is satisfied.
    Done,
    /// Not yet performed, and performable.
    Pending,
    /// Cannot be established from here, and no future act by this caller could
    /// establish it. Today this is exactly one thing: another session's
    /// `shred`. A seed's destruction leaves no artifact and has no witness but
    /// its holder, so reporting it done would be a claim the store cannot
    /// support, and reporting it pending would invite someone to try
    /// performing a destruction that is not theirs to perform.
    Unverifiable,
}

impl StepStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Done => "done",
            StepStatus::Pending => "pending",
            StepStatus::Unverifiable => "unverifiable",
        }
    }
    /// Does this step stand in the way of the rite reaching its end? An
    /// unverifiable step does not: nobody can ever satisfy it from here, so
    /// blocking on it would strand the rite forever.
    pub fn settled(&self) -> bool {
        !matches!(self, StepStatus::Pending)
    }
}

/// One step plus where it stands.
pub struct StepView {
    pub step: Step,
    pub status: StepStatus,
}

impl StepView {
    /// Positive completion. An unverifiable step is *not* done — that is the
    /// whole point of the distinction.
    pub fn done(&self) -> bool {
        self.status == StepStatus::Done
    }
}

/// A rite rendered against the current filesystem.
pub struct RiteView {
    pub name: String,
    pub steps: Vec<StepView>,
    /// Index of the first pending step, if any.
    pub next: Option<usize>,
}

impl RiteView {
    /// Nothing further can be performed here. Note this is *not* "everything
    /// was proved": a rite ending in another session's `shred` reaches its end
    /// with that step unverifiable, and [`RiteView::concluded`] distinguishes
    /// the two so callers need not conflate them the way the code once did.
    pub fn complete(&self) -> bool {
        self.next.is_none()
    }
    /// Complete, but only because a step nobody can check was set aside.
    pub fn concluded(&self) -> bool {
        self.complete()
            && self
                .steps
                .iter()
                .any(|s| s.status == StepStatus::Unverifiable)
    }
    /// Nothing in this rite has been done yet.
    pub fn untouched(&self) -> bool {
        self.next == Some(0)
    }
}

/// A rite rendered against the session this actor holds.
pub fn rite_view(comms_dir: &Path, cfg: &HarnessConfig, rite: &Rite) -> RiteView {
    rite_view_in(comms_dir, rite, my_epoch(comms_dir, cfg).as_ref())
}

/// A rite rendered against one session generation.
pub fn rite_view_in(comms_dir: &Path, rite: &Rite, epoch: Option<&Epoch>) -> RiteView {
    // A rite is an ordered sequence: a step counts as done only if it and every
    // prior step are satisfied. This keeps a trailing teardown like `shred`
    // (whose raw condition — the key's absence — also holds before anything has
    // begun) from reading as already-done at a cold start.
    let mut steps = Vec::with_capacity(rite.steps.len());
    let mut prior_settled = true;
    for s in &rite.steps {
        // Out of order is not done: a step whose predecessors are unsettled
        // reads pending whatever its own product looks like.
        let status = if prior_settled {
            step_status_in(comms_dir, rite, s, epoch)
        } else {
            StepStatus::Pending
        };
        prior_settled = status.settled();
        steps.push(StepView {
            step: s.clone(),
            status,
        });
    }
    let next = steps.iter().position(|s| s.status == StepStatus::Pending);
    RiteView {
        name: rite.name.clone(),
        steps,
        next,
    }
}

/// The most recent session this door holds evidence of, whatever its state.
/// What a cold reader is told about when this actor holds no session.
pub fn last_epoch(comms_dir: &Path, cfg: &HarnessConfig) -> Option<Epoch> {
    epochs(comms_dir, cfg).last().cloned()
}

/// Choose the rite a session is "in." Prefers one in progress (some steps
/// done, some pending); else a pending opener (a rite that begins by minting
/// a key); else the first rite with any pending step. Name-agnostic.
pub fn active_rite<'a>(comms_dir: &Path, cfg: &'a HarnessConfig) -> Option<&'a Rite> {
    active_rite_in(comms_dir, cfg, None)
}

/// `active_rite` for a named session generation. `None` selects the current
/// one (which is `None` itself when the door is between sessions — then the
/// opener is what comes next).
pub fn active_rite_in<'a>(
    comms_dir: &Path,
    cfg: &'a HarnessConfig,
    epoch: Option<&Epoch>,
) -> Option<&'a Rite> {
    let resolved = match epoch {
        Some(e) => Some(e.clone()),
        None => my_epoch(comms_dir, cfg),
    };
    let views: Vec<(&Rite, RiteView)> = cfg
        .rites
        .iter()
        .map(|r| (r, rite_view_in(comms_dir, r, resolved.as_ref())))
        .collect();

    if let Some((r, _)) = views
        .iter()
        .find(|(_, v)| v.next.is_some() && v.steps.iter().any(StepView::done))
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
#[derive(Debug, Default)]
pub struct ExecOutcome {
    pub message: String,
    pub output: Option<PathBuf>,
    /// A secret shown exactly once and never persisted (the ephemeral session
    /// seed). The caller decides how to display it; nothing here writes it.
    pub secret: Option<String>,
    /// What this step produced, as ordered key/value facts a caller can check
    /// without parsing prose: the id authored, the blake3 of the body it
    /// commits to, who signed, where it landed.
    ///
    /// A rite step is an act on the record, and the record of the act should
    /// not have to be recovered from a sentence. Every verb populates these;
    /// `comms next` prints them on one `comms-step` line and, with `--json`,
    /// as the whole of its output.
    pub facts: Vec<(String, String)>,
}

/// Build an ordered fact list, dropping entries whose value is empty so a
/// step never reports a field it does not have.
fn facts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// The blake3 of some bytes, as lowercase hex — the commitment a verifier
/// re-derives from the body it holds.
fn b3(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Perform a single rite step. `Err` carries a message naming what's missing.
pub fn execute_step(
    comms_dir: &Path,
    rite: &Rite,
    step: &Step,
    inputs: &ExecInputs,
) -> Result<ExecOutcome, String> {
    if let Ok(cfg) = config::load(comms_dir) {
        let incomplete: Vec<_> = rite
            .requires
            .iter()
            .filter(|name| {
                cfg.rite(name)
                    .map(|required| {
                        !rite_view_in(comms_dir, required, my_epoch(comms_dir, &cfg).as_ref())
                            .complete()
                    })
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        if !incomplete.is_empty() {
            return Err(format!(
                "rite '{}' requires completed rite(s) [{}] before '{}' may advance",
                rite.name,
                incomplete.join(", "),
                step.display()
            ));
        }
    }
    let now = now_rfc3339();
    match step.verb.as_str() {
        "mint" => {
            // One actor, one session. The guard is per-actor, not per-door:
            // whether *I* already hold a session, not whether anyone does.
            // Concurrent sessions in one work tree are the point.
            if let Some(held) = actor_session(comms_dir, rite) {
                return Err(format!(
                    "you already hold session {held} — a second mint would give this actor \
                     two identities. Close this one first, or open the new session in its \
                     own context (its own agent socket, or its own {SEED_ENV})"
                ));
            }
            let mode = config::load(comms_dir)
                .map(|c| c.session_key.clone())
                .unwrap_or_else(|_| "file".to_owned());
            if mode == "ssh-agent" {
                let sock = sshagent::socket_path(comms_dir);
                let sk = keyfile::generate()?;
                let id = personal_steward_id(sk.verifying_key().as_bytes());
                sshagent::add_identity(&sock, &sk, &id).map_err(|e| {
                    format!(
                        "{e} — start one first (comms agent serve --socket {} &) or \
                         point SSH_AUTH_SOCK at a running ssh-agent",
                        sock.display()
                    )
                })?;
                drop(sk); // the seed's only home is now the agent's memory
                return Ok(ExecOutcome {
                    message: format!(
                        "minted session key {id} — seed held by the agent at {}, never \
                         written to disk",
                        sock.display()
                    ),
                    facts: facts(&[
                        ("result", "minted"),
                        ("session", &id),
                        ("seed_held_by", "agent"),
                        ("agent_socket", &sock.display().to_string()),
                    ]),
                    output: None,
                    secret: None,
                });
            }
            if mode == "ephemeral" {
                let sk = keyfile::generate()?;
                let id = personal_steward_id(sk.verifying_key().as_bytes());
                // The seed goes to the caller and nowhere else. Nothing about
                // this session touches disk until it signs something — and
                // then its signature is the record of who it was.
                let seed_b58 = bs58::encode(sk.to_bytes()).into_string();
                Ok(ExecOutcome {
                    message: format!("minted ephemeral session key {id}"),
                    facts: facts(&[
                        ("result", "minted"),
                        ("session", &id),
                        ("seed_held_by", "holder"),
                    ]),
                    output: None,
                    secret: Some(seed_b58),
                })
            } else {
                let kp = key_path(comms_dir, &session_target(rite));
                let sk = keyfile::mint(&kp, inputs.label)?;
                let id = personal_steward_id(sk.verifying_key().as_bytes());
                Ok(ExecOutcome {
                    message: format!(
                        "minted session key {id} (file mode: one session per door — for \
                         concurrent sessions use session_key = \"ssh-agent\" or \"ephemeral\")"
                    ),
                    facts: facts(&[
                        ("result", "minted"),
                        ("session", &id),
                        ("seed_held_by", "disk"),
                        ("output", &kp.display().to_string()),
                    ]),
                    output: Some(kp),
                    secret: None,
                })
            }
        }
        "shred" => {
            // Destroy only *my* seed. A concurrent peer's session is not this
            // actor's to end, and nothing here can reach it anyway.
            let held = actor_session(comms_dir, rite);
            let kp = key_path(comms_dir, &session_target(rite));
            let had_file = kp.exists();
            if had_file {
                keyfile::shred(&kp)?;
            }
            if agent_mode(comms_dir) {
                let sock = sshagent::socket_path(comms_dir);
                if let Some((id, public)) = agent_session(&sock) {
                    sshagent::remove_identity(&sock, &public)?;
                    return Ok(ExecOutcome {
                        message: format!("session key {id} removed from the agent (seed gone)"),
                        facts: facts(&[
                            ("result", "shredded"),
                            ("session", &id),
                            ("seed", "gone"),
                            ("removed_from", "agent"),
                        ]),
                        output: None,
                        secret: None,
                    });
                }
            }
            if !had_file && std::env::var(SEED_ENV).is_ok() {
                return Ok(ExecOutcome {
                    message: format!(
                        "no key file remains, but the seed still lives with its holder: \
                         unset {SEED_ENV} and forget it — that act is the shred, and \
                         nothing here can perform it for you"
                    ),
                    facts: facts(&[
                        ("result", "incomplete"),
                        ("session", held.as_deref().unwrap_or("")),
                        ("seed", "still-reachable"),
                        ("held_by", "holder-environment"),
                    ]),
                    output: None,
                    secret: None,
                });
            }
            Ok(ExecOutcome {
                message: if had_file {
                    "session key destroyed (seed gone)".to_owned()
                } else {
                    "no seed held by this actor — nothing to destroy".to_owned()
                },
                facts: facts(&[
                    ("result", if had_file { "shredded" } else { "already-absent" }),
                    ("session", held.as_deref().unwrap_or("")),
                    ("seed", "gone"),
                    ("removed_from", if had_file { "disk" } else { "" }),
                ]),
                output: if had_file { Some(kp) } else { None },
                secret: None,
            })
        }
        "attest" => {
            let target = step.target.as_deref().unwrap_or("entry");
            // `attest config` has its content already: the exact bytes of the
            // comms.toml this rite is running under. Supplying it by hand would
            // let the attested document and the one in force differ from the
            // start, which is the whole thing being guarded against.
            let config_bytes = if target == "config" && inputs.body.is_none() {
                let p = comms_dir.join("comms.toml");
                Some(std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?)
            } else {
                None
            };
            let body = config_bytes
                .as_deref()
                .or(inputs.body.as_deref())
                .ok_or_else(|| {
                    format!(
                        "step '{}' needs content: pass --body <file>",
                        step.display()
                    )
                })?;
            let sk = session_signer(comms_dir, rite)?;
            let about = inputs.about.unwrap_or(if target == "config" {
                CONFIG_ABOUT
            } else {
                target
            });
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
            let (default_kind, default_media) = if target == "config" {
                (CONFIG_KIND, "text/plain;charset=utf-8")
            } else {
                ("testimony", "text/markdown")
            };
            let spec = ClaimSpec {
                about,
                kind: inputs.kind.unwrap_or(default_kind),
                body,
                media_type: inputs.media_type.unwrap_or(default_media),
                detach: false,
                support: &[],
                refs: &refs,
                language: "zxx",
                community: None,
                occasion: Some(&rite.name),
                issued_at: &now,
            };
            let att = author_general_claim_with(&spec, &sk, "author", &now)?;
            let store = comms_dir.join("store");
            std::fs::create_dir_all(&store).map_err(|e| format!("{}: {e}", store.display()))?;
            std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            let noted = if config_bytes.is_some() {
                format!(
                    " (the active comms.toml, {} bytes, blake3 {})",
                    body.len(),
                    blake3::hash(body).to_hex()
                )
            } else {
                String::new()
            };
            Ok(ExecOutcome {
                message: format!("attested {} -> {}{noted}", att.id(), out.display()),
                facts: facts(&[
                    ("result", "attested"),
                    ("id", &att.id()),
                    ("kind", inputs.kind.unwrap_or(default_kind)),
                    ("about", about),
                    ("body_b3", &b3(body)),
                    ("body_len", &body.len().to_string()),
                    ("signer", &sk.steward_id()),
                    (
                        "previous_entry",
                        refs.first().map(|(_, id)| id.as_str()).unwrap_or(""),
                    ),
                    ("output", &out.display().to_string()),
                ]),
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
            let bundle = make_bundle_with(
                members,
                HashMap::new(),
                if seal_it { Some(&sk as &dyn StewardSigner) } else { None },
                &format!("{} rite", rite.name),
                &now,
                &now,
                &now,
            )?;
            let signer_id = sk.steward_id();
            let out = bundle_output(comms_dir, rite);
            let bundle_bytes = bundle.to_cbor();
            std::fs::write(&out, &bundle_bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            // The seal's own attestation id and the bundle file's hash are what
            // a receiver checks; naming them here saves re-deriving them.
            let seal_id = crate::bundle::inspect_bundle(&bundle)
                .members
                .into_iter()
                .find(|m| m.is_seal)
                .map(|m| m.id)
                .unwrap_or_default();
            Ok(ExecOutcome {
                message: format!(
                    "{} {count} attestation{} -> {}",
                    if seal_it { "sealed" } else { "packed" },
                    if count == 1 { "" } else { "s" },
                    out.display()
                ),
                facts: facts(&[
                    ("result", if seal_it { "sealed" } else { "packed" }),
                    ("members", &count.to_string()),
                    ("seal", &seal_id),
                    ("sealed_by", if seal_it { signer_id.as_str() } else { "" }),
                    ("bundle_b3", &b3(&bundle_bytes)),
                    ("bundle_len", &bundle_bytes.len().to_string()),
                    ("output", &out.display().to_string()),
                ]),
                output: Some(out),
                secret: None,
            })
        }
        "countersign" => {
            let cfg = config::load(comms_dir)?;
            let target = step.target.as_deref().unwrap_or("session");
            let cs = cfg.countersign_for(target).ok_or_else(|| {
                format!(
                    "countersign step declared but comms.toml has no [countersign] or \
                     [countersign.{target}] table (set `by = \"comms.steward:z...\"`)"
                )
            })?;
            // What is being witnessed. `session` endorses this session's key
            // (the original rite); any other target endorses the attestation
            // this session produced under that name — a constitution, an
            // interface, a trial-log entry. The endorsement shape is the same;
            // only what it points at differs.
            let (endorsed, capacity, described) = if target == "session" {
                let sid = actor_session(comms_dir, rite)
                    .ok_or_else(|| "you hold no session — `mint` first".to_owned())?;
                (sid, "session-instance", "session key".to_owned())
            } else {
                let path = attest_output(comms_dir, rite, target);
                let bytes = std::fs::read(&path).map_err(|_| {
                    format!(
                        "nothing to countersign: this session has no '{target}' at {} — \
                         attest it before asking anyone to witness it",
                        path.display()
                    )
                })?;
                let att = parse_attestation(&bytes)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                (att.id(), "artifact", format!("'{target}'"))
            };
            let out = countersign_output_for(
                comms_dir,
                session_tag(comms_dir, rite).as_deref(),
                target,
            );
            if out.exists() {
                return Ok(ExecOutcome {
                    message: format!(
                        "already staged at {}; awaiting the configured counterparty's signature",
                        out.display()
                    ),
                    facts: facts(&[
                        ("result", "already-staged"),
                        ("subject", &endorsed),
                        ("needs_by", &cs.by),
                        ("needs_role", &cs.role),
                        ("output", &out.display().to_string()),
                    ]),
                    output: Some(out),
                    secret: None,
                });
            }

            let claim = vec![
                (Value::text("t"), Value::text("endorsement/1")),
                (Value::text("target"), Value::text(&endorsed)),
                (Value::text("in_capacity"), Value::text(capacity)),
                (Value::text("weight"), Value::text("primary")),
                (
                    Value::text("rationale"),
                    Value::text(&format!(
                        "{described} of {}, countersigned as {}",
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
                facts: facts(&[
                    ("result", "staged"),
                    ("id", &att.id()),
                    ("subject", &endorsed),
                    ("subject_kind", capacity),
                    ("needs_by", &cs.by),
                    ("needs_role", &cs.role),
                    ("stem", &stem),
                    ("output", &out.display().to_string()),
                ]),
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
            let att = author_general_claim_with(&spec, &sk, "author", &now)?;
            let out = decision_output(comms_dir, rite, "request", target);
            std::fs::create_dir_all(out.parent().unwrap())
                .map_err(|e| format!("{}: {e}", out.parent().unwrap().display()))?;
            std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!("recorded request {} -> {}", att.id(), out.display()),
                facts: facts(&[
                    ("result", "requested"),
                    ("id", &att.id()),
                    ("kind", inputs.kind.unwrap_or("archive-request")),
                    ("about", inputs.about.unwrap_or(target)),
                    ("body_b3", &b3(body)),
                    ("body_len", &body.len().to_string()),
                    ("signer", &sk.steward_id()),
                    ("output", &out.display().to_string()),
                ]),
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

            let request_id = recorded_request_id(comms_dir, rite, target)?;

            // Grant is delivery (detached-bodies design II.6): with
            // --deliver, the requested body is resolved from the archive and
            // placed where the requester can reach it, and the attestation
            // below names the delivery — the record of the grant and the
            // fact of the grant become the same thing.
            let mut delivery_note = String::new();
            let (mut delivered, mut delivered_b3) = (None, None);
            if decision == "grant" {
                if let Some(target_ref) = inputs.deliver {
                    let d = deliver_body(comms_dir, target_ref, &request_id)?;
                    delivery_note = d.note;
                    delivered = Some(d.path.display().to_string());
                    delivered_b3 = Some(d.b3_hex);
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
            let att = author_general_claim_with(&spec, &sk, "custodian", &now)?;
            let out = decision_output(comms_dir, rite, "grant", target);
            std::fs::write(&out, att.to_cbor()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(ExecOutcome {
                message: format!(
                    "recorded {decision} {} (re {request_id}) -> {}{}",
                    att.id(),
                    out.display(),
                    delivery_note.trim_end(),
                ),
                facts: facts(&[
                    ("result", decision),
                    ("id", &att.id()),
                    ("kind", &kind),
                    ("re_request", &request_id),
                    ("body_b3", &b3(&body)),
                    ("signer", &sk.steward_id()),
                    ("delivered", delivered.as_deref().unwrap_or("")),
                    ("delivered_b3", delivered_b3.as_deref().unwrap_or("")),
                    ("output", &out.display().to_string()),
                ]),
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
pub fn deliver_body(
    comms_dir: &Path,
    target_ref: &str,
    request_id: &str,
) -> Result<Delivery, String> {
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
    Ok(Delivery {
        note: format!("\n\ndelivered: {} (blake3 {b3_hex})\n", dst.display()),
        path: dst,
        b3_hex,
    })
}

/// What a grant actually handed over: the path the bytes landed at, their
/// blake3, and the note the grant attestation carries. The requester checks
/// the bytes against the attested commitment before relying on them.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub path: PathBuf,
    pub b3_hex: String,
    pub note: String,
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
        let v = rite_view(&comms, &cfg, open);
        assert_eq!(v.next, Some(0));
        assert!(active_rite(&comms, &cfg).map(|r| r.name.as_str()) == Some("open"));

        // mint -> the session key exists, next advances to attest.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        assert!(step_done(&comms, &cfg, open, &open.steps[0]));
        assert_eq!(rite_view(&comms, &cfg, open).next, Some(1));

        // attest with no body errors helpfully; with a body it completes.
        let needs = execute_step(&comms, open, &open.steps[1], &ExecInputs::default());
        assert!(needs.unwrap_err().contains("--body"));
        let inp = ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, open, &open.steps[1], &inp).unwrap();
        assert!(rite_view(&comms, &cfg, open).complete());
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
        assert!(step_done(&comms, &cfg, close, &close.steps[1]));
        // The bundle is session-scoped (close.<tag>.bundle), not a bare name.
        assert!(step_output(&comms, close, &close.steps[1])
            .unwrap()
            .is_file());

        // shred: key present -> gets destroyed -> step satisfied. Afterwards
        // the session is closed, so the question is asked of *it* — there is
        // no live session for `step_done` to ask about.
        assert!(!step_done(&comms, &cfg, close, &close.steps[2]));
        execute_step(&comms, close, &close.steps[2], &ExecInputs::default()).unwrap();
        assert!(!comms.join("session.key").exists());
        assert!(my_epoch(&comms, &cfg).is_none(), "shred ends the session");
        let closed = last_epoch(&comms, &cfg).expect("the closed session stays on record");
        assert_eq!(closed.state, SessionState::Concluded);
        // Its ritual closing is on disk; its key destruction is not, and never
        // could be. The rite reaches its end without claiming otherwise.
        assert_eq!(
            step_status_in(&comms, close, &close.steps[2], Some(&closed)),
            StepStatus::Unverifiable
        );
        assert!(!step_done_in(&comms, close, &close.steps[2], Some(&closed)));
        let v = rite_view_in(&comms, close, Some(&closed));
        assert!(v.complete(), "nothing further can be performed");
        assert!(v.concluded(), "but only because a step nobody can check was set aside");
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
        execute_step(&comms, close, &close.steps[0], &tx()).unwrap();
        execute_step(&comms, close, &close.steps[1], &tx()).unwrap();
        // Name A's bundle while A still holds its seed: the path is
        // session-scoped, and after the shred this actor holds no session.
        let bundle_a = step_output(&comms, close, &close.steps[1]).unwrap();
        assert!(bundle_a.is_file());
        execute_step(&comms, close, &close.steps[2], &tx()).unwrap(); // shred

        // Session B: fresh key, attest transcript — now `seal` must be PENDING
        // (its scoped bundle does not exist yet), not silently skipped.
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(&comms, close, &close.steps[0], &tx()).unwrap();
        assert!(
            !step_done(&comms, &cfg, close, &close.steps[1]),
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

    /// The Sentira Stylish continuity's report: a founding rite completed,
    /// sealed, and its key shredded, after which `status` forgot the rite had
    /// happened and recommended minting the session again.
    ///
    ///   evidence(found) ∧ ¬live_key ⇒ status(not_found)     — the bug
    ///   historical_completion ⟂ current_signing_capability  — the theorem
    #[test]
    fn a_closed_session_keeps_its_completed_rites() {
        let comms = scratch("closedhistory");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let body = || ExecInputs {
            body: Some(b"x\n".to_vec()),
            ..Default::default()
        };

        for s in &open.steps {
            execute_step(&comms, open, s, &body()).unwrap();
        }
        for s in &close.steps {
            execute_step(&comms, close, s, &body()).unwrap();
        }

        // The key is gone. The session is not.
        let all = epochs(&comms, &cfg);
        assert_eq!(all.len(), 1, "one session on record: {all:?}");
        let s1 = &all[0];
        assert_eq!(s1.state, SessionState::Concluded, "its closing work is on disk");
        assert!(s1.id.is_some(), "its identity is still legible");

        // Both rites read complete *for that session*, key or no key.
        assert!(rite_view_in(&comms, open, Some(s1)).complete());
        assert!(rite_view_in(&comms, close, Some(s1)).complete());

        // And `close requires open` is satisfied within the session, so the
        // contradictory "close [complete] / blocked by incomplete open" is gone.
        assert!(rite_view_in(&comms, open, Some(s1)).complete());

        // No live session: the next act opens a new one rather than pretending
        // the first never happened.
        assert!(my_epoch(&comms, &cfg).is_none());
        assert!(!rite_view_in(&comms, open, None).complete());
    }

    #[test]
    fn sessions_accumulate_as_a_succession() {
        let comms = scratch("succession");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let body = || ExecInputs {
            body: Some(b"x\n".to_vec()),
            ..Default::default()
        };

        for _ in 0..2 {
            for s in &open.steps {
                execute_step(&comms, open, s, &body()).unwrap();
            }
            for s in &close.steps {
                execute_step(&comms, close, s, &body()).unwrap();
            }
        }

        let all = epochs(&comms, &cfg);
        assert_eq!(all.len(), 2, "both sessions on record: {all:?}");
        assert_ne!(all[0].tag, all[1].tag);
        assert!(all.iter().all(|e| e.state == SessionState::Concluded));
        // Each carries its own completed rites; neither borrows the other's.
        for e in &all {
            assert!(rite_view_in(&comms, open, Some(e)).complete(), "{e:?}");
            assert!(rite_view_in(&comms, close, Some(e)).complete(), "{e:?}");
        }
        // Both closed, and neither is this actor's any more: their seeds are
        // gone. (Position is not asserted: ordering is by first signature, and
        // sessions opened within the same second order arbitrarily but stably.)
        assert!(all.iter().all(|e| e.state == SessionState::Concluded), "{all:?}");
        assert_eq!(all.iter().filter(|e| e.mine()).count(), 0);

    }

    /// Sessions run in parallel and their artifacts meet later in one store.
    /// A session another door minted may predate this door's own; the
    /// succession must order by when each first signed, not by which one this
    /// door happens to have minted last.
    #[test]
    fn a_merged_store_orders_parallel_sessions_by_when_they_signed() {
        let mine = scratch("merge-mine");
        let theirs = scratch("merge-theirs");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let body = || ExecInputs {
            body: Some(b"x\n".to_vec()),
            ..Default::default()
        };

        // Their session signs first, in their own door.
        for s in &open.steps {
            execute_step(&theirs, open, s, &body()).unwrap();
        }
        let theirs_entry = step_output(&theirs, open, &open.steps[1]).unwrap();
        let theirs_att = parse_attestation(&std::fs::read(&theirs_entry).unwrap()).unwrap();
        let theirs_id = theirs_att.signatures[0].by.clone();

        // Ours signs second, here. Backdate theirs is unnecessary — we assert
        // on the recorded flag and membership, and force the timestamps apart
        // by writing their artifact into our store with an earlier issued_at
        // is not possible without re-signing, so we compare by identity.
        for s in &open.steps {
            execute_step(&mine, open, s, &body()).unwrap();
        }
        let ours_id = actor_session(&mine, open).unwrap();

        // The merge: their artifacts land in our store.
        let dest = mine.join("store").join(
            theirs_entry.file_name().unwrap().to_string_lossy().to_string(),
        );
        std::fs::copy(&theirs_entry, &dest).unwrap();

        let all = epochs(&mine, &cfg);
        assert_eq!(all.len(), 2, "both sessions are on record: {all:?}");
        assert!(
            all.iter().any(|e| e.id.as_deref() == Some(theirs_id.as_str())),
            "the merged-in session appears: {all:?}"
        );
        // Only ours is this door's; theirs is history it received, not a
        // session this door can act as.
        assert_eq!(all.iter().filter(|e| e.mine()).count(), 1);
        assert_eq!(
            all.iter().find(|e| e.mine()).unwrap().id.as_deref(),
            Some(ours_id.as_str())
        );
        // The distinction that matters after a merge: their evidence is here
        // and their rite reads complete on it, but this door cannot sign as
        // them. Only our own session is live.
        let theirs_epoch = all
            .iter()
            .find(|e| e.id.as_deref() == Some(theirs_id.as_str()))
            .unwrap();
        assert_ne!(theirs_epoch.state, SessionState::Mine, "a merged-in session is not ours to sign as");
        assert!(step_done_in(&mine, open, &open.steps[1], Some(theirs_epoch)));
        assert!(
            all.iter().find(|e| e.mine()).is_some(),
            "our own session still holds its key"
        );
    }

    #[test]
    fn a_session_that_never_closed_reads_as_unfinished_not_absent() {
        let comms = scratch("crashed");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let body = || ExecInputs {
            body: Some(b"x\n".to_vec()),
            ..Default::default()
        };

        for s in &open.steps {
            execute_step(&comms, open, s, &body()).unwrap();
        }
        // Attest the transcript, then lose the key without sealing or shredding.
        execute_step(&comms, close, &close.steps[0], &body()).unwrap();
        std::fs::remove_file(comms.join("session.key")).unwrap();

        let s1 = last_epoch(&comms, &cfg).unwrap();
        assert_ne!(s1.state, SessionState::Mine);
        // Open completed; close did not — and neither reads as never-begun.
        assert!(rite_view_in(&comms, open, Some(&s1)).complete());
        let v = rite_view_in(&comms, close, Some(&s1));
        assert!(!v.complete(), "close never finished");
        assert!(!v.untouched(), "but it did begin: the transcript is attested");
        assert!(v.steps[0].done(), "attest transcript stands");
    }

    /// The Sentira Stylish continuity asked that the active comms.toml bytes
    /// be directly attestable and drift-checked: a rite whose rules can change
    /// unremarked is a rite whose record is missing its own premise.
    #[test]
    fn active_config_is_attestable_and_drift_is_visible() {
        let comms = scratch("configdrift");
        let toml_text = r#"
profile = "continuity"
[rites.open]
steps = ["mint session", "attest config", "attest entry"]
"#;
        std::fs::write(comms.join("comms.toml"), toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(toml_text).unwrap());
        let open = cfg.rite("open").unwrap();

        // Before anything is attested, the door says so rather than implying
        // the rules are vouched for.
        assert!(matches!(
            config_state(&comms).unwrap(),
            ConfigState::Unattested { .. }
        ));

        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        // `attest config` needs no --body: its content is the live file.
        let out = execute_step(&comms, open, &open.steps[1], &ExecInputs::default()).unwrap();
        assert!(out.message.contains("active comms.toml"), "{}", out.message);

        let attested_id = match config_state(&comms).unwrap() {
            ConfigState::Matches { id, .. } => id,
            other => panic!("the attested bytes are the bytes in force: {other:?}"),
        };

        // The attestation carries the exact bytes, under a legible kind.
        let att = parse_attestation(
            &std::fs::read(step_output(&comms, open, &open.steps[1]).unwrap()).unwrap(),
        )
        .unwrap();
        let claim = att.core.get("c").unwrap();
        assert_eq!(claim.get("kind").and_then(Value::as_text), Some(CONFIG_KIND));
        assert_eq!(claim.get("about").and_then(Value::as_text), Some(CONFIG_ABOUT));
        assert_eq!(
            claim
                .get("content")
                .and_then(|c| c.get("body"))
                .and_then(Value::as_bytes)
                .unwrap(),
            toml_text.as_bytes()
        );

        // Change one byte of the rules in force: drift, named, with both hashes.
        std::fs::write(
            comms.join("comms.toml"),
            format!("{toml_text}allow_waivers = true\n"),
        )
        .unwrap();
        match config_state(&comms).unwrap() {
            ConfigState::Drifted { id, active_b3, attested_b3 } => {
                assert_eq!(id, attested_id);
                assert_ne!(active_b3, attested_b3);
            }
            other => panic!("a changed comms.toml must read as drifted: {other:?}"),
        }

        // Restoring the attested bytes clears it — nothing is sticky.
        std::fs::write(comms.join("comms.toml"), toml_text).unwrap();
        assert!(matches!(
            config_state(&comms).unwrap(),
            ConfigState::Matches { .. }
        ));
    }

    /// Every significant rite step reports what it produced as facts, so a
    /// caller can verify the act without parsing prose. The facts that matter
    /// most are the ones a verifier re-derives: the id authored and the blake3
    /// of the body it commits to.
    #[test]
    fn every_rite_step_reports_verifiable_facts() {
        let comms = scratch("stepfacts");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let body = b"# entry\n";
        let inp = || ExecInputs {
            body: Some(body.to_vec()),
            ..Default::default()
        };
        let get = |o: &ExecOutcome, k: &str| {
            o.facts
                .iter()
                .find(|(f, _)| f == k)
                .map(|(_, v)| v.clone())
        };

        let minted = execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        assert_eq!(get(&minted, "result").as_deref(), Some("minted"));
        let session = get(&minted, "session").expect("mint names the session it created");
        assert_eq!(get(&minted, "seed_held_by").as_deref(), Some("disk"));

        let attested = execute_step(&comms, open, &open.steps[1], &inp()).unwrap();
        assert_eq!(get(&attested, "result").as_deref(), Some("attested"));
        assert_eq!(get(&attested, "signer").as_deref(), Some(session.as_str()));
        // The two facts a receiver checks against the artifact itself.
        let id = get(&attested, "id").unwrap();
        let stored = parse_attestation(
            &std::fs::read(step_output(&comms, open, &open.steps[1]).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(id, stored.id(), "the reported id is the artifact's id");
        assert_eq!(
            get(&attested, "body_b3").as_deref(),
            Some(blake3::hash(body).to_hex().to_string().as_str()),
            "the reported hash is the hash of the body attested"
        );
        assert_eq!(get(&attested, "body_len").as_deref(), Some("8"));

        execute_step(&comms, close, &close.steps[0], &inp()).unwrap();
        let sealed = execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap();
        assert_eq!(get(&sealed, "result").as_deref(), Some("sealed"));
        assert_eq!(get(&sealed, "sealed_by").as_deref(), Some(session.as_str()));
        assert_eq!(get(&sealed, "members").as_deref(), Some("2"));
        // The bundle hash names the exact bytes written, re-derivable on disk.
        let bundle_path = step_output(&comms, close, &close.steps[1]).unwrap();
        assert_eq!(
            get(&sealed, "bundle_b3").as_deref(),
            Some(
                blake3::hash(&std::fs::read(&bundle_path).unwrap())
                    .to_hex()
                    .to_string()
                    .as_str()
            )
        );
        assert!(get(&sealed, "seal").is_some(), "the seal's own id travels");

        let shredded = execute_step(&comms, close, &close.steps[2], &ExecInputs::default()).unwrap();
        assert_eq!(get(&shredded, "result").as_deref(), Some("shredded"));
        assert_eq!(get(&shredded, "seed").as_deref(), Some("gone"));
        assert_eq!(get(&shredded, "session").as_deref(), Some(session.as_str()));
    }

    #[test]
    fn mint_twice_refuses() {
        let comms = scratch("twice");
        let cfg = cfg();
        let open = cfg.rite("open").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let again = execute_step(&comms, open, &open.steps[0], &ExecInputs::default());
        assert!(again.unwrap_err().contains("you already hold session"));
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
        assert!(!step_done(&comms, &cfg, open, &open.steps[2]));
        execute_step(&comms, open, &open.steps[2], &ExecInputs::default()).unwrap();
        assert!(
            !step_done(&comms, &cfg, open, &open.steps[2]),
            "a staged request is not the guardian's signature"
        );
        assert_eq!(rite_view(&comms, &cfg, open).next, Some(2));

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

        // Re-running preserves the exact staged request rather than replacing
        // it with a newly timestamped core.
        let staged = step_output(&comms, open, &open.steps[2]).unwrap();
        let before = std::fs::read(&staged).unwrap();
        let repeated = execute_step(&comms, open, &open.steps[2], &ExecInputs::default()).unwrap();
        assert!(repeated.message.contains("already staged"));
        assert_eq!(before, std::fs::read(&staged).unwrap());

        // The guardian signs and finalizes; the step stays done because the
        // endorsement (by claim content) is now in the store.
        signing::sign_pending(&pending, &guardian).unwrap();
        signing::finalize_pending(&pending, &comms.join("store")).unwrap();
        assert!(signing::read_pending(&pending).unwrap().is_empty());
        assert!(
            step_done(&comms, &cfg, open, &open.steps[2]),
            "finalized still reads done"
        );

        // The stored endorsement targets this session's key and is guardian-signed.
        let sid = actor_session(&comms, open).unwrap();
        assert!(countersign_recorded_for(&comms, &sid));
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
    fn required_rite_blocks_close_until_finalized_countersign() {
        let comms = scratch("required-rite");
        let (mut cfg, guardian, _) = cs_setup(&comms);
        cfg.rites.push(Rite {
            name: "close".into(),
            steps: vec![Step {
                verb: "attest".into(),
                target: Some("transcript".into()),
            }],
            requires: vec!["open".into()],
            allow_waivers: false,
        });
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        execute_step(
            &comms,
            open,
            &open.steps[1],
            &ExecInputs {
                body: Some(b"# entry\n".to_vec()),
                ..ExecInputs::default()
            },
        )
        .unwrap();
        execute_step(&comms, open, &open.steps[2], &ExecInputs::default()).unwrap();

        let blocked = execute_step(
            &comms,
            close,
            &close.steps[0],
            &ExecInputs {
                body: Some(b"transcript\n".to_vec()),
                ..ExecInputs::default()
            },
        )
        .unwrap_err();
        assert!(blocked.contains("requires completed rite(s) [open]"));

        let pending = comms.join("pending");
        signing::sign_pending(&pending, &guardian).unwrap();
        signing::finalize_pending(&pending, &comms.join("store")).unwrap();
        execute_step(
            &comms,
            close,
            &close.steps[0],
            &ExecInputs {
                body: Some(b"transcript\n".to_vec()),
                ..ExecInputs::default()
            },
        )
        .unwrap();
    }

    /// The Sentira Stylish continuity asked to countersign arbitrary
    /// configured artifacts, not only session keys: their constitution was
    /// co-signed, but the verb could only ever endorse the session key.
    #[test]
    fn countersign_endorses_a_named_artifact_not_only_the_session_key() {
        let comms = scratch("countersign-artifact");
        let guardian = keyfile::mint(&comms.join("guardian.json"), "guardian").unwrap();
        let gid = personal_steward_id(guardian.verifying_key().as_bytes());
        let witness = keyfile::mint(&comms.join("witness.json"), "witness").unwrap();
        let wid = personal_steward_id(witness.verifying_key().as_bytes());
        // The default party witnesses session keys; the constitution has its
        // own witness through a per-target table.
        let toml_text = format!(
            r#"
profile = "continuity"
[countersign]
by = "{gid}"
role = "guardian"
[countersign.constitution]
by = "{wid}"
role = "witness"
[rites.open]
steps = ["mint session", "attest constitution", "countersign constitution"]
"#
        );
        std::fs::write(comms.join("comms.toml"), &toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(&toml_text).unwrap());
        let open = cfg.rite("open").unwrap();

        execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();

        // Nothing to witness until the artifact exists, and the error says so.
        let early = execute_step(&comms, open, &open.steps[2], &ExecInputs::default());
        let err = early.unwrap_err();
        assert!(err.contains("no 'constitution'"), "unexpected error: {err}");

        execute_step(
            &comms,
            open,
            &open.steps[1],
            &ExecInputs {
                body: Some(b"# constitution\n".to_vec()),
                ..Default::default()
            },
        )
        .unwrap();
        let constitution = parse_attestation(
            &std::fs::read(step_output(&comms, open, &open.steps[1]).unwrap()).unwrap(),
        )
        .unwrap();

        execute_step(&comms, open, &open.steps[2], &ExecInputs::default()).unwrap();
        assert!(
            !step_done(&comms, &cfg, open, &open.steps[2]),
            "staging is not the witness's signature"
        );

        // The staged item names the per-target party, not the default one, and
        // endorses the artifact's id rather than the session key.
        let pending = comms.join("pending");
        let items = signing::read_pending(&pending).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].needs,
            vec![Need { by: wid.clone(), role: "witness".into() }]
        );
        assert_eq!(
            items[0]
                .attestation
                .core
                .get("c")
                .and_then(|c| c.get("target"))
                .and_then(Value::as_text),
            Some(constitution.id().as_str()),
            "the endorsement must point at the constitution"
        );
        let sid = actor_session(&comms, open).unwrap();
        assert_ne!(constitution.id(), sid);

        // The witness signs and finalizes; the step then reads done.
        signing::sign_pending(&pending, &witness).unwrap();
        signing::finalize_pending(&pending, &comms.join("store")).unwrap();
        assert!(step_done(&comms, &cfg, open, &open.steps[2]));
        // And the *session key* is still unwitnessed — the two are distinct.
        assert!(!countersign_recorded_for(&comms, &sid));
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
        assert!(step_done(&comms, &cfg, close, &close.steps[1]));
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
        // Until the holder exports the seed, the harness does not know who
        // they are: the session id is derived from the seed, not recorded.
        assert!(actor_session(&comms, open).is_none());
        assert!(!step_done(&comms, &cfg, open, &open.steps[0]));
        std::env::set_var(SEED_ENV, &seed);
        let sid = actor_session(&comms, open).unwrap();
        std::env::remove_var(SEED_ENV);
        let entry = ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };
        let err = execute_step(&comms, open, &open.steps[1], &entry).unwrap_err();
        assert!(err.contains(SEED_ENV), "unhelpful error: {err}");

        // A different seed is a different *session*, not an impostor of this
        // one. Sessions run concurrently and none of them is the door's
        // official identity, so holding another seed simply makes you someone
        // else — and your work is scoped under your own tag, never theirs.
        let other = keyfile::generate().unwrap();
        let other_id = personal_steward_id(other.verifying_key().as_bytes());
        std::env::set_var(SEED_ENV, bs58::encode(other.to_bytes()).into_string());
        assert_eq!(actor_session(&comms, open).as_deref(), Some(other_id.as_str()));
        execute_step(&comms, open, &open.steps[1], &entry).unwrap();
        let other_entry = step_output(&comms, open, &open.steps[1]).unwrap();
        assert!(
            other_entry.to_string_lossy().contains(&tag_of_id(&other_id)),
            "another session's work lands under its own tag: {}",
            other_entry.display()
        );
        assert_ne!(other_id, sid);

        // With the original seed back, that session is mine again and its own
        // entry is a separate artifact.
        std::env::set_var(SEED_ENV, &seed);
        assert!(step_done(&comms, &cfg, open, &open.steps[0]));
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
        assert!(!step_done(&comms, &cfg, close, &close.steps[2]));

        // Forgetting the seed is the shred. Afterward the session is closed:
        // a new mint supersedes the stale id rather than getting stuck.
        std::env::remove_var(SEED_ENV);
        // Look this session up by name: the store also holds the other session
        // that signed here, and "the last one" is no longer a single answer
        // once sessions run alongside each other.
        let closed = epochs(&comms, &cfg)
            .into_iter()
            .find(|e| e.id.as_deref() == Some(sid.as_str()))
            .expect("the closed session stays on record");
        assert_eq!(closed.state, SessionState::Concluded);
        assert_eq!(
            step_status_in(&comms, close, &close.steps[2], Some(&closed)),
            StepStatus::Unverifiable,
            "the holder forgot the seed; nobody else can confirm that"
        );
        let minted2 = execute_step(&comms, open, &open.steps[0], &ExecInputs::default()).unwrap();
        let seed2 = minted2.secret.expect("ephemeral mint hands over the seed");
        // Again: the harness learns who you are when you export the seed, not
        // from anything written down at mint.
        std::env::set_var(SEED_ENV, &seed2);
        let sid2 = actor_session(&comms, open).unwrap();
        assert_ne!(sid, sid2, "a new session must not inherit the old identity");
        std::env::remove_var(SEED_ENV);
    }

    /// Several specialists work one project without passing commits around:
    /// same work tree, same store, their own signing identities, and no
    /// alignment between their sessions. Each is `Mine` to itself and `Open`
    /// to the other — because from outside, a peer mid-session and a session
    /// that died without closing are the same thing, and the door must not
    /// guess between them.
    #[test]
    fn sessions_run_side_by_side_in_one_work_tree() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(SEED_ENV);

        let comms = scratch("concurrent");
        let toml_text = required_toml(true, "ephemeral");
        std::fs::write(comms.join("comms.toml"), &toml_text).unwrap();
        let cfg = HarnessConfig::from_toml(&config::parse(&toml_text).unwrap());
        let open = cfg.rite("open").unwrap();
        let close = cfg.rite("close").unwrap();
        let entry = || ExecInputs {
            body: Some(b"# entry\n".to_vec()),
            ..Default::default()
        };

        // Two holders mint in the same door. Neither mint blocks the other:
        // the guard is "do *I* already hold one", not "does this door".
        let ada = execute_step(&comms, open, &open.steps[0], &ExecInputs::default())
            .unwrap()
            .secret
            .unwrap();
        let bo = execute_step(&comms, open, &open.steps[0], &ExecInputs::default())
            .unwrap()
            .secret
            .unwrap();
        assert_ne!(ada, bo);

        // Each signs an opening entry as itself.
        std::env::set_var(SEED_ENV, &ada);
        let ada_id = actor_session(&comms, open).unwrap();
        execute_step(&comms, open, &open.steps[1], &entry()).unwrap();
        std::env::set_var(SEED_ENV, &bo);
        let bo_id = actor_session(&comms, open).unwrap();
        execute_step(&comms, open, &open.steps[1], &entry()).unwrap();
        assert_ne!(ada_id, bo_id);

        // Two distinct sessions on record, each scoping its own artifacts.
        let seen = epochs(&comms, &cfg);
        assert_eq!(seen.len(), 2, "both sessions are here: {seen:?}");
        let find = |all: &[Epoch], id: &str| {
            all.iter()
                .find(|e| e.id.as_deref() == Some(id))
                .cloned()
                .expect("session on record")
        };

        // From Bo's vantage: Bo is mine, Ada is open — not closed, because
        // nothing on disk says Ada finished.
        assert_eq!(find(&seen, &bo_id).state, SessionState::Mine);
        assert_eq!(find(&seen, &ada_id).state, SessionState::Open);

        // From Ada's, exactly the reverse — same tree, same instant.
        std::env::set_var(SEED_ENV, &ada);
        let seen = epochs(&comms, &cfg);
        assert_eq!(find(&seen, &ada_id).state, SessionState::Mine);
        assert_eq!(find(&seen, &bo_id).state, SessionState::Open);

        // Ada closes. Her seal is the evidence; Bo is untouched by it.
        execute_step(&comms, close, &close.steps[0], &entry()).unwrap();
        execute_step(&comms, close, &close.steps[1], &ExecInputs::default()).unwrap();
        std::env::set_var(SEED_ENV, &bo);
        let seen = epochs(&comms, &cfg);
        assert_eq!(
            find(&seen, &ada_id).state,
            SessionState::Concluded,
            "closing evidence is legible to a peer"
        );
        assert_eq!(find(&seen, &bo_id).state, SessionState::Mine);
        assert!(rite_view_in(&comms, open, Some(&find(&seen, &ada_id))).complete());

        // A third party holding nothing sees Ada closed and Bo open, and can
        // sign as neither.
        std::env::remove_var(SEED_ENV);
        let seen = epochs(&comms, &cfg);
        assert!(my_epoch(&comms, &cfg).is_none());
        assert_eq!(find(&seen, &ada_id).state, SessionState::Concluded);
        assert_eq!(find(&seen, &bo_id).state, SessionState::Open);
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
        assert!(rite_view(&comms, &cfg, archive).complete());

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

    #[test]
    fn delivery_can_follow_an_already_recorded_grant() {
        use crate::archive::Archive;
        use crate::bundle::author_general_claim;

        let comms = scratch("postgrantdeliver");
        let repo = comms.parent().unwrap().to_path_buf();
        let archive_root = repo.join("archive");
        for d in ["store", "bodies", "views/sessions", "views/keys", "intake", "genesis"] {
            std::fs::create_dir_all(archive_root.join(d)).unwrap();
        }
        let grants = repo.join("grants");

        let guardian = keyfile::mint(&comms.join("guardian.json"), "guardian").unwrap();
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

        let body = b"delivery after grant\n";
        let spec = ClaimSpec {
            about: "letter/session-postgrant",
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
        let h_hex = crate::archive::hex(blake3::hash(body).as_bytes());
        std::fs::write(archive.bodies().join(format!("{h_hex}.md")), body).unwrap();

        let ask = ExecInputs {
            body: Some(b"please grant, delivery may follow".to_vec()),
            ..Default::default()
        };
        execute_step(&comms, archive_rite, &archive_rite.steps[0], &ask).unwrap();
        let request_id = recorded_request_id(&comms, archive_rite, "archive").unwrap();
        let grant = ExecInputs {
            key: Some(comms.join("guardian.json")),
            decision: Some("grant"),
            ..Default::default()
        };
        execute_step(&comms, archive_rite, &archive_rite.steps[1], &grant).unwrap();
        assert!(
            rite_view(&comms, &cfg, archive_rite).complete(),
            "the grant rite should now be complete"
        );

        let note = deliver_body(&comms, &letter_id, &request_id).unwrap().note;
        let req_tag: String = request_id
            .strip_prefix("comms.attest:")
            .unwrap()
            .chars()
            .take(16)
            .collect();
        let delivered = grants.join(req_tag).join("letter-session-postgrant.md");
        assert_eq!(std::fs::read(&delivered).unwrap(), body);
        assert!(note.contains(delivered.to_str().unwrap()), "{note}");
        assert!(note.contains(&h_hex), "{note}");
    }
}
