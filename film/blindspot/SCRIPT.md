# BLINDSPOT — Life in the Unobserved

**Shooting script and technical specification, v1**

| | |
|---|---|
| Format | Photoreal wildlife documentary (pastiche), 16:9, 24fps |
| Runtime | 2:52 |
| Structure | 3 chapters × 1 specimen, + appendix |
| Generated shots | 20, plus alternates on Specimen 001 |
| Narration | 258 words, 28 lines, 59% speaking density |
| Dialogue | None. Single narration track, no sync sound. |
| On-screen text | 100% post. Never generated. |

Timings are not written into this document. `manifests/timeline.json` is the
source of truth and `python3 scripts/timing.py` prints the current cut.

**Premise.** A prestige natural-history survey of four organisms whose biology
violates identity, time, or observation. Played entirely straight. The narration
never marvels, never jokes, and never acknowledges that anything is impossible —
it reports logistics.

**Register rules (non-negotiable, these are the film):**

1. State each impossibility **exactly once**, as a logistical detail, then move on.
2. Use the real hedged vocabulary of the genre: *it is thought*, *no satisfactory
   explanation*, *the Survey records*, *a complete recording is not expected*.
3. Never let the narrator marvel. The moment there is wonder in the voice, the
   film becomes a sketch.
4. Every impossibility is **ontological, never morphological.** No hybrid animals,
   no invented anatomy. A moth is an ordinary moth. This is the entire defence
   against the saturated AI-fake-nature-doc genre — we do not share its visual
   signature.
5. The Survey is a real institution with staff, records and maintenance costs. It
   is referenced throughout. Something must be keeping the count for the last
   line to land.

---

## 1. GLOBAL BLOCKS

Pasted **verbatim into every video prompt**. Redundancy is deliberate: these
models weight recent context heavily, so global constraints must be restated per
shot rather than assumed from a header. Machine-readable copies live in
`manifests/blocks.json`.

### 1.1 STYLE (LOCK)

The anti-slop mechanism. A prestige wildlife documentary has a specific optical
signature — long lens, shallow focus, ambient light, a human operator who does
not quite anticipate the movement. Generated slop has the opposite signature:
impossible flying camera moves, everything in focus, subject dead centre,
over-lit. Encoding the real signature into every shot buys authenticity on every
frame rather than only in the cold open.

```
STYLE (LOCK - identical in every shot): Photoreal wildlife documentary footage,
BBC Natural History Unit house style, mid-2010s digital broadcast capture.
RENDERING: naturalistic colour, desaturated cool greens greys and browns, no
grading flourish, no teal-and-orange, no stylisation; exposure metered for
ambient light only - no artificial, supplementary or practical light anywhere in
frame. OPTICS: long telephoto lens, 400-600mm equivalent, heavy background
compression, very shallow depth of field, subject isolated against soft
unreadable background bokeh, faint optical vignetting, occasional focus hunt.
CAMERA LANGUAGE: operated by a human being from a fixed hide - locked off on a
heavy tripod, or a slow deliberate pan that LAGS slightly behind the subject.
The operator does not anticipate movement perfectly and does not reframe
elegantly. No drone moves, no dollies, no cranes, no gimbals, no whip pans, no
impossible camera positions, no camera move that a person with a heavy long lens
on a tripod could not perform. MOTION: 24fps, 180-degree shutter, natural motion
blur, real time - no slow motion unless explicitly specified in the shot.
PHYSICS: real weight, real gravity, real fluid behaviour, real wind in
vegetation.
```

### 1.2 BEHAVIOUR (LOCK)

The inverse of a conventional acting rule. Normal direction pushes for legible
emotion; we push for the absence of event. The standing temptation is a dramatic
moth, and a dramatic moth is slop.

```
BEHAVIOUR (LOCK): The animal is entirely unaware of the camera and does nothing
dramatic. Movement is mundane, incidental and uneventful. No charging, no
display, no confrontation, no eye contact with lens, no anthropomorphic
expression, no heroic silhouette. Nothing in the frame is staged. This is
observational footage of an animal not doing very much.
```

### 1.3 NEGATIVE (LOCK)

```
NEGATIVE: no people, no human figures, no hands, no text, no captions, no
watermarks, no logos, no UI, no split screen, no montage, no multi-panel layout,
no drone or aerial motion, no lens flare, no artificial lighting, no colour
grading, no slow motion, no time-lapse, no hybrid or invented creatures, no
fantasy anatomy, no glowing eyes, no anthropomorphism, no music, no centred
hero-portrait composition.
```

### 1.4 ATTACHMENT NOTE — fal Assets

Each reference is saved as a named fal asset and `@`-tagged, so identity is
locked once and never re-uploaded. Every asset has exactly one job, spelled out.
Full prompts in `manifests/sheets.json`.

| Tag | Job |
|---|---|
| `@hedgerow` | Location for Specimens 001 and 004. Somerset, late autumn, no human presence. |
| `@moth` | Specimen 001 — defines **appearance only**, not visibility. See Composition Lock 001. |
| `@whale` | Specimen 002 — the callosity pattern, which is the identity key. |
| `@n_atlantic` | Heavy grey sea, force 4, overcast, no land. |
| `@s_pacific` | Kaikōura: blue-green water, low sun, faint range on the horizon. |
| `@thrush` | Specimen 003 — an ordinary song thrush. |
| `@branch` | The Y-fork with a broken left stub. Continuity anchor across four hundred years. |

The whale's callosity pattern is worth noting: cetacean researchers genuinely
identify individual right whales this way. The mechanism keeping our clips
consistent is the same evidence a real scientist would cite to claim these are
one animal. A specialist also gets a second, quieter joke — a right whale in both
the North Atlantic and the South Pacific is already impossible before you reach
the identity problem.

### 1.5 Storyboards

Nine-panel boards are generated **for planning only**, to approve shot order and
geometry before any video spend. They are **never attached to a video
generation** — a board plus a text prompt gives the model two competing sources of
truth and it averages them.

Single-frame conditioning is a different thing and is permitted: `image_url` off
one approved plate is one source of truth for frame one. Multi-panel layouts are
not.

---

## 2. AUDIO ARCHITECTURE

**No native audio is used from any video model.** Clips are generated mute where
the endpoint allows it, and anything returned is discarded — five clips from a
generative model give five different room tones and five different implied
microphones, none of which can be levelled against one narration track.

**Every shot still carries an SFX line in its prompt.** That line is an *input*,
never an output we keep. Telling the model what a shot sounds like makes it
decide when things happen, which is the cheapest timing control available. We
write it, we use its effect on the picture, we throw the audio away.

| Layer | Source | Notes |
|---|---|---|
| Narration | fal TTS, one voice, one pass | Generic British RP. **Not an Attenborough clone** — a recognisable clone makes the film about the impersonation, and he is a living person. |
| Field ambience | fal audio, or synthesised | One bed per location, −24 to −28dB. Hedgerow: wind in dry leaves, distant rook. Sea: swell and wind, no gulls. |
| Archive layer | Synthesised in ffmpeg | The 1890 shellac degradation: bandpass 300–3200Hz, wow and flutter, hard compression. No model required. |
| Room tone | Synthesised | Pink noise at −42dB under everything, so cuts to silence read as recorded quiet rather than as a dropout. |

---

## 3. TYPOGRAPHY

All text is rendered in post by `scripts/cards.py`. Models still mangle on-screen
lettering, and this film's credibility lives in its chyrons.

| Element | Font | Treatment |
|---|---|---|
| Chapter cards | Tinos | Roman numeral and title, tracked capitals, centred on near-black |
| Specimen chyron | Liberation Sans | Lower third, left aligned, 60% black plate, hairline rule |
| Binomial | Liberation Sans Italic | Beneath the specimen number |
| Location stamp | Liberation Mono | Upper right, small |
| End slate | Tinos | Centred |

The chyron template is the same for all four specimens. The audience is trained
on it for five minutes so that `APPENDIX A` arrives on furniture they already
trust:

```
SPECIMEN 001
Noctuella evanescens
KNOWN POPULATION: —
```

---

## 4. THE SCRIPT

Narration is marked **VO**. Everything in `[brackets]` is post. Durations are cut
lengths.

---

### COLD OPEN — 7s

`[SLATE — near black, Tinos, centred]`

```
THE SURVEY

RECORD OF SPECIES NOT RELIABLY OBSERVED

VOLUME IV
```

Opening on typography rather than a hero animal plate is deliberate. The
saturated genre we sit next to announces itself in the first two seconds with a
money shot; we announce an archive.

### SHOT 01 — 8s

**Action.** Wide hedgerow in late autumn. Overcast, thin ground fog, wet leaf
litter. Wind moves the bramble. Nothing else happens. No animal enters.

**Camera.** Locked off, heavy tripod, long lens. Static throughout. No reframe.

**SFX** *(input only)*. Wind in dry hawthorn, water dripping from leaves, a rook
calling twice at distance.

**VO.** Four species that cannot be reliably observed. The following footage is
the best available.

---

`[CHAPTER CARD — 5s]`  `I.  PROBLEMS OF OBSERVATION`

---

## SPECIMEN 001 — *Noctuella evanescens*

`[CHYRON: SPECIMEN 001 / Noctuella evanescens / KNOWN POPULATION: —]`

### COMPOSITION LOCK 001

Pasted into every Specimen 001 prompt. **Without this block the segment fails.**
Centring the subject is the single most trained-in behaviour these models have,
and a beautifully centred macro moth destroys the premise.

```
COMPOSITION LOCK (Specimen 001 - identical in every shot): The moth is NEVER in
the centre third of frame and NEVER in critical focus. It appears ONLY as (a) a
partially cropped form at the extreme edge of frame, cut off by the frame line,
(b) a soft out-of-focus pale shape drifting through the extreme foreground, or
(c) not at all - an empty plate. The camera drifts or racks toward where it was
and finds empty leaf litter. NEVER centre the subject. NEVER bring it into
focus. NEVER show a whole moth. The moth does not approach camera, does not
land in frame centre, and is never fully visible. Never flip or relax this.
```

### SHOT 02 — 8s

**Action.** Wet leaf litter and the base of a bramble stem, long lens, very
shallow focus. Empty. A single dry leaf shifts in the wind. Nothing arrives.

**Camera.** Locked off at ground level, background compressed to unreadable bokeh.

**SFX.** Close wind in leaf litter, one wingbeat flutter off-frame that is never
sourced.

**VO.** A hedgerow in Somerset. Eleven weeks of continuous filming.

**VO.** *Noctuella evanescens* is present in this frame.

### SHOT 03 — 7s

**Action.** Slow pan right across bramble and hawthorn. Empty. The pan lags — it
arrives late at each part of the hedge, as though following something it cannot
keep up with.

**Camera.** Slow deliberate tripod pan right, lagging, imprecise. Ends on an empty
twig.

**SFX.** Tripod head friction, wind, the same unsourced flutter, now to the right.

**VO.** It cannot be filmed directly. The species is absent from wherever the
operator is attending.

### SHOT 04 — 7s

**Action.** Background twigs held in focus. A soft pale out-of-focus form drifts
through the extreme foreground, bottom-left, and exits frame left. Never
identifiable. Focus never changes to follow it.

**Camera.** Locked off. Focus stays on the background. The operator does not react.

**SFX.** A single close wingbeat, very brief, passing left.

**VO.** Only at the frame edge. Only out of focus.

**VO.** That is the specimen.

### SHOT 05 — 5s

**Action.** Rack focus from a mid-ground twig to a nearer twig. Both empty. Wet
bark, a trembling leaf.

**Camera.** Locked off, one deliberate focus pull with a slight overshoot and
correction — a human hand on the barrel.

**SFX.** Lens barrel, wind, no wings.

**VO.** No centred image was obtained. This is routine.

### SHOT 06 — 6s

**Action.** At the extreme right edge of frame, a partially cropped pale wing edge
rests against a stem, bisected by the frame line, mostly out of frame and soft.
It does not move. The rest of the frame is empty hedgerow in focus.

**Camera.** Locked off. No reframe toward it. Never corrected.

**SFX.** Wind only. The absence of wing sound is deliberate.

**VO.** Automatic cameras, with no operator present, return empty frames.

### SHOT 06B — 8s

**Action.** Empty leaf litter again, marginally different framing, last light.

**Camera.** Locked off, long lens, static.

**SFX.** Wind dropping, distant rook.

**VO.** The requirement is not the camera. It is the attention.

**VO.** The Survey lists it as common.

---

`[CHAPTER CARD — 5s]`  `II.  PROBLEMS OF IDENTITY`

---

## SPECIMEN 002 — *Eubalaena indivisa*

`[CHYRON: SPECIMEN 002 / Eubalaena indivisa / KNOWN POPULATION: 1]`

The population figure is the joke and it is never mentioned aloud.

### SPATIAL GEOGRAPHY LOCK 002

Restated in every Specimen 002 prompt. The matched-timecode cut only reads as one
animal if the surfacing motion matches across it.

```
SPATIAL GEOGRAPHY (LOCK - identical in every shot): The whale surfaces moving
FRAME-LEFT to FRAME-RIGHT, dorsal side toward camera, head entering frame left
first. Camera is on a vessel to the animal's LEFT flank, shooting across it.
The whale NEVER swims toward camera, NEVER right-to-left, NEVER shows the
ventral side, NEVER breaches. It surfaces, blows, and submerges on the same
axis. Never flip these directions between shots.
```

**No split screen.** Split screen is television-news grammar and punctures the
prestige-documentary frame the whole film depends on. The two oceans are cut
together straight, with matched location and time stamps upper-right doing the
work. Never ask a model for split screen; never ask the edit for it either.

### SHOT 07 — 6s

`[STAMP: NORTH ATLANTIC · 04:11 GMT]`

**Action.** Heavy grey sea. A broad black back breaks the surface left to right,
blows — the V-shaped spout of a right whale — and begins to submerge. Cold flat
light, wind-torn spray.

**Camera.** Long lens from a vessel, handheld weight but not shaky, sea motion
under the operator, slight lag behind the animal.

**SFX.** Wind across the mic, swell, the low wet exhalation of the blow.

**VO.** Eleven minutes past four. A right whale surfaces west of the Hebrides.

### SHOT 08 — 5s

**Action.** Long lens detail: the rostrum, water streaming off the white callosity
pattern — one large patch above the lip, two islands behind the blowholes, a
crescent over the right eye. Cold grey light.

**Camera.** Locked long lens, very shallow focus on the callosity, sea moving
beneath.

**SFX.** Water sheeting off skin, swell.

**VO.** *None.* Held in silence for the whole shot, so the audience can compare
this pattern to the one in Shot 10. This silence is load-bearing and the pacing
check protects it.

### SHOT 09 — 6s

`[STAMP: SOUTH PACIFIC · 04:11 GMT]`

**Action.** Clearer blue-green water, low golden morning sun, faint snow-capped
range on the horizon. A broad black back breaks the surface left to right, blows,
submerges. Identical action, entirely different ocean.

**Camera.** Matched to Shot 07 — same lens, same left-flank axis, same lag.

**SFX.** Calmer swell, warmer air, the same low exhalation.

**VO.** Eleven minutes past four. A right whale surfaces off the Kaikōura Trench.

*Deliberately near-identical phrasing to the previous line. The repetition is the
evidence.*

### SHOT 10 — 7s

**Action.** The rostrum again, water streaming off it. The same callosity pattern,
in warm morning light.

**Camera.** Matched to Shot 08.

**SFX.** Water sheeting, calmer sea.

**VO.** The callosity pattern is identical. Callosities identify an individual.

**VO.** The Survey records one animal.

### SHOT 11 — 10s

**Action.** Empty sea and horizon. Nothing surfaces.

**Camera.** Locked long lens on the horizon, sea motion, no subject.

**SFX.** Wind, swell, no blow.

**VO.** Neither body has been observed to travel.

**VO.** Catalogued separately for nineteen years, until the night both were
filmed at once.

### SHOT 11B — 6s

**Action.** Empty grey sea, marginally different framing, light beginning to go.

**Camera.** Locked long lens, no subject.

**SFX.** Wind, swell, nothing else.

**VO.** It is not known what the animal was before that.

This is the only foreshadow in the film, and it is doing structural work: it
establishes that in this world **observation does things**, so the appendix is a
rule the audience has already been taught rather than a twist.

---

`[CHAPTER CARD — 5s]`  `III.  PROBLEMS OF TIME`

---

## SPECIMEN 003 — *Cantator saecularis*

`[CHYRON: SPECIMEN 003 / Cantator saecularis / KNOWN POPULATION: UNCOUNTED]`

This segment's impossibility lives in the **audio**. The picture's only job is to
establish that it is the same branch.

### SHOT 12 — 6s

`[STAMP: WAX CYLINDER · 1890]`
`[POST: sepia, 4:3 pillarbox, gate weave, dust, heavy grain — generated clean and
aged entirely in post]`

**Action.** A song thrush on a hawthorn branch — the Y-fork with the broken stub on
the left limb. Beak open mid-note, throat distended. Almost no movement.

**Camera.** Locked off, long lens, static. A period plate that barely breathes.

**SFX.** Shellac surface noise, then a single sustained bird note, bandlimited.

**VO.** A wax cylinder, 1890. A hedgerow outside Ludlow.

### SHOT 13 — 6s

`[POST: crossfade from Shot 12 over 1.5s — same framing, sepia resolving to modern
colour, pillarbox opening to 16:9]`
`[STAMP: FIELD RECORDING · APRIL]`

**Action.** The same branch, the same Y-fork, the same broken stub, in modern
broadcast colour. A song thrush mid-note. Wet spring hawthorn.

**Camera.** Matched exactly to Shot 12.

**SFX.** Clean modern field recording, a single sustained note at a different pitch
to the 1890 note.

**VO.** Last April. The same hedgerow.

The crossfade is the whole segment. One generated still, aged in post for 1890
and image-to-video for the modern plate — the same asset, two centuries, and the
cheapest high-impact beat in the film.

### SHOT 14 — 8s

**Action.** Wide hedgerow at dusk. The branch is empty.

**Camera.** Locked off, long lens, static.

**SFX.** Evening ambience, no birdsong at all.

**VO.** Consecutive notes of the same song.

**VO.** The song takes three to four hundred years. The Survey holds nine notes.

### SHOT 14B — 8s

**Action.** The hedgerow at dusk, tighter, wet leaves. No bird, no movement beyond
the wind.

**Camera.** Locked off, long lens, static.

**SFX.** Wind only.

**VO.** No individual has heard more than one note.

**VO.** The species is not thought to know that it is singing.

---

`[CHAPTER CARD — 5s]`  `APPENDIX A`

---

Not `IV.` — an appendix. The film has run out of chapters and is now filing
something it could not classify. It arrives on the same furniture as everything
else, which is why it works.

## SPECIMEN 004 — classification pending

`[CHYRON, same template as all previous:
SPECIMEN 004 / CLASSIFICATION: PENDING / KNOWN POPULATION: 312]`

### SHOT 15 — 8s

**Action.** The hedgerow from Specimen 001, wider, at last light. Wet leaf litter,
thin fog. Empty. Nothing enters.

**Camera.** Locked off, long lens, static, one faint involuntary focus hunt.

**SFX.** Wind in dry leaves. No wings, no birds.

**VO.** One further species. It has not been named.

**VO.** No footage exists. It has never been photographed.

### SHOT 15B — 6s

**Action.** The same hedgerow, marginally tighter, last light almost gone. Empty.

**Camera.** Locked off, static.

**SFX.** Wind almost nothing.

**VO.** It comes into existence upon being observed. One instance per observer.

### SHOT 16 — 10s

**Action.** Identical framing to Shot 15B, last light going. Still empty. Held
slightly too long.

**Camera.** Locked off, static, no reframe.

**SFX.** Wind dropping to near silence.

**VO.** At the beginning of this programme, the known population was three
hundred and twelve.

`[two seconds of silence]`

`[CHYRON: the population figure ticks 312 → 313. No sound. No sting, no zoom, no
music. The number simply changes.]`

`[HARD CUT to black]`

---

`[END SLATE — 6s]`

```
THE SURVEY

VOLUME IV OF NINE
```

**VO.** The Survey thanks you for your attention.

---

### Notes on the ending

The film opens and closes on the same empty frame. Specimen 001 cannot be
observed; Specimen 004 exists only because it was. Identical shot, inverted
meaning, and it costs nothing but a second plate of the same hedgerow.

Two things are deliberately withheld. There is **no menace** — no drone move
toward a window, no whisper, no sting. The horror is arithmetic and it is
delivered by a chyron. And the last word is **attention**, which Specimen 001
established as the mechanism forty seconds in. That callback is what makes the
taxonomy a system rather than a list.

`312 → 313` is a single increment, not a crowd. One instance per observer,
singular address.

---

## 5. CONTINUITY LEDGER

| Anchor | Must match across | Enforced by |
|---|---|---|
| Callosity pattern | Shots 07, 08, 09, 10 | `@whale` asset, restated in every prompt |
| Surfacing direction (L→R, dorsal to camera) | Shots 07, 09 | Spatial Geography Lock 002 |
| Y-fork branch with broken left stub | Shots 12, 13 | `@branch` asset, one shared still |
| Hedgerow identity | Shots 01, 02, 03, 05, 06, 06B, 14, 14B, 15, 15B, 16 | `@hedgerow` asset, last-frame conditioning |
| Framing identity | Shots 15, 15B, 16 | Each conditioned on the previous shot's final frame |
| Moth never centred, never sharp | Shots 02–06B | Composition Lock 001 |

Continuity across plates uses the previous approved clip's final frame as the
conditioning image, never a storyboard panel.

## 6. PRODUCTION TRIAGE

| Segment | Difficulty | Why | Approach |
|---|---|---|---|
| 001 Moth | **Trivial** | Empty macro plates and focus pulls. The flagship is also the loss-leader — generate a dozen, keep four. | Text-to-video, high volume, cheap model |
| 003 Bird | **Easy** | One still, reused twice. The impossibility is in the audio. | One image → i2v, aged in post |
| 002 Whale | **Medium** | Callosity consistency across four shots and two lighting conditions. | Lock a still, i2v, distinctive backlit silhouette |
| 004 Appendix | **Trivial** | Reuses 001's location, conditioned on its own previous frame. | i2v chain |
| ~~Chorus Elk~~ | **Cut from v1** | Coherent multi-animal unison is where diffusion video fails hardest, and quadruped gait is the worst case. Redundant with the whale now that Specimen 002 is about observation completing identity. | Held for v2 |

**Honest risk.** Photoreal broadcast documentary is the least forgiving mode
available — a stylised painterly film hides model artefacts inside its style, and
we have no style to hide behind. Every error reads as an error. Mitigations are
built into the style block rather than bolted on: long-lens shallow focus buries
background artefacts, overcast and fog diffuse lighting errors, short clips carry
one motion each, and every chosen subject — moths, water surfaces, a bird on a
branch, a whale's back breaking the surface — is something these models render
well. Nothing in v1 requires a walking animal.

## 7. SHOT COUNT AND SPEND

| | |
|---|---|
| Reference sheets (images) | 7 |
| Planning boards (images, 9-panel) | 4 |
| Hero video shots | 20 |
| Alternates budgeted | 2 each on shots 02, 04, 06 |
| Realistic total video generations | 26–30 |
| Narration | 258 words, 28 lines, one TTS pass |

Iterate every prompt on the cheapest usable video model until the framing and
motion are right, then re-run the locked prompt on a premium model. Prompts are
portable across fal video endpoints — only the endpoint string changes — so
nothing is wasted by drafting cheap.
