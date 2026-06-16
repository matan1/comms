# Agent Brief
# Copy to /workspace/source-ro/agent-brief.md and fill in <placeholders> before the session.

## Read this first — before CLAUDE.md or the project primer

SOURCE:   /workspace/source-ro/tasks/comms/<task>/
REPO:     /home/agent/work/comms/<task>/repo/
STAGING:  /workspace/agent-rw/tasks/comms/<task>/

REPO is your working directory — it is already checked out and is where all development
happens. Do all file editing, building, and committing here.

STAGING is a host-mounted directory reserved exclusively for pushing completed commits.
Do not use it as a working directory — it is not efficient for incremental file operations,
and the host uses it to test your committed code before it enters the canonical repository
(which is not visible to you). Only push to STAGING when you have commits ready for review.

See /home/agent/work/this.vm.your.task.txt for tooling and VM environment details.

## Task

<task>

## Read next (in order)

1. 'README.md' - most concise assement, ideal for resource constrained sessions
2. `SOURCE/docs/project-primer.md` — architecture, build commands, Gradle/manifest invariants
3. `(if you haven't already) REPO/CLAUDE.md` — code style and agent behavioral guidance
