"""Generate keyframes for the tone proof and three production-risk tests."""

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
DEST = PROJECT / "out" / "proof_keyframes"
sys.path.insert(0, str(ROOT / "scripts"))

from falgen import Fal  # noqa: E402


def data_uri(path: pathlib.Path) -> str:
    mime = mimetypes.guess_type(path.name)[0] or "image/jpeg"
    return f"data:{mime};base64," + base64.b64encode(path.read_bytes()).decode()


def edit(fal: Fal, name: str, prompt: str, images: list[pathlib.Path]) -> pathlib.Path:
    generated = fal.generate(
        job=f"continuity_proof_{name}",
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
    if generated is None:
        raise RuntimeError(name)
    DEST.mkdir(parents=True, exist_ok=True)
    destination = DEST / f"{name}.jpg"
    shutil.copy2(generated, destination)
    return destination


def main() -> None:
    fal = Fal()

    # Correct the one failed state edit before it enters any proof.
    lina = edit(
        fal,
        "lina_allocated_corrected",
        "Use image 1 as immutable identity and layout. Correct ONLY the scar: remove "
        "the red vertical forehead marks in every panel and replace them with one "
        "subtle healed 12-millimeter HORIZONTAL skin-toned scar immediately above "
        "her LEFT eyebrow. Preserve face, hair, badge, wardrobe, satchel, wooden "
        "airplane, pose, crop and background exactly. No fresh blood or redness.",
        [STATES / "lina_allocated.jpg"],
    )

    p03 = edit(
        fal,
        "P03_dome_master",
        "Use image 1 as immutable payroll-office architecture, camera and lighting. "
        "Place the exact woman from image 2 standing frame-left in her gray field "
        "jacket, restrained and procedural. Place the exact older woman from image 3 "
        "seated naturally behind the desk frame-right. On the clean right side of the "
        "desk place the exact cobalt-striped mug from image 4 beneath a clear glass "
        "evidence dome. The standing woman has just finished lowering the dome and "
        "keeps both hands clear of it. Preserve CRT, cabinets, walls, desk geometry "
        "and empty institutional light. Static 40mm frame. No text or extra objects.",
        [REFS / "payroll.jpg", REFS / "mara.jpg", REFS / "june.jpg", REFS / "mug.jpg"],
    )
    edit(
        fal,
        "P04_mara_close",
        "Derive a locked 65mm medium close-up from image 1. Preserve exact woman, "
        "payroll room and lighting. Mara studies condensation inside the glass dome "
        "with technical concentration. Dome edge and cobalt-striped mug occupy soft "
        "foreground frame-right; her face is sharp frame-left. No speaking pose, no "
        "fear, no dramatic lighting, no text, no new objects.",
        [p03, REFS / "mara.jpg", REFS / "mug.jpg"],
    )

    loop_a = edit(
        fal,
        "E01_loop_start",
        "Use image 1 as immutable street architecture, camera, clock and wet lighting. "
        "Place the exact man from image 2 in the FAR background pedestrian lane at "
        "frame-right, full body, starting to walk RIGHT-TO-LEFT, green umbrella in "
        "right hand and newspaper under left arm. Place the exact rust-coated woman "
        "from image 3 seated at the foreground table, small in frame, unaware of him. "
        "Keep the lane clear and all perspective natural. Locked 40mm master. No text.",
        [REFS / "street.jpg", REFS / "peter.jpg", REFS / "witness.jpg"],
    )
    edit(
        fal,
        "E04_awareness_end",
        "Preserve image 1 pixel-for-pixel except Peter: in the same far pedestrian "
        "lane, stop him near frame-left and turn only his head and upper body toward "
        "the camera. Keep exact identity, suit, green umbrella, newspaper, scale, "
        "lighting and ground contact. Witness, clock, table, building, rain and every "
        "background pixel remain unchanged. Restrained questioning expression.",
        [loop_a, REFS / "peter.jpg"],
    )

    recognition_a = edit(
        fal,
        "I06_recognition_peter",
        "Use image 1 as immutable kitchen architecture, camera and lighting. Place the "
        "exact elderly woman from image 2 centered in the doorway, naturally looking "
        "with recognition toward the exact man from image 3 standing frame-left. "
        "Place the exact allocated woman from image 4 frame-right, waiting neutrally. "
        "All three full or three-quarter bodies, restrained posture, no overlap. "
        "Perfectly locked symmetrical 40mm composition. No forms, text or mug.",
        [STATES / "kitchen_clean.jpg", REFS / "mrs_gray.jpg", STATES / "peter_no_scar.jpg", lina],
    )
    edit(
        fal,
        "I07_recognition_lina",
        "Preserve image 1 architecture, crop, lighting, people, clothing, body position "
        "and every background pixel. Change ONLY Mrs. Gray's head direction and eyes "
        "so she now recognizes Lina at frame-right rather than Peter at frame-left. "
        "Peter and Lina do not move. No morphing, text, paper or added objects.",
        [recognition_a],
    )

    edit(
        fal,
        "T03_opening_set",
        "Use image 1 as immutable training-room architecture, camera and lighting. "
        "Place the exact field-costume woman from image 2 behind the center lectern, "
        "looking down as she opens the oxblood binder. Preserve drawers rear-left, "
        "door, projector frame-right, ceiling, floor and every straight line. Locked "
        "28mm master. No text or mug.",
        [REFS / "training_room.jpg", REFS / "mara.jpg"],
    )
    edit(
        fal,
        "R01_reconciled_set",
        "Use image 1 as immutable training-room background and exact camera. Composite "
        "the exact instructor-costume woman from image 2 behind the lectern. Place the "
        "exact cobalt-striped mug from image 3 at her right hand (viewer frame-left) "
        "and the oxblood binder open before her. Preserve every drawer, door, projector, "
        "ceiling tile, floor seam, wall edge and perspective from image 1. Locked 28mm "
        "master. No text or additional props.",
        [REFS / "training_room.jpg", STATES / "mara_instructor.jpg", REFS / "mug.jpg"],
    )

    print(DEST)


if __name__ == "__main__":
    main()

