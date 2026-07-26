"""Generate scratch dialogue and verify the measured edit timing.

The narrator uses ElevenLabs Bill: a seasoned American broadcast voice. Actor
voices are placeholders used only to expose pacing collisions; final sync lines
will be generated or recorded after casting approval.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
PROJECT = pathlib.Path(__file__).resolve().parents[1]
MANIFESTS = PROJECT / "manifests"
OUT = PROJECT / "out" / "scratch_audio"
sys.path.insert(0, str(ROOT / "scripts"))

from falgen import Fal  # noqa: E402

RATE = 48000
VOICES = {
    "NARRATOR": "Bill",
    "MARA": "Laura",
    "PETER": "Brian",
    "WITNESS": "Alice",
}


def run(command: list[str]) -> None:
    subprocess.run(command, check=True, capture_output=True)


def duration(path: pathlib.Path) -> float:
    result = subprocess.run(
        [
            "ffprobe", "-v", "error", "-show_entries", "format=duration",
            "-of", "default=nw=1:nk=1", str(path),
        ],
        check=True, capture_output=True, text=True,
    )
    return float(result.stdout.strip())


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    shots = json.loads((MANIFESTS / "shots.json").read_text())
    dialogue = json.loads((MANIFESTS / "dialogue.json").read_text())
    starts = {shot["id"]: float(shot["in"]) for shot in shots["shots"]}
    total = float(shots["spec"]["runtime_seconds"])
    fal = Fal(dry_run=args.dry_run)

    OUT.mkdir(parents=True, exist_ok=True)
    placed = []
    for line in dialogue["lines"]:
        wav = OUT / f"{line['id']}.wav"
        if not wav.exists():
            raw = fal.generate(
                job=f"continuity_scratch_{line['id']}",
                endpoint="fal-ai/elevenlabs/tts/multilingual-v2",
                payload={
                    "text": line["text"],
                    "voice": VOICES[line["speaker"]],
                    "stability": 0.72,
                    "similarity_boost": 0.82,
                    "speed": 1.0,
                    "language_code": "en",
                    "apply_text_normalization": "auto",
                },
                media_key="audio",
                suffix=".mp3",
            )
            if raw:
                run([
                    "ffmpeg", "-y", "-loglevel", "error", "-i", str(raw),
                    "-ac", "1", "-ar", str(RATE), "-c:a", "pcm_s16le", str(wav),
                ])
        if args.dry_run:
            continue
        start = starts[line["at"]["shot"]] + float(line["at"]["offset"])
        placed.append({
            "id": line["id"],
            "speaker": line["speaker"],
            "start": round(start, 3),
            "duration": round(duration(wav), 3),
            "end": round(start + duration(wav), 3),
            "text": line["text"],
        })

    if args.dry_run:
        return

    collisions = []
    for left, right in zip(placed, placed[1:]):
        if left["end"] > right["start"]:
            collisions.append({
                "left": left["id"],
                "right": right["id"],
                "seconds": round(left["end"] - right["start"], 3),
            })
    if placed[-1]["end"] > total:
        collisions.append({
            "left": placed[-1]["id"],
            "right": "END",
            "seconds": round(placed[-1]["end"] - total, 3),
        })

    report = {
        "runtime": total,
        "lines": placed,
        "collisions": collisions,
        "speaking_seconds": round(sum(item["duration"] for item in placed), 3),
    }
    report["speaking_density"] = round(report["speaking_seconds"] / total, 4)
    (OUT / "timing_report.json").write_text(json.dumps(report, indent=2) + "\n")

    # Place every line against a silent 48kHz bed for a frame-accurate scratch mix.
    inputs = ["-f", "lavfi", "-i", f"anullsrc=r={RATE}:cl=mono:d={total}"]
    chains = [f"[0:a]volume=0[bed]"]
    labels = ["[bed]"]
    for index, item in enumerate(placed, start=1):
        inputs += ["-i", str(OUT / f"{item['id']}.wav")]
        delay = int(item["start"] * 1000)
        chains.append(f"[{index}:a]adelay={delay},volume=0dB[a{index}]")
        labels.append(f"[a{index}]")
    chains.append(
        "".join(labels)
        + f"amix=inputs={len(labels)}:normalize=0:dropout_transition=0,"
          f"atrim=0:{total},alimiter=limit=0.92[out]"
    )
    run([
        "ffmpeg", "-y", "-loglevel", "error", *inputs,
        "-filter_complex", ";".join(chains), "-map", "[out]",
        "-ac", "1", "-ar", str(RATE), "-c:a", "pcm_s16le",
        str(OUT / "scratch_dialogue.wav"),
    ])

    print(json.dumps({
        "runtime": total,
        "speaking_seconds": report["speaking_seconds"],
        "speaking_density": report["speaking_density"],
        "collisions": collisions,
        "mix": str(OUT / "scratch_dialogue.wav"),
    }, indent=2))
    if collisions:
        raise SystemExit(2)


if __name__ == "__main__":
    main()

