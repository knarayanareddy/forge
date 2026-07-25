"""fal queue client and prompt composition for BLINDSPOT.

Every generation writes a metadata record to out/meta/<job>.json containing the
exact composed prompt, endpoint, payload, request id and seed, so any shot in the
finished film can be traced back to the call that produced it and re-run.
"""

from __future__ import annotations

import json
import os
import pathlib
import time
import urllib.parse

import requests

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFESTS = ROOT / "manifests"
OUT = ROOT / "out"


def load(name: str) -> dict:
    with open(MANIFESTS / f"{name}.json") as fh:
        return json.load(fh)


class FalError(RuntimeError):
    pass


class Fal:
    def __init__(self, dry_run: bool = False, models: dict | None = None):
        self.models = models or load("models")
        self.api = self.models["api"]
        self.dry_run = dry_run
        self.key = os.environ.get("FAL_KEY")
        if not self.dry_run and not self.key:
            raise FalError(
                "FAL_KEY is not set. Add it as a Cloud Agents secret named FAL_KEY, "
                "or export it locally. Use --dry-run to compose prompts without it."
            )
        self.session = requests.Session()

    # -- transport ---------------------------------------------------------

    def _headers(self) -> dict:
        return {
            "Authorization": f"Key {self.key}",
            "Content-Type": "application/json",
        }

    def _request(self, method: str, url: str, **kw):
        delay = 4
        for attempt in range(5):
            try:
                r = self.session.request(method, url, headers=self._headers(), timeout=120, **kw)
            except requests.RequestException as exc:
                if attempt == 4:
                    raise FalError(f"network failure calling {url}: {exc}") from exc
            else:
                if r.status_code < 500 and r.status_code != 429:
                    return r
                if attempt == 4:
                    raise FalError(f"{r.status_code} from {url}: {r.text[:400]}")
            time.sleep(delay)
            delay *= 2
        raise FalError("unreachable")

    def submit(self, endpoint: str, payload: dict) -> dict:
        url = f"{self.api['queue_base']}/{endpoint}"
        r = self._request("POST", url, data=json.dumps(payload))
        if r.status_code >= 400:
            raise FalError(f"submit failed {r.status_code}: {r.text[:400]}")
        return r.json()

    def wait(self, status_url: str, label: str = "") -> dict:
        deadline = time.time() + self.api["poll_timeout_s"]
        last = None
        while time.time() < deadline:
            r = self._request("GET", status_url)
            body = r.json()
            status = body.get("status")
            if status != last:
                pos = body.get("queue_position")
                extra = f" (queue position {pos})" if pos is not None else ""
                print(f"  {label}: {status}{extra}", flush=True)
                last = status
            if status == "COMPLETED":
                if body.get("error"):
                    raise FalError(f"{label} failed: {body['error']}")
                return body
            time.sleep(self.api["poll_interval_s"])
        raise FalError(f"{label}: timed out after {self.api['poll_timeout_s']}s")

    def result(self, response_url: str) -> dict:
        r = self._request("GET", response_url)
        if r.status_code >= 400:
            raise FalError(f"result fetch failed {r.status_code}: {r.text[:400]}")
        return r.json()

    def run(self, endpoint: str, payload: dict, label: str = "") -> dict:
        queued = self.submit(endpoint, payload)
        self.wait(queued["status_url"], label or endpoint)
        return self.result(queued["response_url"])

    def download(self, url: str, dest: pathlib.Path) -> pathlib.Path:
        dest.parent.mkdir(parents=True, exist_ok=True)
        with self.session.get(url, stream=True, timeout=600) as r:
            r.raise_for_status()
            with open(dest, "wb") as fh:
                for chunk in r.iter_content(1 << 20):
                    fh.write(chunk)
        return dest

    # -- job wrapper -------------------------------------------------------

    def generate(self, job: str, endpoint: str, payload: dict, media_key: str, suffix: str) -> pathlib.Path | None:
        """Run one generation, download the media, record metadata. Returns local path."""
        prompts_dir = OUT / "prompts"
        prompts_dir.mkdir(parents=True, exist_ok=True)
        (prompts_dir / f"{job}.txt").write_text(payload.get("prompt", ""))

        if self.dry_run:
            print(f"[dry-run] {job} -> {endpoint}  ({len(payload.get('prompt',''))} chars)")
            return None

        print(f"[{job}] submitting to {endpoint}", flush=True)
        started = time.time()
        result = self.run(endpoint, payload, label=job)

        media = result.get(media_key)
        if isinstance(media, list):
            media = media[0]
        if isinstance(media, dict):
            url = media.get("url")
        elif isinstance(media, str):
            url = media
        else:
            url = result.get(f"{media_key}_url")
        if not url:
            raise FalError(f"{job}: no '{media_key}' in response: {json.dumps(result)[:400]}")

        dest = OUT / ("clips" if suffix in (".mp4", ".webm") else "assets") / f"{job}{suffix}"
        self.download(url, dest)

        meta_dir = OUT / "meta"
        meta_dir.mkdir(parents=True, exist_ok=True)
        (meta_dir / f"{job}.json").write_text(
            json.dumps(
                {
                    "job": job,
                    "endpoint": endpoint,
                    "payload": payload,
                    "response": result,
                    "local_path": str(dest.relative_to(ROOT)),
                    "elapsed_s": round(time.time() - started, 1),
                    "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                },
                indent=2,
            )
        )
        print(f"[{job}] -> {dest.relative_to(ROOT)}", flush=True)
        return dest


# -- prompt composition ----------------------------------------------------


def attachment_note(tags: list[str], sheets: dict) -> str:
    """Render the ATTACHMENT NOTE block, one line per referenced asset.

    Each asset gets its role spelled out so the model is never guessing which
    reference is which subject.
    """
    if not tags:
        return ""
    by_tag = {s["tag"]: s for s in sheets["sheets"]}
    lines = ["ATTACHMENT NOTE:"]
    for tag in tags:
        sheet = by_tag.get(tag)
        if not sheet:
            raise KeyError(f"unknown asset tag {tag}")
        lines.append(f"{tag} = {sheet['role']} -> identity reference: {sheet['prompt']}")
        if sheet.get("note"):
            lines.append(f"  {sheet['note']}")
    return "\n".join(lines)


def compose_prompt(shot: dict, blocks: dict, sheets: dict) -> str:
    """Assemble a shot's final prompt from the global blocks plus its own lines.

    Order matters. The global style and behaviour locks come first, then the
    per-segment locks, then identity, then the shot itself. The SFX line is last
    because it is the timing instruction and benefits from recency.
    """
    parts = [blocks["style"], blocks["behaviour"]]
    for lock in shot.get("locks", []):
        parts.append(blocks["locks"][lock])
    note = attachment_note(shot.get("assets", []), sheets)
    if note:
        parts.append(note)
    parts.append(f"ACTION: {shot['action']}")
    parts.append(f"CAMERA: {shot['camera']}")
    parts.append(f"SFX: {shot['sfx']}")
    return "\n\n".join(parts)


def video_payload(shot: dict, blocks: dict, sheets: dict, model: dict, defaults: dict,
                  image_url: str | None = None) -> dict:
    payload = {
        "prompt": compose_prompt(shot, blocks, sheets),
        "aspect_ratio": defaults["aspect_ratio"],
        "duration": str(shot["duration"]),
    }
    if model.get("supports_negative"):
        payload["negative_prompt"] = blocks["negative"]
    if model.get("native_audio"):
        # We discard model audio; see blocks.json audio_policy.
        payload["generate_audio"] = False
    if image_url:
        payload["image_url"] = image_url
    return payload
