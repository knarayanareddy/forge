# BLINDSPOT — Life in the Unobserved

A short film built as a repo rather than as an editing session. The edit is data:
prompts, cut lengths, narration placement and typography all live in
`manifests/`, and the film renders from them with one command. Regenerating a
shot because the whale looks wrong is a one-line change and a re-run, not a
re-export.

**This project is unrelated to the rest of this repository.** It lives on a
branch so it survives; move it to its own repo whenever convenient.

- [`SCRIPT.md`](SCRIPT.md) — the shooting script: premise, register rules, global
  prompt blocks, shot-by-shot breakdown, continuity ledger, production triage.
  Read this first.
- `manifests/` — the film as data.
- `scripts/` — generation and assembly.

## Status

The pipeline runs end to end and has been validated by rendering a **2:52
placeholder animatic** with real cards, chyrons, the population tick, the sepia
and pillarbox treatment, and the narration burned in at its scheduled times. No
video has been generated. That step needs `FAL_KEY`.

## Quick start

```bash
pip install -r requirements.txt

# 1. Check the writing before spending anything.
python3 scripts/animatic.py --check-only

# 2. Build a placeholder cut to feel the pacing. No API key needed.
python3 scripts/animatic.py
python3 scripts/build_audio.py --silent
python3 scripts/assemble.py --animatic        # -> out/animatic.mp4

# 3. See every composed prompt without calling anything.
python3 scripts/make_shots.py --dry-run       # -> out/prompts/

# --- everything below needs FAL_KEY ---

export FAL_KEY=...

# 4. Reference sheets, then save the chosen frames as fal Assets under their tags.
python3 scripts/make_sheets.py
python3 scripts/make_sheets.py --boards       # planning only, never attached

# 5. Draft the whole film cheaply, look at it, then promote what works.
python3 scripts/make_shots.py --tier draft
python3 scripts/make_shots.py --only shot04 shot06 --tier hero --alternates 2

# 6. Narration and beds, then cut.
python3 scripts/build_audio.py
python3 scripts/assemble.py                   # -> out/blindspot.mp4
```

## How the pieces fit

| File | Role |
|---|---|
| `manifests/blocks.json` | Style, behaviour, negative and per-segment locks, pasted verbatim into every prompt |
| `manifests/sheets.json` | Reference sheets (`@hedgerow`, `@whale`, …) and planning storyboards |
| `manifests/shots.json` | 20 shots: action, camera, SFX, locks, assets, conditioning, tier |
| `manifests/timeline.json` | The cut: order, durations, cards, chyrons, stamps, transitions |
| `manifests/narration.json` | Voice direction, 28 narration lines, ambience beds, room tone |
| `manifests/models.json` | fal endpoints, with `verified` flags |
| `scripts/falgen.py` | Queue client and prompt composition |
| `scripts/animatic.py` | Pacing check and placeholder cut |
| `scripts/assemble.py` | Post treatments, overlays, crossfade, concat, mux |

## Three things worth knowing before you change anything

**Cut length and generation length are different numbers.** `shots.json`
durations are what fal is asked for; `timeline.json` durations are how long the
shot stays on screen. Never cut longer than you generated — wind and water make
clone-padding obvious. Extend a beat by cutting to a second plate instead, which
is what `shot06b`, `shot11b`, `shot14b` and `shot15b` are for.

**Narration density is the dial that decides whether this feels austere or
crowded.** The first draft ran 440 words over 130 seconds, which is 140% density
and unplayable. It is now 258 words over 172 seconds at 59%. `animatic.py
--check-only` reports density and flags collisions, and once narration has been
generated it measures the real audio instead of estimating from word count.

**No audio from any video model is used.** Clips are generated mute and any
returned audio is discarded, because five clips give five room tones that cannot
be levelled against one narration track. Every shot still carries an SFX line in
its prompt — that line is an *input* that tells the model when things happen, and
it is one of the cheapest timing controls available.

## Before the first paid run

Confirm the endpoints marked `"verified": false` in `manifests/models.json`
(`video.i2v` and `audio.sfx`) against their fal model pages. Everything else was
taken from published docs. Every endpoint is overridable with `--endpoint`.

## Cost shape

Roughly 20 hero shots plus alternates, 7 reference sheets, 4 planning boards, and
one narration pass. Specimen 001 is the cheap segment and also the flagship —
empty plates and focus pulls, so generate a lot and keep the best four. The
whale is the expensive one because the callosity pattern has to survive four
shots and two lighting conditions.

Draft on the cheap endpoint first. Prompts are portable across fal video models —
only the endpoint string changes — so nothing is wasted by iterating cheaply and
re-running the locked prompt on a premium model.
