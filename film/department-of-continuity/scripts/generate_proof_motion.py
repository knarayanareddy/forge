"""Animate approved proof keyframes with Kling 3 Pro image-to-video.

Tone proof hero motion: P03, P04.
Risk reel motion: E01 (source loop plate). E04 awareness is a short end-frame
guided take used only for the masked alternate tail.
"""

from __future__ import annotations

import argparse
import base64
import mimetypes
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
PROJECT = pathlib.Path(__file__).resolve().parents[1]
FRAMES = PROJECT / "out" / "proof_keyframes"
CLIPS = PROJECT / "out" / "proof_clips"
sys.path.insert(0, str(ROOT / "scripts"))

from falgen import Fal  # noqa: E402

ENDPOINT = "fal-ai/kling-video/v3/pro/image-to-video"
NEG = (
    "camera pan, camera tilt, zoom, handheld shake, morphing faces, rubber hands, "
    "extra fingers, text, logos, subtitles, VHS damage, digital glitch, horror "
    "lighting, exaggerated expression, sliding feet, warping walls"
)

SHOTS = [
    {
        "id": "P03",
        "frame": "P03_dome_master.jpg",
        "duration": "5",
        "prompt": (
            "Late-1990s institutional training film, locked heavy tripod. "
            "Keep the approved start frame as visual truth. Mara stands still "
            "after placing the glass evidence dome over the cobalt-striped mug; "
            "tiny settling of her hands away from the dome, one natural blink, "
            "subtle fluorescent flicker. June remains seated with minimal "
            "breathing. No dome lift, no mug movement, no new objects, no text."
        ),
    },
    {
        "id": "P04",
        "frame": "P04_mara_close.jpg",
        "duration": "5",
        "prompt": (
            "Locked 65mm close-up from the approved start frame. Mara studies "
            "condensation inside the glass dome with technical concentration. "
            "Micro eye movement and one restrained blink only. Dome and mug stay "
            "fixed in soft foreground. No speaking mouth shapes, no emotion spike, "
            "no camera move, no text."
        ),
    },
    {
        "id": "E01",
        "frame": "E01_loop_start.jpg",
        "duration": "11",
        "prompt": (
            "Locked 40mm surveillance master. Preserve architecture, clock, rain, "
            "awning and seated witness exactly. Peter Gray walks RIGHT-TO-LEFT in "
            "the FAR background pedestrian lane at a steady bureaucratic pace, "
            "green umbrella in right hand, newspaper under left arm. Natural gait, "
            "real gravity, no face morph. Witness stays seated and unaware. No "
            "camera movement, no text, no added people."
        ),
    },
    {
        "id": "E04_tail",
        "frame": "E04_awareness_end.jpg",
        "duration": "3",
        "prompt": (
            "Locked surveillance frame. Peter stands near frame-left in the far "
            "lane and turns only his head and upper body toward the camera with a "
            "restrained questioning look. Tiny rain continuity and breathing only. "
            "Witness, clock and architecture remain fixed. No walk, no morph, no text."
        ),
    },
    {
        "id": "T03",
        "frame": "T03_opening_set.jpg",
        "duration": "4",
        "prompt": (
            "Locked 28mm training-room master. Mara behind lectern opens the "
            "oxblood binder with one slow hand motion. Tiny fluorescent flicker "
            "and breath. Architecture, drawers, door and projector stay pixel-stable. "
            "No mug, no text, no camera move."
        ),
    },
    {
        "id": "R01",
        "frame": "R01_reconciled_set.jpg",
        "duration": "4",
        "prompt": (
            "Locked 28mm training-room master. Instructor Mara stands still behind "
            "lectern with open binder and cobalt-striped mug at her right hand. "
            "Breath and one blink only. Room architecture remains fixed. No text, "
            "no stamp, no camera move."
        ),
    },
]


def data_uri(path: pathlib.Path) -> str:
    mime = mimetypes.guess_type(path.name)[0] or "image/jpeg"
    return f"data:{mime};base64," + base64.b64encode(path.read_bytes()).decode()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--only", nargs="*", default=None)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    fal = Fal(dry_run=args.dry_run)
    CLIPS.mkdir(parents=True, exist_ok=True)

    selected = [s for s in SHOTS if not args.only or s["id"] in args.only]
    for shot in selected:
        frame = FRAMES / shot["frame"]
        if not frame.exists():
            raise SystemExit(f"missing keyframe {frame}")
        payload = {
            "prompt": shot["prompt"],
            "start_image_url": data_uri(frame),
            "duration": shot["duration"],
            "generate_audio": False,
            "negative_prompt": NEG,
            "cfg_scale": 0.55,
        }
        generated = fal.generate(
            job=f"continuity_motion_{shot['id']}",
            endpoint=ENDPOINT,
            payload=payload,
            media_key="video",
            suffix=".mp4",
        )
        if generated is None:
            continue
        dest = CLIPS / f"{shot['id']}.mp4"
        shutil.copy2(generated, dest)
        print(f"copied -> {dest}")


if __name__ == "__main__":
    main()
