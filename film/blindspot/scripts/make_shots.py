"""Generate the hero video shots.

Prompts are composed from the global blocks in blocks.json plus each shot's own
action, camera and SFX lines - see falgen.compose_prompt. Shots declaring a
`conditioning` block are generated image-to-video so continuity comes from an
actual frame rather than from a description of one.

Draft everything on the cheap endpoint first. Prompts are portable across fal
video models - only the endpoint string changes - so nothing is wasted by
iterating cheaply and re-running the locked prompt on a premium model.
"""

from __future__ import annotations

import argparse
import base64
import mimetypes
import pathlib
import subprocess

import falgen

ROOT = falgen.ROOT
OUT = falgen.OUT


def data_uri(path: pathlib.Path) -> str:
    """Inline a conditioning frame as a data URI.

    Avoids depending on fal's storage API for what is always a small JPEG.
    """
    mime = mimetypes.guess_type(path.name)[0] or "image/jpeg"
    return f"data:{mime};base64," + base64.b64encode(path.read_bytes()).decode()


def last_frame(clip: pathlib.Path, dest: pathlib.Path) -> pathlib.Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-sseof", "-0.5", "-i", str(clip),
         "-vsync", "0", "-q:v", "2", "-update", "1", "-frames:v", "1", str(dest)],
        check=True,
    )
    return dest


def resolve_conditioning(shot: dict) -> str | None:
    """Turn a shot's conditioning block into an image_url, or explain what's missing."""
    cond = shot.get("conditioning")
    if not cond:
        return None
    kind = cond["type"]

    if kind == "last_frame_of":
        src = OUT / "clips" / f"{cond['shot']}.mp4"
        if not src.exists():
            raise FileNotFoundError(
                f"{shot['id']} is conditioned on the last frame of {cond['shot']}, "
                f"which has not been generated yet. Run {cond['shot']} first."
            )
        frame = last_frame(src, OUT / "frames" / f"{cond['shot']}_last.jpg")
        return data_uri(frame)

    if kind == "same_still_as":
        still = OUT / "assets" / f"still_{cond['shot']}.jpg"
        if not still.exists():
            raise FileNotFoundError(
                f"{shot['id']} shares a still with {cond['shot']}. Put the chosen frame at "
                f"{still.relative_to(ROOT)} - one generated still serves both centuries."
            )
        return data_uri(still)

    if kind == "reference_shot":
        # Motion and framing are matched through the locks and the text, not a frame.
        print(f"  note: {shot['id']} must match {cond['shot']} - {cond.get('note', '')}")
        return None

    raise ValueError(f"unknown conditioning type {kind}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dry-run", action="store_true", help="compose and write prompts, call nothing")
    ap.add_argument("--only", nargs="*", default=None, help="shot ids to generate")
    ap.add_argument("--segment", default=None, help="generate one segment (e.g. specimen_001)")
    ap.add_argument("--tier", default=None, choices=["draft", "hero", "hero_alt"],
                    help="override each shot's tier")
    ap.add_argument("--endpoint", default=None, help="override the endpoint entirely")
    ap.add_argument("--alternates", type=int, default=0, help="extra takes per shot")
    args = ap.parse_args()

    blocks = falgen.load("blocks")
    sheets = falgen.load("sheets")
    shots_doc = falgen.load("shots")
    models = falgen.load("models")
    defaults = shots_doc["defaults"]

    fal = falgen.Fal(dry_run=args.dry_run, models=models)

    selected = [
        s for s in shots_doc["shots"]
        if (not args.only or s["id"] in args.only)
        and (not args.segment or s["segment"] == args.segment)
    ]
    if not selected:
        raise SystemExit("no shots matched")

    for shot in selected:
        tier = args.tier or shot.get("tier", "draft")
        image_url = resolve_conditioning(shot)
        model_key = "i2v" if image_url else tier
        model = models["video"][model_key]
        endpoint = args.endpoint or model["endpoint"]
        if not model.get("verified", True):
            print(f"  warning: endpoint '{endpoint}' is unverified - confirm it on fal before a paid run")

        payload = falgen.video_payload(shot, blocks, sheets, model, defaults, image_url=image_url)

        takes = 1 + max(args.alternates, shot.get("alternates", 0) if args.alternates == 0 else args.alternates)
        for take in range(takes):
            job = shot["id"] if take == 0 else f"{shot['id']}_alt{take}"
            fal.generate(job=job, endpoint=endpoint, payload=payload, media_key="video", suffix=".mp4")

        if shot.get("note"):
            print(f"  {shot['id']}: {shot['note']}")

    if args.dry_run:
        print(f"\nComposed prompts written to {(OUT / 'prompts').relative_to(ROOT)}/")


if __name__ == "__main__":
    main()
