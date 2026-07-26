#!/usr/bin/env bash
# Full build. Stops at the first failure so a bad shot never reaches the cut.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${FAL_KEY:-}" ]]; then
  echo "FAL_KEY is not set. For a no-key dry run:" >&2
  echo "  python3 scripts/animatic.py && python3 scripts/build_audio.py --silent && python3 scripts/assemble.py --animatic" >&2
  exit 1
fi

echo "== pacing check =="
python3 scripts/animatic.py --check-only

echo "== reference sheets =="
python3 scripts/make_sheets.py
cat <<'EOF'

Pick one frame per sheet and save it in fal Assets under its tag
(@hedgerow @moth @whale @n_atlantic @s_pacific @thrush @branch), then re-run
this script with SKIP_SHEETS=1 to continue.
EOF
[[ "${SKIP_SHEETS:-0}" == "1" ]] || exit 0

echo "== shots =="
python3 scripts/make_shots.py

echo "== audio =="
python3 scripts/build_audio.py

echo "== assemble =="
python3 scripts/assemble.py

echo
echo "done -> out/blindspot.mp4"
