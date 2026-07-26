"""Cut BLINDSPOT together from generated clips, cards, chyrons and the audio mix.

The edit is data: reordering or retiming the film means editing timeline.json and
re-running this, not re-exporting from an NLE. Regenerating one shot because the
elk look wrong is a one-line change and a re-run.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import shutil
import subprocess

import cards
import falgen
import timing

ROOT = falgen.ROOT
OUT = falgen.OUT
EDIT = OUT / "edit"

VENC = ["-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-r", "24", "-an"]

# Applied in this order regardless of the order given in the manifest: weave the
# gate first so it moves real image, grade before grain, and pillarbox last so the
# bars stay clean black.
POST_ORDER = ["gate_weave", "sepia", "dust", "heavy_grain", "pillarbox_4x3"]
POST_FILTERS = {
    "sepia": "colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131:0,eq=saturation=0.72:contrast=1.06",
    "gate_weave": "crop=iw-12:ih-12:x='6+3*sin(n/6)':y='6+3*sin(n/9)',scale=1920:1080",
    # Approximation. Convincing dust and scratches need a real overlay plate;
    # temporal noise is a stand-in that reads correctly at this scale.
    "dust": "noise=alls=8:allf=t+u",
    "heavy_grain": "noise=alls=16:allf=t",
    "pillarbox_4x3": "scale=1440:1080:force_original_aspect_ratio=increase,crop=1440:1080,pad=1920:1080:240:0",
}


def run(args: list[str]) -> None:
    proc = subprocess.run(args, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"ffmpeg failed:\n{' '.join(args[:12])}...\n{proc.stderr[-1500:]}")


def base_clip(entry: dict, spec: dict, source_dir: pathlib.Path, dest: pathlib.Path) -> pathlib.Path:
    dur = float(entry["duration"])
    chain = ["scale=1920:1080:force_original_aspect_ratio=decrease",
             "pad=1920:1080:(ow-iw)/2:(oh-ih)/2", "setsar=1", "fps=24"]

    if entry["type"] == "card":
        src = OUT / "cards" / f"{entry['id']}.png"
        if not src.exists():
            raise FileNotFoundError(f"missing card {src} - run cards.py")
        run(["ffmpeg", "-y", "-loglevel", "error", "-loop", "1", "-t", f"{dur}",
             "-i", str(src), "-vf", ",".join(chain), *VENC, str(dest)])
        return dest

    src = source_dir / f"{entry['id']}.mp4"
    if not src.exists():
        raise FileNotFoundError(f"missing clip for {entry['id']} at {src}")
    for name in POST_ORDER:
        if name in entry.get("post", []):
            chain.append(POST_FILTERS[name])
    chain.append("tpad=stop_mode=clone:stop_duration=60")
    run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(src),
         "-vf", ",".join(chain), "-t", f"{dur}", *VENC, str(dest)])
    return dest


def overlay(base: pathlib.Path, png: pathlib.Path, dest: pathlib.Path, duration: float,
            appear: float, vanish: float, fade_in: float = 0.4, fade_out: float = 0.4,
            enable: str | None = None) -> pathlib.Path:
    """Composite one PNG over a clip, fading the whole overlay as a single object."""
    steps = ["format=rgba"]
    if fade_in > 0:
        steps.append(f"fade=t=in:st={appear:.3f}:d={fade_in}:alpha=1")
    if fade_out > 0:
        steps.append(f"fade=t=out:st={max(vanish - fade_out, 0):.3f}:d={fade_out}:alpha=1")
    ov = "overlay=0:0:format=auto"
    if enable:
        ov += f":enable='{enable}'"
    fc = f"[1:v]{','.join(steps)}[ov];[0:v][ov]{ov}"
    run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(base),
         "-loop", "1", "-t", f"{duration}", "-i", str(png),
         "-filter_complex", fc, "-t", f"{duration}", *VENC, str(dest)])
    return dest


def decorate(entry: dict, clip: pathlib.Path) -> pathlib.Path:
    dur = float(entry["duration"])
    stage = 0

    def nxt() -> pathlib.Path:
        nonlocal stage
        stage += 1
        return EDIT / f"{entry['id']}_v{stage}.mp4"

    chyron = entry.get("chyron")
    if chyron:
        appear = float(chyron["in"])
        vanish = appear + float(chyron["duration"])
        tick = chyron.get("population_tick")
        if tick:
            # Two states, hard swap. The number simply changes - no sound, no sting.
            at = float(tick["at"])
            dest = nxt()
            overlay(clip, OUT / "cards" / f"chyron_{entry['id']}.png", dest, dur,
                    appear, at, fade_out=0, enable=f"lt(t,{at})")
            clip = dest
            dest = nxt()
            overlay(clip, OUT / "cards" / f"chyron_{entry['id']}_tick.png", dest, dur,
                    at, vanish, fade_in=0, enable=f"gte(t,{at})")
            clip = dest
        else:
            dest = nxt()
            overlay(clip, OUT / "cards" / f"chyron_{entry['id']}.png", dest, dur, appear, vanish)
            clip = dest

    if entry.get("stamp"):
        dest = nxt()
        overlay(clip, OUT / "cards" / f"stamp_{entry['id']}.png", dest, dur, 0.4, dur - 0.3)
        clip = dest

    return clip


def crossfade(prev: pathlib.Path, cur: pathlib.Path, prev_dur: float, d: float,
              dest: pathlib.Path) -> pathlib.Path:
    fc = f"[0:v][1:v]xfade=transition=fade:duration={d}:offset={prev_dur - d:.3f}"
    run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(prev), "-i", str(cur),
         "-filter_complex", fc, *VENC, str(dest)])
    return dest


def concat(clips: list[pathlib.Path], dest: pathlib.Path) -> pathlib.Path:
    listing = EDIT / "concat.txt"
    listing.write_text("".join(f"file '{c.resolve()}'\n" for c in clips))
    run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0",
         "-i", str(listing), "-c", "copy", str(dest)])
    return dest


def write_srt(dest: pathlib.Path, starts: dict, narration: dict) -> pathlib.Path:
    """Burn-in narration for the animatic.

    Durations are estimated from word count at a measured documentary delivery,
    which is close enough to judge pacing and to catch lines that collide.
    """
    def stamp(t: float) -> str:
        h, rem = divmod(t, 3600)
        m, s = divmod(rem, 60)
        return f"{int(h):02d}:{int(m):02d}:{s:06.3f}".replace(".", ",")

    blocks = []
    for i, line in enumerate(narration["lines"], start=1):
        at = timing.resolve(line["at"], starts)
        est = max(1.6, len(line["text"].split()) / 2.4)
        blocks.append(f"{i}\n{stamp(at)} --> {stamp(at + est)}\n{line['text']}\n")
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text("\n".join(blocks))
    return dest


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--animatic", action="store_true",
                    help="cut from placeholder plates and burn in the narration")
    ap.add_argument("--audio", default=None, help="audio mix to lay under the picture")
    ap.add_argument("--out", default=None)
    ap.add_argument("--keep-intermediates", action="store_true")
    args = ap.parse_args()

    timeline = timing.load_timeline()
    narration = falgen.load("narration")
    starts, total = timing.entry_starts(timeline)

    source_dir = OUT / ("placeholders" if args.animatic else "clips")
    out_path = pathlib.Path(args.out) if args.out else OUT / ("animatic.mp4" if args.animatic else "blindspot.mp4")

    if EDIT.exists():
        shutil.rmtree(EDIT)
    EDIT.mkdir(parents=True)

    print("rendering text elements")
    cards.build_all()

    clips: list[pathlib.Path] = []
    durations: list[float] = []
    for entry in timeline["entries"]:
        print(f"  {entry['id']}")
        clip = base_clip(entry, timeline, source_dir, EDIT / f"{entry['id']}_v0.mp4")
        clip = decorate(entry, clip)
        transition = entry.get("transition")
        if transition and transition["type"] == "xfade" and clips:
            d = float(transition["duration"])
            merged = crossfade(clips[-1], clip, durations[-1], d, EDIT / f"{entry['id']}_xf.mp4")
            clips[-1] = merged
            durations[-1] = durations[-1] + float(entry["duration"]) - d
        else:
            clips.append(clip)
            durations.append(float(entry["duration"]))

    print("concatenating")
    silent = concat(clips, EDIT / "picture.mp4")

    if args.animatic:
        srt = write_srt(EDIT / "narration.srt", starts, narration)
        burned = EDIT / "picture_burned.mp4"
        # Top-aligned so the burn-in never covers the lower-third chyron, which is
        # one of the things the animatic exists to check. libass reads this style
        # field in the legacy SSA scheme, where top-centre is 6 rather than 8.
        style = ("FontName=Liberation Sans,FontSize=16,PrimaryColour=&H00E0DEDA,"
                 "OutlineColour=&HA0000000,BorderStyle=3,Outline=3,Shadow=0,"
                 "Alignment=6,MarginV=40")
        run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(silent),
             "-vf", f"subtitles={srt}:force_style='{style}'", *VENC, str(burned)])
        silent = burned

    audio = pathlib.Path(args.audio) if args.audio else OUT / "audio" / "mix.wav"
    if audio.exists():
        print(f"muxing {audio.name}")
        run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(silent), "-i", str(audio),
             "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
             "-shortest", str(out_path)])
    else:
        print(f"no audio mix at {audio.relative_to(ROOT)}; picture only")
        shutil.copy(silent, out_path)

    if not args.keep_intermediates:
        for stray in EDIT.glob("*_v*.mp4"):
            stray.unlink()

    probe = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration:stream=width,height,r_frame_rate",
         "-of", "json", str(out_path)], capture_output=True, text=True, check=True)
    info = json.loads(probe.stdout)
    dur = float(info["format"]["duration"])
    v = next(s for s in info["streams"] if "width" in s)
    print(f"\n{out_path.relative_to(ROOT)}  {v['width']}x{v['height']} @ {v['r_frame_rate']}  "
          f"{timing.tc(dur)}  (timeline says {timing.tc(total)})")


if __name__ == "__main__":
    main()
