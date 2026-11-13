//! `macro_rules!` expansion of the items a source declares through its own
//! macros, as rustc expands them: each rule's matcher read against an
//! invocation in item position, the first that matches transcribed with its
//! fragments, and the transcription read again as items. Core declares much
//! of its surface this way (`define_valid_range_type!`,
//! `impl_zeroable_primitive!`), so its types are read after expansion.

use proc_macro2::{Delimiter, Group, Ident, TokenStream, TokenTree};
use std::collections::BTreeMap;
use syn::ext::IdentExt;
use syn::parse::discouraged::Speculative;
use syn::parse::{ParseStream, Parser};

/// One element of a rule's matcher.
#[derive(Clone, Debug)]
enum Matcher {
    /// A token the invocation writes as it is.
    Token(String),
    /// A delimited group and what it holds.
    Group(Delimiter, Vec<Matcher>),
    /// `$name:fragment`.
    Fragment(String, String),
    /// `$( ... ) sep op`: its elements, separator and operator (`*`, `+`
    /// or `?`).
    Repeat(Vec<Matcher>, Option<String>, char),
}

/// What a fragment of a matched invocation binds: its tokens, or one
/// binding per repetition.
#[derive(Clone, Debug)]
enum Binding {
    One(TokenStream),
    Many(Vec<Binding>),
}

type Bindings = BTreeMap<String, Binding>;

/// The `macro_rules!` macros a source declares, by name: each rule's
/// matcher and transcriber, in the order the macro writes them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Macros {
    rules: BTreeMap<String, Vec<(Vec<Matcher>, TokenStream)>>,
}

/// How deep expansions may nest: rustc's default `recursion_limit`.
const RECURSION_LIMIT: usize = 128;

impl Macros {
    /// Every `macro_rules!` of `items`, in inline modules too. A name two
    /// macros share keeps the first, as a later one shadows it only after
    /// its own definition.
    pub(crate) fn collect(&mut self, items: &[syn::Item]) {
        for item in items {
            match item {
                syn::Item::Macro(m) if m.mac.path.is_ident("macro_rules") => {
                    let Some(name) = &m.ident else {
                        continue;
                    };
                    if let Some(rules) = rules(m.mac.tokens.clone()) {
                        self.rules.entry(name.to_string()).or_insert(rules);
                    }
                }
                syn::Item::Mod(m) => {
                    if let Some((_, inner)) = &m.content {
                        self.collect(inner);
                    }
                }
                _ => {}
            }
        }
    }

    /// `items` with each invocation of a known macro in item position
    /// replaced by the items it expands to, `read` turning a transcription
    /// into items; an invocation no rule matches is kept as it is.
    pub(crate) fn expand(
        &self,
        items: Vec<syn::Item>,
        read: &dyn Fn(TokenStream) -> Option<Vec<syn::Item>>,
    ) -> Vec<syn::Item> {
        self.expand_at(items, read, 0)
    }

    fn expand_at(
        &self,
        items: Vec<syn::Item>,
        read: &dyn Fn(TokenStream) -> Option<Vec<syn::Item>>,
        depth: usize,
    ) -> Vec<syn::Item> {
        let mut out = Vec::new();
        for item in items {
            match item {
                syn::Item::Macro(m) if m.ident.is_none() && depth < RECURSION_LIMIT => {
                    let name = m.mac.path.segments.last().map(|s| s.ident.to_string());
                    let expanded = name
                        .and_then(|n| self.rules.get(&n))
                        .and_then(|rules| {
                            rules.iter().find_map(|(matcher, body)| {
                                let bound = matched(matcher, m.mac.tokens.clone())?;
                                transcribe(body.clone(), &bound, &[])
                            })
                        })
                        .and_then(|tokens| read(tokens));
                    match expanded {
                        Some(items) => out.extend(self.expand_at(items, read, depth + 1)),
                        None => out.push(syn::Item::Macro(m)),
                    }
                }
                syn::Item::Mod(mut m) => {
                    if let Some((brace, inner)) = m.content.take() {
                        m.content = Some((brace, self.expand_at(inner, read, depth)));
                    }
                    out.push(syn::Item::Mod(m));
                }
                other => out.push(other),
            }
        }
        out
    }
}

/// The rules of a `macro_rules!` body: `(matcher) => {transcriber}`, each
/// ended by `;`.
fn rules(body: TokenStream) -> Option<Vec<(Vec<Matcher>, TokenStream)>> {
    let tts: Vec<TokenTree> = body.into_iter().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tts.len() {
        let (
            TokenTree::Group(m),
            Some(TokenTree::Punct(eq)),
            Some(TokenTree::Punct(gt)),
            Some(TokenTree::Group(t)),
        ) = (&tts[i], tts.get(i + 1), tts.get(i + 2), tts.get(i + 3))
        else {
            return None;
        };
        if eq.as_char() != '=' || gt.as_char() != '>' {
            return None;
        }
        out.push((matchers(m.stream())?, t.stream()));
        i += 4;
        if matches!(tts.get(i), Some(TokenTree::Punct(p)) if p.as_char() == ';') {
            i += 1;
        }
    }
    Some(out)
}

/// A matcher's elements.
fn matchers(tokens: TokenStream) -> Option<Vec<Matcher>> {
    let tts: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tts.len() {
        match (&tts[i], tts.get(i + 1)) {
            (TokenTree::Punct(p), Some(TokenTree::Ident(name))) if p.as_char() == '$' => {
                match (tts.get(i + 2), tts.get(i + 3)) {
                    (Some(TokenTree::Punct(c)), Some(TokenTree::Ident(frag)))
                        if c.as_char() == ':' =>
                    {
                        out.push(Matcher::Fragment(name.to_string(), frag.to_string()));
                        i += 4;
                    }
                    _ => return None,
                }
            }
            (TokenTree::Punct(p), Some(TokenTree::Group(g)))
                if p.as_char() == '$' && g.delimiter() == Delimiter::Parenthesis =>
            {
                let inner = matchers(g.stream())?;
                let op = |t: Option<&TokenTree>| match t {
                    Some(TokenTree::Punct(p)) if matches!(p.as_char(), '*' | '+' | '?') => {
                        Some(p.as_char())
                    }
                    _ => None,
                };
                if let Some(o) = op(tts.get(i + 2)) {
                    out.push(Matcher::Repeat(inner, None, o));
                    i += 3;
                } else if let Some(o) = op(tts.get(i + 3)) {
                    let sep = tts.get(i + 2)?.to_string();
                    out.push(Matcher::Repeat(inner, Some(sep), o));
                    i += 4;
                } else {
                    return None;
                }
            }
            (TokenTree::Group(g), _) => {
                out.push(Matcher::Group(g.delimiter(), matchers(g.stream())?));
                i += 1;
            }
            (t, _) => {
                out.push(Matcher::Token(t.to_string()));
                i += 1;
            }
        }
    }
    Some(out)
}

/// The bindings of `tokens` matched in full against `matcher`.
fn matched(matcher: &[Matcher], tokens: TokenStream) -> Option<Bindings> {
    let mut bound = Bindings::new();
    let parser = |input: ParseStream| {
        match_all(input, matcher, &mut bound)?;
        if input.is_empty() {
            Ok(())
        } else {
            Err(input.error("tokens past the matcher"))
        }
    };
    parser.parse2(tokens).ok()?;
    Some(bound)
}

/// Each name a matcher binds, in its repetitions too.
fn names(matcher: &[Matcher], out: &mut Vec<String>) {
    for m in matcher {
        match m {
            Matcher::Fragment(n, _) => out.push(n.clone()),
            Matcher::Group(_, inner) | Matcher::Repeat(inner, _, _) => names(inner, out),
            Matcher::Token(_) => {}
        }
    }
}

fn match_all(input: ParseStream, matcher: &[Matcher], bound: &mut Bindings) -> syn::Result<()> {
    for m in matcher {
        match m {
            Matcher::Token(t) => {
                let tt: TokenTree = input.parse()?;
                if tt.to_string() != *t {
                    return Err(input.error("another token"));
                }
            }
            Matcher::Group(d, inner) => {
                let g: Group = input.parse()?;
                if g.delimiter() != *d {
                    return Err(input.error("another delimiter"));
                }
                let mut inside = Bindings::new();
                let parser = |s: ParseStream| {
                    match_all(s, inner, &mut inside)?;
                    if s.is_empty() {
                        Ok(())
                    } else {
                        Err(s.error("tokens past the group's matcher"))
                    }
                };
                parser.parse2(g.stream())?;
                bound.extend(inside);
            }
            Matcher::Fragment(name, frag) => {
                let start = input.cursor();
                fragment(input, frag)?;
                let end = input.cursor();
                let mut tokens = TokenStream::new();
                let mut at = start;
                while at != end {
                    let Some((tt, next)) = at.token_tree() else {
                        break;
                    };
                    tokens.extend([tt]);
                    at = next;
                }
                bound.insert(name.clone(), Binding::One(tokens));
            }
            Matcher::Repeat(inner, sep, op) => {
                let mut each: Vec<Bindings> = Vec::new();
                loop {
                    if *op == '?' && !each.is_empty() {
                        break;
                    }
                    let fork = input.fork();
                    if !each.is_empty() {
                        if let Some(s) = sep {
                            match fork.parse::<TokenTree>() {
                                Ok(tt) if tt.to_string() == *s => {}
                                _ => break,
                            }
                        }
                    }
                    let mut one = Bindings::new();
                    let before = fork.cursor();
                    if match_all(&fork, inner, &mut one).is_err() || fork.cursor() == before {
                        break;
                    }
                    input.advance_to(&fork);
                    each.push(one);
                }
                if *op == '+' && each.is_empty() {
                    return Err(input.error("a repetition of none"));
                }
                let mut vars = Vec::new();
                names(inner, &mut vars);
                for v in vars {
                    let seq = each
                        .iter()
                        .map(|b| b.get(&v).cloned().unwrap_or(Binding::Many(Vec::new())))
                        .collect();
                    bound.insert(v, Binding::Many(seq));
                }
            }
        }
    }
    Ok(())
}

/// Parse one fragment of kind `frag` from `input`.
fn fragment(input: ParseStream, frag: &str) -> syn::Result<()> {
    match frag {
        "ident" => input.call(Ident::parse_any).map(drop),
        "ty" => input.parse::<syn::Type>().map(drop),
        "pat" => syn::Pat::parse_multi_with_leading_vert(input).map(drop),
        "pat_param" => syn::Pat::parse_single(input).map(drop),
        "expr" => input.parse::<syn::Expr>().map(drop),
        "vis" => input.parse::<syn::Visibility>().map(drop),
        "meta" => input.parse::<syn::Meta>().map(drop),
        "literal" => input.parse::<syn::Lit>().map(drop),
        "lifetime" => input.parse::<syn::Lifetime>().map(drop),
        "path" => input.parse::<syn::Path>().map(drop),
        "block" => input.parse::<syn::Block>().map(drop),
        "item" => input.parse::<syn::Item>().map(drop),
        "stmt" => input.parse::<syn::Stmt>().map(drop),
        "tt" => input.parse::<TokenTree>().map(drop),
        _ => Err(input.error("a fragment kind not read")),
    }
}

/// The binding of `name` at the repetition indices `at`, or the bindings it
/// repeats over where `at` stops short of its depth.
fn binding<'b>(bound: &'b Bindings, name: &str, at: &[usize]) -> Option<&'b Binding> {
    let mut b = bound.get(name)?;
    for &i in at {
        match b {
            Binding::Many(each) => b = each.get(i)?,
            Binding::One(_) => break,
        }
    }
    Some(b)
}

/// The names a transcriber's tokens substitute.
fn mentioned(tokens: &TokenStream, out: &mut Vec<String>) {
    let tts: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (i, tt) in tts.iter().enumerate() {
        match (tt, tts.get(i + 1)) {
            (TokenTree::Punct(p), Some(TokenTree::Ident(n))) if p.as_char() == '$' => {
                out.push(n.to_string())
            }
            (TokenTree::Group(g), _) => mentioned(&g.stream(), out),
            _ => {}
        }
    }
}

/// `tokens` with each fragment substituted at the repetition indices `at`
/// and each repetition written once for each of its bindings.
fn transcribe(tokens: TokenStream, bound: &Bindings, at: &[usize]) -> Option<TokenStream> {
    let tts: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out = TokenStream::new();
    let mut i = 0;
    while i < tts.len() {
        match (&tts[i], tts.get(i + 1)) {
            (TokenTree::Punct(p), Some(TokenTree::Ident(n))) if p.as_char() == '$' => {
                if n == "crate" {
                    out.extend([TokenTree::Ident(Ident::new("crate", n.span()))]);
                } else {
                    match binding(bound, &n.to_string(), at)? {
                        Binding::One(t) => out.extend(t.clone()),
                        Binding::Many(_) => return None,
                    }
                }
                i += 2;
            }
            (TokenTree::Punct(p), Some(TokenTree::Group(g)))
                if p.as_char() == '$' && g.delimiter() == Delimiter::Parenthesis =>
            {
                let is_op = |t: Option<&TokenTree>| matches!(t, Some(TokenTree::Punct(p)) if matches!(p.as_char(), '*' | '+' | '?'));
                let (sep, next) = if is_op(tts.get(i + 2)) {
                    (None, i + 3)
                } else if is_op(tts.get(i + 3)) {
                    (tts.get(i + 2).cloned(), i + 4)
                } else {
                    return None;
                };
                let mut vars = Vec::new();
                mentioned(&g.stream(), &mut vars);
                let count = vars.iter().find_map(|v| match binding(bound, v, at) {
                    Some(Binding::Many(each)) => Some(each.len()),
                    _ => None,
                })?;
                for k in 0..count {
                    if k > 0 {
                        if let Some(s) = &sep {
                            out.extend([s.clone()]);
                        }
                    }
                    let mut deeper = at.to_vec();
                    deeper.push(k);
                    out.extend(transcribe(g.stream(), bound, &deeper)?);
                }
                i = next;
            }
            (TokenTree::Group(g), _) => {
                let mut ng = Group::new(g.delimiter(), transcribe(g.stream(), bound, at)?);
                ng.set_span(g.span());
                out.extend([TokenTree::Group(ng)]);
                i += 1;
            }
            (t, _) => {
                out.extend([t.clone()]);
                i += 1;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> Vec<syn::Item> {
        syn::parse_file(text).expect("parses").items
    }

    fn read(tokens: TokenStream) -> Option<Vec<syn::Item>> {
        syn::parse2::<syn::File>(tokens).ok().map(|f| f.items)
    }

    #[test]
    fn repetitions_expand_to_items() {
        let items = file(
            "macro_rules! valid { ($($(#[$m:meta])* $vis:vis struct $name:ident($int:ident is $pat:pat);)+) => {$(\
                $(#[$m])* $vis struct $name(pattern_type!($int is $pat));\
             )+}; }\
             valid! { pub struct A(u8 is 1..); pub struct B(u32 is ..0 | 1..); }",
        );
        let mut macros = Macros::default();
        macros.collect(&items);
        let out = macros.expand(items, &read);
        let structs: Vec<String> = out
            .iter()
            .filter_map(|i| match i {
                syn::Item::Struct(s) => Some(s.ident.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(structs, ["A", "B"]);
    }

    #[test]
    fn separated_lists_and_crate_paths_transcribe() {
        let items = file(
            "macro_rules! prim { ($($inner:ident ( $p:ty )),+ $(,)?) => {$(\
                impl Z for $p { type Inner = $crate::n::$inner; }\
             )+}; }\
             prim!(IA(u8), IB(u16),);",
        );
        let mut macros = Macros::default();
        macros.collect(&items);
        let out = macros.expand(items, &read);
        let impls: Vec<String> = out
            .iter()
            .filter_map(|i| match i {
                syn::Item::Impl(im) => {
                    let ty = &im.self_ty;
                    Some(quote::quote!(#ty).to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(impls, ["u8", "u16"]);
    }

    #[test]
    fn an_unmatched_invocation_is_kept() {
        let items = file("macro_rules! one { (x) => { struct X; }; } one!(y);");
        let mut macros = Macros::default();
        macros.collect(&items);
        let out = macros.expand(items, &read);
        assert!(matches!(out.last(), Some(syn::Item::Macro(_))));
    }
}
