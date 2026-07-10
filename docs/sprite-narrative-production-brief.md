# Future Brief — Sprite Narratives and Task Demonstrations

Status: production handoff after the Archive Harbor renderer migration spike.

## Outcome

Build short, reproducible animated stories in which expressive real, fictional,
or deliberately idealized characters use Comms tools to complete recognizable
tasks. A story may explain a protocol distinction, demonstrate a real command,
or show the social consequences of evidence arriving late. It should be useful
as documentation without losing the warmth and legibility of a small cartoon.

The production system should reuse simulator events and projection models. It
must not encode a second, animation-only account of what happened.

## First productions

1. A courier delivers a sealed bundle whose attestations verify but whose
   detached body is missing. The custodian requests context rather than treating
   the seal as sufficient authority.
2. A reviewer receives an opaque pending flood, asks one useful clarification,
   declines an evasive proposal, and signs only the selected act.
3. A workstation resident receives a legitimate resource grant while the host
   fails to enforce it; a later unauthorized host action demonstrates the
   opposite divergence.
4. Two communities read identical evidence under different signed policies and
   cooperate for a named purpose without acquiring one global trust system.
5. A practical toolkit walkthrough uses recorded commands and sanitized fixture
   artifacts to initialize custody, request a body, grant delivery, verify the
   result, and inspect the audit record.

## Production model

Represent each story as data:

```ts
interface Storyboard {
  id: string;
  scenario: ScenarioSeed;
  beats: StoryBeat[];
  cast: CastMember[];
  narration: NarrationCue[];
  captions: CaptionCue[];
  taskEvidence: DemonstratedArtifact[];
}
```

A beat selects simulator time, camera framing, projection, visible annotations,
and optional character performance. Dialogue and narration describe the event;
they do not mutate simulation state.

## Visual system

- Use PixiJS sprites, sprite sheets, containers, masks, particles, and camera
  motion for characters and spatial action.
- Permit raster backgrounds, SVG-derived textures, and authored vector maps.
- Keep interface chrome, long text, transcripts, and controls in accessible
  HTML rather than baking them into the scene.
- Give each character a stable identity design independent of temporary role,
  process location, or signing authority.
- Support both restrained documentary art and openly cartoonish idealization.
  Never present an idealized reconstruction as documentary footage.

Required animation vocabulary includes idle, walk, carry, handoff, inspect,
request, sign, decline, wait, celebrate, repair, and exit. Directional variants
may be mirrored only where symbols, handedness, or readable text are unaffected.

## Voice, captions, and accessibility

- Narration is optional; complete captions and a downloadable transcript are
  mandatory.
- Store narration text, timing, speaker, pronunciation notes, and audio asset
  provenance separately.
- Human voices require explicit release and intended-use scope. Synthetic voices
  must not imitate an identifiable person without permission and must be labeled
  where a viewer could reasonably mistake them for a real performance.
- Provide audio description cues for action that is not conveyed by dialogue.
- Respect reduced-motion, pause, scrubbing, mute, caption styling, and keyboard
  navigation preferences.

## Demonstrating real tasks

When a story claims to show the toolkit performing a task:

- run commands against disposable fixtures in an isolated workspace;
- capture command, exit status, relevant output, and resulting artifact hashes;
- redact secrets, private paths, personal data, and live credentials;
- bind each depicted terminal beat to its captured execution record;
- label shortened output and dramatized timing;
- fail production if the demonstrated commands no longer pass.

No sprite, narrator, courier, custodian, or verifier gains authority merely
because the production centers them visually.

## Rendering and export

The interactive browser version is the reference playback. Add deterministic
frame stepping so the same storyboard can be exported to image sequences and
encoded to common video formats without depending on wall-clock timing. Preserve
captions as both burned-in optional output and a separate timed-text track.

Record the simulator commit, scenario seed, storyboard version, asset manifest,
font versions, voice provenance, viewport, frame rate, and output hashes in a
production manifest.

## Testing

- Kernel parity: story playback must not change headless scenario outcomes.
- Projection safety: participant shots contain no simulator-only ground truth.
- Beat contracts: referenced actors, events, bodies, proposals, and receipts
  exist at the selected time.
- Determinism: fixed input produces the same ordered semantic frames.
- Golden frames: maintain a small set of deliberately reviewed visual snapshots.
- Task replay: every real command demonstration succeeds from a clean fixture.
- Accessibility: captions cover all speech; keyboard and reduced-motion paths
  remain operable.

Golden images are a presentation regression aid, not the behavioral oracle.
Semantic projection assertions remain primary.

## Acceptance for the first narrated short

- 45–120 seconds, using the missing-body Harbor scenario.
- At least two expressive characters, one courier handoff, one archive request,
  and one policy-relative decision.
- Voiceover, captions, transcript, audio description, and reduced-motion mode.
- Every visible protocol event resolves to the simulator event that caused it.
- Reproducible interactive playback and deterministic frame-sequence export.
- No participant view reveals hidden harmfulness, adversary type, or omniscient
  policy conclusions.

