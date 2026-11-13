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
    bodies: BTreeMap<String, Vec<String>>,
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
}

impl CoreLink {
    /// The link of core's dumps `texts` (one body each), with core's
    /// declarations.
    pub(crate) fn new<'t>(
        decls: CoreDecls,
        root: &std::path::Path,
        texts: impl IntoIterator<Item = &'t String>,
    ) -> Self {
        let mut bodies: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut lasts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for text in texts {
            // A dump names its body `// MIR for `path` before ..`.
            let Some(path) = text
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("// MIR for `"))
                .and_then(|r| Some(&r[..r.find("` ")?]))
            else {
                continue;
            };
            if path.contains("{closure") || path.contains("promoted[") || path.contains("{constant")
            {
                continue;
            }
            let last = split_path(path).last().copied().unwrap_or(path).to_string();
            let same = bodies.entry(path.to_string()).or_default();
            if same.is_empty() {
                lasts.entry(last).or_default().push(path.to_string());
            }
            same.push(text.clone());
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
                                let ty = if ty.contains('$') {
                                    "_"
                                } else if blanket {
                                    "*"
                                } else {
                                    ty
                                };
                                (Some(tr.to_string()), ty.to_string())
                            }
                            None if h.contains('$') => (None, "_".to_string()),
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
        CoreLink {
            decls,
            bodies,
            lasts,
            impls,
            wild,
            defaults,
            methods,
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
            unify(ret, dest, &mut generics)?;
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
                // An impl for `X`'s own type is chosen over a blanket impl,
                // whose bounds the link does not check (`impl<F: FnPtr> Ord
                // for F` is not `bool`'s).
                let blanket = |p: &String| impl_of(p).is_some_and(|(_, (_, ty))| ty == "*");
                if found.iter().any(|p| !blanket(p)) {
                    found.retain(|p| !blanket(p));
                }
                // An impl of the trait the call names is chosen over one of
                // a trait a macro names by a metavariable.
                let named =
                    |p: &String| impl_of(p).is_some_and(|(_, (t, _))| t.as_deref() != Some("_"));
                if found.iter().any(named) {
                    found.retain(named);
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
        let mut fits = paths.iter().flat_map(|p| {
            self.bodies
                .get(p)
                .into_iter()
                .flatten()
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
                    let mut own = super::core::canonical_bindings(text);
                    own.extend(super::core::canonical_bindings(&sig));
                    let declares = self.decls.generics.get(p.as_str());
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
                    let mut g = g;
                    g.retain(|k, v| param(k) && !itself(k, v));
                    // A type the signature prints by its path is the type of that
                    // path: `range::RangeInclusive<T>` is not
                    // `ops::range::RangeInclusive<usize>`.
                    let head = |t: &str| -> Option<String> {
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
                            return Some(name.to_string());
                        }
                        let mut ends = structs.iter().filter(|s| s.path.ends_with(&tail));
                        Some(match (ends.next(), ends.next()) {
                            (Some(s), None) => s.path.clone(),
                            _ => name.to_string(),
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
                        if let (Some(x), Some(y)) = (head(p), head(a)) {
                            differ |= x != y;
                        }
                    }
                    (!declared && !differ).then(|| (p.clone(), text, g))
                })
        });
        let (path, text, generics) = match (fits.next(), fits.next()) {
            (Some(one), None) => one,
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
            let [text] = self.bodies.get(printed)?.as_slice() else {
                return None;
            };
            return text
                .lines()
                .any(|l| l.starts_with("const "))
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
