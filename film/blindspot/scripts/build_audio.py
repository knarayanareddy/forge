"""Build the complete audio bed and mix it to a single track.

No audio from any video model is used. Video clips are generated mute where the
endpoint allows it and any returned audio is discarded - five clips from a
generative model give five different room tones and five different implied
microphones, none of which can be levelled against one narration track.

Four layers: narration, field ambience, the 1890 archive treatment, and a room
tone floor so that cuts to silence read as recorded quiet rather than a dropout.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess

import falgen
import timing

ROOT = falgen.ROOT
OUT = falgen.OUT
AUDIO = OUT / "audio"

RATE = 48000

# The 1890 wax cylinder, synthesised. Bandlimited to a shellac passband, with
# slow pitch drift for wow and flutter and a hard compression curve.
SHELLAC = "highpass=f=300,lowpass=f=3200,vibrato=f=1.6:d=0.12,acompressor=threshold=0.15:ratio=6:attack=5:release=200"


def run(args: list[str]) -> None:
    subprocess.run(args, check=True, capture_output=True)


def to_wav(src: pathlib.Path, dest: pathlib.Path) -> pathlib.Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(src),
         "-ac", "1", "-ar", str(RATE), "-c:a", "pcm_s16le", str(dest)])
    return dest


def duration_of(path: pathlib.Path) -> float:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "default=nw=1:nk=1", str(path)],
        check=True, capture_output=True, text=True,
    )
    return float(out.stdout.strip())


def synth_wind(dest: pathlib.Path, seconds: float, colour: str = "pink") -> pathlib.Path:
    """Fallback ambience when no SFX endpoint is available.

    The beds sit at -24 to -28dB and are not doing dramatic work, so filtered
    noise is an acceptable stand-in for hedgerow wind or distant swell.
    """
    dest.parent.mkdir(parents=True, exist_ok=True)
    run(["ffmpeg", "-y", "-loglevel", "error",
         "-f", "lavfi", "-i", f"anoisesrc=color={colour}:r={RATE}:d={seconds:.2f}",
         "-af", "lowpass=f=700,highpass=f=80,tremolo=f=0.15:d=0.6",
         "-ac", "1", str(dest)])
    return dest


def generate_speech(fal: falgen.Fal, narration: dict, models: dict, force: bool) -> dict[str, pathlib.Path]:
    endpoint = models["audio"]["tts"]["endpoint"]
    voice = narration["voice"]
    made: dict[str, pathlib.Path] = {}
    for line in narration["lines"]:
        dest = AUDIO / "vo" / f"{line['id']}.wav"
        if dest.exists() and not force:
            made[line["id"]] = dest
            continue
        payload = {"text": line["text"]}
        if voice.get("speed"):
            payload["speed"] = voice["speed"]
        raw = fal.generate(job=f"vo_{line['id']}", endpoint=endpoint, payload=payload,
                           media_key="audio", suffix=".mp3")
        if raw:
            made[line["id"]] = to_wav(raw, dest)
    return made


def generate_effects(fal: falgen.Fal, narration: dict, models: dict, starts: dict, total: float,
                     force: bool) -> tuple[dict, dict]:
    endpoint = models["audio"]["sfx"]["endpoint"]
    verified = models["audio"]["sfx"].get("verified", False)

    beds: dict[str, pathlib.Path] = {}
    for bed in narration["ambience"]:
        dest = AUDIO / "amb" / f"{bed['id']}.wav"
        span_start = min(starts[e] for e in bed["entries"])
        span_end = max(starts[e] for e in bed["entries"]) + 8.0
        seconds = max(4.0, span_end - span_start)
        if dest.exists() and not force:
            beds[bed["id"]] = dest
            continue
        raw = None
        if verified:
            try:
                raw = fal.generate(job=f"amb_{bed['id']}", endpoint=endpoint,
                                   payload={"text": bed["prompt"], "duration_seconds": min(seconds, 22)},
                                   media_key="audio", suffix=".mp3")
            except Exception as exc:  # noqa: BLE001 - ambience is not worth failing the build over
                print(f"  ambience {bed['id']} failed ({exc}); synthesising instead")
        else:
            print(f"  sfx endpoint unverified; synthesising {bed['id']}")
        beds[bed["id"]] = to_wav(raw, dest) if raw else synth_wind(dest, seconds)

    notes: dict[str, pathlib.Path] = {}
    for note in narration.get("notes", []):
        dest = AUDIO / "notes" / f"{note['id']}.wav"
        if dest.exists() and not force:
            notes[note["id"]] = dest
            continue
        if not verified:
            print(f"  sfx endpoint unverified; {note['id']} not generated - "
                  "Specimen 003 needs these two notes to work")
            continue
        raw = fal.generate(job=note["id"], endpoint=endpoint,
                           payload={"text": note["prompt"], "duration_seconds": 4},
                           media_key="audio", suffix=".mp3")
        if raw:
            notes[note["id"]] = to_wav(raw, dest)
    return beds, notes


def mix(narration: dict, starts: dict, total: float, speech: dict, beds: dict, notes: dict,
        dest: pathlib.Path) -> pathlib.Path:
    inputs: list[str] = []
    chains: list[str] = []
    labels: list[str] = []
    idx = 0

    def add_input(path: pathlib.Path, delay_s: float, gain_db: float, extra: str = "") -> None:
        nonlocal idx
        inputs.extend(["-i", str(path)])
        filters = [f"aresample={RATE}", "aformat=sample_fmts=fltp:channel_layouts=mono"]
        if extra:
            filters.append(extra)
        filters.append(f"volume={gain_db}dB")
        if delay_s > 0:
            filters.append(f"adelay={int(delay_s * 1000)}")
        label = f"a{idx}"
        chains.append(f"[{idx}:a]" + ",".join(filters) + f"[{label}]")
        labels.append(f"[{label}]")
        idx += 1

    # Room tone floor, generated inline for the full runtime.
    inputs.extend(["-f", "lavfi", "-i", f"anoisesrc=color=pink:r={RATE}:d={total:.2f}"])
    chains.append(
        f"[{idx}:a]aresample={RATE},aformat=sample_fmts=fltp:channel_layouts=mono,"
        f"lowpass=f=2000,volume={narration['room_tone']['gain_db']}dB[a{idx}]"
    )
    labels.append(f"[a{idx}]")
    idx += 1

    for bed in narration["ambience"]:
        path = beds.get(bed["id"])
        if not path:
            continue
        start = min(starts[e] for e in bed["entries"])
        add_input(path, start, bed["gain_db"], extra="apad")

    for note in narration.get("notes", []):
        path = notes.get(note["id"])
        if not path:
            continue
        at = timing.resolve(note["at"], starts)
        extra = SHELLAC if note.get("treatment") == "shellac" else ""
        add_input(path, at, note["gain_db"], extra=extra)

    for line in narration["lines"]:
        path = speech.get(line["id"])
        if not path:
            continue
        add_input(path, timing.resolve(line["at"], starts), 0.0)

    chains.append(
        "".join(labels) + f"amix=inputs={len(labels)}:normalize=0:dropout_transition=0[mixed]"
    )
    chains.append(f"[mixed]atrim=0:{total:.2f},alimiter=limit=0.92[outa]")

    dest.parent.mkdir(parents=True, exist_ok=True)
    run(["ffmpeg", "-y", "-loglevel", "error", *inputs,
         "-filter_complex", ";".join(chains), "-map", "[outa]",
         "-ac", "1", "-ar", str(RATE), "-c:a", "pcm_s16le", str(dest)])
    return dest


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--silent", action="store_true",
                    help="room tone and synthesised ambience only, no TTS (for the animatic)")
    ap.add_argument("--force", action="store_true", help="regenerate audio that already exists")
    ap.add_argument("--out", default=str(AUDIO / "mix.wav"))
    args = ap.parse_args()

    narration = falgen.load("narration")
    models = falgen.load("models")
    timeline = timing.load_timeline()
    starts, total = timing.entry_starts(timeline)

    speech: dict[str, pathlib.Path] = {}
    beds: dict[str, pathlib.Path] = {}
    notes: dict[str, pathlib.Path] = {}

    if args.silent:
        print("silent mode: room tone plus synthesised ambience, no narration")
        for bed in narration["ambience"]:
            span = max(starts[e] for e in bed["entries"]) + 8.0 - min(starts[e] for e in bed["entries"])
            beds[bed["id"]] = synth_wind(AUDIO / "amb" / f"{bed['id']}.wav", max(4.0, span))
    else:
        fal = falgen.Fal(dry_run=args.dry_run, models=models)
        print(f"narration: {len(narration['lines'])} lines")
        speech = generate_speech(fal, narration, models, args.force)
        beds, notes = generate_effects(fal, narration, models, starts, total, args.force)
        if args.dry_run:
            print("dry run complete; nothing mixed")
            return

    out = mix(narration, starts, total, speech, beds, notes, pathlib.Path(args.out))
    print(f"mixed {duration_of(out):.2f}s -> {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
