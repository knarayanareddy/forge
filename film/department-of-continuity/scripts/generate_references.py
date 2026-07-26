"""Generate controlled reference packs for The Department of Continuity.

The bake-off subcommand sends identical creative briefs to Nano Banana Pro and
FLUX.2 Pro. Nothing downstream is generated until one result is explicitly
selected. All requests and public result URLs are retained by the shared
falgen.Fal metadata recorder.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
PROJECT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from falgen import Fal  # noqa: E402


STYLE = (
    "Late-1990s American public-institution training film photographed on 35mm. "
    "Restrained bureaucratic realism, earnest and non-horror. Muted skin tones, "
    "lifted blacks, restrained cyan shadows and weak amber highlights. Natural "
    "material texture and anatomy. No glossy advertising light, cyberpunk, "
    "shallow-focus beauty styling, VHS damage, text, labels, logos or watermarks."
)

BRIEFS = {
    "mara": (
        "Character identity board for MARA VOSS, a 38-year-old American continuity "
        "technician. One consistent woman shown in four views: neutral front headshot, "
        "left three-quarter headshot, right profile and full-length standing view. "
        "Dark blunt bob ending at jaw, gray-green eyes, fine vertical scar directly "
        "below the right ear, no earrings, minimal makeup, intelligent reserved face. "
        "Field costume: charcoal-gray government field jacket over cream blouse, "
        "charcoal trousers, black practical shoes, plain steel watch on LEFT wrist, "
        "oxblood notebook. Restrained neutral expression in every view. Warm-gray "
        "seamless studio background. Same exact identity, hair, scar and wardrobe in "
        "all four views. No captions or layout text."
    ),
    "mug": (
        "Museum-grade object identity board of one exact cream stoneware coffee mug "
        "shown in six consistent views. One 14-millimeter cobalt-blue horizontal "
        "stripe around the body, small chip on the upper-right edge of the handle, "
        "faint oxblood lipstick arc at 11 o'clock on the rim, hairline glaze crack "
        "under the base. Heavy ordinary 1990s institutional ceramic, no logo, no text. "
        "Front, back, left, right, top and bottom views, all the same object, neutral "
        "gray studio, soft flat reference lighting. No labels."
    ),
    "training_room": (
        "Empty government employee training room, squared symmetrical master wide. "
        "Nicotine-cream upper walls, celadon-painted lower walls, gray linoleum, "
        "fluorescent ceiling practicals, gray steel furniture. Steel lectern centered. "
        "Three identical blank-label card-catalog drawers grouped rear-left. 16mm "
        "projector on cart frame-right. Closed exit door behind lectern's right "
        "shoulder. Red three-ring binder centered on lectern. No people. Every line "
        "straight, clean and architecturally plausible. Camera fixed at 1.45 meters, "
        "28mm spherical lens, 1.66:1 composition centered in 16:9."
    ),
    "payroll": (
        "Empty late-1990s government payroll office master. One employee desk at a "
        "shallow three-quarter angle, green metal filing cabinets, beige CRT computer, "
        "paper in-trays, fluorescent ceiling practicals, nicotine-cream walls and "
        "celadon lower paint. Preserve a deliberately EMPTY clean area on the desk's "
        "frame-right side for a future coffee mug. No cups, mugs, glasses, bottles or "
        "people anywhere. Camera locked at 1.4 meters on a 40mm spherical lens, "
        "institutional depth of field, 1.66:1 composition centered in 16:9."
    ),
}

FULL_BRIEFS = {
    "peter": (
        "Character identity board for PETER GRAY, a 43-year-old American man. "
        "One consistent person in neutral front headshot, left three-quarter, right "
        "profile and full-length standing view. Tired kind face, short dark-brown "
        "hair receding slightly, small horizontal childhood scar directly above LEFT "
        "eyebrow. Gray 1990s business suit, white shirt, dark knit tie, forest-green "
        "umbrella in RIGHT hand, folded newspaper beneath LEFT arm. Restrained neutral "
        "expression. Warm-gray seamless studio. Same exact identity, scar, wardrobe "
        "and props in every view. No captions or text."
    ),
    "lina": (
        "Character identity board for LINA ORTIZ, a 32-year-old Mexican-American "
        "government records employee. One consistent woman in neutral front headshot, "
        "left three-quarter, right profile and full-length standing view. Dark wavy "
        "hair in a low practical ponytail, brown eyes, no facial scar, small gold stud "
        "earrings. Burgundy blouse, gray knee-length skirt, cream government badge on "
        "LEFT chest, black low shoes. Empty hands. Restrained neutral expression. "
        "Warm-gray seamless studio. Same exact identity and wardrobe in every view. "
        "No captions or text."
    ),
    "june": (
        "Character identity board for JUNE HAVEL, a practical 54-year-old American "
        "payroll clerk. One consistent woman in neutral front headshot, three-quarter "
        "headshot and full-length standing view. Short softly curled salt-and-pepper "
        "hair, rectangular brown reading glasses, reserved dry expression. Muted blue "
        "cardigan, cream patterned blouse, charcoal slacks, simple wedding band. "
        "Warm-gray seamless studio, flat reference lighting. No captions or text."
    ),
    "mrs_gray": (
        "Character identity board for MRS. ELEANOR GRAY, Peter's 72-year-old American "
        "mother. One consistent elderly woman in neutral front headshot, left "
        "three-quarter, right profile and full-length standing view. Fine silver hair "
        "in a low bun, pale blue eyes, ordinary lived-in face, no glamorous styling. "
        "Soft moss-green cardigan, ivory blouse, dark skirt, house shoes. Restrained "
        "warm neutral expression. Warm-gray studio. No captions or text."
    ),
    "witness": (
        "Character identity board for a 35-year-old American female government "
        "cafeteria worker used as the street witness. One consistent woman shown in "
        "front headshot, three-quarter and full-length view. Auburn hair tied back, "
        "rust raincoat over modest office clothes, navy shoulder bag. Ordinary face, "
        "casual neutral expression, no glamour. Warm-gray studio. No captions."
    ),
    "street": (
        "Locked master wide outside a late-1990s government cafeteria on an overcast "
        "wet morning. Couple position in foreground center is empty for later actors. "
        "Deep background pedestrian lane runs unambiguously RIGHT-TO-LEFT. Large round "
        "analog clock fixed upper frame-right, dark green café awning rear-left, broad "
        "window behind foreground table. No people, no umbrellas, no vehicles crossing "
        "the lane. Static 40mm lens, camera 1.5 meters high, straight architecture, "
        "1.66:1 composition centered inside 16:9."
    ),
    "records_office": (
        "Empty late-1990s government records office master. Lunch entrance frame-left, "
        "Lina desk center-right, Peter waiting position center, family-photograph wall "
        "rear-center, Mara position frame-right. Cream walls, celadon lower paint, "
        "fluorescent practicals, gray steel desks and filing shelves. No people, no "
        "photographs yet, no mugs. Locked 40mm lens at 1.45 meters, straight plausible "
        "geometry, 1.66:1 composition centered in 16:9."
    ),
    "kitchen": (
        "Empty modest late-1990s American kitchen prepared for a recognition test. "
        "Doorway centered with Mrs. Gray mark in the threshold, Peter mark frame-left "
        "and Lina mark frame-right, enough separation for three layered performances. "
        "Pale cream cabinets, faded green walls, old refrigerator, small wall clock, "
        "soft overcast window light. No people, no paperwork, no mugs. Perfectly locked "
        "symmetrical 40mm camera, no perspective distortion, 1.66:1 in 16:9."
    ),
    "hospital_photo": (
        "A late-1990s hospital documentation photograph, landscape 3:2, showing an "
        "empty made hospital bed in a plain institutional room. Patient name-card slot "
        "at foot of bed left blank. Pillow has a subtle human indentation. Fluorescent "
        "ceiling light, pale green wall, steel bedside cabinet. Camera squared and "
        "locked, flat clinical exposure, no patient, no staff, no text, no mug."
    ),
    "props": (
        "Museum-grade prop identity board on neutral gray showing four separate exact "
        "objects: oxblood red three-ring government binder; scuffed red child's leather "
        "satchel with brass buckle; small hand-carved wooden propeller airplane; folded "
        "late-1990s broadsheet newspaper. Each object isolated with front, side and "
        "three-quarter view, consistent damage and materials. No umbrella, no mug, no "
        "labels, logos or readable text."
    ),
}


def generate_one(fal: Fal, model: str, name: str, prompt: str) -> pathlib.Path | None:
    if model == "nano":
        endpoint = "fal-ai/nano-banana-pro"
        payload = {
            "prompt": STYLE + " " + prompt,
            "num_images": 1,
            "aspect_ratio": "16:9",
            "resolution": "2K",
            "output_format": "jpeg",
        }
    elif model == "flux":
        endpoint = "fal-ai/flux-2-pro"
        payload = {
            "prompt": STYLE + " " + prompt,
            "image_size": {"width": 1792, "height": 1080},
            "safety_tolerance": "2",
            "output_format": "jpeg",
        }
    else:
        raise ValueError(model)
    return fal.generate(
        job=f"continuity_ref_{name}_{model}",
        endpoint=endpoint,
        payload=payload,
        media_key="images",
        suffix=".jpg",
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["bakeoff", "full"])
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    fal = Fal(dry_run=args.dry_run)
    if args.mode == "bakeoff":
        for name, brief in BRIEFS.items():
            for model in ("nano", "flux"):
                generated = generate_one(fal, model, name, brief)
                if generated and not args.dry_run:
                    review = PROJECT / "out" / "reference_bakeoff"
                    review.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(generated, review / f"{name}_{model}.jpg")
        if not args.dry_run:
            print(f"Review candidates in {PROJECT / 'out' / 'reference_bakeoff'}")
        return

    approved = PROJECT / "out" / "approved_references"
    approved.mkdir(parents=True, exist_ok=True)
    for name in BRIEFS:
        winner = PROJECT / "out" / "reference_bakeoff" / f"{name}_nano.jpg"
        if winner.exists():
            shutil.copy2(winner, approved / f"{name}.jpg")
    for name, brief in FULL_BRIEFS.items():
        generated = generate_one(fal, "nano", name, brief)
        if generated and not args.dry_run:
            shutil.copy2(generated, approved / f"{name}.jpg")
    if not args.dry_run:
        print(f"Approved/reference candidates in {approved}")


if __name__ == "__main__":
    main()

