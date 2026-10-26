#!/usr/bin/env bash
# Creates the labels the AI-DLC workflows use. Existing labels are left as they are.
# Usage: GH_REPO=owner/repo .github/ai/ensure-labels.sh   (needs `gh` with issues write access)
set -euo pipefail

while IFS='|' read -r name color desc; do
  [ -n "$name" ] || continue
  gh label create "$name" --color "$color" --description "$desc" >/dev/null 2>&1 || true
done <<'LABELS'
ai:triage|ededed|AI task waiting for a maintainer to check the spec
ai:ready|0e8a16|Maintainer approved: the AI worker may implement this issue
ai:in-progress|fbca04|The AI worker is running
ai:needs-info|d876e3|The AI worker needs clearer acceptance criteria
ai:needs-human|b60205|Automation stopped; a human has to take over
ai:pr-open|1d76db|The AI worker opened a draft PR
ai:generated|5319e7|PR written by the AI worker
ai:sensitive|e99695|AI PR touches paths that need a deliberate human review
ai:review-passed|0e8a16|AI review found no blocking issues at the current head
LABELS
