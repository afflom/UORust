"""Removing a [[patch]] row from upstream.toml removes only that row.

Every case reads only scripts/ci/fixtures/upstream.toml (a snapshot of
upstream.toml at 28926f95, copied to a temporary directory wherever a script
writes) or an inline fixture; none reads the live root upstream.toml.
"""
import importlib.util
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
FIXTURE = (HERE / "fixtures" / "upstream.toml").read_text(encoding="utf-8")
LINES = FIXTURE.splitlines(keepends=True)


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


rows = load("upstream_rows")
upstream = load("upstream")
repin = load("repin")

PATCH_IDS = [row["id"] for row in tomllib.loads(FIXTURE)["patch"]]
FIRST, LAST = PATCH_IDS[0], PATCH_IDS[-1]


def keep(*ranges):
    """The fixture's lines in the given 1-based inclusive ranges."""
    return "".join("".join(LINES[a - 1:b]) for a, b in ranges)


class WithoutPatch(unittest.TestCase):
    def test_last_patch_keeps_gaps_and_their_comment(self):
        self.assertEqual(LAST, "template-frame-live-controls")
        self.assertEqual(rows.without_patch(FIXTURE, LAST), keep((1, 74), (84, 105)))

    def test_first_patch_keeps_the_preamble(self):
        self.assertEqual(FIRST, "prismpm-sdk-view-closure")
        self.assertEqual(rows.without_patch(FIXTURE, FIRST), keep((1, 19), (30, 105)))

    def test_unknown_id_changes_nothing(self):
        self.assertEqual(rows.without_patch(FIXTURE, "no-such-patch"), FIXTURE)

    def test_comment_before_a_patch_is_kept(self):
        text = '[[patch]]\nid = "a"\n\n# Section.\n[[patch]]\nid = "b"\n'
        self.assertEqual(rows.without_patch(text, "b"), '[[patch]]\nid = "a"\n\n# Section.\n')

    def test_last_table_at_end_of_file_leaves_one_newline(self):
        text = '# Head.\n\n[[gap]]\nid = "g"\n\n[[patch]]\nid = "p"\nx = 1\n'
        self.assertEqual(rows.without_patch(text, "p"), '# Head.\n\n[[gap]]\nid = "g"\n')

    def test_comment_inside_a_patch_body_fails_closed(self):
        text = '[[patch]]\nid = "p"\n# note\nx = 1\n'
        with self.assertRaisesRegex(ValueError, "p"):
            rows.without_patch(text, "p")

    def test_gap_with_the_same_id_is_never_removed(self):
        text = '[[gap]]\nid = "x"\n\n[[patch]]\nid = "x"\n'
        self.assertEqual(rows.without_patch(text, "x"), '[[gap]]\nid = "x"\n')

    def test_indented_comment_and_header_after_the_row_are_kept(self):
        text = '[[patch]]\nid = "p"\n  # kept\n  [[gap]]\n  id = "g"\n'
        self.assertEqual(rows.without_patch(text, "p"), '  # kept\n  [[gap]]\n  id = "g"\n')

    def test_shapes_the_line_rule_cannot_handle_raise(self):
        shapes = [
            '[[patch]]\nwhat = "w"\nid = "p"\n',
            '[[patch]]\nid="p"\n',
            '[[patch]] # c\nid = "p"\n',
            '[[patch]]\nid = "p"\ns = """\n[[gap]]\n"""\n',
            '[[patch]]\nid = "p"\n\n[[patch]]\nid = "p"\n',
        ]
        for text in shapes:
            with self.subTest(text=text), self.assertRaises(ValueError):
                rows.without_patch(text, "p")

    def test_only_remaining_patch(self):
        text = "# Head.\n\n[[patch]]\nid = \"p\"\n"
        result = rows.without_patch(text, "p")
        self.assertEqual(result, "# Head.\n")
        self.assertNotIn("patch", tomllib.loads(result))
        self.assertEqual(repin.drop_rows(FIXTURE, PATCH_IDS), keep((1, 19), (84, 105)))


class Callers(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())

    def test_main_with_no_patches_acts_on_none(self):
        (self.root / "upstream.toml").write_text(repin.drop_rows(FIXTURE, PATCH_IDS), encoding="utf-8")
        with mock.patch.object(upstream, "ROOT", self.root), \
                mock.patch.object(upstream, "run") as run, \
                mock.patch.object(upstream, "get") as get, \
                mock.patch.object(upstream, "pull") as pull, \
                mock.patch("sys.argv", ["upstream.py"]):
            upstream.main()
        run.assert_not_called()
        get.assert_not_called()
        pull.assert_not_called()

    def test_drop_rows_and_frame_pin_use_the_rule(self):
        a, b = PATCH_IDS[1], PATCH_IDS[2]
        expected = rows.without_patch(rows.without_patch(FIXTURE, a), b)
        self.assertEqual(repin.drop_rows(FIXTURE, [a, b]), expected)
        (self.root / "upstream.toml").write_text(FIXTURE, encoding="utf-8")
        patch = {"id": LAST, "repository": "UOR-Foundation/template"}
        with mock.patch.object(upstream, "ROOT", self.root), mock.patch.object(upstream, "run") as run:
            upstream.frame_pin(patch, {"commit": "0" * 40})
        written = (self.root / "upstream.toml").read_text(encoding="utf-8")
        self.assertEqual(written, rows.without_patch(FIXTURE, LAST))
        self.assertEqual([c.args[0] for c in run.call_args_list], ["node", "scripts/ci/push.sh"])


if __name__ == "__main__":
    unittest.main()
