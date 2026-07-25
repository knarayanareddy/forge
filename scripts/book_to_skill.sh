#!/usr/bin/env bash
# book_to_skill.sh — offline distillation: source doc tree → progressive-disclosure skill
#
# Copies a validated skill-creator layout (SKILL.md + chapters/ + references/) into skills/.
# Full PDF pipeline deferred; harness fixtures use pre-built trees under fixtures/skills/.

set -euo pipefail

NAME=""
SOURCE=""
OUT="skills"

usage() {
  cat <<'EOF'
book_to_skill.sh — offline book-to-skill distillation

  --name NAME       Skill directory name (e.g. book_skill)
  --source PATH     Source doc tree (e.g. tests/golden_harness/fixtures/skills/book_skill/)
  --out PATH        Output skills root (default: skills/)

Requires SKILL.md, chapters/*.md, and references/source.md in --source.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name) NAME="$2"; shift 2 ;;
    --source) SOURCE="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ -z "$NAME" || -z "$SOURCE" ]]; then
  echo "error: --name and --source required" >&2
  usage
  exit 1
fi

if [[ ! -f "$SOURCE/SKILL.md" ]]; then
  echo "error: missing SKILL.md in $SOURCE" >&2
  exit 1
fi

if [[ ! -d "$SOURCE/chapters" ]]; then
  echo "error: missing chapters/ in $SOURCE" >&2
  exit 1
fi

DEST="$OUT/$NAME"
mkdir -p "$DEST/chapters" "$DEST/references"

cp "$SOURCE/SKILL.md" "$DEST/SKILL.md"
cp "$SOURCE"/chapters/*.md "$DEST/chapters/"
if [[ -d "$SOURCE/references" ]]; then
  cp -R "$SOURCE/references/." "$DEST/references/"
fi

if [[ -f "$SOURCE/skill_eval_rubric.json" ]]; then
  cp "$SOURCE/skill_eval_rubric.json" "$DEST/skill_eval_rubric.json"
fi

echo "book_to_skill: distilled '$SOURCE' -> '$DEST/'"
echo "  chapters: $(find "$DEST/chapters" -name '*.md' | wc -l | tr -d ' ') files"
