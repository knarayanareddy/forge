# THE DEPARTMENT OF CONTINUITY — PRODUCTION BIBLE

## 1. Production thesis

This must look like a controlled live-action production that happens to use
generative tools. The surreal events are authored continuity violations inside
an otherwise flawless film.

The image grows cleaner as reality becomes less trustworthy:

| Section | Image state |
|---|---|
| Orientation | 1990s 35mm release print, mild wear |
| Mug | Stable, restrained grain |
| Pedestrian | Cleaner transfer, increasingly exact geometry |
| Identity | Almost pristine |
| Reconciliation | Immaculate new scan, no gate weave or dirt |

No digital glitch vocabulary is permitted.

## 2. Delivery specification

- **Master:** 3840×2160 ProRes 422 HQ, 24fps constant frame rate.
- **Composition:** 1.66:1 active image (3584×2160) centered in 16:9 delivery.
- **Working color:** scene-linear composites; Rec.709 Gamma 2.4 delivery.
- **Audio:** 48kHz, 24-bit stereo master; −16 LUFS web mix; −1.5 dBTP.
- **Subtitles:** separate SRT; never burned into the festival master.
- **Runtime:** 156 seconds ±2 frames.

## 3. Locked visual grammar

Paste this block unchanged into all reference-image prompts:

```text
A late-1990s American public-institution training film photographed on 35mm.
Restrained bureaucratic surrealism, presented earnestly and without horror
lighting. Government interiors with nicotine-cream walls, celadon lower paint,
oxblood vinyl, gray steel furniture and fluorescent practicals. Muted skin
tones, deep but lifted blacks, restrained cyan shadows, weak amber highlights.
Squared compositions, static heavy tripod frames, occasional slow mechanical
dolly. Spherical 28mm, 40mm and 65mm lenses with normal perspective and
institutional depth of field. Ordinary wardrobe, restrained acting, natural
anatomy, real material texture. No glossy advertising light, shallow-focus
beauty imagery, cyberpunk, exaggerated expression, floating objects, arbitrary
camera movement, generated text, VHS damage or digital glitches.
```

Motion prompts are shorter. The approved start frame is the visual source of
truth; motion text describes only action, camera and timing.

## 4. Palette and materials

| Function | Color / material |
|---|---|
| Institutional neutral | nicotine cream `#D8CEAA` |
| Secondary walls | celadon `#829B83` |
| Authority / correction | oxblood `#7C242B` |
| Records / steel | cool gray `#777B79` |
| Premature object | cream ceramic + cobalt stripe |
| Displaced history | scuffed red leather satchel |
| Peter’s recurring marker | dark green umbrella |

Red appears only in approved forms, the binder, the satchel and the final stamp.
It is not a warning color.

## 5. Reference packs

No motion generation begins until every pack passes at 200% magnification.

### Character pack: Mara Voss

- Neutral 65mm headshot, front and both three-quarter views.
- Full-length field costume: gray jacket, charcoal trousers, oxblood notebook.
- Full-length instructor costume: celadon suit, small oxblood scarf.
- Left/right hand reference with red binder.
- Restrained expressions: neutral, concentration, procedural concern,
  recognition without visible fear.
- Immutable markers: dark blunt bob; fine vertical scar under right ear; watch
  on left wrist; no earrings.

### Character pack: Peter Gray

- Gray suit, white shirt, dark knit tie.
- Green umbrella in right hand; newspaper under left arm.
- Small scar above left eyebrow in initial state.
- Neutral walk cycle reference, right-to-left only.
- Identity-drift state: same face and wardrobe, scar absent.

### Character pack: Lina Ortiz

- Burgundy blouse, gray skirt, cream badge.
- Initial state without scar, satchel or airplane.
- Allocated state with Peter’s scar, red satchel and wooden airplane.
- Never changes hairstyle, eye color or badge placement.

### Supporting characters

June, Mrs. Gray and the cafeteria couple each receive a headshot, full-length
costume view and one required performance state. They never carry the burden of
multi-shot facial continuity.

### Prop pack

The mug needs six exact views and cannot be regenerated independently:

- cream stoneware;
- one 14mm cobalt stripe;
- chipped upper-right handle;
- lipstick arc at 11 o’clock;
- hairline glaze crack under base;
- no logo or text.

Also lock the red binder, allocation form, green umbrella, newspaper, red
satchel and wooden airplane.

## 6. Location bible and geography

### Training room

```text
LOCK: Mara faces camera behind the lectern. PROPERTY / EVENT / PERSON drawers
are rear-left. Projector is frame-right. Exit is behind Mara’s right shoulder.
Camera never crosses the lectern axis. Mug is on Mara’s right, frame-left to us.
```

### Payroll office

```text
LOCK: June’s desk faces camera at a shallow angle. Empty mug position is desk
frame-right. Mara enters from frame-left and stands on June’s right. Filing
window rear-center. Nobody crosses behind the desk.
```

### Cafeteria street

```text
LOCK: Couple foreground center. Café window behind them. Peter travels
RIGHT-TO-LEFT in the far pedestrian lane. Clock remains upper frame-right.
Green awning is rear-left. Never mirror or reverse the crossing.
```

### Records office

```text
LOCK: Lunch entrance frame-left. Lina travels LEFT-TO-RIGHT. Peter waits
center-right. Photograph wall rear-center. Mara stands frame-right with binder.
```

### Recognition kitchen

```text
LOCK: Mrs. Gray remains centered in doorway. Peter is frame-left; Lina
frame-right. The allocation form wipes LEFT-TO-RIGHT across the lens. Nobody
changes marks. Eyelines transfer only after the paper wipe.
```

For each location, approve a floor plan, master wide, reverse, two matching
medium angles, lighting diagram and prop map. Derive new angles by editing the
master, not regenerating the room from prose.

## 7. Model strategy

Model names are implementation candidates, not artistic commitments. Confirm
current endpoints and prices before spending.

| Task | Preferred approach |
|---|---|
| Character/prop sheets | Nano Banana Pro or FLUX.2 Pro, two-model bake-off |
| Multi-reference still editing | Nano Banana 2 Edit / Seedream 4.5 Edit |
| Hero face motion | Kling 3 Pro image-to-video |
| Restrained walking | Seedance 2 reference-to-video |
| Locked environmental motion | Kling 3 Standard or a real plate |
| Exact duplicate loop | one approved pass, duplicated in post |
| Narrator | ElevenLabs Multilingual v2, mature American civil-service voice |
| Music | ElevenLabs Music from a sectioned composition plan |
| Room tone / foley | ElevenLabs SFX plus hand-built post layers |

Disable native video audio. Never pay a model to generate sound that will be
discarded.

## 8. Generated versus post-produced

| Generate | Create in post |
|---|---|
| Restrained performances | All titles, labels, receipts, dates and badges |
| Peter’s one normal walking pass | Exact +11-second repeated pass |
| Peter’s separate stop-and-look tail | Masked replacement of final frames |
| Natural room and street movement | Mug appearance between clean cuts |
| Mug already resting in approved setups | Receipt date change and stamp |
| Before/after character states | Hospital and family-photo replacements |
| Two recognition-test performances | Paper-wipe hidden cut |
| Mild fluorescent flicker plates | State transition on exact selected frame |
| Slow mechanical dolly | Grain, gate weave, dirt, halation, print response |

Plot-critical timing is never delegated to generation.

## 9. Signature effects

### Exact eleven-second recurrence

1. Generate one approved locked-off wide with Peter crossing right-to-left.
2. Retain a 5–6 second crossing window.
3. Construct the remaining eleven-second interval from couple close-ups and
   environmental inserts.
4. Reuse the exact original crossing frames at `+11.000s`.
5. Repeat the same bell, heels, newspaper snap and cough sample-accurately.
6. Replace only the last 12–18 frames with the separate awareness pass.

The recurrence must be mathematically exact. “Similar” reads as a production
mistake.

### Hospital displacement

Generate one locked hospital photograph. Create name-card and patient
replacement as a still composite. Preserve pillow indentation, shadows,
reflections and lens distortion. Mara slides the revised receipt across the
patient region; switch states only while the paper fully occludes the change.
The earlier `AUTHORITATIVE` stamp changes the receipt date only.

### Childhood allocation photograph

Use one base plate and three edited states. First, child Peter changes to child
Lina while adults, frame, glare, creases and room geometry remain pixel-locked.
Only after Mara objects does a third state add adult Mara and the cobalt mug.
Do not add teacher Peter or ask a model to discover either final clue.

### Recognition transfer

Generate two 4-second locked performances:

- Take A: Mrs. Gray recognizes Peter.
- Take B: Mrs. Gray recognizes Lina.

Camera and blocking are identical. A practical form passes within 5cm of lens,
providing a full-frame wipe. Cut while the frame is occluded. Add matching paper
motion blur and one continuous room-tone take.

## 10. Narration and performance

Record the entire narration in one continuous session before motion generation.
Do not synthesize one sentence at a time.

Voice direction:

```text
Mature American civil-service narrator, medium-low register, dry and precise.
Reading an approved internal procedure, not telling a story. No menace, warmth,
irony, wonder or imitation of a recognizable broadcaster. Every impossible fact
receives the same emphasis as a date or form number.
```

Mara’s performance is smaller than the narrator’s. Her only openly oppositional
line is “They cannot both be true,” delivered as a technical objection, not an
emotional speech.

## 11. Sound and score

### Beds

- Training room: projector chatter, fluorescent ballast, distant ventilation.
- Payroll: paper movement, HVAC, ceramic resonance, quiet office telephones.
- Street: wet tires, umbrella tip, clock mechanism, distant bicycle.
- Records office: fluorescent hum, badge printer, satchel buckle.
- Kitchen: refrigerator motor, wall clock, no score during recognition transfer.

### Motifs

- **Three-note chime:** forward at title and clean at completion.
- **Eleven-second cell:** muted vibraphone, tape pulse and bass clarinet; repeats
  exactly with Peter, but one note continues after he stops.
- **Identity allocation:** two instruments exchange motifs instead of adding a
  horror sting.
- **Contamination:** room tones appear in the wrong location before images do.

Score target: 68–72 BPM; detuned educational synth, muted vibraphone, bass
clarinet, restrained cello. No trailer percussion, risers, braams or jump stings.

## 12. Continuity ledger

Every shot records:

- shot ID and timeline in/out;
- approved reference versions;
- model, endpoint, seed and exact prompt;
- source duration and selected frame range;
- lens, height, camera motion and screen direction;
- character state, scar state and wardrobe;
- mug, binder, umbrella, satchel and photograph state;
- light and fluorescent phase;
- intentional discrepancy;
- accidental discrepancies checked and rejected;
- dialogue, foley and repeated-cue sync;
- approval status.

Intentional errors are red. Immutable facts are blue. Anything unmarked is a
defect.

## 13. Failure gates

1. **Still:** face, hands, anatomy, wardrobe, props, geography and light pass at
   200%.
2. **Motion:** no identity drift, sliding feet, breathing walls, rubber hands or
   invented camera movement.
3. **Narrative:** the anomaly is legible to a first-time silent viewer.
4. **Continuity:** every accidental discrepancy is fixed.
5. **Loop:** recurrence is frame- and sample-exact.
6. **Composite:** masks hold on glass, hair, motion blur, shadows and grain.
7. **Sound:** no ambience discontinuity at source edits.
8. **Taste:** ominous lighting or exaggerated reactions trigger a redesign.
9. **Technical:** constant 24fps, one color transform, one global grain pass.
10. **Spend:** after two failed final attempts, split or simplify the shot.

## 14. Proof gates

### Tone and compositing proof

Before full production, make shots P03–P08:

- mug under evidence dome;
- future receipt;
- authoritative stamp;
- purchase date moves backward;
- hospital photograph;
- June is displaced and Peter’s name appears.

This 26-second unit tests the film’s central promise: can bureaucracy, not visual
spectacle, make a reality correction feel cinematic? It requires no lip sync,
crowd, long performance or identity transfer. If this proof is not convincing,
the full film does not proceed.

### Risk reel

The compositing proof does not de-risk the hardest production problems. Produce
these non-contiguous tests before scheduling the rest:

1. **Exact loop:** E01 and E04, including the masked alternate tail. Pixel and
   sample comparison must be exact before Peter stops.
2. **Recognition transfer:** I06 and I07 with the full-frame paper wipe.
   Background alignment error must remain below one pixel at normal speed.
3. **Set reconciliation:** T03/R01. Reuse T03's literal background pixels;
   composite Mara and props only. Do not regenerate immutable architecture.

Full production begins only when the tone proof and all three risk tests pass at
normal playback speed and under difference-overlay inspection.

