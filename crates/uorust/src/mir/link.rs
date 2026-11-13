//! Calls between core's own bodies, in core's scope: a call of a function
//! core defines is that function's own MIR, instantiated at the call's
//! types and lowered as one definition its callers share, as a std body is
//! on the workspace's path. Where core bottoms out in an intrinsic with no
//! body of its own, the call is the intrinsic's model, keyed by the
//! signature core declares it with: `size_of::<T>()` is `T`'s size in the
//! target data layout, and `const_eval_select` is the function it selects
//! at run time.

use std::collections::BTreeMap;

use super::core::CoreDecls;
use super::parse::{split_path, split_top};
use super::stdlib::{substitute, unify, Instance};

/// Core's bodies and declarations, for one census.
pub(crate) struct CoreLink {
    pub(crate) decls: CoreDecls,
    /// Each body's MIR text, by the path its dump names it by: a macro's
    /// impls share the path of the macro's text.
    pub(crate) bodies: BTreeMap<String, Vec<String>>,
    /// For each last segment of a body's path, the paths that have it: MIR
    /// prints a path by its last segment where no other item has it.
    pub(crate) lasts: BTreeMap<String, Vec<String>>,
    /// Each impl MIR names `<impl at span>`, by its span: the last segment
    /// of the trait it implements (`None` for an inherent impl) and the
    /// type it implements it for, as its source states them. A derive's
    /// span is the trait's name alone, and a macro's names its type by a
    /// metavariable: either type is `_`, for the types its bodies name. A
    /// blanket impl's, for a parameter of its own, is `*`, every type.
    pub(crate) impls: BTreeMap<String, (Option<String>, String)>,
    /// For each impl whose type is `_`, the names its bodies' signatures
    /// mention: among them, the types a macro's or a derive's impls are
    /// for.
    pub(crate) wild: BTreeMap<String, std::collections::BTreeSet<String>>,
    /// For each impl, the methods its block declares `default fn`, which
    /// an impl that specializes it replaces.
    defaults: BTreeMap<String, std::collections::BTreeSet<String>>,
    /// The paths of inherent methods named by their impl's type
    /// (`time::Duration::saturating_sub`): methods, not free functions.
    methods: std::collections::BTreeSet<String>,
    /// For a macro's impls whose trait is a metavariable, each body's
    /// trait, where its expansion order tells it.
    traits: BTreeMap<String, Vec<Option<String>>>,
    /// Each body's impl's index (`{impl#N}`) for each of its texts, in
    /// the order `bodies` holds them.
    index: BTreeMap<String, Vec<Option<u64>>>,
    /// The root of core's sources (rust-src), whose `library/core/src`
    /// declares the constant items no dump holds.
    root: std::path::PathBuf,
    /// Every type a local of any of core's bodies has, as its dump prints
    /// it: the instances a call into another module's body needs in scope.
    pub(crate) locals: std::collections::BTreeSet<String>,
}

impl CoreLink {
    /// The link of core's dumps `texts` (one body each), with core's
    /// declarations.
    pub(crate) fn new<'t>(
        decls: CoreDecls,
        root: &std::path::Path,
        texts: impl IntoIterator<Item = (&'t str, &'t String)>,
    ) -> Self {
        let mut bodies: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut lasts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // Each body's impl's index among its module's impls, `{impl#N}` in
        // the name of the file rustc dumps it to: the order the impls'
        // items are made in, a macro's in the order of its expansions.
        let mut index: BTreeMap<String, Vec<Option<u64>>> = BTreeMap::new();
        for (name, text) in texts {
            // A dump names its body `// MIR for `path` before ..`.
            let Some(path) = text
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("// MIR for `"))
                .and_then(|r| Some(&r[..r.find("` ")?]))
            else {
                continue;
            };
            if path.contains("{closure") || path.contains("promoted[") {
                continue;
            }
            let last = split_path(path).last().copied().unwrap_or(path).to_string();
            let same = bodies.entry(path.to_string()).or_default();
            if same.is_empty() {
                lasts.entry(last).or_default().push(path.to_string());
            }
            same.push(text.clone());
            let n = name.rfind("{impl#").and_then(|at| {
                let r = &name[at + "{impl#".len()..];
                r[..r.find('}')?].parse().ok()
            });
            index.entry(path.to_string()).or_default().push(n);
        }
        // A method of an inherent impl, as MIR prints its calls: by the
        // impl's type (`fmt::Formatter::write_str`), where its dump names it
        // by the impl's span. A trait's impl keeps its span alone: its calls
        // name the trait.
        let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut aliases: Vec<(String, Vec<String>)> = Vec::new();
        for (path, texts) in &bodies {
            let mut inherent = path.contains("<impl at ");
            let mut rest = path.as_str();
            while let Some(at) = rest.find("<impl at ") {
                let after = &rest[at + "<impl at ".len()..];
                let Some(end) = after.find('>') else {
                    inherent = false;
                    break;
                };
                let span = &after[..end];
                rest = &after[end + 1..];
                let header = span.split_once(':').and_then(|(file, range)| {
                    let (from, to) = range.split_once(": ")?;
                    let (l1, c1) = from.split_once(':')?;
                    let (l2, c2) = to.split_once(':')?;
                    let (l1, c1, l2, c2): (usize, usize, usize, usize) = (
                        l1.parse().ok()?,
                        c1.parse().ok()?,
                        l2.parse().ok()?,
                        c2.parse().ok()?,
                    );
                    let lines = sources.entry(file.to_string()).or_insert_with(|| {
                        std::fs::read_to_string(root.join(file))
                            .map(|t| t.lines().map(str::to_string).collect())
                            .unwrap_or_default()
                    });
                    let mut text = String::new();
                    for l in l1..=l2 {
                        let chars: Vec<char> = lines.get(l.checked_sub(1)?)?.chars().collect();
                        let from = if l == l1 { c1.saturating_sub(1) } else { 0 };
                        let to = if l == l2 {
                            c2.saturating_sub(1).min(chars.len())
                        } else {
                            chars.len()
                        };
                        text.extend(chars.get(from..to)?);
                        text.push(' ');
                    }
                    Some(text)
                });
                match header {
                    Some(h) if h.trim_start().starts_with("impl") && !h.contains(" for ") => {}
                    _ => inherent = false,
                }
            }
            if inherent {
                if let Some(resolved) = super::resolve_impls(root, path) {
                    aliases.push((resolved, texts.clone()));
                }
            }
        }
        let mut methods = std::collections::BTreeSet::new();
        for (resolved, texts) in aliases {
            methods.insert(resolved.clone());
            bodies.entry(resolved).or_insert(texts);
        }
        // Each impl's header, read from the source its span names.
        let mut impls = BTreeMap::new();
        let mut defaults: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
        for path in bodies.keys() {
            let mut rest = path.as_str();
            while let Some(at) = rest.find("<impl at ") {
                let after = &rest[at + "<impl at ".len()..];
                let Some(end) = after.find('>') else {
                    break;
                };
                let span = &after[..end];
                rest = &after[end + 1..];
                if impls.contains_key(span) {
                    continue;
                }
                // `file:l1:c1: l2:c2`, lines and columns from one.
                let Some((file, range)) = span.split_once(':') else {
                    continue;
                };
                let nums: Vec<usize> = range
                    .split([':', ' '])
                    .filter(|n| !n.is_empty())
                    .filter_map(|n| n.parse().ok())
                    .collect();
                let [l1, c1, l2, c2] = nums.as_slice() else {
                    continue;
                };
                let lines = sources.entry(file.to_string()).or_insert_with(|| {
                    std::fs::read_to_string(root.join(file))
                        .unwrap_or_default()
                        .lines()
                        .map(str::to_string)
                        .collect()
                });
                let mut src = String::new();
                for l in *l1..=*l2 {
                    let chars: Vec<char> =
                        lines.get(l - 1).map_or(Vec::new(), |t| t.chars().collect());
                    let from = if l == *l1 { c1 - 1 } else { 0 };
                    let to = if l == *l2 {
                        (c2 - 1).min(chars.len())
                    } else {
                        chars.len()
                    };
                    src.extend(chars.get(from.min(to)..to).into_iter().flatten());
                    src.push(' ');
                }
                // The methods the impl's block declares `default`: an impl
                // another specializes.
                let mut block = String::new();
                let mut depth = 0i64;
                'scan: for (n, line) in lines.iter().enumerate().skip(l2 - 1) {
                    let from = if n + 1 == *l2 { c2 - 1 } else { 0 };
                    for c in line.chars().skip(from) {
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break 'scan;
                                }
                            }
                            _ => {}
                        }
                        if depth > 0 {
                            block.push(c);
                        }
                    }
                    block.push(' ');
                }
                let words: Vec<&str> = block.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "default" {
                        let rest = &words[i + 1..];
                        let at = rest.iter().position(|w| *w == "fn").filter(|at| *at <= 2);
                        if let Some(name) = at.and_then(|at| rest.get(at + 1)) {
                            let name = name.split(['(', '<']).next().unwrap_or(name);
                            defaults
                                .entry(span.to_string())
                                .or_default()
                                .insert(name.to_string());
                        }
                    }
                }
                let head = src.trim();
                let head = head.strip_prefix("unsafe ").unwrap_or(head);
                let entry = match head.strip_prefix("impl") {
                    Some(h) if !h.starts_with(|c: char| c.is_alphanumeric() || c == '_') => {
                        // `impl<'a, T: X>`: the impl's own generics skipped.
                        let mut depth = 0i64;
                        let mut start = 0;
                        if h.trim_start().starts_with('<') {
                            for (i, c) in h.char_indices() {
                                match c {
                                    '<' => depth += 1,
                                    '>' if depth > 0 && !h[..i].ends_with('-') => {
                                        depth -= 1;
                                        if depth == 0 {
                                            start = i + 1;
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        let params = &h[..start];
                        let h = h[start..].split(" where").next().unwrap_or("").trim();
                        let h = h.strip_prefix("const ").unwrap_or(h).trim();
                        match h.split_once(" for ") {
                            Some((tr, ty)) => {
                                let tr = tr.trim().trim_start_matches('!');
                                let tr = tr.split('<').next().unwrap_or(tr);
                                let tr = tr.rsplit("::").next().unwrap_or(tr).trim();
                                // A macro's trait named by a metavariable is
                                // any trait.
                                let tr = if tr.contains('$') { "_" } else { tr };
                                let ty = ty.trim();
                                // A blanket impl, for a parameter of its
                                // own, is for every type: `*`.
                                let blanket = params
                                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                                    .any(|w| !w.is_empty() && w == ty);
                                // A macro's type is `_` where its head is a
                                // metavariable; `NonZero<$Int>` keeps its own.
                                let meta = ty.split('<').next().unwrap_or(ty).contains('$');
                                let ty = if meta {
                                    "_"
                                } else if blanket {
                                    "*"
                                } else {
                                    ty
                                };
                                (Some(tr.to_string()), ty.to_string())
                            }
                            None if h.split('<').next().unwrap_or(h).contains('$') => {
                                (None, "_".to_string())
                            }
                            None => (None, h.to_string()),
                        }
                    }
                    // A derive's span: the trait's name.
                    _ => (
                        Some(head.rsplit("::").next().unwrap_or(head).to_string()),
                        "_".to_string(),
                    ),
                };
                impls.insert(span.to_string(), entry);
            }
        }
        let mut wild: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
        for (path, texts) in &bodies {
            let Some(at) = path.rfind("<impl at ") else {
                continue;
            };
            let after = &path[at + "<impl at ".len()..];
            let Some(span) = after.find('>').map(|e| &after[..e]) else {
                continue;
            };
            if impls.get(span).is_none_or(|(_, ty)| ty != "_") {
                continue;
            }
            for text in texts {
                let Some(head) = text.lines().find_map(|l| l.strip_prefix("fn ")) else {
                    continue;
                };
                // Every name its signature's types mention: a body that
                // takes no `self` may name its type only inside another
                // (`Result<Self, E>`).
                let types = head.get(head.find('(').unwrap_or(0)..).unwrap_or("");
                let types = types.split(|c: char| !(c.is_alphanumeric() || c == '_'));
                wild.entry(span.to_string())
                    .or_default()
                    .extend(types.filter(|w| !w.is_empty()).map(str::to_string));
            }
        }
        // The trait of each of the impls a macro writes with its trait a
        // metavariable (`impl fmt::$Trait for $Unsigned`), which MIR names
        // by the one span: the macro's invocations in its file, in order,
        // bind the metavariable; for one type, its impls' indices, in
        // order, are those invocations' that are for it.
        let mut traits: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
        // A text's tokens: a word, a literal, `$name` (its fragment kind
        // dropped), or one punctuation character.
        let tokens = |t: &str| -> Vec<String> {
            let cs: Vec<char> = t.chars().collect();
            let word = |c: char| c.is_alphanumeric() || c == '_';
            let mut out = Vec::new();
            let mut i = 0;
            while i < cs.len() {
                let c = cs[i];
                if c.is_whitespace() {
                    i += 1;
                } else if c == '"' || (c == 'b' && cs.get(i + 1) == Some(&'"')) {
                    let start = i;
                    i += if c == 'b' { 2 } else { 1 };
                    while i < cs.len() && cs[i] != '"' {
                        i += if cs[i] == '\\' { 2 } else { 1 };
                    }
                    i += 1;
                    out.push(cs[start..i.min(cs.len())].iter().collect());
                } else if word(c) || (c == '$' && cs.get(i + 1).is_some_and(|c| word(*c))) {
                    let start = i;
                    i += 1;
                    while i < cs.len() && word(cs[i]) {
                        i += 1;
                    }
                    out.push(cs[start..i].iter().collect());
                    // `$name:kind`: the kind is the matcher's, not a token.
                    if c == '$'
                        && cs.get(i) == Some(&':')
                        && cs.get(i + 1).is_some_and(|c| word(*c))
                    {
                        i += 1;
                        while i < cs.len() && word(cs[i]) {
                            i += 1;
                        }
                    }
                } else {
                    out.push(c.to_string());
                    i += 1;
                }
            }
            out
        };
        for (path, texts) in &bodies {
            let Some(at) = path.rfind("<impl at ") else {
                continue;
            };
            let after = &path[at + "<impl at ".len()..];
            let Some(span) = after.find('>').map(|e| &after[..e]) else {
                continue;
            };
            let (Some((Some(tr), _)), Some(idx)) = (impls.get(span), index.get(path)) else {
                continue;
            };
            if tr != "_" || texts.len() < 2 {
                continue;
            }
            let Some((file, range)) = span.split_once(':') else {
                continue;
            };
            let Some(l1) = range
                .split(':')
                .next()
                .and_then(|n| n.parse::<usize>().ok())
            else {
                continue;
            };
            let Some(lines) = sources.get(file) else {
                continue;
            };
            let Some(header) = lines.get(l1.saturating_sub(1)) else {
                continue;
            };
            // The impl's trait and type, as metavariables or tokens.
            let head = tokens(header);
            let Some(for_at) = head.iter().position(|t| t == "for") else {
                continue;
            };
            let Some(trait_var) = head[..for_at].iter().find(|t| t.starts_with('$')).cloned()
            else {
                continue;
            };
            let type_var = head.get(for_at + 1).filter(|t| t.starts_with('$')).cloned();
            // The macro it is in, and the matcher of the arm before it.
            let Some(def) = (0..l1).rev().find(|n| lines[*n].contains("macro_rules!")) else {
                continue;
            };
            let name = tokens(&lines[def]).get(2).cloned().unwrap_or_default();
            let Some(arm) = (def..l1)
                .rev()
                .find(|n| lines[*n].trim_start().starts_with('(') && lines[*n].contains("=>"))
            else {
                continue;
            };
            let matcher = tokens(&lines[arm]);
            let matcher = &matcher[1..matcher.iter().rposition(|t| t == ")").unwrap_or(1)];
            let Some(ti) = matcher.iter().position(|t| *t == trait_var) else {
                continue;
            };
            let yi = type_var.and_then(|v| matcher.iter().position(|t| *t == v));
            // Each invocation's trait, and its type where it names one.
            let mut calls: Vec<(String, Option<String>)> = Vec::new();
            for (n, line) in lines.iter().enumerate() {
                let ts = tokens(line);
                let Some(bang) = ts.windows(2).position(|w| w[0] == name && w[1] == "!") else {
                    continue;
                };
                if n == def
                    || !matches!(ts.get(bang + 2).map(String::as_str), Some("{" | "(" | "["))
                {
                    continue;
                }
                let args = &ts[bang + 3..];
                // A matcher that is one repetition, `$($tr:ident),*`: each
                // of the invocation's items, split at the separator outside
                // brackets, binds the metavariable once, in order.
                let close = (matcher.first().map(String::as_str) == Some("$")
                    && matcher.get(1).map(String::as_str) == Some("("))
                .then(|| {
                    let mut depth = 0i64;
                    matcher.iter().enumerate().skip(1).find_map(|(j, t)| {
                        depth += match t.as_str() {
                            "(" | "[" | "{" => 1,
                            ")" | "]" | "}" => -1,
                            _ => 0,
                        };
                        (depth == 0).then_some(j)
                    })
                })
                .flatten()
                .filter(|c| ti > 1 && ti < *c);
                let items: Vec<&[String]> = match close {
                    Some(c) => {
                        let sep = matcher
                            .get(c + 1)
                            .filter(|t| !matches!(t.as_str(), "*" | "+" | "?"));
                        let mut depth = 0i64;
                        let mut out = Vec::new();
                        let mut from = 0;
                        for (j, a) in args.iter().enumerate() {
                            match a.as_str() {
                                "(" | "[" | "{" => depth += 1,
                                ")" | "]" | "}" if depth == 0 => {
                                    out.push(&args[from..j]);
                                    from = args.len();
                                    break;
                                }
                                ")" | "]" | "}" => depth -= 1,
                                _ if depth == 0 && Some(a) == sep => {
                                    out.push(&args[from..j]);
                                    from = j + 1;
                                }
                                _ => {}
                            }
                        }
                        if from < args.len() {
                            out.push(&args[from..]);
                        }
                        out.into_iter().filter(|i| !i.is_empty()).collect()
                    }
                    None => vec![args],
                };
                // Within a repetition, the metavariables' places in it.
                let (skip, ti) = close.map_or((0, ti), |_| (2, ti - 2));
                let yi = yi.and_then(|i| i.checked_sub(skip));
                for args in items {
                    // The tokens before the trait's are the matcher's literals.
                    let literal = matcher[skip..skip + ti]
                        .iter()
                        .zip(args)
                        .all(|(m, a)| m.starts_with('$') || m == a);
                    if let (true, Some(t)) = (literal, args.get(ti)) {
                        let ty = yi.and_then(|i| args.get(i)).filter(|t| !t.starts_with('$'));
                        calls.push((t.clone(), ty.cloned()));
                    }
                }
            }
            // Each body's type: its first parameter's, `&` dropped.
            let ty_of = |text: &String| -> Option<String> {
                let head = text.lines().find_map(|l| l.strip_prefix("fn "))?;
                let first = head[head.find('(')? + 1..].split([',', ')']).next()?;
                let t = first
                    .split_once(": ")?
                    .1
                    .trim_start_matches('&')
                    .trim_start_matches("mut ");
                Some(t.rsplit("::").next().unwrap_or(t).to_string())
            };
            let mut tags = vec![None; texts.len()];
            let types: std::collections::BTreeSet<Option<String>> =
                texts.iter().map(ty_of).collect();
            for t in types {
                let Some(t) = t else { continue };
                let mut own: Vec<(u64, usize)> = texts
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| ty_of(x).as_deref() == Some(t.as_str()))
                    .filter_map(|(i, _)| Some((idx.get(i).copied().flatten()?, i)))
                    .collect();
                own.sort_unstable();
                let theirs: Vec<&String> = calls
                    .iter()
                    .filter(|(_, ty)| ty.as_ref().is_none_or(|ty| *ty == t))
                    .map(|(tr, _)| tr)
                    .collect();
                if own.len() == theirs.len() {
                    for ((_, i), tr) in own.iter().zip(theirs) {
                        tags[*i] = Some(tr.clone());
                    }
                }
            }
            traits.insert(path.clone(), tags);
        }
        let locals = bodies
            .values()
            .flatten()
            .flat_map(|t| t.lines())
            .filter_map(|l| {
                let l = l.trim().strip_prefix("let ")?;
                let l = l.strip_prefix("mut ").unwrap_or(l);
                Some(l.strip_suffix(';')?.split_once(": ")?.1.to_string())
            })
            .collect();
        CoreLink {
            decls,
            bodies,
            lasts,
            impls,
            wild,
            defaults,
            methods,
            traits,
            index,
            root: root.to_path_buf(),
            locals,
        }
    }

    /// The path of the body a call names, as MIR prints it with its
    /// generic arguments erased.
    pub(crate) fn path(&self, erased: &str) -> Option<String> {
        if self.bodies.contains_key(erased) || self.decls.intrinsics.contains(erased) {
            return Some(erased.to_string());
        }
        let tail = format!("::{erased}");
        let last = split_path(erased).last().copied().unwrap_or(erased);
        if let Some([one]) = self.lasts.get(last).map(Vec::as_slice) {
            if one.ends_with(&tail) {
                return Some(one.clone());
            }
        }
        // A bare name, as MIR prints a free function its caller has in
        // scope: the one free function or intrinsic core declares by it,
        // where methods of the same name are not free.
        if !erased.contains("::") {
            let mut free = self.decls.generics.keys().filter(|p| {
                split_path(p).last() == Some(&erased)
                    && !self.methods.contains(*p)
                    && (self.bodies.contains_key(*p) || self.decls.intrinsics.contains(*p))
            });
            if let (Some(p), None) = (free.next(), free.next()) {
                return Some(p.clone());
            }
        }
        // A path MIR prints by its last segments, which one body's path ends.
        let mut ends = self.bodies.keys().filter(|p| p.ends_with(&tail));
        match (ends.next(), ends.next()) {
            (Some(p), None) => Some(p.clone()),
            _ => None,
        }
    }

    /// The generic parameters of `path` bound by a call `callee` with
    /// arguments and destination of the types MIR prints: the call's type
    /// arguments in the order the signature declares the parameters, then
    /// what the arguments' types bind.
    pub(crate) fn bind(
        &self,
        path: &str,
        callee: &str,
        head: Option<(&[&str], &str)>,
        arg_texts: &[Option<String>],
        dest: &str,
    ) -> Option<BTreeMap<String, String>> {
        let mut generics = BTreeMap::new();
        // The type arguments of the callee's last segment, `size_of::<u8>`
        // binding `u8`.
        let last = split_path(callee).last().copied().unwrap_or_default();
        let explicit: Vec<String> = match last.find("::<") {
            Some(at) => split_top(last[at + 3..].strip_suffix('>').unwrap_or(""), ',')
                .into_iter()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty() && !a.starts_with('\''))
                .collect(),
            None => Vec::new(),
        };
        // A method's own parameters, which core's declarations do not
        // list, are bound by its arguments' types alone.
        if let (false, Some(names)) = (explicit.is_empty(), self.decls.generics.get(path)) {
            if names.len() != explicit.len() {
                return None;
            }
            generics.extend(names.iter().cloned().zip(explicit));
        }
        if let Some((params, ret)) = head {
            if params.len() != arg_texts.len() {
                return None;
            }
            for (p, a) in params.iter().zip(arg_texts) {
                let (_, pt) = p.split_once(": ")?;
                if let Some(a) = a {
                    unify(pt, a, &mut generics)?;
                }
            }
            // A destination the call does not type binds nothing.
            if !dest.is_empty() {
                unify(ret, dest, &mut generics)?;
            }
        }
        // A trait's default method: the trait's parameters, which the
        // call's `<X as Trait<A>>` binds.
        let as_trait = callee
            .strip_prefix('<')
            .and_then(|r| r.rsplit_once(">::"))
            .and_then(|(q, _)| q.split_once(" as "))
            .map(|(_, tr)| tr);
        if let (Some(names), Some(tr)) = (
            path.rsplit_once("::")
                .and_then(|(t, _)| self.decls.trait_params.get(t)),
            as_trait,
        ) {
            let last = split_path(tr).last().copied().unwrap_or(tr);
            let args: Vec<String> = last
                .find('<')
                .and_then(|at| last[at + 1..].strip_suffix('>'))
                .map(|r| {
                    split_top(r, ',')
                        .into_iter()
                        .map(|a| a.trim().to_string())
                        .filter(|a| !a.is_empty() && !a.starts_with('\''))
                        .collect()
                })
                .unwrap_or_default();
            if names.len() == args.len() {
                for (n, a) in names.iter().zip(args) {
                    generics.entry(n.clone()).or_insert(a);
                }
            }
        }
        // MIR prints a type without the trailing arguments its defaults
        // give (`ControlFlow<bool>` for `ControlFlow<bool, ()>`): a
        // parameter the signature names in their place takes the default.
        if let Some((params, ret)) = head {
            let mut work: Vec<(String, String)> = params
                .iter()
                .zip(arg_texts)
                .filter_map(|(p, a)| Some((p.split_once(": ")?.1.to_string(), a.clone()?)))
                .collect();
            if !dest.is_empty() {
                work.push((ret.to_string(), dest.to_string()));
            }
            let parts = |t: &str| -> Option<(String, Vec<String>)> {
                let t = t.trim().trim_start_matches('&').trim_start_matches("mut ");
                let last = split_path(t).last().copied()?;
                let at = last.find('<')?;
                let args = split_top(last[at + 1..].strip_suffix('>')?, ',')
                    .into_iter()
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty() && !x.starts_with('\''))
                    .collect();
                Some((last[..at].trim_end_matches("::").to_string(), args))
            };
            while let Some((p, a)) = work.pop() {
                let (Some((pn, pa)), Some((an, aa))) = (parts(&p), parts(&a)) else {
                    continue;
                };
                if pn != an {
                    continue;
                }
                work.extend(pa.iter().cloned().zip(aa.iter().cloned()));
                let Some(Some(ds)) = self.decls.defaults.get(&pn) else {
                    continue;
                };
                for (j, x) in pa.iter().enumerate().skip(aa.len()) {
                    let bare = x.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                    if let (true, Some(Some(d))) = (bare && !generics.contains_key(x), ds.get(j)) {
                        let d = super::stdlib::substitute(d, &generics);
                        generics.insert(x.clone(), d);
                    }
                }
            }
        }
        // A type the signature names by its own name (`Argument<'_>`,
        // printed `fmt::rt::Argument<'_>` at the call) binds no parameter.
        generics.retain(|k, v| split_path(v).last().copied() != Some(k.as_str()));
        Some(generics)
    }

    /// Core's own body of `callee`, its generic parameters bound by the
    /// call's type arguments and the types of its arguments and
    /// destination; `None` when core has no body for it or a parameter its
    /// body names stays unbound.
    ///
    /// A method MIR names by its impl's type, `<X as Trait>::m` or
    /// `<impl X>::m`, is the one method `m` of an impl whose signature the
    /// call's types instantiate.
    pub(crate) fn instance(
        &self,
        callee: &str,
        arg_texts: &[Option<String>],
        dest: &str,
    ) -> Option<Instance> {
        let erased = super::lower::erase(callee);
        // A destination typed by a projection (`<char as Pattern>::Searcher<'_>`)
        // names no type the signature's own can be read against.
        let dest = if dest.trim_start().starts_with('<') && dest.contains(" as ") {
            ""
        } else {
            dest
        };
        let signature = |text: &str| -> Option<(Vec<String>, String)> {
            let head = text.lines().find_map(|l| l.strip_prefix("fn "))?;
            let open = head.find('(')?;
            let close = head.rfind(") -> ")?;
            let params = split_top(&head[open + 1..close], ',')
                .into_iter()
                .filter(|p| !p.trim().is_empty())
                .map(str::to_string)
                .collect();
            Some((
                params,
                head[close + ") -> ".len()..]
                    .trim_end_matches(" {")
                    .trim()
                    .to_string(),
            ))
        };
        // The type an impl is for, by its head: a path's last segment, or
        // the kind of a pointer, slice or array.
        let type_head = |t: &str| -> String {
            let t = t.trim();
            for k in ["*const ", "*mut ", "&mut ", "&", "[", "("] {
                if t.starts_with(k) {
                    return k.trim().to_string();
                }
            }
            let name = t.split('<').next().unwrap_or(t).trim_end_matches("::");
            name.rsplit("::").next().unwrap_or(name).to_string()
        };
        // The impl a method's path names it in: its last `<impl at span>`.
        let impl_of = |p: &str| -> Option<(String, &(Option<String>, String))> {
            let at = p.rfind("<impl at ")?;
            let after = &p[at + "<impl at ".len()..];
            let span = &after[..after.find('>')?];
            Some((span.to_string(), self.impls.get(span)?))
        };
        // Whether the impl at `span`, for `ty`, may be for a type of head
        // `x`: a `_` impl is for the types its bodies take.
        let is_for = |span: &str, ty: &str, x: &str| {
            if ty == "*" {
                true
            } else if ty == "_" {
                self.wild.get(span).is_none_or(|w| w.contains(x))
            } else {
                type_head(ty) == x
            }
        };
        let segs = split_path(&erased);
        let method = segs.last().copied().unwrap_or_default();
        // `<X as Trait>::m`: a method of an impl of `Trait`, never an
        // inherent one; `X::m` or `<impl X>::m`: a method of an inherent impl
        // of `X`'s type.
        let qualified = erased
            .strip_prefix('<')
            .and_then(|r| r.rsplit_once(">::"))
            .and_then(|(q, _)| q.split_once(" as "))
            .map(|(x, tr)| {
                let tr = tr.split('<').next().unwrap_or(tr);
                (
                    x.to_string(),
                    tr.rsplit("::").next().unwrap_or(tr).to_string(),
                )
            });
        let inherent: Option<String> = match segs.as_slice() {
            [.., x, _] if qualified.is_none() => Some(
                x.strip_prefix("<impl ")
                    .and_then(|r| r.strip_suffix('>'))
                    .map_or_else(|| type_head(x), type_head),
            ),
            _ => None,
        };
        let candidates = || -> Vec<String> {
            self.lasts
                .get(method)
                .into_iter()
                .flatten()
                .filter(|p| match (impl_of(p), &qualified, &inherent) {
                    (Some((span, (Some(t), ty))), Some((x, tr)), _) => {
                        (t == tr || t == "_") && is_for(&span, ty, &type_head(x))
                    }
                    (Some((span, (None, ty))), None, Some(x)) => is_for(&span, ty, x),
                    _ => false,
                })
                .cloned()
                .collect()
        };
        let paths: Vec<String> = match self.path(&erased) {
            Some(p) if qualified.is_none() => vec![p],
            _ => {
                let mut found = candidates();
                // An impl of the trait the call names is chosen over one of
                // a trait a macro names by a metavariable, both for `X`'s
                // own type; a blanket impl is weighed after fitting.
                let named =
                    |p: &String| impl_of(p).is_some_and(|(_, (t, _))| t.as_deref() != Some("_"));
                let blanket = |p: &String| impl_of(p).is_some_and(|(_, (_, ty))| ty == "*");
                if found.iter().any(|p| named(p) && !blanket(p)) {
                    found.retain(|p| named(p) || blanket(p));
                }
                // A specializing impl is chosen over the impl whose
                // `default fn` it replaces.
                let general = |p: &String| {
                    impl_of(p).is_some_and(|(span, _)| {
                        self.defaults.get(&span).is_some_and(|d| d.contains(method))
                    })
                };
                if found.iter().any(|p| !general(p)) {
                    found.retain(|p| !general(p));
                }
                // An impl that does not define `m` has the trait's provided
                // body, its `Self` the impl's type.
                if let (true, Some((x, tr))) = (found.is_empty(), &qualified) {
                    let implemented = self.impls.iter().any(|(span, (t, ty))| {
                        matches!(t.as_deref(), Some(t) if t == tr || t == "_")
                            && is_for(span, ty, &type_head(x))
                    });
                    if implemented {
                        found = self
                            .lasts
                            .get(method)
                            .into_iter()
                            .flatten()
                            .filter(|p| {
                                let s = split_path(p);
                                !p.contains("<impl") && s.len() >= 2 && s[s.len() - 2] == tr
                            })
                            .cloned()
                            .collect();
                    }
                }
                found
            }
        };
        let named_trait = qualified.as_ref().map(|(_, tr)| tr.clone());
        let fits = paths.iter().flat_map(|p| {
            let tags = self.traits.get(p);
            let named_trait = named_trait.clone();
            self.bodies
                .get(p)
                .into_iter()
                .flatten()
                .enumerate()
                // A body whose trait its expansion tells is the named
                // trait's only.
                .filter(move |(i, _)| {
                    match (
                        tags.and_then(|t| t.get(*i)).cloned().flatten(),
                        &named_trait,
                    ) {
                        (Some(t), Some(n)) => t == *n,
                        _ => true,
                    }
                })
                .map(|(_, text)| text)
                .filter_map(move |text| {
                    let (params, ret) = signature(text)?;
                    let params: Vec<&str> = params.iter().map(String::as_str).collect();
                    let g = self.bind(p, callee, Some((&params, &ret)), arg_texts, dest)?;
                    // A name the signature binds is one of the body's own
                    // parameters, the bare names its locals' types mention, not
                    // a type rustc prints by its bare name, or one the source
                    // declares it with.
                    // The bare names of its signature's types too, which a
                    // body may mention nowhere else.
                    let mut sig = String::new();
                    for t in params
                        .iter()
                        .filter_map(|p| p.split_once(": "))
                        .map(|(_, t)| t)
                    {
                        sig.push_str(&format!("let _: {t};\n"));
                    }
                    sig.push_str(&format!("let _: {ret};\n"));
                    let mut own = super::core::canonical_bindings(text, &|_| false, &|_| None);
                    own.extend(super::core::canonical_bindings(&sig, &|_| false, &|_| None));
                    let declares = self.decls.generics.get(p.as_str());
                    // A default method's trait's parameters, which bind
                    // only where the body names them.
                    let traits = p
                        .rsplit_once("::")
                        .and_then(|(t, _)| self.decls.trait_params.get(t));
                    let mut g = g;
                    g.retain(|k, _| !traits.is_some_and(|d| d.contains(k)) || own.contains_key(k));
                    let param = |k: &String| {
                        own.contains_key(k)
                            || k == "Self"
                            || declares.is_some_and(|d| d.contains(k))
                    };
                    // A type rustc prints by its bare name binds itself:
                    // `ByteStr` is `bstr::ByteStr`.
                    let itself = |k: &String, v: &String| {
                        let v = v.trim_start_matches('&').trim_start_matches("mut ");
                        let v = v.split('<').next().unwrap_or(v);
                        v.rsplit("::").next() == Some(k.as_str())
                    };
                    let declared = g.iter().any(|(k, v)| {
                        !itself(k, v)
                            && (!param(k) || self.decls.structs.iter().any(|s| s.name() == k))
                    });
                    g.retain(|k, v| param(k) && !itself(k, v));
                    // A type the signature prints by its path is the type of that
                    // path: `range::RangeInclusive<T>` is not
                    // `ops::range::RangeInclusive<usize>`.
                    // With whether it is a struct core declares.
                    let head = |t: &str| -> Option<(String, bool)> {
                        let t = t
                            .trim()
                            .trim_start_matches('&')
                            .trim_start_matches("mut ")
                            .trim();
                        let name = &t[..t.find('<').unwrap_or(t.len())];
                        if !name.contains("::") {
                            return None;
                        }
                        // The struct core declares at that path, or the one
                        // whose path it ends: `alignment::Alignment` is
                        // `mem::alignment::Alignment`.
                        let tail = format!("::{name}");
                        let structs = &self.decls.structs;
                        if structs.iter().any(|s| s.path == name) {
                            return Some((name.to_string(), true));
                        }
                        let mut ends = structs.iter().filter(|s| s.path.ends_with(&tail));
                        Some(match (ends.next(), ends.next()) {
                            (Some(s), None) => (s.path.clone(), true),
                            _ => (name.to_string(), false),
                        })
                    };
                    let pairs = params
                        .iter()
                        .filter_map(|p| p.split_once(": ").map(|(_, t)| t))
                        .zip(arg_texts.iter())
                        .filter_map(|(p, a)| a.as_deref().map(|a| (p, a)))
                        .chain(std::iter::once((ret.as_str(), dest)));
                    let mut differ = false;
                    for (p, a) in pairs {
                        // A type that is no struct (a union, a type a macro
                        // declares) is the one its shorter path ends.
                        if let (Some((x, kx)), Some((y, ky))) = (head(p), head(a)) {
                            let ends =
                                x.ends_with(&format!("::{y}")) || y.ends_with(&format!("::{x}"));
                            differ |= x != y && (kx && ky || !ends);
                        }
                    }
                    (!declared && !differ).then(|| (p.clone(), text, g))
                })
        });
        let mut fits: Vec<_> = fits.collect();
        // Of the impls whose signatures take the call's arguments, one for
        // `X`'s own type is chosen over a blanket impl, whose bounds the
        // link does not check (`impl<F: FnPtr> Ord for F` is not `bool`'s).
        // A blanket impl the call's arguments alone fit is the impl: the
        // trait's arguments, which the printed callee erases, rule out the
        // others (`impl<T> From<T> for T` where `X: From<!>` is no fit).
        let blanket = |p: &String| impl_of(p).is_some_and(|(_, (_, ty))| ty == "*");
        if fits.iter().any(|(p, _, _)| !blanket(p)) {
            fits.retain(|(p, _, _)| !blanket(p));
        }
        let (path, text, generics) = match fits.len() {
            1 => fits.remove(0),
            _ => return None,
        };
        // Every parameter the signature declares is bound.
        if let Some(names) = self.decls.generics.get(&path) {
            if names.iter().any(|n| !generics.contains_key(n)) {
                return None;
            }
        }
        let mut out = String::new();
        for line in text.lines() {
            out.push_str(&substitute(line, &generics));
            out.push('\n');
        }
        let mut bound: Vec<(String, String)> = generics.into_iter().collect();
        // Impls a macro writes share its path: each is named by the types
        // its signature takes too.
        if self.bodies.get(&path).is_some_and(|b| b.len() > 1) {
            let (params, _) = signature(&out)?;
            for p in params {
                let (_, t) = p.split_once(": ")?;
                bound.push(("Self".to_string(), t.trim_start_matches('&').to_string()));
            }
        }
        Some(Instance {
            path,
            text: out,
            bound,
        })
    }

    /// The body of the constant item `<X as Trait>::C` that the trait
    /// provides, with `Self` as `X`.
    pub(crate) fn constant(&self, printed: &str) -> Option<Instance> {
        // A constant item of another module, by the path its dump names.
        if !printed.starts_with('<') {
            // A primitive's inherent impl MIR prints `<impl char>`, where
            // the link names its items by the impl's type alone.
            let alias = || {
                let (head, rest) = printed.split_once("<impl ")?;
                let (x, tail) = rest.split_once('>')?;
                self.bodies.get(&format!("{head}{x}{tail}"))
            };
            // A constant item no dump holds (rustc evaluates it where
            // it is used): its declaration in core's source, where that
            // is a literal of a primitive type (`const TAG_CONT: u8 =
            // 0b1000_0000;`), as the body MIR would print for it. Its
            // module's file is the one the path's longest module
            // prefix names; an item of the module is declared at its
            // top level, one of a function or an impl (`<impl char>::
            // MAX`) inside, the file's one such declaration.
            let from_source = || -> Option<Instance> {
                let segs = split_path(printed);
                let (name, module) = segs.split_last()?;
                let src = self.root.join("library/core/src");
                let (k, file) = (1..=module.len()).rev().find_map(|k| {
                    let dir = src.join(module[..k].join("/"));
                    [dir.with_extension("rs"), dir.join("mod.rs")]
                        .into_iter()
                        .find(|f| f.is_file())
                        .map(|f| (k, f))
                })?;
                let top = k == module.len();
                let source = std::fs::read_to_string(file).ok()?;
                let prefix = format!("const {name}: ");
                let mut decls = source.lines().filter_map(|l| {
                    let indented = l.starts_with(char::is_whitespace);
                    let t = l.trim();
                    let t = match t.strip_prefix("pub") {
                        Some(r) if r.starts_with('(') => r[r.find(')')? + 1..].trim_start(),
                        Some(r) => r.trim_start(),
                        None => t,
                    };
                    let (ty, value) = t.strip_prefix(&prefix)?.split_once(" = ")?;
                    let value = value.strip_suffix(';')?.trim();
                    (indented != top).then_some((ty.trim(), value))
                });
                let (ty, value) = match (decls.next(), decls.next()) {
                    (Some(d), None) => d,
                    _ => return None,
                };
                let int = ty.strip_prefix(['u', 'i']).is_some_and(|w| {
                    w == "size" || matches!(w.parse::<u32>(), Ok(8 | 16 | 32 | 64 | 128))
                });
                let literal = if int {
                    let digits: String = value.chars().filter(|c| *c != '_').collect();
                    let digits = digits.strip_suffix(ty).unwrap_or(&digits);
                    let (radix, digits) = match digits.get(..2) {
                        Some("0x") => (16, &digits[2..]),
                        Some("0o") => (8, &digits[2..]),
                        Some("0b") => (2, &digits[2..]),
                        _ => (10, digits),
                    };
                    format!("{}_{ty}", i128::from_str_radix(digits, radix).ok()?)
                } else if ty == "bool" && (value == "true" || value == "false")
                    || ty == "char" && value.starts_with('\'') && value.ends_with('\'')
                {
                    value.to_string()
                } else {
                    return None;
                };
                Some(Instance {
                    path: printed.to_string(),
                    text: format!(
                        "const {printed}: {ty} = {{\n    let mut _0: {ty};\n\n    bb0: {{\n        _0 = const {literal};\n        return;\n    }}\n}}\n"
                    ),
                    bound: Vec::new(),
                })
            };
            // A constant of the impls a macro writes for `X<A>`
            // (`NonZero::<u8>::BITS`), which share the path of the
            // macro's impl in `X`'s module: the text of the impl, by
            // its index (`{impl#N}`), whose methods' signatures take
            // `X<A>` as `self`.
            let from_macro = || -> Option<&String> {
                let (rest, name) = printed.rsplit_once("::")?;
                let (path, args) = rest.strip_suffix('>')?.rsplit_once("::<")?;
                let head = split_path(path).last().copied()?;
                // Taken as `self`, its first argument: the macro's
                // other types are in its signatures too.
                let selfs = [
                    format!("(_1: {head}<{args}>)"),
                    format!("(_1: {head}<{args}>, "),
                ];
                let module = path.strip_suffix(head)?;
                let mut found = self.bodies.iter().filter_map(|(p, texts)| {
                    let span = p.strip_prefix(module)?.strip_prefix("<impl at ")?;
                    let span = &span[..span.find('>')?];
                    let (_, ty) = self.impls.get(span)?;
                    let own = format!("{module}<impl at {span}>::");
                    (ty.split('<').next() == Some(head) && p.strip_prefix(&own) == Some(name))
                        .then_some((own, p, texts))
                });
                let (own, path, texts) = match (found.next(), found.next()) {
                    (Some(f), None) => f,
                    _ => return None,
                };
                let idx = self.index.get(path)?;
                let takes = |n: &Option<u64>| {
                    n.is_some()
                        && self.bodies.iter().any(|(q, ts)| {
                            q.starts_with(&own)
                                && self.index.get(q).is_some_and(|qi| {
                                    ts.iter().zip(qi).any(|(t, m)| {
                                        m == n
                                            && t.lines().find(|l| l.starts_with("fn ")).is_some_and(
                                                |l| selfs.iter().any(|t| l.contains(t)),
                                            )
                                    })
                                })
                        })
                };
                let mut fit = texts.iter().zip(idx).filter(|(_, n)| takes(n));
                match (fit.next(), fit.next()) {
                    (Some((t, _)), None) => Some(t),
                    _ => None,
                }
            };
            let text = match self.bodies.get(printed).or_else(alias).map(Vec::as_slice) {
                Some([text]) => text,
                None => match from_source() {
                    Some(i) => return Some(i),
                    None => from_macro()?,
                },
                _ => from_macro()?,
            };
            return text
                .lines()
                .any(|l| {
                    l.starts_with("const ") || l.contains("::{constant#") && l.ends_with(" = {")
                })
                .then(|| Instance {
                    path: printed.to_string(),
                    text: text.clone(),
                    bound: Vec::new(),
                });
        }
        let inner = printed.strip_prefix('<')?;
        let (self_ty, rest) = inner.split_once(" as ")?;
        let (tr, name) = rest.rsplit_once(">::")?;
        let path = format!("{}::{name}", super::lower::erase(tr));
        let [text] = self.bodies.get(&path)?.as_slice() else {
            return None;
        };
        let generics = BTreeMap::from([("Self".to_string(), self_ty.to_string())]);
        let mut out = String::new();
        for line in text.lines() {
            out.push_str(&substitute(line, &generics));
            out.push('\n');
        }
        Some(Instance {
            path: printed.to_string(),
            text: out,
            bound: generics.into_iter().collect(),
        })
    }
}
