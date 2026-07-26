"""Typography for BLINDSPOT: chapter cards, slates, chyrons and location stamps.

All on-screen text is rendered here and composited in post. Video models still
mangle lettering, and this film's credibility lives entirely in its chyrons.

Rendered as RGBA PNGs rather than ffmpeg drawtext so we get real letter-spacing
and so the whole chyron can be faded as one object.
"""

from __future__ import annotations

import argparse
import json
import pathlib

from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "out"
CARDS = OUT / "cards"

BG = (8, 8, 10)
INK = (222, 220, 214)
INK_DIM = (150, 148, 143)
MARGIN = 140


def _spec() -> dict:
    with open(ROOT / "manifests" / "timeline.json") as fh:
        return json.load(fh)


def font(path: str, size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(path, size)


def tracked_width(draw: ImageDraw.ImageDraw, text: str, f: ImageFont.FreeTypeFont, tracking: int) -> int:
    if not text:
        return 0
    return sum(int(draw.textlength(ch, font=f)) for ch in text) + tracking * (len(text) - 1)


def draw_tracked(draw: ImageDraw.ImageDraw, xy: tuple[int, int], text: str,
                 f: ImageFont.FreeTypeFont, fill, tracking: int) -> None:
    """PIL has no letter-spacing, so step through the string a glyph at a time.

    Tracked-out capitals are most of what makes a title card read as broadcast
    rather than as a slide.
    """
    x, y = xy
    for ch in text:
        draw.text((x, y), ch, font=f, fill=fill)
        x += int(draw.textlength(ch, font=f)) + tracking


def render_card(style: str, lines: list[str], dest: pathlib.Path, spec: dict) -> pathlib.Path:
    w, h = spec["spec"]["width"], spec["spec"]["height"]
    fonts = spec["fonts"]
    img = Image.new("RGB", (w, h), BG)
    draw = ImageDraw.Draw(img)

    if style == "chapter":
        # Roman numeral above, title below, both tracked out.
        numeral = lines[0]
        title = lines[1] if len(lines) > 1 else ""
        f_num = font(fonts["serif"], 62)
        f_title = font(fonts["serif"], 46)
        tr_num, tr_title = 10, 14
        block_h = 62 + (54 if title else 0)
        y = (h - block_h) // 2
        tw = tracked_width(draw, numeral, f_num, tr_num)
        draw_tracked(draw, ((w - tw) // 2, y), numeral, f_num, INK, tr_num)
        if title:
            y += 96
            tw = tracked_width(draw, title, f_title, tr_title)
            draw_tracked(draw, ((w - tw) // 2, y), title, f_title, INK, tr_title)
    else:
        f_main = font(fonts["serif"], 54)
        f_sub = font(fonts["serif"], 34)
        rendered = [(ln, f_main if i == 0 else f_sub) for i, ln in enumerate(lines)]
        total = sum(0 if not ln else (f.size + 26) for ln, f in rendered)
        total += sum(28 for ln, _ in rendered if not ln)
        y = (h - total) // 2
        for i, (ln, f) in enumerate(rendered):
            if not ln:
                y += 28
                continue
            tracking = 12 if i == 0 else 8
            tw = tracked_width(draw, ln, f, tracking)
            draw_tracked(draw, ((w - tw) // 2, y), ln, f,
                         INK if i == 0 else INK_DIM, tracking)
            y += f.size + 26

    dest.parent.mkdir(parents=True, exist_ok=True)
    img.save(dest)
    return dest


def render_chyron(lines: list[str], dest: pathlib.Path, spec: dict) -> pathlib.Path:
    """Lower-third specimen chyron. Transparent everywhere except the plate.

    The audience is trained on this exact template for five minutes so that
    APPENDIX A arrives on furniture they already trust.
    """
    w, h = spec["spec"]["width"], spec["spec"]["height"]
    fonts = spec["fonts"]
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    f_id = font(fonts["sans_bold"], 34)
    f_bin = font(fonts["sans_italic"], 30)
    f_pop = font(fonts["sans"], 24)
    specs = [(lines[0], f_id, INK, 6), (lines[1], f_bin, INK, 0), (lines[2], f_pop, INK_DIM, 3)]

    pad = 26
    line_gap = 12
    text_h = sum(f.size for _, f, _, _ in specs) + line_gap * (len(specs) - 1)
    plate_h = text_h + pad * 2
    plate_w = max(tracked_width(draw, t, f, tr) for t, f, _, tr in specs) + pad * 2
    top = h - MARGIN - plate_h

    draw.rectangle([MARGIN, top, MARGIN + plate_w, top + plate_h], fill=(0, 0, 0, 150))
    # Single hairline rule on the left edge - the one graphic flourish in the film.
    draw.rectangle([MARGIN, top, MARGIN + 3, top + plate_h], fill=INK + (210,))

    y = top + pad
    for text, f, colour, tracking in specs:
        draw_tracked(draw, (MARGIN + pad, y), text, f, colour + (255,), tracking)
        y += f.size + line_gap

    dest.parent.mkdir(parents=True, exist_ok=True)
    img.save(dest)
    return dest


def render_stamp(text: str, dest: pathlib.Path, spec: dict) -> pathlib.Path:
    """Upper-right location and time stamp. Monospace, small, unglamorous."""
    w, h = spec["spec"]["width"], spec["spec"]["height"]
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    f = font(spec["fonts"]["mono"], 26)
    tracking = 2
    tw = tracked_width(draw, text, f, tracking)
    x = w - MARGIN - tw
    y = MARGIN - 30
    draw.rectangle([x - 16, y - 12, x + tw + 16, y + f.size + 12], fill=(0, 0, 0, 120))
    draw_tracked(draw, (x, y), text, f, INK + (235,), tracking)
    dest.parent.mkdir(parents=True, exist_ok=True)
    img.save(dest)
    return dest


def build_all() -> dict:
    """Render every text element referenced by the timeline. Returns id -> path."""
    spec = _spec()
    CARDS.mkdir(parents=True, exist_ok=True)
    made = {}
    for entry in spec["entries"]:
        eid = entry["id"]
        if entry["type"] == "card":
            made[f"card:{eid}"] = render_card(entry["style"], entry["lines"], CARDS / f"{eid}.png", spec)
        chyron = entry.get("chyron")
        if chyron:
            made[f"chyron:{eid}"] = render_chyron(chyron["lines"], CARDS / f"chyron_{eid}.png", spec)
            tick = chyron.get("population_tick")
            if tick:
                ticked = list(chyron["lines"])
                ticked[tick["line_index"]] = tick["to"]
                made[f"chyron:{eid}:tick"] = render_chyron(ticked, CARDS / f"chyron_{eid}_tick.png", spec)
        if entry.get("stamp"):
            made[f"stamp:{eid}"] = render_stamp(entry["stamp"], CARDS / f"stamp_{eid}.png", spec)
    return made


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description="Render all BLINDSPOT text elements as PNGs.")
    ap.parse_args()
    for key, path in build_all().items():
        print(f"{key:34s} {path.relative_to(ROOT)}")
