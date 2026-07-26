"""Shared timeline arithmetic.

Entry start times are derived rather than stored so the edit can be reordered or
retimed in timeline.json without hand-recomputing narration placement. Crossfades
overlap, so an entry with a transition starts before the previous one ends.
"""

from __future__ import annotations

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent


def load_timeline() -> dict:
    with open(ROOT / "manifests" / "timeline.json") as fh:
        return json.load(fh)


def entry_starts(timeline: dict) -> tuple[dict[str, float], float]:
    """Return {entry_id: absolute start seconds} and the total runtime."""
    starts: dict[str, float] = {}
    t = 0.0
    prev_id = None
    prev_dur = 0.0
    for entry in timeline["entries"]:
        transition = entry.get("transition")
        if transition and transition.get("type") == "xfade":
            if transition.get("from") not in (None, prev_id):
                raise ValueError(
                    f"{entry['id']}: xfade declares from={transition['from']} but follows {prev_id}"
                )
            t -= float(transition["duration"])
        starts[entry["id"]] = round(t, 3)
        t += float(entry["duration"])
        prev_id, prev_dur = entry["id"], float(entry["duration"])
    return starts, round(t, 3)


def resolve(at: dict, starts: dict[str, float]) -> float:
    """Absolute time for a narration line's {entry, offset} anchor."""
    if at["entry"] not in starts:
        raise KeyError(f"narration anchored to unknown entry {at['entry']}")
    return round(starts[at["entry"]] + float(at.get("offset", 0.0)), 3)


def tc(seconds: float) -> str:
    m, s = divmod(seconds, 60)
    return f"{int(m):01d}:{s:05.2f}"


if __name__ == "__main__":
    tl = load_timeline()
    starts, total = entry_starts(tl)
    print(f"{'entry':16s} {'start':>8s} {'dur':>6s} {'end':>8s}")
    for entry in tl["entries"]:
        s = starts[entry["id"]]
        d = float(entry["duration"])
        print(f"{entry['id']:16s} {tc(s):>8s} {d:6.1f} {tc(s + d):>8s}")
    print(f"\ntotal runtime {tc(total)} ({total}s)")
