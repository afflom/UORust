"""Emit the brand, design-system and context-of-use facets of a Prism model.

Three facets every professional product has and the base Prism facets
(prism.arch, prism.sec, prism.qual) leave out, each read mechanically from its
source and each modeled after an established framework, cited in the records:

  Brand      the brand identity elements of Alina Wheeler, Designing Brand
             Identity (5th ed., Wiley, 2017): name, tagline, promise,
             positioning, values, mission, steward, call to action, mark,
             palette and typography, read from uor.foundation (the copy that
             brand/foundation.lock pins), and the product's own name and
             promise, read from site/app.json.
  Design     the design tokens of uor.foundation in the W3C Design Tokens
             Format (DTCG 2025.10): colour, dimension, number, gradient and
             font family, each with the CSS span it is read from, and every
             text and focus colour the site sets on a surface it sets, with
             its WCAG 2.2 contrast ratio measured against the success
             criterion that bounds it (SC 1.4.3 text, SC 1.4.11 non-text).
  Users      the context of use of ISO 9241-11:2018 (3.1.15) and ISO
             9241-210:2019: user groups, their goals and tasks and the
             technical environment, read from the model's own stakeholders,
             journeys, features and components, and the usability of each
             goal (effectiveness, efficiency, satisfaction, 3.1.12-3.1.14) as
             observed from the model, or recorded open where nothing measures
             it.

Every record carries the span it was read from and that span's kappa, the
SHA-256 of the domain, the path and the span's bytes, so a change of the
source is a change of the model.

    python3 -I experience.py OUT_ROOT [SOURCE_ROOT]

Outputs (under OUT_ROOT): app/src/Foundation/Experience/V1/Model.lex.tex,
app/src/Brand.lex.tex, app/src/Users.lex.tex, the glossary entries of their
titles, the DTCG token file brand/tokens.json and the stylesheet
brand/foundation.css generated from it.
"""
import colorsys, hashlib, json, math, pathlib, re, sys, tomllib

OUT = pathlib.Path(sys.argv[1])
SRC = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else sys.argv[1])
CONFIG = tomllib.loads((SRC / "model/experience.toml").read_text())
PRODUCT = CONFIG["product"]
SLUG = re.sub(r"[^a-z0-9]+", "-", PRODUCT.lower()).strip("-")
LOCK = json.loads((SRC / "brand/foundation.lock").read_text())
AUTH = SRC / "brand/foundation"
SPAN_DOMAIN = "prism/experience-span/1"

# The criteria of WCAG 2.2 that bound a contrast, as the standard states them:
# (success criterion, what it covers, minimum ratio as numerator:denominator).
WCAG = {
    "text": ("WCAG 2.2 SC 1.4.3 Contrast (Minimum)", "text and images of text", (45, 10)),
    "nonText": ("WCAG 2.2 SC 1.4.11 Non-text Contrast", "user interface components and focus indicators", (3, 1)),
}
# Ratios are kept as whole thousandths, floored, so an observed value is never
# rounded up past its criterion.
SCALE = 1000

# ------------------------------------------------------------------ sources
SOURCES = []  # (record, path, first, last, kappa)


def lines_of(path):
    return (SRC / path).read_text(encoding="utf-8").split("\n")


def kappa_bytes(path, data):
    return hashlib.sha256(SPAN_DOMAIN.encode() + b"\0" + path.encode() + b"\0" + data).hexdigest()


def cite(record, path, first, last):
    text = "\n".join(lines_of(path)[first - 1:last]).encode()
    SOURCES.append((record, path, first, last, kappa_bytes(path, text)))
    return len(SOURCES) - 1


def cite_file(record, path):
    SOURCES.append((record, path, 0, 0, kappa_bytes(path, (SRC / path).read_bytes())))
    return len(SOURCES) - 1


def line_of(path, needle, start=1):
    for at, line in enumerate(lines_of(path)[start - 1:], start):
        if needle in line:
            return at
    raise SystemExit(f"experience: {path} has no line containing {needle!r}")


def q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


# ----------------------------------------------------------- design tokens
CSS = "brand/foundation/src/index.css"
css = lines_of(CSS)


def blocks(selector):
    """Every top-level `selector {` ... `}` block of the authority CSS: its
    declarations, each with the lines it spans."""
    out, at = [], 0
    while at < len(css):
        if css[at].rstrip() == f"{selector} {{":
            at += 1
            while css[at].rstrip() != "}":
                m = re.match(r"\s*(--fd-[a-z0-9-]+):\s*(.*)$", css[at])
                if m:
                    first, value = at + 1, m.group(2)
                    while not value.rstrip().rstrip("/* ").endswith(";") and ";" not in value:
                        at += 1
                        value += " " + css[at].strip()
                    value = re.sub(r"\s*/\*.*?\*/\s*", "", value).strip().rstrip(";").strip()
                    value = re.sub(r"\s+", " ", value).replace("( ", "(")
                    out.append((m.group(1)[2:], value, first, at + 1))
                at += 1
        at += 1
    return out


def token_type(value):
    if re.fullmatch(r"\d+(\.\d+)? \d+(\.\d+)?% \d+(\.\d+)?%", value):
        return "color"
    if re.fullmatch(r"-?\d*\.?\d+", value):
        return "number"
    if value.startswith("linear-gradient("):
        return "gradient"
    if re.search(r"\d(px|rem|em|vw|vh)\b", value):
        return "dimension"
    raise SystemExit(f"experience: {CSS}: no DTCG type for {value!r}")


def hsl_rgb(value):
    h, s, l = (float(x.rstrip("%")) for x in value.split())
    r, g, b = colorsys.hls_to_rgb(h / 360, l / 100, s / 100)
    return tuple(round(c * 255) for c in (r, g, b))


def luminance(rgb):
    """WCAG 2.2 relative luminance of an sRGB colour."""
    def lin(c):
        c = c / 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = (lin(c) for c in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


tokens = []  # (name, type, value, hex or "", source)
seen = set()
for selector in (":root", ".foundation"):
    for name, value, first, last in blocks(selector):
        if name in seen:
            raise SystemExit(f"experience: {CSS}: --{name} is declared twice")
        seen.add(name)
        kind = token_type(value)
        hexv = "#%02x%02x%02x" % hsl_rgb(value) if kind == "color" else ""
        tokens.append((name, kind, value, hexv, cite(f"Brand::token {name}", CSS, first, last)))

def gradient_stops(value):
    """A CSS linear-gradient of hsl() stops as DTCG gradient stops: (hex, position 0..1)."""
    return [("#%02x%02x%02x" % hsl_rgb(f"{h} {sat} {l}"), float(p) / 100)
            for h, sat, l, p in re.findall(r"hsl\(([\d.]+) ([\d.]+%) ([\d.]+%)\) ([\d.]+)%", value)]


def canonical(kind, value, hexv):
    """A token's value as the model states it: a colour by its sRGB hex, a
    gradient by its DTCG stops, anything else as written. LexLean admits no
    raw percent sign, and these forms need none."""
    if kind == "color":
        return hexv
    if kind == "gradient":
        angle = re.match(r"linear-gradient\(([\d.]+deg)", value).group(1)
        return angle + ", " + ", ".join(f"{h} {p:g}" for h, p in gradient_stops(value))
    assert "%" not in value, f"{value!r} has a percent sign the model cannot state"
    return value


# Font families: the families the .foundation scope names for its heading and
# mono text, as DTCG fontFamily tokens.
for role in ("fd-head", "fd-mono"):
    at = line_of(CSS, f".foundation .{role} {{")
    family = re.search(r"font-family:\s*([^;]+);", css[at - 1]).group(1)
    stack = [f.strip().strip('"') for f in family.split(",")]
    tokens.append((role, "fontFamily", ", ".join(stack), "", cite(f"Brand::token {role}", CSS, at, at)))

TOKEN = {t[0]: i for i, t in enumerate(tokens)}
COLOR = {t[0]: t[3] for t in tokens if t[1] == "color"}

# ------------------------------------------------------------------ contrast
# The rules of the authority CSS that set a colour token as text, as a focus
# outline, or as a background, line by line (every rule the site writes is one
# line or a block whose declarations are each on their own line).
TEXT = re.compile(r"(?<![\w-])color:\s*hsl\(var\(--(fd-[a-z-]+)\)\)")
FOCUS = re.compile(r"(?<![\w-])outline:[^;]*hsl\(var\(--(fd-[a-z-]+)\)\)")
SURFACE = re.compile(r"(?<![\w-])background(?:-color)?:\s*hsl\(var\(--(fd-[a-z-]+)\)(?:\s*/\s*([0-9.]+))?\)")


def rules():
    """(first line, last line, body) of every rule: a one-line rule, or a
    multi-line block from its `{` line to its `}` line."""
    at = 0
    while at < len(css):
        line = css[at]
        if "{" in line and "}" in line[line.index("{"):]:
            yield at + 1, at + 1, line
        elif line.rstrip().endswith("{") and not line.lstrip().startswith("@"):
            first = at
            while not css[at].strip().startswith("}"):
                at += 1
            yield first + 1, at + 1, "\n".join(css[first:at + 1])
        at += 1


pairs = {}  # (fg, bg, criterion) -> first rule that sets it
uses = [(first, last, {m for m in TEXT.findall(body) if m in COLOR}, {m for m in FOCUS.findall(body) if m in COLOR},
         {(m, float(a or 1)) for m, a in SURFACE.findall(body) if m in COLOR}, "padding" in body) for first, last, body in rules()]
# A text colour set without a background sits on the surface around it. Those
# surfaces are the boxes that hold text: a rule that sets a background and the
# site's body text colour (the text colour its rules set most), or a padded
# box that sets a background and no text colour. A background with neither (a
# divider, a dot, a bar) holds no text and pairs with nothing.
counts = {}
for _, _, text, _, _, _ in uses:
    for t in text:
        counts[t] = counts.get(t, 0) + 1
body_text = max(sorted(counts), key=lambda t: counts[t])
containers = set()
for _, _, text, _, surface, padded in uses:
    if body_text in text or (padded and not text):
        containers |= {m for m, a in surface if a == 1}
# SC 1.4.3 and SC 1.4.11 exempt an inactive user interface component.
INACTIVE = re.compile(r":disabled|\[disabled\]|\[aria-disabled")
for first, last, text, focus, surface, _ in uses:
    if INACTIVE.search(css[first - 1].split("{")[0]):
        continue
    for criterion, fgs in (("text", text), ("nonText", focus)):
        for fg in sorted(fgs):
            for bg, alpha in sorted(surface or {(c, 1.0) for c in containers}):
                if fg != bg:
                    pairs.setdefault((fg, bg, round(alpha * SCALE), criterion), (first, last))

def over(top, alpha, under):
    """`top` at `alpha` composited over `under` (CSS Compositing, source-over)."""
    return tuple(round(t * alpha + u * (1 - alpha)) for t, u in zip(top, under))


contrasts = []  # (fg, bg, alpha milli, criterion, observed milli, required milli, source)
for (fg, bg, alpha, criterion), (first, last) in sorted(pairs.items()):
    num, den = WCAG[criterion][2]
    top = hsl_rgb(tokens[TOKEN[bg]][2])
    # A translucent background shows the box under it; the ratio kept is the
    # worst over every surface it can sit on.
    grounds = [top] if alpha == SCALE else [over(top, alpha / SCALE, hsl_rgb(tokens[TOKEN[c]][2])) for c in sorted(containers)]
    ratio = min(contrast(hsl_rgb(tokens[TOKEN[fg]][2]), g) for g in grounds)
    contrasts.append((fg, bg, alpha, criterion, math.floor(ratio * SCALE), num * SCALE // den,
                      cite(f"Brand::contrast {fg} on {bg}", CSS, first, last)))

# ----------------------------------------------------------- brand elements
HTML = "brand/foundation/index.html"
COPY = "brand/foundation/src/site/copy.ts"
APP = "site/app.json"
elements = []  # (kind, text, source)


def html_meta(attr, value):
    at = line_of(HTML, f'{attr}="{value}"')
    return re.search(r'content="([^"]*)"', lines_of(HTML)[at - 1]).group(1), at


def copy_field(const, key, start=None):
    """The string `key: "..."` of `export const CONST = {` in copy.ts, at the
    first occurrence at or after `start`."""
    begin = line_of(COPY, f"export const {const} = {{")
    at = line_of(COPY, f"{key}:", start or begin)
    m = re.search(rf'\b{key}:\s*"((?:[^"\\]|\\.)*)"', lines_of(COPY)[at - 1])
    if not m:
        raise SystemExit(f"experience: {COPY}:{at}: {const}.{key} is not a one-line string")
    return m.group(1).replace("\\n", " "), at


def element(kind, text, path, at):
    elements.append((kind, text, cite(f"Brand::{kind}", path, at, at)))


name, at = html_meta("property", "og:site_name")
element("name", name, HTML, at)
at = line_of(HTML, "<title>")
title = re.search(r"<title>(.*)</title>", lines_of(HTML)[at - 1]).group(1)
element("tagline", title.split(" · ", 1)[1], HTML, at)
promise, at = html_meta("name", "description")
element("promise", promise, HTML, at)
element("positioning", *copy_field("FRAMEWORK", "line")[:1], COPY, copy_field("FRAMEWORK", "line")[1])
element("mission", *copy_field("ABOUT", "body")[:1], COPY, copy_field("ABOUT", "body")[1])
points = line_of(COPY, "points:", line_of(COPY, "export const ABOUT = {"))
for value_at in [i for i, l in enumerate(lines_of(COPY), 1) if i > points and re.match(r'\s*t:\s*"', l)][:3]:
    element("value", re.search(r't:\s*"([^"]*)"', lines_of(COPY)[value_at - 1]).group(1).replace("\\n", " "), COPY, value_at)
element("steward", *copy_field("HERO", "steward")[:1], COPY, copy_field("HERO", "steward")[1])
element("callToAction", *copy_field("START", "command")[:1], COPY, copy_field("START", "command")[1])
elements.append(("mark", "public/favicon.png", cite_file("Brand::mark", "brand/foundation/public/favicon.png")))
elements.append(("palette", ", ".join(f"{n} {h}" for n, h in COLOR.items()), tokens[TOKEN["fd-bg"]][4]))
elements.append(("typography", "; ".join(t[2] for t in tokens if t[1] == "fontFamily"), tokens[TOKEN["fd-head"]][4]))
app = json.loads((SRC / APP).read_text())
element("productName", app["name"], APP, line_of(APP, '"name":'))
element("productPromise", app["promise"]["headline"], APP, line_of(APP, '"headline":'))

# --------------------------------------------------------- context of use
MODEL = SRC / "app/src"


def records(module, kind):
    """(name, body) of every `definition{ name, result: kind, body }` in a
    generated or hand-written facet module."""
    text = (MODEL / f"{module}.lex.tex").read_text()
    out = []
    for m in re.finditer(r"definition\{\n  name: (\w+),\n  parameters: \[\],\n  result: ([\w.:]+),\n  body: (.*)\n\}", text):
        if m.group(2) == kind:
            out.append((m.group(1), m.group(3)))
    return out


def field(body, name):
    m = re.search(rf'\b{name} := "((?:[^"\\]|\\.)*)"', body)
    return m.group(1) if m else None


def strings(body, name):
    m = re.search(rf"\b{name} := (\(cons .*?\(nil String\)\)+|\(nil String\))", body)
    return re.findall(r'\(cons "((?:[^"\\]|\\.)*)"', m.group(1)) if m else []


stakeholders = {n: b for n, b in records("Architecture", "Foundation.Arch::Stakeholder")}
features = {n: b for n, b in records("Features", "Foundation.Arch::Feature")}
journeys = records("Features", "Foundation.Arch::Journey")
components = records("Architecture", "Foundation.Arch::Component")
# The interfaces a user touches (ISO 9241-11 technical environment): the
# components whose kind is a released program, a command-line adapter or a
# site. Services and external systems are behind them.
FACING = ("releaseBinary", "cliAdapter", "staticSite")
environments = [(n, re.search(r"ComponentKind\.(\w+)", b).group(1), field(b, "name"))
                for n, b in components if re.search(r"ComponentKind\.(\w+)", b).group(1) in FACING]
if not environments:
    raise SystemExit("experience: the model declares no user-facing component (ISO 9241-11 technical environment)")

groups, goals, tasks = [], [], []
for journey, body in journeys:
    actor = field(body, "actor").split("::")[1]
    if actor not in stakeholders:
        raise SystemExit(f"experience: {journey}'s actor {actor} is not a declared stakeholder")
    if actor not in [g[0] for g in groups]:
        groups.append((actor, field(stakeholders[actor], "text")))
    steps = [s.split("::")[1] for s in strings(body, "steps")]
    outcome = field(features[steps[-1]], "statement")
    goals.append(([g[0] for g in groups].index(actor), journey, outcome, steps))
    for order, step in enumerate(steps):
        tasks.append((len(goals) - 1, order, step, len(strings(features[step], "acceptance"))))

# --------------------------------------------------------------- emitting
HEADER = """\\begin{{lexlean}}{{{module}}}
\\useglossary{{lexlean.std.bool@1.2.0}}
\\useglossary{{lexlean.std.nat@1.2.0}}
\\useglossary{{prism.arch@1.0.0}}
{imports}\\title{{{title}}}
\\begin{{semanticmodule}}
"""
FOOTER = "\\end{semanticmodule}\n\\end{lexlean}\n"


def header(module, title, imports=()):
    return HEADER.format(module=module, title=title, imports="".join(f"\\importmodule{{{i}}}\n" for i in imports))


def defn(name, result, body):
    return f"definition{{\n  name: {name},\n  parameters: [],\n  result: {result},\n  body: {body}\n}}\n\n"


def lst(items, ty):
    out = f"(nil {ty})"
    for item in reversed(items):
        out = f"(cons {item} {out})"
    return out


def theorem(name, statement):
    return f"theorem{{\n  name: {name},\n  parameters: [],\n  statement: {statement},\n  proof: decide{{}}\n}}\n\n"


def all_of(name, ty, condition, extra=""):
    """A recursive Bool check that `condition` holds of every item."""
    params = f"[items : (List {ty}){extra}]"
    args = " ".join(p.split(" : ")[0] for p in extra.split(", ")[1:]) if extra else ""
    return (f"definition{{\n  name: {name},\n  parameters: {params},\n  result: Bool,\n  body: (match items\n"
            f"    | List.nil => true\n    | List.cons head rest => ({condition} && ({name} rest{' ' + args if args else ''}))),\n"
            f"  recursive_argument: items\n}}\n\n")


M = "Foundation.Experience.V1.Model"
model = header(M, "Brand, design and context of use", ["Foundation.Arch"])
model += """inductive{
  name: BrandElementKind,
  type_parameters: [],
  parameters: [],
  constructors: [
    {name: name, fields: []},
    {name: tagline, fields: []},
    {name: promise, fields: []},
    {name: positioning, fields: []},
    {name: mission, fields: []},
    {name: value, fields: []},
    {name: steward, fields: []},
    {name: callToAction, fields: []},
    {name: mark, fields: []},
    {name: palette, fields: []},
    {name: typography, fields: []},
    {name: productName, fields: []},
    {name: productPromise, fields: []}
  ]
}

inductive{
  name: TokenType,
  type_parameters: [],
  parameters: [],
  constructors: [
    {name: color, fields: []},
    {name: dimension, fields: []},
    {name: number, fields: []},
    {name: gradient, fields: []},
    {name: fontFamily, fields: []}
  ]
}

inductive{
  name: ContrastCriterion,
  type_parameters: [],
  parameters: [],
  constructors: [{name: text, fields: []}, {name: nonText, fields: []}]
}

inductive{
  name: EnvironmentKind,
  type_parameters: [],
  parameters: [],
  constructors: [{name: technical, fields: []}, {name: physical, fields: []}, {name: social, fields: []}]
}

inductive{
  name: UsabilityOutcome,
  type_parameters: [],
  parameters: [],
  constructors: [{name: effectiveness, fields: []}, {name: efficiency, fields: []}, {name: satisfaction, fields: []}]
}

structure{
  name: Authority,
  type_parameters: [],
  parameters: [],
  fields: [{name: framework, type: String}, {name: repository, type: String}, {name: commit, type: String}]
}

structure{
  name: Source,
  type_parameters: [],
  parameters: [],
  fields: [
    {name: record, type: String},
    {name: path, type: String},
    {name: firstLine, type: Nat},
    {name: lastLine, type: Nat},
    {name: kappa, type: String}
  ]
}

structure{
  name: BrandElement,
  type_parameters: [],
  parameters: [],
  fields: [{name: kind, type: BrandElementKind}, {name: text, type: String}, {name: source, type: Nat}]
}

structure{
  name: DesignToken,
  type_parameters: [],
  parameters: [],
  fields: [
    {name: name, type: String},
    {name: type, type: TokenType},
    {name: value, type: String},
    {name: hex, type: String},
    {name: source, type: Nat}
  ]
}

structure{
  name: ContrastPair,
  type_parameters: [],
  parameters: [],
  fields: [
    {name: foreground, type: Nat},
    {name: background, type: Nat},
    {name: backgroundAlphaMilli, type: Nat},
    {name: criterion, type: ContrastCriterion},
    {name: observedMilli, type: Nat},
    {name: requiredMilli, type: Nat},
    {name: source, type: Nat}
  ]
}

structure{
  name: UserGroup,
  type_parameters: [],
  parameters: [],
  fields: [{name: stakeholder, type: Foundation.Arch::Stakeholder}, {name: characteristics, type: String}]
}

structure{
  name: Goal,
  type_parameters: [],
  parameters: [],
  fields: [{name: user, type: Nat}, {name: journey, type: Foundation.Arch::Journey}, {name: outcome, type: String}]
}

structure{
  name: Task,
  type_parameters: [],
  parameters: [],
  fields: [{name: goal, type: Nat}, {name: order, type: Nat}, {name: feature, type: Foundation.Arch::Feature}]
}

structure{
  name: Environment,
  type_parameters: [],
  parameters: [],
  fields: [{name: kind, type: EnvironmentKind}, {name: component, type: Foundation.Arch::Component}]
}

structure{
  name: UsabilityMeasure,
  type_parameters: [],
  parameters: [],
  fields: [
    {name: goal, type: Nat},
    {name: outcome, type: UsabilityOutcome},
    {name: observed, type: (Option Nat)},
    {name: method, type: String}
  ]
}

definition{
  name: shortfallUnder,
  parameters: [required : Nat, observed : Nat],
  result: Nat,
  body: (prim subtract required observed : Nat)
}

theorem{
  name: noShortfallIsMet,
  parameters: [required : Nat, observed : Nat],
  statement: (((shortfallUnder required observed) = 0) -> (required <= observed)),
  proof: linear_arithmetic{definitions: [shortfallUnder]},
  axioms: [Quot.sound, propext]
}

theorem{
  name: metHasNoShortfall,
  parameters: [required : Nat, observed : Nat],
  statement: ((required <= observed) -> ((shortfallUnder required observed) = 0)),
  proof: linear_arithmetic{definitions: [shortfallUnder]},
  axioms: [Quot.sound, propext]
}

theorem{
  name: shortfallFallsAsTheObservationRises,
  parameters: [required : Nat, before : Nat, after : Nat],
  statement: ((before <= after) -> ((shortfallUnder required after) <= (shortfallUnder required before))),
  proof: linear_arithmetic{definitions: [shortfallUnder]},
  axioms: [Quot.sound, propext]
}

"""
model += all_of("contrastsMet", "ContrastPair",
                "((shortfallUnder (project head requiredMilli) (project head observedMilli)) =? 0)")
model += all_of("contrastsWithin", "ContrastPair",
                "(((project head foreground) <? tokens) && (((project head background) <? tokens) && ((project head source) <? sources)))",
                ", tokens : Nat, sources : Nat")
model += all_of("tokensSourced", "DesignToken", "((project head source) <? sources)", ", sources : Nat")
model += all_of("elementsSourced", "BrandElement", "((project head source) <? sources)", ", sources : Nat")
model += all_of("goalsOfGroups", "Goal", "((project head user) <? groups)", ", groups : Nat")
model += all_of("tasksOfGoals", "Task", "((project head goal) <? goals)", ", goals : Nat")
model += """definition{
  name: named,
  parameters: [items : (List String)],
  result: Bool,
  body: (match items
    | List.nil => false
    | List.cons head rest => true)
}

"""
model += all_of("tasksAccepted", "Task", "(named (project (project head feature) acceptance))")
model += all_of("measuresOfGoals", "UsabilityMeasure", "((project head goal) <? goals)", ", goals : Nat")
model += FOOTER

# Brand.lex.tex
auth = LOCK["source"]
brand = header("Brand", f"{PRODUCT} brand identity", [M])
brand += defn("authority", f"{M}::Authority",
              f"(record {M}::Authority {{framework := {q('Alina Wheeler, Designing Brand Identity, 5th ed., Wiley, 2017; W3C Design Tokens Format Module 2025.10; WCAG 2.2')}, "
              f"repository := {q(auth['repository'])}, commit := {q(auth['commit'])}}})")
brand += defn("sources", f"(List {M}::Source)", lst(
    [f"(record {M}::Source {{record := {q(r)}, path := {q(p)}, firstLine := {a}, lastLine := {b}, kappa := {q(k)}}})"
     for r, p, a, b, k in SOURCES], f"{M}::Source"))
brand += defn("elements", f"(List {M}::BrandElement)", lst(
    [f"(record {M}::BrandElement {{kind := (ctor {M}::BrandElementKind.{k}), text := {q(t)}, source := {s}}})"
     for k, t, s in elements], f"{M}::BrandElement"))
brand += defn("tokens", f"(List {M}::DesignToken)", lst(
    [f"(record {M}::DesignToken {{name := {q(n)}, type := (ctor {M}::TokenType.{k}), value := {q(canonical(k, v, h))}, hex := {q(h)}, source := {s}}})"
     for n, k, v, h, s in tokens], f"{M}::DesignToken"))
brand += defn("contrasts", f"(List {M}::ContrastPair)", lst(
    [f"(record {M}::ContrastPair {{foreground := {TOKEN[f]}, background := {TOKEN[b]}, backgroundAlphaMilli := {a}, criterion := (ctor {M}::ContrastCriterion.{c}), observedMilli := {o}, requiredMilli := {r}, source := {s}}})"
     for f, b, a, c, o, r, s in contrasts], f"{M}::ContrastPair"))
for criterion, (sc, covers, (num, den)) in WCAG.items():
    brand += defn(f"required{criterion[0].upper()}{criterion[1:]}Milli", "Nat", f"{num * SCALE // den}")
n_src, n_tok = len(SOURCES), len(tokens)
brand += theorem("everyElementIsSourced", f"(({M}::elementsSourced (elements) {n_src}) = true)")
brand += theorem("everyTokenIsSourced", f"(({M}::tokensSourced (tokens) {n_src}) = true)")
brand += theorem("everyContrastNamesItsTokens", f"(({M}::contrastsWithin (contrasts) {n_tok} {n_src}) = true)")
brand += theorem("everyContrastMeetsItsCriterion", f"(({M}::contrastsMet (contrasts)) = true)")
brand += FOOTER

# Users.lex.tex
users = header("Users", f"{PRODUCT} context of use", [M, "Architecture", "Features"])
users += defn("authority", f"{M}::Authority",
              f"(record {M}::Authority {{framework := {q('ISO 9241-11:2018 context of use and usability; ISO 9241-210:2019 human-centred design')}, repository := {q(CONFIG['repository'])}, commit := {q('the revision of this model')}}})")
users += defn("groups", f"(List {M}::UserGroup)", lst(
    [f"(record {M}::UserGroup {{stakeholder := (Architecture::{s}), characteristics := {q(c)}}})" for s, c in groups], f"{M}::UserGroup"))
users += defn("goals", f"(List {M}::Goal)", lst(
    [f"(record {M}::Goal {{user := {u}, journey := (Features::{j}), outcome := {q(o)}}})" for u, j, o, _ in goals], f"{M}::Goal"))
users += defn("tasks", f"(List {M}::Task)", lst(
    [f"(record {M}::Task {{goal := {g}, order := {o}, feature := (Features::{f})}})" for g, o, f, _ in tasks], f"{M}::Task"))
users += defn("environments", f"(List {M}::Environment)", lst(
    [f"(record {M}::Environment {{kind := (ctor {M}::EnvironmentKind.technical), component := (Architecture::{n})}})" for n, _, _ in environments],
    f"{M}::Environment"))
measures = []
for g, (_, journey, _, steps) in enumerate(goals):
    accepted = sum(t[3] for t in tasks if t[0] == g)
    measures.append((g, "effectiveness", f"(ctor Option.some [Nat] {accepted})",
                     "acceptance rows of the goal's tasks: each is a test that the task's feature completes"))
    measures.append((g, "efficiency", f"(ctor Option.some [Nat] {len(steps)})",
                     "tasks the goal takes, from its journey's steps"))
    measures.append((g, "satisfaction", "(ctor Option.none [Nat])",
                     "open: no instrument in the model measures it"))
users += defn("measures", f"(List {M}::UsabilityMeasure)", lst(
    [f"(record {M}::UsabilityMeasure {{goal := {g}, outcome := (ctor {M}::UsabilityOutcome.{o}), observed := {v}, method := {q(m)}}})"
     for g, o, v, m in measures], f"{M}::UsabilityMeasure"))
users += theorem("everyGoalHasItsUser", f"(({M}::goalsOfGroups (goals) {len(groups)}) = true)")
users += theorem("everyTaskServesAGoal", f"(({M}::tasksOfGoals (tasks) {len(goals)}) = true)")
users += theorem("everyTaskIsAccepted", f"(({M}::tasksAccepted (tasks)) = true)")
users += theorem("everyMeasureNamesAGoal", f"(({M}::measuresOfGoals (measures) {len(goals)}) = true)")
users += FOOTER


def write(path, text):
    path = OUT / path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


write("app/src/Foundation/Experience/V1/Model.lex.tex", model)
write("app/src/Brand.lex.tex", brand)
write("app/src/Users.lex.tex", users)

ENTRY = """spec = "lexlean/entry/1"
id = "{id}"
category = "label-word"
surface_arity = 0
frame = "atom"

[denotation]
kind = "document"
module = "{module}"
component = "{component}"

[[form]]
id = "{id}"
channel = "text"
surface = "{surface}"
canonical_source = true
features = ["sentence-case", "singular"]
"""
for module, component, surface in ((M, "BrandElement", "Brand, design and context of use"),
                                   ("Brand", "elements", f"{PRODUCT} brand identity"),
                                   ("Users", "groups", f"{PRODUCT} context of use")):
    ident = re.sub(r"[^a-z0-9]+", "-", surface.lower()).strip("-")
    write(f"app/language/prism.arch/entries/{ident}.toml",
          ENTRY.format(id=ident, module=module, component=component, surface=surface))

# brand/tokens.json in the W3C DTCG format, and the stylesheet made from it.
DTCG = {"color": "color", "dimension": "dimension", "number": "number", "gradient": "gradient", "fontFamily": "fontFamily"}
doc = {"$description": f"uor.foundation design tokens, read from {auth['repository']}@{auth['commit']} {CSS[len('brand/foundation/'):]} by scripts/experience/experience.py. W3C Design Tokens Format 2025.10.", "fd": {}}
for name, kind, value, hexv, source in tokens:
    entry = {"$type": DTCG[kind]}
    if kind == "color":
        entry["$value"] = {"colorSpace": "hsl", "components": [float(x.rstrip("%")) for x in value.split()], "hex": hexv}
    elif kind == "fontFamily":
        entry["$value"] = value.split(", ")
    elif kind == "gradient":
        entry["$value"] = [{"color": h, "position": p} for h, p in gradient_stops(value)]
    else:
        entry["$value"] = value
    s = SOURCES[source]
    entry["$extensions"] = {"foundation.uor.source": {"path": s[1], "lines": [s[2], s[3]], "kappa": s[4]}}
    doc["fd"][name] = entry
write("brand/tokens.json", json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
css_out = [f"/* uor.foundation design tokens, generated from brand/tokens.json by\n   scripts/experience/experience.py. Do not edit: run just experience. */",
           ":root, .foundation {"]
for name, kind, value, _, _ in tokens:
    if kind == "fontFamily":
        value = ", ".join(f'"{f}"' if " " in f else f for f in value.split(", "))
    css_out.append(f"  --{name}: {value};")
css_out.append("  color-scheme: dark;\n}")
write("brand/foundation.css", "\n".join(css_out) + "\n")

print(f"experience: {len(elements)} brand elements, {len(tokens)} tokens, {len(contrasts)} contrasts, "
      f"{len(groups)} user groups, {len(goals)} goals, {len(tasks)} tasks, {len(environments)} environments, "
      f"{len(SOURCES)} sources")
short = [c for c in contrasts if c[4] < c[5]]
for fg, bg, _, criterion, observed, required, _ in short:
    print(f"experience: --{fg} on --{bg} is {observed / SCALE:.3f}:1, under {WCAG[criterion][0]} ({required / SCALE}:1)")
if short:
    # A brand the theorem everyContrastMeetsItsCriterion would refute: the
    # writer fails, so the pin that brought it is not kept.
    raise SystemExit(3)
