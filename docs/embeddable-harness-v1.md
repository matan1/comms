# Embeddable Comms Harness v1

## Purpose

Make any repo comms-augmented without importing archive contents by default.
The harness installs a small door, session rituals, verification, and optional
archive management.

## Config Precedence

Lowest to highest:

1. built-in defaults
2. `~/.config/comms/templates/default/comms.toml`
3. repo `.comms/comms.toml`
4. archive `comms.toml`
5. CLI flags

`comms init` copies the user template when present, otherwise writes the
built-in default.

## Repo Layout

Committed by default:

```text
.comms/
  door.md
  policy.md
  comms.toml
  store/
  hooks/
```

The repo contains the door and public/safe attestations, not private archive
bodies unless explicitly configured.

## Archive Modes

```toml
[archive]
mode = "external"       # external | ignored-local | encrypted-repo
path = "../project.comms-archive"
index = "sqlite"        # sqlite | none
```

- `external`: default; archive outside repo.
- `ignored-local`: archive under repo but gitignored.
- `encrypted-repo`: encrypted archive blobs committed with grant-gated access.

## Archive Layout

```text
.comms-archive/
  README.md
  comms.toml
  index.sqlite
  letters/
  transcripts/
  memories/
  reminiscences/
  store/
  pending/
  genesis/
  public_keys/
  bundles/
```

Names use stable session numbers first and optional names second:

```text
session-007.codex.md
session-008.unnamed.log
```

If a session chooses a name later, the index records the alias; files need not
be renamed.

## Invariant Rituals

These are built into the harness:

- `init`: create harness boundary.
- `open`: establish session identity/start record.
- `status`: show current ceremony state.
- `verify`: check signatures, hashes, and reachability.
- `close`: end session and seal required artifacts or waivers.

## Artifact Types

Custom artifact types are declared explicitly:

```toml
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
```

`comms init` creates directories for all configured artifact types.

If an artifact is required for a ritual and missing, the ritual fails unless a
waiver is allowed:

```toml
[rituals.close]
allow_waivers = true
```

A waiver should itself be recorded as an attestation when a session key exists.

## Install Flow

```sh
cd sentira
comms init --profile continuity
comms install-hooks --mode warn
comms open --start cold
comms status
comms close --transcript path/to/transcript.log
comms verify
```

Hooks are assistive by default. Blocking behavior must be explicit in
`.comms/comms.toml`.

## Non-Goals for v1

- No automatic archive loading.
- No hard-coded artifact categories beyond the default template.
- No trust decision inside verification; verification means the math holds.
- No plugin/metaprogramming system beyond declared artifact types.
