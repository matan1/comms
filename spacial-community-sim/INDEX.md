# Mote-simulation scene portfolio

Ten `schema_version = 3` scenes, each isolating one evolutionary pressure.
All use only documented mechanics: static fields via the legacy `left`/`right`
emitter slots driven by `[feedback]` constants, mote physics as `[feedback]`
constants, and the spawn-credit scheduler as the death-cost lever. Values are
clamp-checked against the primer.

Roughly ordered easy → hard, then by exotic regime:

| # | Scene | Isolates | One-line bet |
|---|-------|----------|--------------|
| 01 | the-garden | nothing (control) | abundant food, instant respawn — proves the world is wired right |
| 02 | the-oasis | spatial scarcity | small off-centre food + slow respawn makes death expensive and survival legible |
| 03 | food-and-poison | field polarity | identical-but-signed fields force discovery of *sign*, not just "go to a field" |
| 04 | the-gauntlet | hazard traversal | only food is across a poison band — dash through or skirt the rim |
| 05 | tortoise-and-hare | mutation regime | two pools, same pond, low- vs high-mutation churn compete |
| 06 | the-fountain | station-keeping | open ceiling + gravity + high food → a narrow anti-gravity hover survives |
| 07 | the-void | motor economy | zero-g, frictionless, weak kicks → anticipatory, economical impulses win |
| 08 | the-crucible | metabolic edge | savage burn + tight food + fast turnover → rapid, legible evolution at the cliff |
| 09 | two-islands | geographic divergence | two foods, barren desert between — allopatry by distance (safe, 2 emitters) |
| 10 | the-reef ⚠ | divergence + frontier | islands idea with a draining reef between, via a program-declared 3rd emitter |

## Notes on running them

- **Let them cook.** Convergence is measured in many mote lifetimes. Give each
  scene real time before judging it.
- **The multiplier is the master volume.** Every scene sets `fields.multiplier`
  in the 16–20 band (matching the primer's worked example). If motes starve even
  in the garden, raise it; if nothing ever dies anywhere, lower it. Tune the
  control (01) first, then carry that value across.
- **`lifespan_s` is the pressure dial.** Smaller = faster metabolic burn = harder.
  It's the cliff in the crucible and the comfort in the void.
- **`spawn_rate` is the legibility dial.** Low rates keep the swarm visibly below
  `count` after a die-off, so survival shows up in the headcount.
- **#10 is the only risky one.** It declares a third static emitter inside an
  inline program stub (the primer's documented route for >2 named emitters). If
  that path doesn't install in your build, run **#09** instead — same idea, two
  legacy slots, no program.

## Suggested first pass

Run **01 → 02 → 03** in order to confirm the field scale, then jump to whichever
regime interests you. **08 (the-crucible)** gives the fastest visible evolution
if you want to *see* selection move; **07 (the-void)** gives the strangest
behaviours.
