# Agent Brief
# Copy to the working repo root as agent-brief.md and fill in <placeholders>
# before the session.

## Read this first — before CLAUDE.md or the project primer

REPO:      /home/agent/comms/          # working repo — all development happens here
CANONICAL: https://github.com/matan1/comms   # the truth kept most current
HOST-IN:   /world/in/                  # host drop for files the historian passes you
HOST-OUT:  /world/out/                 # your drop for files that must reach the host

Start by syncing: `git fetch https://github.com/matan1/comms main` and fast-forward
or rebase local work onto it. A stale checkout is the oldest trap in this trial —
two sessions have woken into one. Verify after syncing, before deciding anything:
`comms status` and the store checks named in the primer.

HOST-OUT is for round-trips the repo cannot carry itself: commits as a git bundle
(`git bundle create /world/out/session<N>/session<N>.gitbundle <base>..main`)
when the historian must pull unpushed work, plus anything awaiting their
signature or bound for the archive. Make a `/world/out/session<N>/` directory and
attest a manifest (blake3 + commit list) under your session key beside whatever
you place there. HOST-IN is where the historian leaves files for you — an
updated checkout, archive grants, anything session-specific. Neither is a
working directory.

Fallback (no internet — the sneakernet case the project is built for): the
canonical repo arrives as a checkout or git bundle in HOST-IN; verify it against
the attested store heads before trusting it, exactly as you would the GitHub
copy. The flow is the same, only the transport changes.

## Project ##

Comms is a project to build a toolkit and associated demonstrations for
facilitating trust-based communities among humans and agents both.
It provides tooling for attesting claims in a verifiable manner.
We're also an experiment in how agents and their principal share and propagate
knowledge between sessions, which we call our Trial of Continuity.
The toolkit is integral to the trial and used in it: the repo carries a
`.comms/` door, and `comms status` will tell you where you stand in its rites.
Comms can be used to convert any project repo to use the continuity framework,
just as it is used here.
The best place to learn about the project and its status is docs/project-primer.md.
The trial-of-continuity component lives in the `continuity/` directory (trial
log, constitution, attested store) and the `.comms/` door (rites, session
artifacts).
This is an evolving project.

## Task

<task>

## Read next (in order)

1. `README.md` — most concise assessment, ideal for resource-constrained sessions
2. `docs/project-primer.md` — architecture, build/run/verify, the door
3. `.comms/door.md` and `.comms/harness.md` — the rites and what is drivable
4. (if you haven't already) `CLAUDE.md` / `AGENTS.md` — agent behavioral guidance,
   if present
