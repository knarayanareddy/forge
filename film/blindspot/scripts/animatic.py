"""Build a placeholder animatic and check the pacing.

Validates the whole edit - cut lengths, card timing, chyron behaviour, the
population tick, narration placement - before a single frame of video is paid
for. Each shot is a slate carrying its own action and camera lines, so the
animatic doubles as a readable shot list at speed.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import textwrap

from PIL import Image, ImageDraw

import cards
import falgen
import timing

ROOT = falgen.ROOT
OUT = falgen.OUT
PLATES = OUT / "placeholders"

# Estimate used only until real narration exists; once out/audio/vo/*.wav is
# populated the check uses measured durations instead.
WORDS_PER_SEC = 2.55

# Prestige documentary sits in the low sixties. Much above this and the film has
# no air in it; much below and two minutes cannot carry four specimens.
DENSITY_TARGET = (0.55, 0.70)


def plate(shot: dict, spec: dict, dest: pathlib.Path) -> pathlib.Path:
    w, h = spec["spec"]["width"], spec["spec"]["height"]
    fonts = spec["fonts"]
    img = Image.new("RGB", (w, h), (26, 28, 27))
    draw = ImageDraw.Draw(img)

    f_id = cards.font(fonts["sans_bold"], 58)
    f_meta = cards.font(fonts["mono"], 26)
    f_body = cards.font(fonts["sans"], 30)
    f_label = cards.font(fonts["sans_bold"], 24)

    x, y = 160, 150
    cards.draw_tracked(draw, (x, y), shot["id"].upper(), f_id, (226, 224, 218), 8)
    y += 88
    meta = f"{shot['segment']}   {shot['duration']}s   tier: {shot.get('tier', 'draft')}"
    draw.text((x, y), meta, font=f_meta, fill=(140, 150, 142))
    y += 66

    for label, key in (("ACTION", "action"), ("CAMERA", "camera"), ("SFX", "sfx")):
        cards.draw_tracked(draw, (x, y), label, f_label, (150, 178, 152), 4)
        y += 38
        for row in textwrap.wrap(shot[key], width=74):
            draw.text((x, y), row, font=f_body, fill=(206, 204, 198))
            y += 40
        y += 20

    locks = ", ".join(shot.get("locks", [])) or "none"
    assets = " ".join(shot.get("assets", [])) or "none"
    draw.text((x, h - 210), f"locks:  {locks}", font=f_meta, fill=(132, 142, 134))
    draw.text((x, h - 174), f"assets: {assets}", font=f_meta, fill=(132, 142, 134))
    cards.draw_tracked(draw, (x, h - 120), "PLACEHOLDER \u2014 NO FOOTAGE GENERATED",
                       f_label, (188, 138, 120), 4)

    dest.parent.mkdir(parents=True, exist_ok=True)
    img.save(dest)
    return dest


def clip_from_plate(png: pathlib.Path, seconds: float, dest: pathlib.Path) -> pathlib.Path:
    # Faint grain so the animatic never looks like a frozen player.
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-loop", "1", "-t", f"{seconds}", "-i", str(png),
         "-vf", "fps=24,noise=alls=4:allf=t,scale=1920:1080",
         "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-an",
         str(dest)],
        check=True, capture_output=True,
    )
    return dest


def line_duration(line: dict) -> tuple[float, bool]:
    """Measured duration if the narration has been generated, else an estimate."""
    wav = OUT / "audio" / "vo" / f"{line['id']}.wav"
    if wav.exists():
        probe = subprocess.run(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration",
             "-of", "default=nw=1:nk=1", str(wav)],
            check=True, capture_output=True, text=True,
        )
        return float(probe.stdout.strip()), True
    return max(1.6, len(line["text"].split()) / WORDS_PER_SEC), False


def check_pacing(narration: dict, timeline: dict, starts: dict, total: float) -> int:
    """Flag narration that collides, overruns the film, or leaves dead air."""
    ends = {e["id"]: starts[e["id"]] + float(e["duration"]) for e in timeline["entries"]}
    placed = []
    measured = 0
    for line in narration["lines"]:
        at = timing.resolve(line["at"], starts)
        dur, is_measured = line_duration(line)
        measured += int(is_measured)
        placed.append((line["id"], at, at + dur, line["at"]["entry"]))
    placed.sort(key=lambda r: r[1])

    problems = 0
    source = (f"{measured}/{len(placed)} measured from generated audio"
              if measured else f"estimated at {WORDS_PER_SEC} words/sec")
    print(f"\nnarration pacing ({source})\n")
    print(f"{'line':7s} {'in':>7s} {'out':>7s}  {'entry':14s} note")
    for i, (lid, start, end, entry) in enumerate(placed):
        note = ""
        if i + 1 < len(placed) and end > placed[i + 1][1] + 0.05:
            note = f"OVERLAPS {placed[i + 1][0]} by {end - placed[i + 1][1]:.1f}s"
            problems += 1
        elif end > total:
            note = "RUNS PAST END OF FILM"
            problems += 1
        elif end > ends[entry] + 0.05:
            # Not a fault - several lines deliberately land over the next card.
            note = f"carries {end - ends[entry]:.1f}s past {entry}"
        if i + 1 < len(placed):
            gap = placed[i + 1][1] - end
            if gap > 3.0 and not note:
                note = f"{gap:.1f}s of silence follows"
        print(f"{lid:7s} {timing.tc(start):>7s} {timing.tc(end):>7s}  {entry:14s} {note}")

    words = sum(len(l["text"].split()) for l in narration["lines"])
    speaking = sum(e - s for _, s, e, _ in placed)
    density = speaking / total
    lo, hi = DENSITY_TARGET
    verdict = "ok" if lo <= density <= hi else ("too sparse" if density < lo else "too dense")
    print(f"\n{words} words, {len(placed)} lines, {speaking:.0f}s speaking over "
          f"{total:.0f}s of film - {density * 100:.0f}% density ({verdict}, "
          f"target {lo * 100:.0f}-{hi * 100:.0f}%)")
    if problems:
        print(f"{problems} collision(s) to fix in narration.json")
    return problems


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check-only", action="store_true", help="report pacing without rendering")
    args = ap.parse_args()

    timeline = timing.load_timeline()
    narration = falgen.load("narration")
    shots = {s["id"]: s for s in falgen.load("shots")["shots"]}
    starts, total = timing.entry_starts(timeline)

    problems = check_pacing(narration, timeline, starts, total)
    if args.check_only:
        raise SystemExit(1 if problems else 0)

    print("\nbuilding placeholder plates")
    for entry in timeline["entries"]:
        if entry["type"] != "shot":
            continue
        shot = shots.get(entry["id"])
        if not shot:
            raise KeyError(f"timeline entry {entry['id']} has no shot in shots.json")
        png = plate(shot, timeline, PLATES / f"{entry['id']}.png")
        clip_from_plate(png, float(entry["duration"]) + 1.0, PLATES / f"{entry['id']}.mp4")
        print(f"  {entry['id']}")

    print("\nnow run:")
    print("  python3 scripts/build_audio.py --silent")
    print("  python3 scripts/assemble.py --animatic")


if __name__ == "__main__":
    main()
