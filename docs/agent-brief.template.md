# Agent Brief
# Copy to /workspace/source-ro/agent-brief.md and fill in <placeholders> before the session.

## Read this first — before CLAUDE.md or the project primer

SOURCE:   /workspace/in/comms/ #canonical read only repo
REPO:     /home/agent/comms/ #working repo, make changes here
STAGING:  /workspace/out/ #where to put files that need testing on host, staging for upstreaming or archiving

REPO is your working directory — it is already checked out and is where all development
happens. Do all file editing, building, and committing here.

STAGING is a host-mounted directory reserved exclusively for pushing completed commits.
Do not use it as a working directory — it is not efficient for incremental file operations,
and the host uses it to test your committed code before it enters the canonical repository
(which is not visible to you). Only push to STAGING when you have commits ready for review.

## Project ##

Comms is a project to build a toolkit and associated demonstrations for facilitating trust based communities among humans and agents both
It provides tooling for attesting claims in a verifiable manner.
We're also an experiment in how agents and their principal share and propogate knowledge between sessions, which we call our Trial of Continuity.
The toolkit is integral to the trial and used in it.
Comms can be used to convert any project repo to use the continuity framework themselves, just as its used here.
The best place to learn about the project and its status is in the docs/project-primer.md intro document.
The trial of continuity component of the repo is largely in the continuity directory, but may increasingly be based in the .comms directory (TBD)
This is an evolving project


## Task

<task>

## Read next (in order)

1. 'README.md' - most concise assement, ideal for resource constrained sessions
2. `SOURCE/docs/project-primer.md` — architecture, build commands, Gradle/manifest invariants
3. `(if you haven't already) REPO/CLAUDE.md` — code style and agent behavioral guidance
