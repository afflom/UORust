"""Negative controls for the development-only PR classifier."""
import importlib.util
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SOURCE = Path(__file__).with_name("pr_stage.py")
spec = importlib.util.spec_from_file_location("pr_stage", SOURCE)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
REPO = "UOR-Foundation/UORust"
HEAD = "a" * 40


def event(draft=True, action="synchronize"):
    return {
        "action": action,
        "repository": {"full_name": REPO},
        "pull_request": {
            "state": "open", "draft": draft,
            "base": {"ref": "main", "sha": "b" * 40},
            "head": {"sha": HEAD, "repo": {"full_name": "contributor/UORust"}},
            "body": "- [x] I am ready",
            "labels": [{"name": "merge-approved"}],
        },
    }


class StageTest(unittest.TestCase):
    def test_draft_with_forged_body_or_labels_is_development_only(self):
        result = mod.classify(event(), REPO)
        self.assertEqual(result["stage"], "development")
        self.assertFalse(result["merge_qualified"])

    def test_ready_pr_is_candidate_not_native_acceptance(self):
        result = mod.classify(event(False, "ready_for_review"), REPO)
        self.assertEqual(result["stage"], "candidate")
        self.assertEqual(result["head_sha"], HEAD)
        self.assertFalse(result["merge_qualified"])

    def test_missing_or_wrong_inputs_fail_closed(self):
        changes = (
            lambda e: e["pull_request"].pop("draft"),
            lambda e: e["pull_request"].__setitem__("draft", "false"),
            lambda e: e["pull_request"]["base"].__setitem__("ref", "release"),
            lambda e: e["pull_request"]["base"].__setitem__("sha", "short"),
            lambda e: e["pull_request"]["head"].__setitem__("sha", "b" * 12),
            lambda e: e["repository"].__setitem__("full_name", "other/repo"),
            lambda e: e.__setitem__("action", "closed"),
            lambda e: e["pull_request"].__setitem__("state", "closed"),
            lambda e: e.__setitem__("action", "ready_for_review"),
        )
        for change in changes:
            obj = event()
            change(obj)
            with self.subTest(change=change), self.assertRaises(mod.AdmissionError):
                mod.classify(obj, REPO)

    def test_draft_transition_must_match_event(self):
        self.assertEqual(mod.classify(event(True, "converted_to_draft"), REPO)["stage"], "development")
        with self.assertRaises(mod.AdmissionError):
            mod.classify(event(False, "converted_to_draft"), REPO)

    def test_impact_classifier_adversarial_suite(self):
        proc = subprocess.run(
            [sys.executable, "-I", "-m", "unittest", "discover",
             "-s", str(SOURCE.parent), "-p", "test_change_impact.py", "-v"],
            capture_output=True, text=True, timeout=35)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        # unittest exits 0 for a run with no test or with skips, so the count
        # and the summary line decide, as `verdict` in ci.rs does.
        ran = re.search(r"^Ran (\d+) tests? in ", proc.stderr, re.M)
        self.assertTrue(ran and int(ran.group(1)) > 0, proc.stderr)
        last = [line for line in proc.stderr.splitlines() if line.strip()][-1]
        self.assertEqual(last.strip(), "OK", proc.stderr)

    def test_obligation_plan_adversarial_suite(self):
        proc = subprocess.run(
            [sys.executable, "-I", str(SOURCE.parent / "test_obligation_plan.py")],
            capture_output=True, text=True, timeout=35)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_impact_policy_is_loaded_from_base_not_pr_head(self):
        def git(repo, *args):
            return subprocess.check_output(["git", "-C", str(repo), *args],
                                           text=True).strip()
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            git(repo, "init", "-q")
            git(repo, "config", "user.name", "CI Acceptance")
            git(repo, "config", "user.email", "ci@example.invalid")
            policy = repo / "scripts/ci/change_impact.py"
            policy.parent.mkdir(parents=True)
            policy.write_text(
                "import json,sys\n"
                "a=sys.argv\n"
                "base=a[a.index('--base')+1];head=a[a.index('--head')+1]\n"
                "print(json.dumps(dict(route='docs',base_sha=base,head_sha=head,"
                "native_acceptance_required=True,merge_qualified=False,"
                "advisory_only=True,selected_development_suites=['docs'])))\n"
            )
            git(repo, "add", ".")
            git(repo, "commit", "-qm", "trusted base")
            base = git(repo, "rev-parse", "HEAD")
            policy.write_text("raise SystemExit('PR head must not run')\n")
            git(repo, "commit", "-qam", "untrusted head")
            head = git(repo, "rev-parse", "HEAD")
            self.assertEqual(mod.advisory_impact_from_trusted_base(repo, base, head)["route"], "docs")
            self.assertEqual(mod.advisory_impact_from_trusted_base(repo, head, head)["route"], "full-unknown")

    def test_workflow_is_non_skipping_and_non_authorizing(self):
        workflow = (SOURCE.parents[2] / ".github/workflows/try-page.yml").read_text()
        self.assertIn("  pull_request:", workflow)
        self.assertNotIn("    paths:", workflow)
        self.assertNotIn("    paths-ignore:", workflow)
        self.assertIn("fetch-depth: 0", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn("git diff --check HEAD^1 HEAD", workflow)
        self.assertIn("NOT MERGE ACCEPTANCE", workflow)
        self.assertIn("scripts/ci/pr_stage.py", workflow)
        self.assertIn("if: github.event_name == 'pull_request'", workflow)
        self.assertFalse((SOURCE.parents[2] / ".github/workflows/pr-stage.yml").exists())


if __name__ == "__main__":
    unittest.main()
