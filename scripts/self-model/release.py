"""Emit UORust's Release facet (app/src/Release.lex.tex) and the README's table
of release targets from the workflows that build and install the product.

    python3 -I release.py OUT_ROOT [SOURCE_ROOT]

- The release targets are the build matrix of .github/workflows/release.yml:
  each target triple, the runner that builds it, whether the release waits for
  it, and the archive the package step names for it.
- The release artifacts are the archives, each archive's .sha256 and the
  SHA256SUMS the publish job assembles.
- The distribution channels are the jobs of .github/workflows/install.yml that
  carry a CHANNEL environment: each installs uorust through one channel and runs
  it, so a channel is modeled exactly when CI proves it. CHANNEL_SOURCE names
  the release.yml job whose output the channel installs ("none" when it builds
  from source).
- The release version is the workspace version of Cargo.toml, and a theorem
  binds it to the system model's product version.

Every record cites the span it was read from with that span's κ
(Architecture::Source), as facets.py does, so `just self-model-check` reports
any change of the workflows that is not regenerated here.
"""
import hashlib, re, sys

root = sys.argv[1]
SRC = sys.argv[2] if len(sys.argv) > 2 else root
SPAN_DOMAIN = "uorust/span/1"
RELEASE = ".github/workflows/release.yml"
INSTALL = ".github/workflows/install.yml"
SOURCES = []


def lines_of(path):
    return open(f"{SRC}/{path}", encoding="utf-8").read().split("\n")


def cite(record, path, first, last):
    text = "\n".join(lines_of(path)[first - 1:last])
    k = hashlib.sha256(f"{SPAN_DOMAIN}\0{path}\0{text}".encode()).hexdigest()
    SOURCES.append((record, path, first, last, k))


def q(s):
    assert "\n" not in s
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def jobs(lines):
    """The jobs of a workflow: id -> (first line, last line), 1-based."""
    top = lines.index("jobs:")
    heads = [i for i in range(top + 1, len(lines)) if re.fullmatch(r"  [\w-]+:", lines[i])]
    out = {}
    for n, h in enumerate(heads):
        end = (heads[n + 1] if n + 1 < len(heads) else len(lines)) - 1
        while end > h and (not lines[end].strip() or lines[end].startswith("  #")):
            end -= 1
        out[lines[h].strip()[:-1]] = (h + 1, end + 1)
    return out


# ---------------------------------------------------------------- release.yml
rel = lines_of(RELEASE)
release_jobs = jobs(rel)
first, last = release_jobs["build"]
targets = []  # (triple, runner, required, first line, last line)
for i in range(first - 1, last):
    m = re.fullmatch(r"\s+- target: (\S+)", rel[i])
    if not m:
        continue
    fields = {}
    j = i + 1
    while j < last and re.fullmatch(r"\s+(runner|optional): \S+", rel[j]):
        k, v = rel[j].strip().split(": ")
        fields[k] = v
        j += 1
    assert set(fields) == {"runner", "optional"}, f"{RELEASE}:{i + 1}: a target without runner and optional"
    assert fields["optional"] in ("true", "false")
    targets.append((m.group(1), fields["runner"], fields["optional"] == "false", i + 1, j))
assert targets, f"{RELEASE}: no build matrix"

# The archive the package step names: the one under the Windows condition for
# a Windows target, the other otherwise.
names = [(i + 1, l) for i, l in enumerate(rel) if re.search(r'archive="uorust-\$TARGET\.[\w.]+"', l)]
cond = [i + 1 for i, l in enumerate(rel) if "if [[ $TARGET == *windows* ]]" in l]
assert len(names) == 2 and len(cond) == 1, f"{RELEASE}: the package step's archive names changed shape"
(win_line, win), (other_line, other) = names
assert cond[0] < win_line < other_line
win_ext = re.search(r'\$TARGET(\.[\w.]+)"', win).group(1)
other_ext = re.search(r'\$TARGET(\.[\w.]+)"', other).group(1)
cite("Release::archiveNames", RELEASE, cond[0], other_line)
sums_line = next(i + 1 for i, l in enumerate(rel) if ">SHA256SUMS" in l)
cite("Release::releaseArtifacts", RELEASE, sums_line, sums_line)


def archive(triple):
    return f"uorust-{triple}{win_ext if 'windows' in triple else other_ext}"


for triple, runner, required, a, b in targets:
    cite(f"Release::releaseTargets/{triple}", RELEASE, a, b)

# ---------------------------------------------------------------- install.yml
ins = lines_of(INSTALL)
channels = []  # (id, kind, source, command, verify)
FIELDS = ["CHANNEL", "CHANNEL_KIND", "CHANNEL_SOURCE", "CHANNEL_COMMAND", "CHANNEL_VERIFY"]
for job, (a, b) in jobs(ins).items():
    found = {}
    span = []
    for i in range(a - 1, b):
        m = re.fullmatch(r"\s+(CHANNEL\w*): (.+)", ins[i])
        if m and m.group(1) in FIELDS:
            found[m.group(1)] = m.group(2)
            span.append(i + 1)
    if not found:
        continue
    assert set(found) == set(FIELDS), f"{INSTALL}: job {job} misses {set(FIELDS) - set(found)}"
    assert span == list(range(span[0], span[-1] + 1)), f"{INSTALL}: job {job}'s CHANNEL lines are not together"
    source = found["CHANNEL_SOURCE"]
    assert source == "none" or source in release_jobs, f"{INSTALL}: job {job} names no release.yml job {source!r}"
    channels.append(tuple(found[f] for f in FIELDS))
    cite(f"Release::distributionChannels/{found['CHANNEL']}", INSTALL, span[0], span[-1])
assert channels, f"{INSTALL}: no job carries a CHANNEL"
assert len({c[0] for c in channels}) == len(channels), f"{INSTALL}: a channel is named twice"

# ---------------------------------------------------------------- Cargo.toml
cargo = lines_of("Cargo.toml")
ws = cargo.index("[workspace.package]")
vline = next(i for i in range(ws, len(cargo)) if cargo[i].startswith("version = "))
version = re.fullmatch(r'version = "(.+)"', cargo[vline]).group(1)
cite("Release::releaseVersion", "Cargo.toml", vline + 1, vline + 1)

# ---------------------------------------------------------------- the module


def lst(items, ty):
    out = f"(nil {ty})"
    for i in reversed(items):
        out = f"(cons {i} {out})"
    return out


def defn(name, result, body):
    return f"definition{{\n  name: {name},\n  parameters: [],\n  result: {result},\n  body: {body}\n}}\n"


def struct(name, fields):
    fs = ",\n".join(f"    {{name: {n}, type: {t}}}" for n, t in fields)
    return f"structure{{\n  name: {name},\n  type_parameters: [],\n  parameters: [],\n  fields: [\n{fs}\n  ]\n}}\n"


artifacts = []
for triple, *_ in targets:
    artifacts += [archive(triple), archive(triple) + ".sha256"]
artifacts.append("SHA256SUMS")

out = ("\\begin{lexlean}{Release}\n\\useglossary{lexlean.std.bool@1.2.0}\n\\useglossary{lexlean.std.nat@1.2.0}\n"
       "\\useglossary{prism.arch@1.0.0}\n\\importmodule{Architecture}\n\\importmodule{Production.Core}\n"
       "\\importmodule{Production.BrowserSystem}\n\\importmodule{System}\n"
       "\\title{UORust release channels}\n\\begin{semanticmodule}\n")
out += struct("ReleaseTarget", [("triple", "String"), ("runner", "String"), ("archive", "String"), ("required", "Bool")]) + "\n"
out += struct("DistributionChannel", [("id", "String"), ("kind", "String"), ("source", "String"),
                                      ("command", "String"), ("verification", "String")]) + "\n"
out += defn("releaseVersion", "String", q(version)) + "\n"
out += defn("releaseTargets", "(List ReleaseTarget)", lst(
    [f"(record ReleaseTarget {{triple := {q(t)}, runner := {q(r)}, archive := {q(archive(t))}, "
     f"required := {'true' if req else 'false'}}})" for t, r, req, _, _ in targets], "ReleaseTarget")) + "\n"
out += defn("releaseArtifacts", "(List String)", lst([q(a) for a in artifacts], "String")) + "\n"
out += defn("distributionChannels", "(List DistributionChannel)", lst(
    [f"(record DistributionChannel {{id := {q(c)}, kind := {q(k)}, source := {q(s)}, command := {q(cmd)}, "
     f"verification := {q(v)}}})" for c, k, s, cmd, v in channels], "DistributionChannel")) + "\n"
out += """theorem{
  name: releaseVersionIsTheSystemVersion,
  parameters: [],
  statement: ((prim equal (releaseVersion) (project (project (System::systemModel) product) version) : Bool) = true),
  proof: decide{}
}

"""
src = "(nil Architecture::Source)"
for rec, path, a, b, k in reversed(SOURCES):
    src = (f"(cons (record Architecture::Source {{record := {q(rec)}, path := {q(path)}, firstLine := {a}, "
           f"lastLine := {b}, kappa := {q(k)}}}) {src})")
out += defn("sourcesOfRelease", "(List Architecture::Source)", src)
out += "\\end{semanticmodule}\n\\end{lexlean}\n"
open(f"{root}/app/src/Release.lex.tex", "w", encoding="utf-8").write(out)

# ---------------------------------------------------------------- README
BEGIN, END = "<!-- release-targets:begin (scripts/self-model/release.py) -->", "<!-- release-targets:end -->"
readme = open(f"{SRC}/README.md", encoding="utf-8").read()
assert readme.count(BEGIN) == 1 and readme.count(END) == 1, "README.md: no release-targets block"
table = ["| Target | Archive | Built on | Release waits for it |", "| --- | --- | --- | --- |"]
table += [f"| `{t}` | `{archive(t)}` | `{r}` | {'yes' if req else 'no'} |" for t, r, req, _, _ in targets]
head, rest = readme.split(BEGIN)
_, tail = rest.split(END)
open(f"{root}/README.md", "w", encoding="utf-8").write(head + BEGIN + "\n" + "\n".join(table) + "\n" + END + tail)
print({"targets": len(targets), "artifacts": len(artifacts), "channels": len(channels)})
