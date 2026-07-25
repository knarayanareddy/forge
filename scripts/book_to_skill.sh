#!/usr/bin/env bash
# book_to_skill.sh — offline distillation stub (Slice 6.8 / SKILL-02 prep)
#
# Expected flow (not implemented — documents contract only):
#   1. Input: directory or PDF export tree under fixtures/book_skill/
#   2. Split into skills/<name>/chapters/*.md with progressive-disclosure frontmatter
#   3. Emit routing_keywords + chapters_dir in SKILL.md (see fixtures/skills/rust-cookbook/)
#   4. Validate chapter files exist before registering in procedural_skills
#
# Usage (stub):
#   ./scripts/book_to_skill.sh --name rust-cookbook --source fixtures/book_skill/ --out skills/

set -euo pipefail

NAME=""
SOURCE=""
OUT="skills"

usage() {
  cat <<'EOF'
book_to_skill.sh — offline book-to-skill distillation (stub)

  --name NAME       Skill directory name (e.g. rust-cookbook)
  --source PATH     Source doc tree (fixtures/book_skill/ in harness)
  --out PATH        Output skills root (default: skills/)

Full PDF pipeline deferred to Slice 6.8. See tests/golden_harness/fixtures/skills/
for the expected chapter layout borrowed from awesome-claude-skills patterns.
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

echo "book_to_skill stub: would distill '$SOURCE' -> '$OUT/$NAME/'"
echo "Reference fixture: tests/golden_harness/fixtures/skills/rust-cookbook/"
exit 0
