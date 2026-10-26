#!/usr/bin/env python3
"""Deterministic helpers for the AI-DLC workflows (issue -> PR -> review -> fix).

Every step that writes to GitHub runs this script, never model output directly:
the model only produces a patch and a JSON result, and this script checks the
patch against the path policy, applies it, and renders the comments.

Stdlib only. Run with `python3 -I .github/ai/ai_dlc.py <command> ...`.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

# Paths an AI patch may never touch: agent policy, CI, signing, release,
# measured hardware data and accepted decisions. A patch that touches one is
# rejected as a whole. A pattern ending in "/" is a directory prefix; a pattern
# without "/" matches the file name anywhere.
FORBIDDEN = [
    ".github/",
    ".claude/",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
    ".mcp.json",
    ".gitmodules",
    ".gitattributes",
    ".pre-commit-config.yaml",
    "rust-toolchain.toml",
    "deny.toml",
    "LICENSE",
    "signing/",
    "*.entitlements",
    "*.provisionprofile",
    "*.p12",
    "*.pem",
    "*.key",
    "apps/desktop/src-tauri/tauri.release.conf.json",
    "apps/desktop/src-tauri/tauri.appstore.conf.json",
    "tools/release.sh",
    "tools/release-appstore.sh",
    "tools/latest-json.py",
    "fixtures/traces/",
    "docs/spike-report.md",
    "docs/adr/",
    ".cargo/",  # rustc wrappers, registries: code that runs in every later build
    ".npmrc",
]

# Paths an AI patch may touch, but the PR is labelled `ai:sensitive` and lists
# them so a human looks at them on purpose.
SENSITIVE = [
    "apps/desktop/src-tauri/tauri.conf.json",
    "apps/desktop/src-tauri/capabilities/",
    "apps/desktop/src-tauri/Info.plist",
    "apps/desktop/src-tauri/PrivacyInfo.xcprivacy",
    "build.rs",
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "Makefile",
    "tools/",
    "crates/spank-core/src/config.rs",
    "crates/spank-core/src/impact.rs",
    "crates/spank-core/src/calibration.rs",
    "assets/sounds/",
    "website/",
    "PRIVACY.md",
    "docs/privacy.md",
]

DEFAULT_MAX_FILES = 40
DEFAULT_MAX_LINES = 2000

# Issue form headings (.github/ISSUE_TEMPLATE/ai-task.yml) that must be filled in.
REQUIRED_ISSUE_SECTIONS = ["Problem", "Acceptance criteria"]

MARKER_REVIEW = "<!-- ai-dlc:review -->"
MARKER_FIX = "<!-- ai-dlc:fix -->"
MARKER_IMPLEMENT = "<!-- ai-dlc:implement -->"


class PolicyError(Exception):
    pass


# --------------------------------------------------------------------------- text


_CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f​-‏‪-‮⁦-⁩]")


def clean(value: object, limit: int = 4000) -> str:
    """Make model-written text safe to embed in a GitHub comment.

    Drops control and bidi characters, defuses @mentions and HTML comments
    (so the text can't ping people or fake one of our markers), and truncates.
    """
    text = "" if value is None else str(value)
    text = _CONTROL.sub("", text)
    text = text.replace("<!--", "&lt;!--").replace("-->", "--&gt;")
    text = re.sub(r"@(?=[A-Za-z0-9_-])", "@⁠", text)
    if len(text) > limit:
        text = text[: limit - 1].rstrip() + "…"
    return text.strip()


def cell(value: object, limit: int = 300) -> str:
    """Single-line text for a Markdown table cell."""
    return clean(value, limit).replace("\r", " ").replace("\n", " ").replace("|", "\\|")


def one_line(value: object, limit: int = 72) -> str:
    text = cell(value, 1000).replace("`", "'")
    text = re.sub(r"\s+", " ", text).strip()
    if len(text) > limit:
        text = text[: limit - 1].rstrip() + "…"
    return text


def load_json(path: str | None) -> dict:
    if not path:
        return {}
    p = Path(path)
    if not p.exists() or not p.read_text(encoding="utf-8").strip():
        return {}
    data = json.loads(p.read_text(encoding="utf-8"))
    return data if isinstance(data, dict) else {}


# -------------------------------------------------------------------------- issue


def parse_issue_sections(body: str) -> dict[str, str]:
    """Split an issue-form body into {heading: text} using its `### ` headings."""
    sections: dict[str, str] = {}
    current: str | None = None
    lines: list[str] = []
    for line in (body or "").splitlines():
        m = re.match(r"^###\s+(.+?)\s*$", line)
        if m:
            if current is not None:
                sections[current] = "\n".join(lines).strip()
            current, lines = m.group(1), []
        elif current is not None:
            lines.append(line)
    if current is not None:
        sections[current] = "\n".join(lines).strip()
    return sections


def check_issue(body: str) -> list[str]:
    """Return the required sections that are missing or empty."""
    sections = parse_issue_sections(body)
    missing = []
    for name in REQUIRED_ISSUE_SECTIONS:
        text = sections.get(name, "").strip()
        if not text or text == "_No response_":
            missing.append(name)
    return missing


def issue_risk(body: str) -> str:
    """'low', 'medium', 'high' from the form's Risk dropdown; 'unknown' otherwise."""
    text = parse_issue_sections(body).get("Risk", "").strip().lower()
    for level in ("low", "medium", "high"):
        if text.startswith(level):
            return level
    return "unknown"


# -------------------------------------------------------------------------- patch


def matches(path: str, pattern: str) -> bool:
    # Case-insensitive: on macOS `claude.md` and `.GitHub/` are the same files as the real ones.
    path, pattern = path.lower(), pattern.lower()
    if pattern.endswith("/"):
        return path.startswith(pattern)
    if "/" not in pattern:
        return fnmatch.fnmatchcase(path.rsplit("/", 1)[-1], pattern)
    return fnmatch.fnmatchcase(path, pattern)


def classify(path: str) -> str:
    """'forbidden', 'sensitive' or 'ok'."""
    if any(matches(path, p) for p in FORBIDDEN):
        return "forbidden"
    if any(matches(path, p) for p in SENSITIVE):
        return "sensitive"
    return "ok"


@dataclass
class FileChange:
    path: str
    added: int | None  # None for binary
    deleted: int | None
    old_path: str | None = None


@dataclass
class PatchReport:
    files: list[FileChange] = field(default_factory=list)
    sensitive: list[str] = field(default_factory=list)
    mode_changes: list[str] = field(default_factory=list)

    @property
    def lines(self) -> int:
        return sum((f.added or 0) + (f.deleted or 0) for f in self.files)

    def to_dict(self) -> dict:
        return {
            "files": [
                {"path": f.path, "old_path": f.old_path, "added": f.added, "deleted": f.deleted}
                for f in self.files
            ],
            "file_count": len(self.files),
            "lines": self.lines,
            "sensitive": self.sensitive,
            "mode_changes": self.mode_changes,
        }


def parse_numstat_z(raw: str) -> list[FileChange]:
    """Parse `git apply --numstat -z`. Renames carry an empty path then src, dst."""
    tokens = raw.split("\0")
    out: list[FileChange] = []
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        if not tok:
            i += 1
            continue
        parts = tok.split("\t")
        if len(parts) != 3:
            raise PolicyError(f"unexpected numstat entry: {tok!r}")
        a, d, path = parts
        added = None if a == "-" else int(a)
        deleted = None if d == "-" else int(d)
        if path:
            out.append(FileChange(path, added, deleted))
            i += 1
        else:
            if i + 2 >= len(tokens):
                raise PolicyError("truncated rename entry in numstat")
            out.append(FileChange(tokens[i + 2], added, deleted, old_path=tokens[i + 1]))
            i += 3
    return out


def check_path_shape(path: str) -> None:
    if not path or path.startswith("/") or "\\" in path:
        raise PolicyError(f"bad path: {path!r}")
    if any(part in ("", ".", "..", ".git") for part in path.split("/")):
        raise PolicyError(f"bad path: {path!r}")


def evaluate_patch(numstat_z: str, summary: str, max_files: int, max_lines: int) -> PatchReport:
    """Apply the path policy to a patch described by numstat and summary output."""
    files = parse_numstat_z(numstat_z)
    if not files:
        raise PolicyError("patch is empty")
    report = PatchReport(files=files)
    errors: list[str] = []

    for f in files:
        for p in filter(None, (f.path, f.old_path)):
            try:
                check_path_shape(p)
            except PolicyError as e:
                errors.append(str(e))
                continue
            kind = classify(p)
            if kind == "forbidden":
                errors.append(f"forbidden path: {p}")
            elif kind == "sensitive" and p not in report.sensitive:
                report.sensitive.append(p)
        if f.added is None:
            errors.append(f"binary change not allowed: {f.path}")

    for line in summary.splitlines():
        line = line.strip()
        if re.search(r"\bmode (120000|160000)\b", line) or "=> 120000" in line or "=> 160000" in line:
            errors.append(f"symlink or submodule not allowed: {line}")
        elif line.startswith("mode change"):
            report.mode_changes.append(line)
        elif re.match(r"create mode 100755 ", line):
            report.mode_changes.append(line)

    if len(files) > max_files:
        errors.append(f"too many files: {len(files)} > {max_files}")
    if report.lines > max_lines:
        errors.append(f"too many changed lines: {report.lines} > {max_lines}")
    if errors:
        raise PolicyError("; ".join(errors))
    return report


def git(*args: str, cwd: str = ".") -> str:
    result = subprocess.run(
        ["git", "-c", "core.hooksPath=/dev/null", *args],
        cwd=cwd,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise PolicyError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout


def apply_patch(patch: str, cwd: str, max_files: int, max_lines: int) -> PatchReport:
    """Validate the patch, then stage it with `git apply --index` (runs no repo code)."""
    if not Path(patch).exists() or Path(patch).stat().st_size == 0:
        raise PolicyError("patch is empty")
    numstat = git("apply", "--numstat", "-z", patch, cwd=cwd)
    summary = git("apply", "--summary", patch, cwd=cwd)
    report = evaluate_patch(numstat, summary, max_files, max_lines)
    git("apply", "--check", "--index", "--whitespace=nowarn", patch, cwd=cwd)
    git("apply", "--index", "--whitespace=nowarn", patch, cwd=cwd)
    return report


# ------------------------------------------------------------------------ render


def run_footer(run_url: str, extra: str = "") -> str:
    tail = f" · {extra}" if extra else ""
    return f"<sub>AI-DLC · [workflow run]({run_url}){tail}</sub>"


def render_validation(rows: list) -> str:
    if not rows:
        return "_The worker reported no validation commands._"
    out = ["| Command | Result | Note |", "|---|---|---|"]
    for r in rows:
        if not isinstance(r, dict):
            continue
        out.append(f"| `{cell(r.get('command'), 160).replace('`', '')}` | {cell(r.get('result'), 20)} | {cell(r.get('note'))} |")
    return "\n".join(out)


def render_bullets(items: list, empty: str) -> str:
    items = [clean(i, 600) for i in (items or []) if str(i).strip()]
    return "\n".join(f"- {i}" for i in items) if items else empty


def commit_message(result: dict, fallback: str, trailers: list[str]) -> str:
    title = one_line(result.get("title") or fallback) or fallback
    body = clean(result.get("summary"), 2000)
    msg = title + "\n"
    if body:
        msg += "\n" + body + "\n"
    if trailers:
        msg += "\n" + "\n".join(trailers) + "\n"
    return msg


def pr_body(result: dict, report: dict, issue: int, run_url: str, base_sha: str, model: str) -> str:
    sensitive = report.get("sensitive") or []
    mode_changes = report.get("mode_changes") or []
    parts = [
        MARKER_IMPLEMENT,
        f"Closes #{issue}",
        "",
        "## Summary",
        clean(result.get("summary")) or "_No summary._",
        "",
        "## Acceptance criteria",
        render_acceptance(result.get("acceptance") or []),
        "",
        "## Validation (reported by the worker)",
        render_validation(result.get("validation") or []),
        "",
        "CI on this PR is the independent check; the table above is the worker's own report.",
        "",
        "## Risks and rollback",
        render_bullets(result.get("risks") or [], "_None reported._"),
        "",
        "## Not done / follow-ups",
        render_bullets(result.get("not_done") or [], "_None reported._"),
        "",
        "## Patch",
        f"{report.get('file_count', 0)} files, {report.get('lines', 0)} changed lines.",
    ]
    if sensitive:
        parts += ["", "**Sensitive paths (review these on purpose):**", *[f"- `{p}`" for p in sensitive]]
    if mode_changes:
        parts += ["", "**Mode changes:**", *[f"- `{cell(m)}`" for m in mode_changes]]
    parts += [
        "",
        "---",
        "Generated by the AI-DLC worker. It does not merge: CI, the AI review and a human approval decide.",
        run_footer(run_url, f"base `{base_sha[:12]}`" + (f" · model `{cell(model, 60)}`" if model else "")),
    ]
    return "\n".join(parts) + "\n"


def render_acceptance(rows: list) -> str:
    if not rows:
        return "_Not reported._"
    out = ["| Criterion | Status | Evidence |", "|---|---|---|"]
    for r in rows:
        if isinstance(r, dict):
            out.append(f"| {cell(r.get('criterion'))} | {cell(r.get('status'), 20)} | {cell(r.get('evidence'))} |")
    return "\n".join(out)


SEVERITY_ORDER = {"P0": 0, "P1": 1, "P2": 2}


def blocking_findings(review: dict) -> list[dict]:
    findings = [f for f in review.get("findings") or [] if isinstance(f, dict)]
    return [f for f in findings if f.get("severity") in ("P0", "P1")]


def review_is_blocking(review: dict) -> bool:
    if review.get("verdict") != "pass":
        return True
    if blocking_findings(review):
        return True
    return any(
        isinstance(a, dict) and a.get("status") == "not_met" for a in review.get("acceptance") or []
    )


def review_comment(review: dict, sha: str, run_url: str, round_label: str = "") -> str:
    blocking = review_is_blocking(review)
    head = "Changes requested" if blocking else "No blocking findings"
    findings = sorted(
        (f for f in review.get("findings") or [] if isinstance(f, dict)),
        key=lambda f: SEVERITY_ORDER.get(f.get("severity"), 9),
    )
    parts = [
        MARKER_REVIEW,
        f"<!-- ai-dlc:sha={sha} -->",
        f"## AI review: {head}",
        f"Reviewed head `{sha[:12]}`{(' · ' + round_label) if round_label else ''}.",
        "",
        clean(review.get("summary"), 3000) or "_No summary._",
    ]
    if review.get("acceptance"):
        parts += ["", "### Acceptance criteria", render_acceptance(review["acceptance"])]
    parts += ["", "### Findings"]
    if not findings:
        parts.append("_None._ This is not proof that the change is correct.")
    for f in findings:
        loc = cell(f.get("path"), 200)
        if f.get("line"):
            loc += f":{cell(f.get('line'), 10)}"
        parts += [
            "",
            f"**{cell(f.get('severity'), 4)} `{cell(f.get('id'), 40)}`: {cell(f.get('title'), 200)}**  ",
            f"`{loc}`",
            "",
            clean(f.get("detail"), 1500),
        ]
        if f.get("suggestion"):
            parts += ["", f"Suggested fix: {clean(f.get('suggestion'), 800)}"]
    parts += [
        "",
        "---",
        "P0/P1 findings and unmet acceptance criteria block; P2 is advisory. "
        "The AI review never approves a PR: a human approval is still required.",
        run_footer(run_url),
    ]
    return "\n".join(parts) + "\n"


def fix_comment(result: dict, report: dict, attempt: int, max_attempts: int, source: str, run_url: str, pushed: bool) -> str:
    status = cell(result.get("status") or "unknown", 30)
    parts = [
        MARKER_FIX,
        f"## AI fix attempt {attempt}/{max_attempts} ({cell(source, 20)}): {status}",
        "",
        clean(result.get("summary"), 3000) or "_No summary._",
    ]
    addressed = [cell(a, 60) for a in result.get("addressed") or []]
    if addressed:
        parts += ["", "**Addressed:** " + ", ".join(f"`{a}`" for a in addressed)]
    declined = [d for d in result.get("declined") or [] if isinstance(d, dict)]
    if declined:
        parts += ["", "**Declined (the reviewer should not repeat these without new evidence):**"]
        parts += [f"- `{cell(d.get('id'), 60)}`: {cell(d.get('reason'), 600)}" for d in declined]
    parts += ["", "### Validation (reported by the worker)", render_validation(result.get("validation") or [])]
    if pushed:
        parts += ["", f"Pushed {report.get('file_count', 0)} files, {report.get('lines', 0)} changed lines."]
        if report.get("sensitive"):
            parts.append("Sensitive paths: " + ", ".join(f"`{p}`" for p in report["sensitive"]))
    else:
        parts += ["", "No commit was pushed."]
    parts += ["", run_footer(run_url)]
    return "\n".join(parts) + "\n"


def worker_comment(result: dict, run_url: str) -> str:
    status = result.get("status") or "failed"
    if status == "needs_info":
        head = "needs more information before it can be implemented"
    elif status == "out_of_scope":
        head = "is out of scope for the AI worker and needs a human"
    else:
        head = f"did not produce a pull request ({cell(status, 30)})"
    parts = [
        MARKER_IMPLEMENT,
        f"## AI worker: this issue {head}",
        "",
        clean(result.get("summary"), 3000) or "_No summary._",
    ]
    questions = result.get("questions") or []
    if questions:
        parts += ["", "### Questions", render_bullets(questions, "")]
    parts += [
        "",
        "Edit the issue to answer, then add the `ai:ready` label again to retry.",
        "",
        run_footer(run_url),
    ]
    return "\n".join(parts) + "\n"


# --------------------------------------------------------------------------- CLI


def write_output(name: str, value: str, path: str | None) -> None:
    if not path:
        return
    with open(path, "a", encoding="utf-8") as fh:
        if "\n" in value:
            fh.write(f"{name}<<__AI_DLC_EOF__\n{value}\n__AI_DLC_EOF__\n")
        else:
            fh.write(f"{name}={value}\n")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="ai_dlc")
    ap.add_argument("--github-output", default=None, help="append step outputs here (pass $GITHUB_OUTPUT)")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("check-issue", help="check that the issue form's required sections are filled")
    p.add_argument("--body-file", required=True)

    p = sub.add_parser("apply", help="validate a patch against the path policy and stage it")
    p.add_argument("--patch", required=True)
    p.add_argument("--cwd", default=".")
    p.add_argument("--max-files", type=int, default=DEFAULT_MAX_FILES)
    p.add_argument("--max-lines", type=int, default=DEFAULT_MAX_LINES)
    p.add_argument("--report", required=True, help="write the JSON report here")

    p = sub.add_parser("commit-msg")
    p.add_argument("--result", required=True)
    p.add_argument("--fallback", required=True)
    p.add_argument("--trailer", action="append", default=[])

    p = sub.add_parser("pr-body")
    p.add_argument("--result", required=True)
    p.add_argument("--report", required=True)
    p.add_argument("--issue", type=int, required=True)
    p.add_argument("--run-url", required=True)
    p.add_argument("--base-sha", required=True)
    p.add_argument("--model", default="")

    p = sub.add_parser("review-comment")
    p.add_argument("--review", required=True)
    p.add_argument("--sha", required=True)
    p.add_argument("--run-url", required=True)
    p.add_argument("--round", default="")

    p = sub.add_parser("review-verdict", help="print 'blocking' or 'pass'")
    p.add_argument("--review", required=True)

    p = sub.add_parser("fix-comment")
    p.add_argument("--result", required=True)
    p.add_argument("--report", default=None)
    p.add_argument("--attempt", type=int, required=True)
    p.add_argument("--max-attempts", type=int, required=True)
    p.add_argument("--source", required=True)
    p.add_argument("--run-url", required=True)
    p.add_argument("--pushed", action="store_true")

    p = sub.add_parser("worker-comment")
    p.add_argument("--result", required=True)
    p.add_argument("--run-url", required=True)

    args = ap.parse_args(argv)
    out = args.github_output

    if args.cmd == "check-issue":
        body = Path(args.body_file).read_text(encoding="utf-8")
        missing = check_issue(body)
        risk = issue_risk(body)
        write_output("ok", "true" if not missing else "false", out)
        write_output("missing", ", ".join(missing), out)
        write_output("risk", risk, out)
        print(json.dumps({"ok": not missing, "missing": missing, "risk": risk}))
        return 0

    if args.cmd == "apply":
        try:
            report = apply_patch(args.patch, args.cwd, args.max_files, args.max_lines)
        except PolicyError as e:
            Path(args.report).write_text(json.dumps({"error": str(e)}), encoding="utf-8")
            write_output("error", str(e), out)
            print(f"::error::patch rejected: {e}", file=sys.stderr)
            return 2
        data = report.to_dict()
        Path(args.report).write_text(json.dumps(data, indent=2), encoding="utf-8")
        write_output("sensitive", "true" if report.sensitive else "false", out)
        print(json.dumps(data, indent=2))
        return 0

    if args.cmd == "commit-msg":
        sys.stdout.write(commit_message(load_json(args.result), args.fallback, args.trailer))
        return 0

    if args.cmd == "pr-body":
        sys.stdout.write(
            pr_body(load_json(args.result), load_json(args.report), args.issue, args.run_url, args.base_sha, args.model)
        )
        return 0

    if args.cmd == "review-comment":
        sys.stdout.write(review_comment(load_json(args.review), args.sha, args.run_url, args.round))
        return 0

    if args.cmd == "review-verdict":
        review = load_json(args.review)
        verdict = "pass" if review and not review_is_blocking(review) else "blocking"
        write_output("verdict", verdict, out)
        print(verdict)
        return 0

    if args.cmd == "fix-comment":
        sys.stdout.write(
            fix_comment(
                load_json(args.result),
                load_json(args.report),
                args.attempt,
                args.max_attempts,
                args.source,
                args.run_url,
                args.pushed,
            )
        )
        return 0

    if args.cmd == "worker-comment":
        sys.stdout.write(worker_comment(load_json(args.result), args.run_url))
        return 0

    return 1


if __name__ == "__main__":
    sys.exit(main())
