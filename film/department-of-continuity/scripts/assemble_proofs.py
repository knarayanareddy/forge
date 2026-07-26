"""Assemble the 26s tone proof and the three-item risk reel."""

from __future__ import annotations

import json
import pathlib
import shutil
import subprocess
import tempfile

from PIL import Image, ImageChops, ImageDraw, ImageFont, ImageStat

PROJECT = pathlib.Path(__file__).resolve().parents[1]
CLIPS = PROJECT / "out" / "proof_clips"
DOCS = PROJECT / "out" / "proof_documents"
FRAMES = PROJECT / "out" / "proof_keyframes"
AUDIO = PROJECT / "out" / "scratch_audio"
EDIT = PROJECT / "out" / "proof_edit"
ART = pathlib.Path("/opt/cursor/artifacts/department-of-continuity")


def run(cmd: list[str]) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True)


def still_clip(image: pathlib.Path, seconds: float, dest: pathlib.Path, fps: int = 24) -> pathlib.Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    run(
        [
            "ffmpeg", "-y", "-loglevel", "error",
            "-loop", "1", "-i", str(image),
            "-vf", f"scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,fps={fps}",
            "-t", f"{seconds:.3f}", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-an", str(dest),
        ]
    )
    return dest


def trim_clip(src: pathlib.Path, seconds: float, dest: pathlib.Path) -> pathlib.Path:
    run(
        [
            "ffmpeg", "-y", "-loglevel", "error",
            "-i", str(src), "-t", f"{seconds:.3f}",
            "-vf", "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,fps=24",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an", str(dest),
        ]
    )
    return dest


def ken_burns_hold(image: pathlib.Path, seconds: float, dest: pathlib.Path) -> pathlib.Path:
    # true hold — no fake drama; static pad only
    return still_clip(image, seconds, dest)


def concat(paths: list[pathlib.Path], dest: pathlib.Path) -> pathlib.Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as fh:
        for p in paths:
            fh.write(f"file '{p.resolve()}'\n")
        list_path = fh.name
    run(
        [
            "ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0",
            "-i", list_path, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an", str(dest),
        ]
    )
    return dest


def stamp_animation(thu: pathlib.Path, fri: pathlib.Path, stamp: pathlib.Path, dest: pathlib.Path, seconds: float = 5.0) -> pathlib.Path:
    """Build P06: hold Thursday, stamp hits, cut to Friday date."""
    work = EDIT / "tmp_p06"
    work.mkdir(parents=True, exist_ok=True)
    frames_dir = work / "frames"
    if frames_dir.exists():
        shutil.rmtree(frames_dir)
    frames_dir.mkdir()
    n = int(round(seconds * 24))
    stamp_hit = int(round(1.6 * 24))
    a = Image.open(thu).convert("RGBA").resize((1920, 1080))
    b = Image.open(fri).convert("RGBA").resize((1920, 1080))
    s = Image.open(stamp).convert("RGBA")
    s = s.resize((520, 160), Image.Resampling.LANCZOS)
    for i in range(n):
        if i < stamp_hit:
            frame = a.copy()
            # stamp descends
            t = i / max(stamp_hit - 1, 1)
            y = int(-200 + t * (420 + 200))
            alpha = int(40 + 180 * t)
            stamp_i = s.copy()
            stamp_i.putalpha(ImageEnhance_alpha(stamp_i, alpha))
            frame.alpha_composite(stamp_i, (1180, y))
        elif i < stamp_hit + 6:
            frame = a.copy()
            frame.alpha_composite(s, (1180, 420))
        else:
            frame = b.copy()
            frame.alpha_composite(s, (1180, 420))
        frame.convert("RGB").save(frames_dir / f"f{i:04d}.jpg", quality=92)
    run(
        [
            "ffmpeg", "-y", "-loglevel", "error", "-framerate", "24",
            "-i", str(frames_dir / "f%04d.jpg"),
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an", str(dest),
        ]
    )
    return dest


def ImageEnhance_alpha(im: Image.Image, alpha: int) -> Image.Image:
    r, g, b, a = im.split()
    a = a.point(lambda p: min(p, alpha))
    return Image.merge("RGBA", (r, g, b, a))


def hospital_sequence(dest: pathlib.Path, june_dur: float = 4.0, peter_dur: float = 5.0) -> pathlib.Path:
    work = EDIT / "tmp_hospital"
    work.mkdir(parents=True, exist_ok=True)
    parts = []
    # P07: june with receipt sliding in
    slides = sorted(DOCS.glob("P07_slide_*.jpg"))
    if not slides:
        parts.append(still_clip(DOCS / "P07_hospital_june.jpg", june_dur, work / "p07.mp4"))
    else:
        # distribute duration across slides + hold
        hold = still_clip(DOCS / "P07_hospital_june.jpg", 1.2, work / "p07_hold.mp4")
        slide_parts = []
        each = (june_dur - 1.2) / len(slides)
        for i, s in enumerate(slides):
            slide_parts.append(still_clip(s, each, work / f"p07s{i}.mp4"))
        parts.append(concat([hold] + slide_parts, work / "p07.mp4"))

    # P08: fully covered then reveal peter
    reveals = sorted(DOCS.glob("P08_reveal_*.jpg"))
    if not reveals:
        parts.append(still_clip(DOCS / "P08_hospital_peter.jpg", peter_dur, work / "p08.mp4"))
    else:
        each = peter_dur / len(reveals)
        rev_parts = [still_clip(s, each, work / f"p08r{i}.mp4") for i, s in enumerate(reveals)]
        parts.append(concat(rev_parts, work / "p08.mp4"))
    return concat(parts, dest)


def mix_audio(picture: pathlib.Path, start: float, duration: float, dest: pathlib.Path) -> pathlib.Path:
    """Slice scratch dialogue timeline and mux."""
    wav = AUDIO / "scratch_dialogue.wav"
    run(
        [
            "ffmpeg", "-y", "-loglevel", "error",
            "-i", str(picture),
            "-ss", f"{start:.3f}", "-t", f"{duration:.3f}", "-i", str(wav),
            "-filter_complex", "[1:a]aformat=channel_layouts=stereo,volume=1.0[a]",
            "-map", "0:v", "-map", "[a]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
            "-shortest", str(dest),
        ]
    )
    return dest


def recognition_wipe(dest: pathlib.Path, seconds: float = 6.0) -> pathlib.Path:
    a = Image.open(FRAMES / "I06_recognition_peter.jpg").convert("RGB").resize((1920, 1080))
    b = Image.open(FRAMES / "I07_recognition_lina.jpg").convert("RGB").resize((1920, 1080))
    work = EDIT / "tmp_wipe" / "frames"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    n = int(round(seconds * 24))
    wipe_start = int(1.5 * 24)
    wipe_end = int(3.5 * 24)
    paper = Image.new("RGB", (1920, 1080), (236, 228, 208))
    d = ImageDraw.Draw(paper)
    try:
        fnt = ImageFont.truetype("/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf", 42)
    except OSError:
        fnt = ImageFont.load_default()
    d.text((120, 480), "ALLOCATION FORM  —  PERSONNEL LINK", fill=(60, 40, 40), font=fnt)
    for i in range(n):
        if i < wipe_start:
            frame = a
        elif i > wipe_end:
            frame = b
        else:
            t = (i - wipe_start) / max(wipe_end - wipe_start, 1)
            # left-to-right paper wipe: show B on left of leading edge
            edge = int(t * 1920)
            frame = a.copy()
            frame.paste(b.crop((0, 0, edge, 1080)), (0, 0))
            # paper band
            band = min(edge, 220)
            if edge > 0:
                frame.paste(paper.crop((edge - band, 0, edge, 1080)), (edge - band, 0))
        frame.save(work / f"f{i:04d}.jpg", quality=92)
    run(
        [
            "ffmpeg", "-y", "-loglevel", "error", "-framerate", "24",
            "-i", str(work / "f%04d.jpg"),
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an", str(dest),
        ]
    )
    return dest


def exact_loop_test(dest: pathlib.Path) -> pathlib.Path:
    """Duplicate first 11s of E01 twice, then append awareness tail."""
    src = CLIPS / "E01.mp4"
    tail = CLIPS / "E04_tail.mp4"
    work = EDIT / "tmp_loop"
    work.mkdir(parents=True, exist_ok=True)
    loop = trim_clip(src, 11.0, work / "loop11.mp4")
    parts = [loop, loop]
    if tail.exists():
        parts.append(trim_clip(tail, 0.75, work / "tail.mp4"))  # ~18 frames
    return concat(parts, dest)


def set_match_overlay(dest: pathlib.Path) -> pathlib.Path:
    a = Image.open(FRAMES / "T03_opening_set.jpg").convert("RGB").resize((1920, 1080))
    b = Image.open(FRAMES / "R01_reconciled_set.jpg").convert("RGB").resize((1920, 1080))
    diff = ImageChops.difference(a, b)
    # amplify difference for inspection
    diff = diff.point(lambda p: min(255, p * 4))
    # side-by-side + diff
    canvas = Image.new("RGB", (1920, 1080), (0, 0, 0))
    a_s = a.resize((960, 540))
    b_s = b.resize((960, 540))
    d_s = diff.resize((960, 540))
    canvas.paste(a_s, (0, 0))
    canvas.paste(b_s, (960, 0))
    canvas.paste(d_s, (480, 540))
    draw = ImageDraw.Draw(canvas)
    try:
        fnt = ImageFont.truetype("/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf", 28)
    except OSError:
        fnt = ImageFont.load_default()
    draw.text((20, 8), "T03 opening", fill=(255, 255, 200), font=fnt)
    draw.text((980, 8), "R01 reconciled", fill=(255, 255, 200), font=fnt)
    draw.text((500, 548), "4x difference (architecture should be near-black)", fill=(255, 220, 180), font=fnt)
    plate = EDIT / "set_match_plate.jpg"
    canvas.save(plate, quality=95)
    # stats
    stat = ImageStat.Stat(diff)
    report = {
        "mean_rgb": stat.mean,
        "rms_rgb": stat.rms,
        "note": "Architecture reuse gate: mean difference should stay low outside Mara/props region.",
    }
    (EDIT / "set_match_stats.json").write_text(json.dumps(report, indent=2))
    return still_clip(plate, 5.0, dest)


def label_slate(text: str, seconds: float, dest: pathlib.Path) -> pathlib.Path:
    img = Image.new("RGB", (1920, 1080), (18, 18, 18))
    d = ImageDraw.Draw(img)
    try:
        fnt = ImageFont.truetype("/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf", 48)
    except OSError:
        fnt = ImageFont.load_default()
    d.text((80, 500), text, fill=(230, 220, 200), font=fnt)
    plate = dest.with_suffix(".jpg")
    img.save(plate, quality=95)
    return still_clip(plate, seconds, dest)


def assemble_tone_proof() -> pathlib.Path:
    EDIT.mkdir(parents=True, exist_ok=True)
    work = EDIT / "tone_parts"
    work.mkdir(parents=True, exist_ok=True)

    p03 = trim_clip(CLIPS / "P03.mp4", 5.0, work / "P03.mp4")
    p04 = trim_clip(CLIPS / "P04.mp4", 3.0, work / "P04.mp4")
    p05 = still_clip(DOCS / "P05_overhead_thursday.jpg", 4.0, work / "P05.mp4")
    p06 = stamp_animation(
        DOCS / "P05_overhead_thursday.jpg",
        DOCS / "P06_overhead_friday.jpg",
        DOCS / "stamp_authoritative.png",
        work / "P06.mp4",
        5.0,
    )
    hospital = hospital_sequence(work / "P07_P08.mp4", 4.0, 5.0)
    picture = concat([p03, p04, p05, p06, hospital], EDIT / "tone_proof_picture.mp4")
    # Timeline: P03 starts at 32.0 in full film; proof is local 0..26
    # Scratch VO is absolute to film timeline — extract 32.0 for 26s
    final = mix_audio(picture, start=32.0, duration=26.0, dest=EDIT / "tone_proof.mp4")
    return final


def assemble_risk_reel() -> pathlib.Path:
    work = EDIT / "risk_parts"
    work.mkdir(parents=True, exist_ok=True)
    parts = [
        label_slate("RISK 1 — Exact +11.000s loop", 2.0, work / "s1.mp4"),
        exact_loop_test(work / "loop.mp4"),
        label_slate("RISK 2 — Recognition paper wipe", 2.0, work / "s2.mp4"),
        recognition_wipe(work / "wipe.mp4", 6.0),
        label_slate("RISK 3 — T03/R01 set match", 2.0, work / "s3.mp4"),
        set_match_overlay(work / "set.mp4"),
    ]
    # optional motion holds
    if (CLIPS / "T03.mp4").exists() and (CLIPS / "R01.mp4").exists():
        parts.append(label_slate("T03 / R01 motion holds", 1.5, work / "s4.mp4"))
        parts.append(trim_clip(CLIPS / "T03.mp4", 4.0, work / "t03.mp4"))
        parts.append(trim_clip(CLIPS / "R01.mp4", 4.0, work / "r01.mp4"))
    picture = concat(parts, EDIT / "risk_reel_picture.mp4")
    # light room-tone from scratch bed: use silence + tiny VO snippet none
    run(
        [
            "ffmpeg", "-y", "-loglevel", "error",
            "-i", str(picture),
            "-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000",
            "-shortest", "-c:v", "copy", "-c:a", "aac", "-b:a", "128k",
            str(EDIT / "risk_reel.mp4"),
        ]
    )
    return EDIT / "risk_reel.mp4"


def publish(path: pathlib.Path) -> None:
    ART.mkdir(parents=True, exist_ok=True)
    shutil.copy2(path, ART / path.name)
    print(f"published {ART / path.name}")


def main() -> None:
    missing = [s for s in ("P03", "P04", "E01") if not (CLIPS / f"{s}.mp4").exists()]
    if missing:
        raise SystemExit(f"missing motion clips: {missing}. Run generate_proof_motion.py first.")
    for req in ("P05_overhead_thursday.jpg", "P06_overhead_friday.jpg", "stamp_authoritative.png", "P07_hospital_june.jpg", "P08_hospital_peter.jpg"):
        if not (DOCS / req).exists():
            raise SystemExit(f"missing document plate {req}. Run generate_proof_documents.py first.")

    tone = assemble_tone_proof()
    risk = assemble_risk_reel()
    publish(tone)
    publish(risk)
    for extra in ("set_match_plate.jpg", "set_match_stats.json", "tone_proof_picture.mp4"):
        p = EDIT / extra
        if p.exists():
            shutil.copy2(p, ART / p.name)
    print("tone:", tone)
    print("risk:", risk)


if __name__ == "__main__":
    main()
