"""Remove one [[patch]] row from upstream.toml text and nothing else.

upstream.py (frame_pin) and repin.py (drop_rows) both drop patch rows. They run
under `python3 -I`, so they load this module by path.

A row is its header line `[[patch]]`, the `id = "..."` line after it, and the
lines up to the next line whose text, after leading whitespace, starts with
`[` or `#`, or the end of the file. Comments before a table belong to that
table; the preamble before the first table belongs to none. Shapes this line
rule cannot handle raise ValueError rather than change the file.
"""
import tomllib


def patches(text):
    """The parsed [[patch]] rows of `text`."""
    return tomllib.loads(text).get("patch", [])


def without_patch(text, patch_id):
    """`text` without the [[patch]] row whose id is `patch_id`."""
    before = tomllib.loads(text)
    matching = [row for row in before.get("patch", []) if row.get("id") == patch_id]
    if not matching:
        return text
    if len(matching) > 1:
        raise ValueError(f"patch {patch_id!r} appears {len(matching)} times")
    lines = text.splitlines(keepends=True)
    start = header(lines, patch_id)
    end = block_end(lines, start, patch_id)
    if end == len(lines):
        while start > 0 and not lines[start - 1].strip():
            start -= 1
    result = "".join(lines[:start] + lines[end:])
    expect(before, result, patch_id)
    return result


def header(lines, patch_id):
    """The index of the row's `[[patch]]` line."""
    target = f'id = "{patch_id}"\n'
    for i, line in enumerate(lines[:-1]):
        if line == "[[patch]]\n" and lines[i + 1] == target:
            return i
    raise ValueError(f"patch {patch_id!r}: no `[[patch]]` line followed by `id = \"{patch_id}\"`")


def block_end(lines, start, patch_id):
    """The index after the row's last line."""
    for i in range(start + 2, len(lines)):
        stripped = lines[i].lstrip()
        if stripped.startswith("["):
            return i
        if stripped.startswith("#"):
            if comment_inside(lines, i):
                raise ValueError(f"patch {patch_id!r}: a comment inside its body")
            return i
    return len(lines)


def comment_inside(lines, i):
    """True when the comment at `i` is followed by a key line, not a table or the end."""
    for line in lines[i:]:
        stripped = line.lstrip()
        if not stripped or stripped.startswith("#"):
            continue
        return not stripped.startswith("[")
    return False


def expect(before, result, patch_id):
    """The result parses to `before` with exactly that row removed."""
    after = tomllib.loads(result)
    wanted = dict(before)
    kept = [row for row in before["patch"] if row.get("id") != patch_id]
    if kept:
        wanted["patch"] = kept
    else:
        del wanted["patch"]
    if after != wanted:
        raise ValueError(f"patch {patch_id!r}: removing its lines changed other content")
