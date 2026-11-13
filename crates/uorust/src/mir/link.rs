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
use crate::lift::Ty;

/// Core's bodies and declarations, for one census.
pub(crate) struct CoreLink {
    pub(crate) decls: CoreDecls,
    /// Each body's MIR text, by the path its dump names it by: a macro's
    /// impls share the path of the macro's text.
    bodies: BTreeMap<String, Vec<String>>,
    /// For each last segment of a body's path, the paths that have it: MIR
    /// prints a path by its last segment where no other item has it.
    lasts: BTreeMap<String, Vec<String>>,
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
                    let (l1, c1, l2, c2): (usize, usize, usize, usize) =
                        (l1.parse().ok()?, c1.parse().ok()?, l2.parse().ok()?, c2.parse().ok()?);
                    let lines = sources.entry(file.to_string()).or_insert_with(|| {
                        std::fs::read_to_string(root.join(file))
                            .map(|t| t.lines().map(str::to_string).collect())
                            .unwrap_or_default()
                    });
                    let mut text = String::new();
                    for l in l1..=l2 {
                        let chars: Vec<char> = lines.get(l.checked_sub(1)?)?.chars().collect();
                        let from = if l == l1 { c1.saturating_sub(1) } else { 0 };
                        let to = if l == l2 { c2.saturating_sub(1).min(chars.len()) } else { chars.len() };
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
        for (resolved, texts) in aliases {
            bodies.entry(resolved).or_insert(texts);
        }
        CoreLink {
            decls,
            bodies,
            lasts,
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
            return one.ends_with(&tail).then(|| one.clone());
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
        if !explicit.is_empty() {
            let names = self.decls.generics.get(path)?;
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
        let paths: Vec<String> = match self.path(&erased) {
            Some(p) => vec![p],
            None => {
                let method = erased
                    .starts_with('<')
                    .then(|| erased.rsplit_once(">::"))??
                    .1;
                self.lasts
                    .get(method)?
                    .iter()
                    .filter(|p| p.contains("<impl") && arg_texts.iter().all(Option::is_some))
                    .cloned()
                    .collect()
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
                    // A name the signature binds is a parameter, not a type core
                    // declares.
                    let declared = g
                        .keys()
                        .any(|k| self.decls.structs.iter().any(|s| s.name() == k));
                    (!declared).then(|| (p.clone(), text, g))
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

/// The size in bytes of a value of `t` in the target data layout, on each
/// pointer width of `widths`: an integer's width in bytes, a `bool` one
/// byte, a `char` the four of its `u32` scalar, the unit none, and `usize`
/// the pointer's width. `None` for a type whose layout rustc chooses.
pub(crate) fn size_of(t: &Ty, widths: &[(u32, Vec<String>)]) -> Option<Vec<(u32, u128)>> {
    let bytes = |bits: u32| u128::from(bits / u8::BITS);
    let each = |n: u128| Some(widths.iter().map(|(w, _)| (*w, n)).collect());
    match t {
        Ty::Nat(Some(b)) | Ty::Int(b) => each(bytes(*b)),
        Ty::Nat(None) => Some(widths.iter().map(|(w, _)| (*w, bytes(*w))).collect()),
        Ty::Bool => each(1),
        Ty::Char => each(bytes(u32::BITS)),
        Ty::Unit => each(0),
        _ => None,
    }
}
