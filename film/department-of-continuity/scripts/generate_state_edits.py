"""Create continuity-critical edited states from approved references."""

from __future__ import annotations

import base64
import mimetypes
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
PROJECT = pathlib.Path(__file__).resolve().parents[1]
REFS = PROJECT / "out" / "approved_references"
STATES = PROJECT / "out" / "approved_states"
sys.path.insert(0, str(ROOT / "scripts"))

from falgen import Fal  # noqa: E402


def data_uri(path: pathlib.Path) -> str:
    mime = mimetypes.guess_type(path.name)[0] or "image/jpeg"
    return f"data:{mime};base64," + base64.b64encode(path.read_bytes()).decode()


def edit(fal: Fal, name: str, prompt: str, images: list[pathlib.Path]) -> pathlib.Path:
    result = fal.generate(
        job=f"continuity_state_{name}",
        endpoint="fal-ai/nano-banana-pro/edit",
        payload={
            "prompt": prompt,
            "image_urls": [data_uri(path) for path in images],
            "num_images": 1,
            "aspect_ratio": "16:9",
            "resolution": "2K",
            "output_format": "jpeg",
        },
        media_key="images",
        suffix=".jpg",
    )
    if result is None:
        raise RuntimeError(name)
    STATES.mkdir(parents=True, exist_ok=True)
    destination = STATES / f"{name}.jpg"
    shutil.copy2(result, destination)
    return destination


def main() -> None:
    fal = Fal()

    edit(
        fal,
        "kitchen_clean",
        "Preserve this exact kitchen, camera, crop, architecture, lighting, clock, "
        "furniture and every surface. Remove ONLY the white floor tape, arrows and "
        "the words Peter, Mrs. Gray and Lina. Restore ordinary uninterrupted linoleum "
        "where those markings were. Add nothing. No people, text or paper.",
        [REFS / "kitchen.jpg"],
    )
    edit(
        fal,
        "mara_instructor",
        "Use the exact same woman and identity from image 1. Change only her outfit: "
        "late-1990s celadon government instructor suit, cream blouse, small oxblood "
        "scarf knotted on her left. Keep dark bob, face, fine scar below right ear, "
        "body, steel watch on LEFT wrist and neutral expression identical. Four-view "
        "identity board on warm gray, no labels or text.",
        [REFS / "mara.jpg"],
    )
    edit(
        fal,
        "peter_no_scar",
        "Use the exact same man, views, face, hair, gray suit, umbrella and newspaper "
        "from image 1. Remove only the small scar above his LEFT eyebrow in every view. "
        "Do not change age, expression, anatomy, wardrobe, props, crop or background.",
        [REFS / "peter.jpg"],
    )
    edit(
        fal,
        "lina_allocated",
        "Use the exact same woman, face, hair, badge and burgundy-and-gray wardrobe "
        "from image 1. Add a small horizontal scar above her LEFT eyebrow. In the "
        "full-length view only, place the scuffed red child's satchel from image 2 over "
        "her right shoulder and the small wooden airplane from image 2 in her left "
        "hand. Keep every identity view consistent. No labels or text.",
        [REFS / "lina.jpg", REFS / "props.jpg"],
    )
    june_hospital = edit(
        fal,
        "hospital_june",
        "Use image 1 as immutable architecture and framing. Place the exact woman from "
        "image 2 lying naturally in the hospital bed beneath the blanket, wearing a "
        "plain pale hospital gown and looking neutrally toward camera. Preserve every "
        "wall mark, light, cabinet, bed rail, pillow, perspective and empty name-card "
        "slot from image 1. Flat clinical documentation photograph. No text or mug.",
        [REFS / "hospital_photo.jpg", REFS / "june.jpg"],
    )
    edit(
        fal,
        "hospital_peter",
        "Use image 1 as immutable architecture, crop and lighting. Replace ONLY the "
        "woman in the bed with the exact man from image 2, wearing a plain pale hospital "
        "gown in the same body position. Preserve every wall mark, light, cabinet, bed "
        "rail, blanket fold, pillow indentation, perspective and empty name-card slot. "
        "Flat clinical documentation photograph. No text or mug.",
        [june_hospital, REFS / "peter.jpg"],
    )

    print(STATES)


if __name__ == "__main__":
    main()

