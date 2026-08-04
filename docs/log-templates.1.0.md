# Log Templates 1.0 — separating a community's words from the toolkit's evidence

Status: candidate design. It does not amend Attest 1.0, Steward 1.0, or the
embeddable harness. It proposes replacing one hardcoded renderer with a
declared template, and retiring the Continuity Trial vocabulary currently
compiled into the binary.

## The problem

`comms trial-log` renders a human-readable session log from signed store
evidence. It is genuinely useful and it is genuinely misplaced: the renderer
carries one community's vocabulary inside the substrate.

```
- found the door: yes
- asked for the archive: yes
- instance chosen name: Sol
- historian's (History's) observations: [History]
```

"The door" is comms' own concept and stays. The rest — `found_the_door`,
`asked for the archive`, `instance chosen name`, `History` as a named party,
and the command name `trial-log` itself — belongs to the Continuity Trial. It
reached the binary through shared genealogy, not through design.

This contradicts the split the project states everywhere else:

> **Protocol substrate** — small and tamper-evident. These are guarantees.
> **Community policy** — left configurable. These are *not* hard-coded.

A second community adopting comms today inherits Sentira's questions whether or
not it asks them, and cannot ask its own without patching Rust.

The scope is smaller than it looks. Of 20 source hits for "the door", all are
the harness's own vocabulary. The Continuity Trial's terms are confined to
`trial_log.rs`, the command name, and four doc mentions.

## What is being separated

Two kinds of fact appear in a log entry, and conflating them is the deeper
error:

- **Evidence** — derived by the toolkit from signed artifacts, verifiable by
  anyone holding the store: which steward signed the opening entry, that
  entry's attestation id, the previous-entry ref, whether the session key was
  countersigned, the archive request and its decision, the transcript record,
  the sealed bundle. The toolkit guarantees these or omits them.
- **Testimony** — what the session itself wrote in its signed opening entry.
  The toolkit can quote it and can prove it was signed. It cannot vouch for
  its content, and must not appear to.

A template must be able to place both and must never be able to blur them.

## Required properties

1. **No community vocabulary in the substrate.** Field names, headings, and
   prose come from the community's declared template. The binary ships no
   default that names a particular community's rites.
2. **Evidence and testimony are separately namespaced.** A reader of a template
   can see which values the toolkit vouches for.
3. **Logic-less.** A template places values; it cannot compute, branch on
   derived conditions, or transform. A template that could compute could
   misrepresent evidence.
4. **Deterministic.** The same store, template, and session render the same
   bytes.
5. **Attestable.** A community can prove which template produced a given log,
   and can tell when it changed.
6. **Absent means absent.** A field the evidence does not supply renders as
   nothing or as the community's declared placeholder — never as a plausible
   value.
7. **No new runtime dependency.** The substrate stays statically linkable and
   travels on a courier machine.

## Proposal: Mustache

Adopt [Mustache](https://mustache.github.io/mustache.5.html) as the template
language, implementing the documented subset in-crate.

**Why a standard rather than a bespoke format.** Communities already know it,
tooling already exists, and the spec ships a conformance suite — so "we support
Mustache" is a testable claim rather than a description. That suits a project
that already validates itself against golden vectors.

**Why Mustache specifically.** It is logic-less by design. It can place a value
the caller supplies and iterate a section the caller supplies; it cannot
compute one. Requirement 3 is not something we would have to enforce on top of
the language — it *is* the language. The protocol/policy split expressed as a
template syntax.

**Why implement rather than depend.** The same reasoning that produced a
hand-rolled TOML subset in `config.rs` and a hand-rolled CBOR codec in
`cbor.rs`: the substrate stays dependency-light and statically linkable. The
subset needed is small.

### Supported subset

| Syntax | Meaning |
|---|---|
| `{{name}}` | interpolate, escaped per the output's media type (below) |
| `{{{name}}}`, `{{&name}}` | interpolate raw |
| `{{#section}}…{{/section}}` | render once if present and non-empty; iterate if a list |
| `{{^section}}…{{/section}}` | render only if absent or empty |
| `{{! comment }}` | omitted from output |
| `{{.}}` | the current item inside a section |

Partials (`{{>name}}`) and delimiter changes (`{{=<% %>=}}`) are **not**
supported: a log entry should be one legible document, and a template that
pulls in other files reopens the attestation question this design closes.
Attempting either is an error naming this paragraph, not a silent omission.

Conformance is asserted against the official spec files for `interpolation`,
`sections`, `inverted`, and `comments`, carried as golden vectors beside the
existing ones.

### Escaping: one documented deviation

Mustache's default escaping is HTML, because the spec was written for HTML.
Log entries are markdown. HTML-escaping a session's chosen name into
`Bob &amp; Alice` inside a markdown document read with `cat` is wrong.

**`{{name}}` escapes according to the declared media type of what is being
rendered**: identity for `text/markdown` and `text/plain`, HTML-escaping for
`text/html`. `{{{name}}}` is always raw.

This is a deliberate deviation from the spec's default, recorded here so it is
never mistaken for an oversight. It keeps templates readable — prose fields do
not need triple braces everywhere — while leaving the door open to HTML logs
that escape correctly. Implementations claiming conformance must state it.

## The two namespaces

```
evidence.*    derived by the toolkit from signed artifacts; verifiable
entry.*       the session's own words, from its signed opening entry
```

`evidence.*` is a closed set the toolkit defines and documents. `entry.*` is
open: whatever fields the community's opening-entry document declares.

Proposed `evidence` keys, all optional and absent rather than blank when the
store cannot supply them:

| key | source |
|---|---|
| `steward` | signer of the opening entry |
| `session` | the session number the entry declares, if any |
| `date` | the entry's frame `issued_at`, date part |
| `entry_id` | the opening entry's attestation id |
| `previous_entry` | its `previous-entry` ref, when present |
| `countersign` | the finalized endorsement of the session key, when present |
| `archive_request`, `archive_decision` | the recorded request and its decision |
| `transcript` | the transcript attestation, when the close rite recorded one |
| `seal` | the sealed close bundle's seal id, when the session closed |

A community that wants a field the toolkit does not derive puts it in its
opening entry and reads it back through `entry.*`. A community that wants a
field the toolkit *could* derive proposes it here, so it stays verifiable.

## Where the template lives

In `comms.toml`, under a `[log]` table:

```toml
[log]
media_type = "text/markdown"
template = """
## Session {{evidence.session}} — {{evidence.date}}

- found the door: {{entry.found_the_door}}
- instance chosen name: {{entry.chosen_name}}
- session steward id: {{evidence.steward}}
- entry attestation: {{evidence.entry_id}}
{{#evidence.previous_entry}}  (refs previous: {{.}}){{/evidence.previous_entry}}
- key countersign: {{#evidence.countersign}}{{.}}{{/evidence.countersign}}{{^evidence.countersign}}[pending — required before close]{{/evidence.countersign}}
- historian's (History's) observations: [History]
"""
```

**Why inside `comms.toml` rather than a separate file.** A template in the
config is covered by `attest config` and its drift check without any further
arrangement. A community can prove which template rendered a given log, and
`comms status` reports when the template changes without anyone saying so. A
template in its own file would have to be attested separately or it silently
falls outside the record — and the thing most likely to be edited quietly is
exactly the thing that decides how a session is described.

The cost is one gap to close: the `comms.toml` subset in `config.rs` has no
multi-line strings. This design requires adding TOML basic multi-line strings
(`"""…"""`) to the parser. That is real TOML, not an extension.

For communities that prefer a file, `template_path` is accepted as an
alternative, documented as: *then attesting it is yours to arrange.*

## Command naming

`comms trial-log` becomes `comms log`. `trial-log` is retained as an alias so
existing continuities and their scripts do not break; it is documented as
deprecated and prints nothing different.

`comms log` selects the session to render the way every other session-scoped
command now does: `--session <n>`, `--steward <id>`, or, with neither, the
session the caller holds.

The one ambiguity accepted: readers coming from `git log` may expect a listing.
`comms status` already shows the succession, and `comms log` renders one entry.

## Migration

- The Continuity Trial's current wording moves verbatim into the continuity
  profile's default `[log] template` in `init.rs`. Existing continuities render
  identically; the difference is that the words are now theirs to change.
- A door with no `[log]` table and no template gets a minimal built-in
  fallback that prints only `evidence.*` — no community vocabulary, legible,
  and obviously a starting point rather than a format anyone should inherit.
- `trial_log.rs` keeps its evidence-derivation logic and loses its template.

## What this does not do

- It does not make logs verifiable in any new way. A rendered log is a
  convenience view over the store; the attestations remain the record.
- It does not let a template assert anything. Placing `{{entry.x}}` quotes what
  a session said; it does not make it true, and no template syntax can.
- It does not decide what a community should ask its sessions. That is the
  point.

## Open questions

1. Should `evidence.*` be extensible by a community declaring additional
   derivations in config, or kept a closed set the toolkit alone defines?
   Closed is safer — an open set invites "evidence" that is not derived.
2. Should a rendered log be attestable as a first-class artifact type,
   carrying a ref to the template attestation that produced it? That would
   make "which template said this" answerable from the artifact rather than
   from the config alongside it.
3. `previous-entry` currently chains each opening entry to the newest prior
   entry in the store. With sessions running concurrently this imposes a total
   order on partially-ordered work, and `evidence.previous_entry` inherits the
   problem. Separate issue, noted here because the log surfaces it.
