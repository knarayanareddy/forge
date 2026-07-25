"""Generate the reference sheets and the planning storyboards.

Reference sheets get saved as named fal Assets afterwards so that @-tags in the
shot prompts resolve without re-uploading. Storyboards are for approving shot
order only and are never attached to a video generation.
"""

from __future__ import annotations

import argparse

import falgen


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dry-run", action="store_true", help="compose prompts, call nothing")
    ap.add_argument("--boards", action="store_true", help="generate planning storyboards instead of sheets")
    ap.add_argument("--only", nargs="*", default=None, help="asset tags or board ids to limit to")
    ap.add_argument("--tier", default="hero", choices=["draft", "hero"])
    args = ap.parse_args()

    sheets = falgen.load("sheets")
    models = falgen.load("models")
    fal = falgen.Fal(dry_run=args.dry_run, models=models)

    if args.boards:
        endpoint = models["image"]["draft"]["endpoint"]
        for board in sheets["boards"]:
            if args.only and board["id"] not in args.only:
                continue
            fal.generate(
                job=board["id"],
                endpoint=endpoint,
                payload={"prompt": board["prompt"], "image_size": "landscape_16_9"},
                media_key="images",
                suffix=".jpg",
            )
        print("\nBoards are a director's tool. Do not attach them to a video generation.")
        return

    endpoint = models["image"][args.tier]["endpoint"]
    for sheet in sheets["sheets"]:
        if args.only and sheet["tag"] not in args.only:
            continue
        job = sheet["tag"].lstrip("@")
        size = "square_hd" if sheet.get("aspect_ratio") == "1:1" else "landscape_16_9"
        fal.generate(
            job=f"sheet_{job}",
            endpoint=endpoint,
            payload={"prompt": sheet["prompt"], "image_size": size, "num_images": 4},
            media_key="images",
            suffix=".jpg",
        )

    if not args.dry_run:
        print(
            "\nNext: pick one frame per sheet, save it in fal Assets under the same tag "
            "(@hedgerow, @moth, @whale, @n_atlantic, @s_pacific, @thrush, @branch), then run make_shots.py."
        )


if __name__ == "__main__":
    main()
