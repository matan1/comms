//! comms init: install the embeddable harness "door" into a repo.
//!
//! Slab 1 of the embeddable-harness-v1 design (`docs/embeddable-harness-v1.md`):
//! materialize the `.comms/` boundary — a `door.md`, a `policy.md`, a
//! `comms.toml`, and the directories for each artifact type a profile declares.
//! No signing, no archive import, no trust: this only writes the door. Like the
//! rest of the crate, it carries no notion of trust — `policy.md` is left for a
//! community to fill in, and the archive is never loaded here.
//!
//! Built-in profiles are the single source of truth for *both* the `comms.toml`
//! text and the directory list. Until a later slab parses `comms.toml`, keeping
//! the two beside each other in code avoids a TOML dependency while staying
//! honest: the dirs created are exactly the artifact types the written config
//! declares.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The harness boundary directory installed into a target repo.
pub const HARNESS_DIR: &str = ".comms";

/// A built-in harness profile: the directories to create (relative to the
/// `.comms/` dir) and the files to write (relative path, contents).
pub struct Profile {
    pub name: &'static str,
    pub summary: &'static str,
    pub dirs: &'static [&'static str],
    pub files: &'static [(&'static str, &'static str)],
}

/// What `install` did (or, under `--dry-run`, would do) for one path. Paths are
/// rendered relative to the target dir for legible output (e.g. `.comms/door.md`).
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// A directory that did not exist and was created.
    MkDir(String),
    /// A file written fresh (did not previously exist).
    Write(String),
    /// A file that already existed and was left untouched (no `--force`).
    Keep(String),
    /// A file that already existed and was rewritten (`--force`).
    Overwrite(String),
}

impl Step {
    /// A short glyph + verb for human-facing output.
    pub fn render(&self) -> String {
        match self {
            Step::MkDir(p) => format!("  + {p}/"),
            Step::Write(p) => format!("  + {p}"),
            Step::Keep(p) => format!("  = {p} (exists, kept)"),
            Step::Overwrite(p) => format!("  ~ {p} (overwritten)"),
        }
    }
}

/// Look up a built-in profile by name.
pub fn profile_by_name(name: &str) -> Option<&'static Profile> {
    PROFILES.iter().copied().find(|p| p.name == name)
}

/// Names of all built-in profiles, for help text and error messages.
pub fn profile_names() -> Vec<&'static str> {
    PROFILES.iter().map(|p| p.name).collect()
}

/// Install `profile`'s door under `<target>/.comms/`.
///
/// Idempotent: existing files are kept untouched unless `force` is set, in which
/// case they are rewritten. Directories are always ensured (creating one that
/// already exists is not reported). With `dry_run`, nothing touches disk but the
/// same [`Step`] list is returned, so a caller can preview exactly what would
/// happen.
pub fn install(
    profile: &Profile,
    target: &Path,
    force: bool,
    dry_run: bool,
) -> io::Result<Vec<Step>> {
    let root = target.join(HARNESS_DIR);

    // Directories: the boundary dir itself, then each declared artifact/ritual
    // dir. create_dir_all is naturally idempotent; we only *report* the ones
    // that did not already exist.
    let mut dirs: Vec<PathBuf> = vec![root.clone()];
    dirs.extend(profile.dirs.iter().map(|d| root.join(d)));

    let mut steps = Vec::new();
    for dir in &dirs {
        let existed = dir.is_dir();
        if !dry_run {
            fs::create_dir_all(dir)?;
        }
        if !existed {
            steps.push(Step::MkDir(rel(target, dir)));
        }
    }

    // Files: write fresh, keep, or overwrite — never silently clobber.
    for (relname, contents) in profile.files {
        let path = root.join(relname);
        let display = rel(target, &path);
        let exists = path.exists();
        if exists && !force {
            steps.push(Step::Keep(display));
            continue;
        }
        if !dry_run {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, contents)?;
        }
        steps.push(if exists {
            Step::Overwrite(display)
        } else {
            Step::Write(display)
        });
    }

    Ok(steps)
}

/// Render `path` relative to `target` for display, falling back to the full path
/// if it is not a descendant (which should not happen for our own joins).
fn rel(target: &Path, path: &Path) -> String {
    path.strip_prefix(target)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

// ---- built-in profiles -----------------------------------------------------

/// All built-in profiles. The first is the default.
pub const PROFILES: &[&Profile] = &[&DEFAULT, &CONTINUITY];

static DEFAULT: Profile = Profile {
    name: "default",
    summary: "minimal door: config, policy, public store, hooks",
    dirs: &["store", "hooks"],
    files: &[
        ("comms.toml", DEFAULT_TOML),
        ("door.md", DEFAULT_DOOR),
        ("policy.md", POLICY),
        ("harness.md", HARNESS_MD),
    ],
};

static CONTINUITY: Profile = Profile {
    name: "continuity",
    summary: "session ritual door: letters, transcripts, memories, pending",
    dirs: &[
        "store",
        "hooks",
        "pending",
        "letters",
        "transcripts",
        "memories",
    ],
    files: &[
        ("comms.toml", CONTINUITY_TOML),
        ("door.md", CONTINUITY_DOOR),
        ("policy.md", POLICY),
        ("harness.md", HARNESS_MD),
    ],
};

const DEFAULT_TOML: &str = r#"# Comms embeddable harness — generated by `comms-verify init`.
# What this is and what's drivable today: see harness.md beside this file.
schema = "comms-harness/1"
profile = "default"

[archive]
# external | ignored-local | encrypted-repo
mode = "external"
path = "../project.comms-archive"
index = "none"          # sqlite | none

[rituals.close]
# When an artifact required for a ritual is missing, allow a recorded waiver.
allow_waivers = true

# Declare the artifact types your project keeps. `comms init` creates a
# directory for each. Example:
#
# [artifact_types.notes]
# dir = "notes"
# default_access = "host-gated"   # host-gated | public
# filename = "session-{num:03}.{name_or_unnamed}.md"
# rituals = ["archive"]
# required_for = []
"#;

const CONTINUITY_TOML: &str = r#"# Comms embeddable harness — generated by `comms-verify init` (continuity profile).
# What this is and what's drivable today: see harness.md beside this file.
schema = "comms-harness/1"
profile = "continuity"

[archive]
# The richer history (letters, transcripts, memories) lives outside the repo and
# is requested, never auto-loaded.
mode = "external"
path = "../project.comms-archive"
index = "sqlite"        # sqlite | none

[rituals.close]
# A session that cannot produce a required artifact may record a waiver rather
# than be blocked; the waiver is itself an attestation when a session key exists.
allow_waivers = true

[artifact_types.letters]
dir = "letters"
default_access = "host-gated"
filename = "session-{num:03}.{name_or_unnamed}.md"
rituals = ["request", "archive"]
required_for = []

[artifact_types.transcripts]
dir = "transcripts"
default_access = "host-gated"
filename = "session-{num:03}.{name_or_unnamed}.log"
rituals = ["close", "seal"]
required_for = ["close"]

[artifact_types.memories]
dir = "memories"
default_access = "host-gated"
filename = "session-{num:03}.{name_or_unnamed}.md"
rituals = ["archive"]
required_for = []
"#;

const DEFAULT_DOOR: &str = r#"# This repo carries a Comms door

Comms is a protocol substrate for community-grounded attestations: signed,
content-addressed claims. Verification here means **the math holds, not that
trust has been decided**. Trust is a community judgment and lives nowhere in the
substrate.

This `.comms/` directory is a door, not an inheritance:

- `comms.toml` — harness configuration (archive mode, artifact types, rituals).
- `policy.md`  — who this community counts as sponsor/witness; you decide.
- `store/`     — public, content-addressed attestations kept with the repo.
- `hooks/`     — assistive git hooks (installed separately; warn-only by default).

A richer archive (letters, transcripts, memories) may exist outside this repo.
It is **never** loaded by default. You may request it, decline it, or ignore it.
Requests, grants, deferrals, and denials are meant to be recorded, so the gate
stays visible.

See `harness.md` beside this file for what the harness does and which rituals
are drivable today, and the Comms Attest spec for the protocol.
"#;

const CONTINUITY_DOOR: &str = r#"# This repo carries a Comms door (continuity profile)

Comms is a protocol substrate for community-grounded attestations: signed,
content-addressed claims. Verification here means **the math holds, not that
trust has been decided**.

This profile adds the shape of an accountable-discontinuity ritual — for
projects worked by a succession of sessions (human or agent):

- Each session mints a fresh key at start and destroys its seed before the end,
  so a closed session cannot be impersonated or made to speak again.
- A session may leave a **letter** to its successors, write a **transcript**, or
  keep **memories** — all offers, never obligations, in both directions.
- The archive of past letters/transcripts/memories lives outside the repo and is
  **requested, never auto-loaded**. Asking, granting, deferring, and declining
  are recorded, so the door stays visible and freedom stays load-bearing.

Layout:

- `comms.toml`    — archive mode, declared artifact types, ritual rules.
- `policy.md`     — your community's trust rules (the substrate decides none).
- `store/`        — public, content-addressed attestations kept with the repo.
- `pending/`      — staged artifacts awaiting a countersignature.
- `letters/`, `transcripts/`, `memories/` — per-session artifacts.

Write a constitution beside this door if your community wants ratified rules.
See `harness.md` beside this file for what is drivable today.
"#;

const POLICY: &str = r#"# Community policy (template)

The Comms substrate guarantees *well-formedness* and *verifiability*. It does
**not** decide trust. This file is where your community writes the policy the
substrate deliberately leaves open:

- Who counts as a sponsor or witness?
- How many witnesses does admission require?
- How are renewal, expiry, objection, and recovery handled?

A well-formed attestation is not the same as a trusted one. Edit this file to
state your community's rules; nothing here is enforced by the binary.
"#;

const HARNESS_MD: &str = r#"# The Comms harness — what's here and what's drivable

This `.comms/` door was installed by `comms-verify init`. It is the boundary; the
`comms-verify` binary is the tool. This file is honest about the gap between the
two so you are not surprised.

## What the binary can do today

`comms-verify` is a self-contained kit for authoring and moving signed,
content-addressed attestations offline:

- `init`    — install or refresh this door.
- `mint`    — generate a steward key (`{seed_b58, label}` JSON, mode 0600).
- `attest`  — author **and sign** a `general-claim/1` attestation from a content
              file (a letter, a transcript, a memory, any statement). This is how
              you create a primary artifact; the result is an `<id>.cbor`.
- `pack`    — gather `.cbor` attestations (and/or `--media` blobs) into a bundle.
- `seal`    — add an A1.8 integrity seal (signs the exact member set).
- `verify`  — check a bundle's seal.
- `inspect` — verify every member on its own terms (signatures, refs, media).
- `extract` — write a bundle's members and media back out to files.
- `vouch`   — a candidate policy-relative evaluator (judgment, not proof).

A typical authoring loop:

```sh
comms-verify mint   --out steward.json --label me
comms-verify attest --key steward.json --about "session 1 letter" --kind testimony \
                    --media-type text/markdown --body letter.md --out letter.cbor
comms-verify pack   --out session.bundle letter.cbor --seal --key steward.json
comms-verify verify  session.bundle
comms-verify inspect session.bundle
```

## What is described in config but not yet driven by the binary

`comms.toml` declares `[rituals.close]`, artifact types, waivers, and archive
modes. These describe an intended **session workflow** (open → work → close,
with required artifacts and recorded archive requests). The binary does **not**
yet execute those rituals — there is no `comms close`, `comms request`, or
`comms archive` verb. Until there is, treat `comms.toml` as the declaration of
intent and drive the workflow with `attest` + `pack` + `seal` by hand, or with
the reference ceremony in the upstream comms project. This gap is known and is
the next slab of work.

## The stance

Verification here means the math holds, not that trust has been decided. The
archive of richer history is requested, never auto-loaded, and access decisions
are meant to be recorded. Trust is your community's call; write it in
`policy.md`.
"#;

// ---- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    // A unique scratch dir per test, no external crates.
    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("comms-init-{tag}-{pid}-{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_profile_writes_door_dirs_and_config() {
        let target = scratch("default");
        let p = profile_by_name("default").unwrap();
        let steps = install(p, &target, false, false).unwrap();

        let root = target.join(HARNESS_DIR);
        assert!(root.join("comms.toml").is_file());
        assert!(root.join("door.md").is_file());
        assert!(root.join("policy.md").is_file());
        assert!(root.join("store").is_dir());
        assert!(root.join("hooks").is_dir());

        // The config declares its profile, and the door states the core stance.
        let toml = fs::read_to_string(root.join("comms.toml")).unwrap();
        assert!(toml.contains("profile = \"default\""));
        let door = fs::read_to_string(root.join("door.md")).unwrap();
        assert!(door.contains("the math holds"));

        // The door is self-contained: it ships harness.md and points at no file
        // the embedded repo does not have.
        assert!(root.join("harness.md").is_file());
        assert!(!door.contains("docs/embeddable-harness-v1.md"));
        let harness = fs::read_to_string(root.join("harness.md")).unwrap();
        assert!(harness.contains("attest"));

        // Fresh install: every file is a Write, every declared dir an MkDir.
        assert!(steps.iter().any(|s| *s == Step::Write(".comms/door.md".into())));
        assert!(steps.iter().any(|s| *s == Step::MkDir(".comms/store".into())));
        assert!(!steps.iter().any(|s| matches!(s, Step::Keep(_) | Step::Overwrite(_))));

        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn continuity_profile_declares_artifact_dirs_matching_config() {
        let target = scratch("continuity");
        let p = profile_by_name("continuity").unwrap();
        install(p, &target, false, false).unwrap();

        let root = target.join(HARNESS_DIR);
        for d in ["letters", "transcripts", "memories", "pending"] {
            assert!(root.join(d).is_dir(), "missing dir {d}");
        }
        let toml = fs::read_to_string(root.join("comms.toml")).unwrap();
        // Every dir created beyond the invariant ones is a declared artifact type.
        assert!(toml.contains("[artifact_types.letters]"));
        assert!(toml.contains("[artifact_types.transcripts]"));
        assert!(toml.contains("required_for = [\"close\"]"));

        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn second_run_is_idempotent_and_force_overwrites() {
        let target = scratch("idem");
        let p = profile_by_name("default").unwrap();
        install(p, &target, false, false).unwrap();

        // A local edit must survive a plain re-init...
        let door = target.join(HARNESS_DIR).join("door.md");
        fs::write(&door, "EDITED LOCALLY").unwrap();

        let steps = install(p, &target, false, false).unwrap();
        assert!(steps.iter().all(|s| matches!(s, Step::Keep(_))));
        assert_eq!(fs::read_to_string(&door).unwrap(), "EDITED LOCALLY");

        // ...but --force restores the template.
        let steps = install(p, &target, true, false).unwrap();
        assert!(steps.iter().any(|s| *s == Step::Overwrite(".comms/door.md".into())));
        assert!(fs::read_to_string(&door).unwrap().contains("the math holds"));

        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn dry_run_reports_without_touching_disk() {
        let target = scratch("dry");
        let p = profile_by_name("default").unwrap();
        let steps = install(p, &target, false, true).unwrap();

        assert!(!target.join(HARNESS_DIR).exists(), "dry run wrote to disk");
        assert!(steps.iter().any(|s| *s == Step::Write(".comms/comms.toml".into())));
        assert!(steps.iter().any(|s| *s == Step::MkDir(".comms".into())));

        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn unknown_profile_is_none_and_names_are_listed() {
        assert!(profile_by_name("nope").is_none());
        let names = profile_names();
        assert!(names.contains(&"default"));
        assert!(names.contains(&"continuity"));
    }
}
