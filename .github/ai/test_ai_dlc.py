"""Tests for ai_dlc.py. Run: python3 -I -m unittest discover -s .github/ai -v"""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import ai_dlc  # noqa: E402

FORM_BODY = """### Problem

Lid-open sound plays twice after wake.

### Acceptance criteria

- One LidOpened per wake
- Regression test in lid.rs

### Scope / non-goals

_No response_
"""


class IssueTests(unittest.TestCase):
    def test_complete_form_passes(self):
        self.assertEqual(ai_dlc.check_issue(FORM_BODY), [])

    def test_missing_and_empty_sections(self):
        body = "### Problem\n\nsomething\n\n### Acceptance criteria\n\n_No response_\n"
        self.assertEqual(ai_dlc.check_issue(body), ["Acceptance criteria"])
        self.assertEqual(ai_dlc.check_issue("free text issue"), ["Problem", "Acceptance criteria"])

    def test_risk(self):
        self.assertEqual(ai_dlc.issue_risk(FORM_BODY + "\n### Risk\n\nHigh (needs a human)\n"), "high")
        self.assertEqual(ai_dlc.issue_risk(FORM_BODY), "unknown")


class ClassifyTests(unittest.TestCase):
    def test_forbidden(self):
        for p in [
            ".github/workflows/ci.yml",
            "CLAUDE.md",
            "crates/x/CLAUDE.md",
            "apps/desktop/src-tauri/Spank.entitlements",
            "signing/a.provisionprofile",
            "fixtures/traces/m4air-v1/expected.json",
            "docs/adr/0001-x.md",
            "claude.md",
            ".GitHub/workflows/x.yml",
            ".cargo/config.toml",
            "apps/desktop/.npmrc",
        ]:
            self.assertEqual(ai_dlc.classify(p), "forbidden", p)

    def test_sensitive_and_ok(self):
        self.assertEqual(ai_dlc.classify("crates/spank-core/Cargo.toml"), "sensitive")
        self.assertEqual(ai_dlc.classify("apps/desktop/src-tauri/tauri.conf.json"), "sensitive")
        self.assertEqual(ai_dlc.classify("crates/spank-sensors/build.rs"), "sensitive")
        self.assertEqual(ai_dlc.classify("crates/spank-core/src/lid.rs"), "ok")
        self.assertEqual(ai_dlc.classify("docs/design.md"), "ok")


class NumstatTests(unittest.TestCase):
    def test_plain_and_rename(self):
        raw = "3\t1\ta.rs\0" + "0\t0\t\0old.rs\0new.rs\0" + "-\t-\timg.png\0"
        files = ai_dlc.parse_numstat_z(raw)
        self.assertEqual([f.path for f in files], ["a.rs", "new.rs", "img.png"])
        self.assertEqual(files[1].old_path, "old.rs")
        self.assertIsNone(files[2].added)

    def test_policy_errors(self):
        cases = {
            "1\t0\t.github/workflows/x.yml\0": "forbidden path",
            "-\t-\ta.png\0": "binary",
            "1\t0\t../x\0": "bad path",
            "0\t0\t\0CLAUDE.md\0notes.md\0": "forbidden path: CLAUDE.md",
        }
        for raw, msg in cases.items():
            with self.assertRaises(ai_dlc.PolicyError) as ctx:
                ai_dlc.evaluate_patch(raw, "", 40, 2000)
            self.assertIn(msg, str(ctx.exception))

    def test_limits_and_symlinks(self):
        with self.assertRaises(ai_dlc.PolicyError):
            ai_dlc.evaluate_patch("1500\t600\ta.rs\0", "", 40, 2000)
        with self.assertRaises(ai_dlc.PolicyError):
            ai_dlc.evaluate_patch("".join(f"1\t0\tf{i}.rs\0" for i in range(3)), "", 2, 2000)
        with self.assertRaises(ai_dlc.PolicyError):
            ai_dlc.evaluate_patch("1\t0\tlink\0", " create mode 120000 link", 40, 2000)
        with self.assertRaises(ai_dlc.PolicyError):
            ai_dlc.evaluate_patch("", "", 40, 2000)

    def test_sensitive_and_mode_changes_are_reported(self):
        report = ai_dlc.evaluate_patch(
            "1\t0\tCargo.toml\0" "2\t0\ttools/x.sh\0",
            " create mode 100755 tools/x.sh",
            40,
            2000,
        )
        self.assertEqual(report.sensitive, ["Cargo.toml", "tools/x.sh"])
        self.assertEqual(len(report.mode_changes), 1)
        self.assertEqual(report.lines, 3)


class ApplyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = self.tmp.name
        env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull}
        self.env = env

        def run(*args):
            subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True, env=env)

        self.run_git = run
        run("init", "-q")
        run("config", "user.email", "t@example.com")
        run("config", "user.name", "t")
        Path(self.repo, "a.rs").write_text("fn a() {}\n")
        run("add", "-A")
        run("commit", "-qm", "init")

    def tearDown(self):
        self.tmp.cleanup()

    def make_patch(self, edit):
        edit()
        self.run_git("add", "-A")
        diff = subprocess.run(
            ["git", "diff", "--cached", "--binary"], cwd=self.repo, capture_output=True, text=True, check=True, env=self.env
        ).stdout
        self.run_git("reset", "-q", "--hard")
        fd, name = tempfile.mkstemp(suffix=".diff")
        os.close(fd)
        patch = Path(name)
        patch.write_text(diff)
        self.addCleanup(patch.unlink)
        return str(patch)

    def test_apply_good_patch(self):
        patch = self.make_patch(lambda: Path(self.repo, "a.rs").write_text("fn a() { b() }\n"))
        report = ai_dlc.apply_patch(patch, self.repo, 40, 2000)
        self.assertEqual(report.to_dict()["file_count"], 1)
        staged = subprocess.run(
            ["git", "diff", "--cached", "--name-only"], cwd=self.repo, capture_output=True, text=True, env=self.env
        ).stdout.split()
        self.assertEqual(staged, ["a.rs"])

    def test_reject_forbidden_patch_without_applying(self):
        def edit():
            Path(self.repo, ".github").mkdir()
            Path(self.repo, ".github/x.yml").write_text("on: push\n")

        patch = self.make_patch(edit)
        with self.assertRaises(ai_dlc.PolicyError):
            ai_dlc.apply_patch(patch, self.repo, 40, 2000)
        self.assertFalse(Path(self.repo, ".github/x.yml").exists())


class SchemaTests(unittest.TestCase):
    def test_schemas_are_inline_json_without_single_quotes(self):
        # Workflows pass them as --json-schema '<compact json>'.
        for name in ("implement", "review", "fix"):
            text = Path(__file__).with_name("schemas").joinpath(f"{name}.json").read_text()
            schema = json.loads(text)
            self.assertEqual(schema["type"], "object")
            self.assertNotIn("'", json.dumps(schema))
            self.assertNotIn("$ref", text)


class RenderTests(unittest.TestCase):
    def test_clean_defuses_mentions_markers_and_bidi(self):
        text = ai_dlc.clean("hi @owner <!-- ai-dlc:review --> ‮evil")
        self.assertNotIn("@owner", text)
        self.assertNotIn("<!--", text)
        self.assertNotIn("‮", text)

    def test_review_verdict(self):
        self.assertFalse(ai_dlc.review_is_blocking({"verdict": "pass", "findings": [{"severity": "P2"}]}))
        self.assertTrue(ai_dlc.review_is_blocking({"verdict": "pass", "findings": [{"severity": "P1"}]}))
        self.assertTrue(
            ai_dlc.review_is_blocking({"verdict": "pass", "acceptance": [{"status": "not_met"}]})
        )
        self.assertTrue(ai_dlc.review_is_blocking({"verdict": "changes_requested"}))

    def test_review_comment_contains_marker_and_sha(self):
        body = ai_dlc.review_comment(
            {"verdict": "changes_requested", "summary": "s", "findings": [
                {"id": "lid-1", "severity": "P1", "path": "a.rs", "line": 3, "title": "t|x", "detail": "d"}
            ]},
            "0123456789abcdef",
            "https://example.com/run",
        )
        self.assertTrue(body.startswith(ai_dlc.MARKER_REVIEW))
        self.assertIn("ai-dlc:sha=0123456789abcdef", body)
        self.assertIn("Changes requested", body)
        self.assertIn("`a.rs:3`", body)

    def test_pr_body_and_commit_message(self):
        result = {
            "title": "fix(lid): play LidOpened once per wake\nsecond line",
            "summary": "Did it.",
            "validation": [{"command": "cargo test -p spank-core", "result": "pass"}],
        }
        body = ai_dlc.pr_body(result, {"file_count": 2, "lines": 10, "sensitive": ["Cargo.toml"]}, 7, "u", "abc", "m")
        self.assertIn("Closes #7", body)
        self.assertIn("`Cargo.toml`", body)
        msg = ai_dlc.commit_message(result, "fallback", ["AI-Issue: #7"])
        self.assertEqual(msg.splitlines()[0], "fix(lid): play LidOpened once per wake second line")
        self.assertTrue(msg.rstrip().endswith("AI-Issue: #7"))

    def test_cli_review_verdict_writes_output(self):
        with tempfile.TemporaryDirectory() as d:
            review = Path(d, "r.json")
            review.write_text(json.dumps({"verdict": "pass", "findings": []}))
            out = Path(d, "out")
            ai_dlc.main(["--github-output", str(out), "review-verdict", "--review", str(review)])
            self.assertEqual(out.read_text(), "verdict=pass\n")
            empty = Path(d, "empty.json")
            empty.write_text("")
            ai_dlc.main(["--github-output", str(out), "review-verdict", "--review", str(empty)])
            self.assertIn("verdict=blocking", out.read_text())


if __name__ == "__main__":
    unittest.main()
