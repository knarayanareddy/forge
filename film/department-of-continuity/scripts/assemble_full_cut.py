"""Assemble the complete 156-second cinematic cut from approved footage and plates.

Every generated motion clip is kept in its strongest role.  Controlled text,
loop timing, image-state changes, and certificate graphics are authored in
post so the story stays legible instead of asking a video model to invent it.
"""

from __future__ import annotations

import json
import pathlib
import shutil
import subprocess
import tempfile

from PIL import Image, ImageDraw, ImageFont

PROJECT = pathlib.Path(__file__).resolve().parents[1]
OUT = PROJECT / "out"
REFS = OUT / "approved_references"
STATES = OUT / "approved_states"
FRAMES = OUT / "proof_keyframes"
DOCS = OUT / "proof_documents"
CLIPS = OUT / "proof_clips"
EDIT = OUT / "final_edit"
ART = pathlib.Path("/opt/cursor/artifacts/department-of-continuity")
FONT = "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"
BOLD = "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf"


def run(cmd: list[str]) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True)


def fnt(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(BOLD if bold else FONT, size)


def slate(name: str, lines: list[str], *, dark: bool = False) -> pathlib.Path:
    EDIT.mkdir(parents=True, exist_ok=True)
    path = EDIT / f"{name}.jpg"
    bg = (22, 22, 20) if dark else (222, 214, 191)
    fg = (224, 218, 200) if dark else (38, 35, 30)
    img = Image.new("RGB", (1920, 1080), bg)
    draw = ImageDraw.Draw(img)
    y = 350
    for i, line in enumerate(lines):
        draw.text((160, y), line, fill=fg if i != len(lines) - 1 else (120, 38, 40),
                  font=fnt(54 if i == 0 else 36, bold=i == 0))
        y += 80
    img.save(path, quality=95)
    return path


def crop_mug(sheet: pathlib.Path, dest: pathlib.Path) -> pathlib.Path:
    src = Image.open(sheet).convert("RGB")
    w, h = src.size
    # Use a single top-left product-panel crop rather than the reference grid.
    src.crop((0, 0, w // 3, h // 2)).save(dest, quality=95)
    return dest


def family_photo(after: bool, objection: bool = False) -> pathlib.Path:
    """A controlled photo plate made from approved identities, not a fresh scene."""
    canvas = Image.new("RGB", (1920, 1080), (118, 108, 90))
    draw = ImageDraw.Draw(canvas)
    draw.rectangle((200, 110, 1720, 970), fill=(218, 205, 174), outline=(76, 59, 48), width=18)
    draw.rectangle((240, 150, 1680, 930), fill=(173, 168, 146))
    # The plate is deliberately documentary-flat; portraits are small evidence inside it.
    mrs = Image.open(REFS / "mrs_gray.jpg").convert("RGB").resize((300, 360))
    child = Image.open((STATES / "lina_allocated.jpg") if after else (REFS / "peter.jpg")).convert("RGB").resize((250, 300))
    canvas.paste(mrs, (630, 350))
    canvas.paste(child, (965, 420))
    if objection:
        mara = Image.open(REFS / "mara.jpg").convert("RGB").resize((180, 220))
        canvas.paste(mara, (1290, 270))
        mug = crop_mug(REFS / "mug.jpg", EDIT / "mug_single.jpg")
        m = Image.open(mug).convert("RGB").resize((100, 100))
        canvas.paste(m, (1110, 690))
    draw.text((285, 180), "GRAY FAMILY / MUNICIPAL SCHOOL DAY", fill=(55, 48, 40), font=fnt(28, True))
    name = "I09_family_after_objection.jpg" if objection else ("I04_family_lina.jpg" if after else "I03_family_peter.jpg")
    path = EDIT / name
    canvas.save(path, quality=94)
    return path


def certificate() -> pathlib.Path:
    img = Image.new("RGB", (1920, 1080), (228, 219, 194))
    d = ImageDraw.Draw(img)
    d.rectangle((220, 145, 1700, 925), outline=(75, 68, 57), width=5)
    d.text((350, 260), "CONTINUITY COMPLETION CERTIFICATE", fill=(55, 49, 42), font=fnt(48, True))
    d.text((360, 410), "COMPLETED BY: MARA VOSS", fill=(35, 33, 30), font=fnt(42))
    d.rectangle((360, 520, 410, 570), outline=(45, 42, 38), width=4)
    d.text((435, 521), "Employee reconciliation acknowledged", fill=(50, 47, 42), font=fnt(28))
    d.text((360, 710), "The employee declines to mark the box.", fill=(110, 95, 80), font=fnt(30))
    path = EDIT / "R04_certificate.jpg"
    img.save(path, quality=95)
    return path


def still_clip(image: pathlib.Path, dur: float, dest: pathlib.Path, zoom: float = 1.0) -> pathlib.Path:
    """A restrained mechanical post move, never a simulated handheld camera."""
    # pad and zoom no more than 3 percent across a shot
    vf = (
        "scale=2048:1152:force_original_aspect_ratio=decrease,"
        "pad=2048:1152:(ow-iw)/2:(oh-ih)/2,"
        f"zoompan=z='min(zoom+{(zoom - 1) / max(dur * 24, 1):.7f}, {zoom:.4f})':"
        "x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s=1920x1080:fps=24"
    )
    run(["ffmpeg", "-y", "-loglevel", "error", "-loop", "1", "-i", str(image), "-t", f"{dur:.3f}",
         "-vf", vf, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an", str(dest)])
    return dest


def motion_clip(src: pathlib.Path, dur: float, dest: pathlib.Path) -> pathlib.Path:
    run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(src), "-t", f"{dur:.3f}",
         "-vf", f"scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,fps=24,tpad=stop_mode=clone:stop_duration={dur:.3f}",
         "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an", str(dest)])
    return dest


def concat(parts: list[pathlib.Path], dest: pathlib.Path) -> pathlib.Path:
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as fh:
        for part in parts:
            fh.write(f"file '{part.resolve()}'\n")
        listing = fh.name
    run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", listing,
         "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an", str(dest)])
    return dest


def copy_loop(src: pathlib.Path, dur: float, dest: pathlib.Path) -> pathlib.Path:
    """E04 is literal reuse, by construction, before its separate awareness tail."""
    return motion_clip(src, dur, dest)


def title_and_leader(work: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    black = slate("T01_leader", [], dark=True)
    title = slate("T02_title", [
        "OFFICE OF RECORDS INTEGRITY",
        "CONTINUITY ADJUSTMENT UNIT",
        "MODULE 4 — SECONDARY DISPLACEMENT",
        "✓  AUTHORIZED FOR INTERNAL OCCURRENCE",
    ])
    return still_clip(black, 3, work / "T01.mp4"), still_clip(title, 3, work / "T02.mp4")


def build_picture() -> pathlib.Path:
    work = EDIT / "shots"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    # Causal chain is locked to manifest timings, all durations sum to 156.
    T01, T02 = title_and_leader(work)
    p03 = CLIPS / "P03.mp4"
    p04 = CLIPS / "P04.mp4"
    e01 = CLIPS / "E01.mp4"
    t03 = CLIPS / "T03.mp4"
    r01 = CLIPS / "R01.mp4"
    street = FRAMES / "E01_loop_start.jpg"
    records = REFS / "records_office.jpg"
    training = FRAMES / "T03_opening_set.jpg"
    mara = REFS / "mara.jpg"
    peter = STATES / "peter_no_scar.jpg"
    lina = FRAMES / "lina_allocated_corrected.jpg"
    kitchen_a = FRAMES / "I06_recognition_peter.jpg"
    kitchen_b = FRAMES / "I07_recognition_lina.jpg"
    payroll = REFS / "payroll.jpg"
    mug = REFS / "mug.jpg"
    family_before = family_photo(False)
    family_after = family_photo(True)
    family_objection = family_photo(True, True)
    cert = certificate()

    def s(i: str, image: pathlib.Path, d: float, z: float = 1.01) -> pathlib.Path:
        return still_clip(image, d, work / f"{i}.mp4", z)
    def m(i: str, image: pathlib.Path, d: float) -> pathlib.Path:
        return motion_clip(image, d, work / f"{i}.mp4")

    parts = [
        T01, T02, m("T03", t03, 5), s("T04", training, 6, 1.02), s("T05", training, 7, 1.02),
        s("P01", payroll, 4, 1.01), s("P02", mug, 4, 1.02), m("P03", p03, 5), m("P04", p04, 3),
        s("P05", DOCS / "P05_overhead_thursday.jpg", 4), s("P06", DOCS / "P06_overhead_friday.jpg", 5),
        s("P07", DOCS / "P07_hospital_june.jpg", 4), s("P08", DOCS / "P08_hospital_peter.jpg", 5),
        m("E01", e01, 6), s("E02", street, 3, 1.01), s("E03", street, 2),
        copy_loop(e01, 6, work / "E04.mp4"), s("E05", FRAMES / "E04_awareness_end.jpg", 4, 1.01),
        s("E06", mara, 4, 1.02), copy_loop(e01, 6, work / "E07.mp4"),
        s("E08", records, 5, 1.01), s("I01", records, 5, 1.02), s("I02", peter, 4, 1.02),
        s("I03", family_before, 4, 1.01), s("I04", family_after, 5, 1.02),
        s("I05", lina, 5, 1.03), s("I06", kitchen_a, 5, 1.01), s("I07", kitchen_b, 5, 1.01),
        s("I08", mara, 4, 1.02), s("I09", family_objection, 6, 1.03),
        m("R01", r01, 5), m("R02", r01, 4), s("R03", FRAMES / "R01_reconciled_set.jpg", 4, 1.01),
        s("R04", cert, 6, 1.02),
    ]
    return concat(parts, EDIT / "department_of_continuity_picture.mp4")


def sound(picture: pathlib.Path) -> pathlib.Path:
    """Mix scratch dialogue with restrained film/projector/room texture."""
    dialogue = OUT / "scratch_audio" / "scratch_dialogue.wav"
    final = EDIT / "department_of_continuity_final.mp4"
    # Low, neutral institutional bed: projector-like filtered noise, no horror score.
    filt = (
        "[1:a]aformat=channel_layouts=stereo,volume=0.075,"
        "highpass=f=70,lowpass=f=3200[room];"
        "[2:a]aformat=channel_layouts=stereo,volume=1.0[vox];"
        "[room][vox]amix=inputs=2:duration=longest:normalize=0,"
        "loudnorm=I=-16:TP=-1.5:LRA=9[a]"
    )
    run([
        "ffmpeg", "-y", "-loglevel", "error", "-i", str(picture),
        "-f", "lavfi", "-t", "156", "-i", "anoisesrc=color=pink:sample_rate=48000",
        "-i", str(dialogue), "-filter_complex", filt,
        "-map", "0:v", "-map", "[a]", "-t", "156",
        "-c:v", "libx264", "-preset", "medium", "-crf", "17", "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart", str(final),
    ])
    return final


def main() -> None:
    EDIT.mkdir(parents=True, exist_ok=True)
    picture = build_picture()
    final = sound(picture)
    ART.mkdir(parents=True, exist_ok=True)
    published = ART / "department-of-continuity-full-cut.mp4"
    shutil.copy2(final, published)
    run(["ffprobe", "-v", "error", "-show_entries", "format=duration:stream=codec_name,width,height,channels",
         "-of", "default=noprint_wrappers=1", str(published)])
    print(published)


if __name__ == "__main__":
    main()
