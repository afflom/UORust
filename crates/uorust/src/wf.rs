//! Well-founded recursion over a nested or mutual family of types.
//!
//! LexLean recurses structurally over a type only when it is self-recursive
//! and held directly; a family whose members name each other, or hold
//! themselves in a list or an option, has no single eliminator. A function
//! recursing over one is declared well-founded on the family's size
//! function, and each recursive call carries a theorem that it decreases the
//! size, stated exactly as LexLean derives the obligation from the enclosing
//! `if`s and `match`es and proved by linear arithmetic after unfolding the
//! size functions.

use std::collections::{BTreeMap, BTreeSet};

use crate::lift::{Lift, Residue};

/// A term of the notation, read back from the text the lift printed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    /// A word, a literal or punctuation, as written.
    Word(String),
    /// A parenthesised form: its members and its text.
    Form(Vec<Node>, String),
    /// A bracketed or braced group, as written.
    Group(String),
}

impl Node {
    fn text(&self) -> &str {
        match self {
            Self::Word(t) | Self::Form(_, t) | Self::Group(t) => t,
        }
    }

    fn word(&self) -> Option<&str> {
        match self {
            Self::Word(t) => Some(t),
            _ => None,
        }
    }
}

/// The nodes of `text` from `at`, up to the closing character `close`, and
/// the position after it.
fn nodes(text: &[char], mut at: usize, close: Option<char>) -> Option<(Vec<Node>, usize)> {
    let mut out = Vec::new();
    while at < text.len() {
        let c = text[at];
        if Some(c) == close {
            return Some((out, at + 1));
        }
        match c {
            ' ' | '\n' => at += 1,
            '(' => {
                let (inner, next) = nodes(text, at + 1, Some(')'))?;
                out.push(Node::Form(inner, text[at..next].iter().collect()));
                at = next;
            }
            '[' | '{' => {
                let end = if c == '[' { ']' } else { '}' };
                let (_, next) = nodes(text, at + 1, Some(end))?;
                out.push(Node::Group(text[at..next].iter().collect()));
                at = next;
            }
            ')' | ']' | '}' => return None,
            '"' => {
                let start = at;
                at += 1;
                while at < text.len() && text[at] != '"' {
                    at += if text[at] == '\\' { 2 } else { 1 };
                }
                at += 1;
                out.push(Node::Word(text[start..at.min(text.len())].iter().collect()));
            }
            ';' | ',' => {
                out.push(Node::Word(c.to_string()));
                at += 1;
            }
            _ => {
                let start = at;
                while at < text.len() && !" \n()[]{};,\"".contains(text[at]) {
                    at += 1;
                }
                out.push(Node::Word(text[start..at].iter().collect()));
            }
        }
    }
    close.is_none().then_some((out, at))
}

/// A step from the body to a recursive call: a branch of an `if` or of a
/// `match`.
#[derive(Clone, Debug)]
enum Step {
    /// The condition's text and the branch taken.
    If(String, bool),
    /// The scrutinee's text and the branch's pattern words.
    Match(String, Vec<String>),
}

/// The callee, the enclosing branches and the argument texts of one
/// recursive call.
type Site = (String, Vec<Step>, Vec<String>);

/// Whether a node holds the name of any member of `group`.
fn mentions_any(node: &Node, group: &BTreeSet<String>) -> bool {
    group.iter().any(|name| mentions(node, name))
}

/// Whether a node holds the word `name`.
fn mentions(node: &Node, name: &str) -> bool {
    match node {
        Node::Word(w) => w == name,
        Node::Form(inner, _) => inner.iter().any(|n| mentions(n, name)),
        Node::Group(text) => text
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == name),
    }
}

/// The recursive calls of a member of `group` in `node`, in pre-order, or `None` where
/// LexLean refuses the recursion: a call inside a lambda or another call's
/// argument, or an argument or an enclosing test naming a local that is not
/// a parameter or an enclosing match binder.
fn sites(
    node: &Node,
    group: &BTreeSet<String>,
    path: &mut Vec<Step>,
    allowed: &mut Vec<String>,
    lets: &mut Vec<String>,
    out: &mut Vec<Site>,
) -> Option<()> {
    let Node::Form(inner, _) = node else {
        return Some(());
    };
    let head = inner.first().and_then(Node::word);
    let local_free = |n: &Node, lets: &[String]| lets.iter().all(|l| !mentions(n, l));
    match head {
        Some(f) if group.contains(f) => {
            let args = &inner[1..];
            if args
                .iter()
                .any(|a| mentions_any(a, group) || !local_free(a, lets))
            {
                return None;
            }
            out.push((
                f.to_string(),
                path.clone(),
                args.iter().map(|a| a.text().to_string()).collect(),
            ));
        }
        Some("if") => {
            let [_, c, then_w, a, else_w, b] = inner.as_slice() else {
                return None;
            };
            if then_w.word() != Some("then") || else_w.word() != Some("else") {
                return None;
            }
            sites(c, group, path, allowed, lets, out)?;
            let encloses = mentions_any(a, group) || mentions_any(b, group);
            if encloses && !local_free(c, lets) {
                return None;
            }
            for (branch, polarity) in [(a, true), (b, false)] {
                path.push(Step::If(c.text().to_string(), polarity));
                sites(branch, group, path, allowed, lets, out)?;
                path.pop();
            }
        }
        Some("match") => {
            let scrutinee = inner.get(1)?;
            sites(scrutinee, group, path, allowed, lets, out)?;
            let encloses = inner[2..].iter().any(|n| mentions_any(n, group));
            if encloses && !local_free(scrutinee, lets) {
                return None;
            }
            let mut rest = &inner[2..];
            while let [bar, tail @ ..] = rest {
                if bar.word() != Some("|") {
                    return None;
                }
                let arrow = tail.iter().position(|n| n.word() == Some("=>"))?;
                let pattern: Vec<String> = tail[..arrow]
                    .iter()
                    .map(|n| n.word().map(String::from))
                    .collect::<Option<_>>()?;
                let body = tail.get(arrow + 1)?;
                let binders = &pattern[1..];
                if encloses && binders.iter().any(|b| allowed.contains(b)) {
                    return None;
                }
                let before = allowed.len();
                allowed.extend(binders.iter().cloned());
                // A binder shadows a `let` of the same name.
                let hidden: Vec<String> = lets
                    .iter()
                    .filter(|l| binders.contains(l))
                    .cloned()
                    .collect();
                lets.retain(|l| !binders.contains(l));
                path.push(Step::Match(scrutinee.text().to_string(), pattern.clone()));
                sites(body, group, path, allowed, lets, out)?;
                path.pop();
                lets.extend(hidden);
                allowed.truncate(before);
                rest = &tail[arrow + 2..];
            }
        }
        Some("let") => {
            // `(let x : T := value; body)`
            let bound = inner.get(1)?.word()?.to_string();
            let semi = inner.iter().position(|n| n.word() == Some(";"))?;
            for n in &inner[2..semi] {
                sites(n, group, path, allowed, lets, out)?;
            }
            lets.push(bound);
            for n in &inner[semi + 1..] {
                sites(n, group, path, allowed, lets, out)?;
            }
            lets.pop();
        }
        Some("fun") => {
            if mentions_any(node, group) {
                return None;
            }
        }
        _ => {
            for n in inner {
                sites(n, group, path, allowed, lets, out)?;
            }
        }
    }
    Some(())
}

/// One evidence theorem: its parameters, its statement.
pub(crate) type Obligation = (Vec<(String, String)>, String);

/// The decrease obligations of a function `name` with `params`, recursing
/// on the parameter at `arg` and measured by `size`, in call-site order;
/// `binder_types` gives the types of a constructor pattern's binders.
/// `None` where LexLean would refuse the recursion or a pattern is not a
/// constructor of a document type.
pub(crate) fn obligations(
    name: &str,
    params: &[(String, String)],
    arg: usize,
    size: &str,
    body: &str,
    binder_types: &dyn Fn(&str) -> Option<Vec<String>>,
) -> Option<Vec<Obligation>> {
    let text: Vec<char> = body.chars().collect();
    let (top, _) = nodes(&text, 0, None)?;
    let [node] = top.as_slice() else {
        return None;
    };
    let mut found = Vec::new();
    let mut allowed: Vec<String> = params.iter().map(|(p, _)| p.clone()).collect();
    sites(
        node,
        &BTreeSet::from([name.to_string()]),
        &mut Vec::new(),
        &mut allowed,
        &mut Vec::new(),
        &mut found,
    )?;
    let measure = format!("({size} {})", params[arg].0);
    let mut out = Vec::new();
    for (_, path, args) in found {
        if args.len() != params.len() {
            return None;
        }
        let call = format!("({size} {})", args[arg]);
        out.push(obligation(params, &path, &call, &measure, binder_types)?);
    }
    (!out.is_empty()).then_some(out)
}

/// The obligation of one call: the caller's parameters and the binders of
/// the matches around the call, and, under the branches taken to reach
/// it, the callee's measure at the call below the caller's.
fn obligation(
    params: &[(String, String)],
    path: &[Step],
    call: &str,
    measure: &str,
    binder_types: &dyn Fn(&str) -> Option<Vec<String>>,
) -> Option<Obligation> {
    let mut binders = Vec::new();
    let mut premises = Vec::new();
    for step in path {
        match step {
            Step::If(c, polarity) => premises.push(format!("({c} = {polarity})")),
            Step::Match(s, pattern) => {
                let ctor = &pattern[0];
                let typed = params.iter().chain(&binders).find(|(n, _)| n == s);
                // A list's or an option's constructor names its element type.
                let (tys, head) = match typed.and_then(|(_, t)| container_binders(ctor, t)) {
                    Some((tys, element)) => (tys, format!("(ctor {ctor} [{element}]")),
                    None => (binder_types(ctor)?, format!("(ctor {ctor}")),
                };
                if tys.len() + 1 != pattern.len() {
                    return None;
                }
                for (b, t) in pattern[1..].iter().zip(tys) {
                    binders.push((b.clone(), t));
                }
                let value: String = std::iter::once(head)
                    .chain(pattern[1..].iter().map(|b| format!(" {b}")))
                    .collect();
                premises.push(format!("({s} = {value}))"));
            }
        }
    }
    let mut statement = format!("({call} < {measure})");
    for p in premises.into_iter().rev() {
        statement = format!("({p} -> {statement})");
    }
    let mut all = params.to_vec();
    all.extend(binders);
    Some((all, statement))
}

/// The types a list's or an option's constructor binds, from the type of
/// the local it matches, and the element type it names: `None` for any
/// other constructor or type.
fn container_binders(ctor: &str, ty: &str) -> Option<(Vec<String>, String)> {
    let inner = |head: &str| {
        ty.strip_prefix(&format!("({head} "))
            .and_then(|t| t.strip_suffix(')'))
            .map(String::from)
    };
    match ctor {
        "List.nil" | "List.cons" => {
            let element = inner("List")?;
            let tys = if ctor == "List.cons" {
                vec![element.clone(), ty.to_string()]
            } else {
                Vec::new()
            };
            Some((tys, element))
        }
        "Option.none" | "Option.some" => {
            let element = inner("Option")?;
            let tys = if ctor == "Option.some" {
                vec![element.clone()]
            } else {
                Vec::new()
            };
            Some((tys, element))
        }
        _ => None,
    }
}

/// A member of a mutual group: its name, parameters, the parameter its
/// measure sizes, that size function, and its body's text.
pub(crate) struct GroupMember<'a> {
    pub(crate) name: &'a str,
    pub(crate) params: &'a [(String, String)],
    pub(crate) measured: usize,
    pub(crate) size: &'a str,
    pub(crate) body: &'a str,
}

/// How a call in a mutual group passes the callee's measured argument.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Passed {
    /// The caller's own measured argument, unchanged.
    Same,
    /// A part of it: a binder of a match on it or on a part.
    Part,
}

/// Whether `text` is a part of the measured parameter, given the parts the
/// enclosing matches bind. A field read through an accessor is not: the
/// fixed proof splits the accessor's match only after it unfolds the
/// sizes, which leaves the outer size folded.
fn part_of(text: &str, parts: &BTreeSet<String>) -> bool {
    parts.contains(text)
}

/// How a call passes `arg`, reached by `path` in a body whose measured
/// parameter is `param`.
fn passed(arg: &str, param: &str, path: &[Step]) -> Option<Passed> {
    if arg == param {
        return Some(Passed::Same);
    }
    let mut parts = BTreeSet::from([param.to_string()]);
    for step in path {
        if let Step::Match(s, pattern) = step {
            if part_of(s, &parts) {
                parts.extend(pattern[1..].iter().cloned());
            }
        }
    }
    (arg != param && part_of(arg, &parts)).then_some(Passed::Part)
}

/// The rank of each member: above every member it calls on its own
/// argument unchanged, so that such a call still lowers the measure.
/// `None` when such calls go round a cycle.
fn ranks(
    same: &BTreeMap<String, BTreeSet<String>>,
    names: &[String],
) -> Option<BTreeMap<String, usize>> {
    let mut rank: BTreeMap<String, usize> = BTreeMap::new();
    // Each round places the members whose callees are placed: at most one
    // round per member when the calls form no cycle.
    for _ in 0..names.len() {
        for name in names {
            if rank.contains_key(name) {
                continue;
            }
            let callees = same.get(name).cloned().unwrap_or_default();
            if callees.iter().all(|c| rank.contains_key(c)) {
                let r = callees.iter().map(|c| rank[c] + 1).max().unwrap_or(0);
                rank.insert(name.clone(), r);
            }
        }
    }
    (rank.len() == names.len()).then_some(rank)
}

/// A member's measure: its size, taken once more than the highest rank,
/// plus its rank; the size alone when no member has a rank.
fn measure_of(size: &str, term: &str, copies: usize, rank: usize) -> String {
    let one = format!("({size} {term})");
    if copies == 1 {
        return one;
    }
    let many = (1..copies).fold(one.clone(), |sum, _| format!("({sum} + {one})"));
    format!("({many} + {rank})")
}

/// The measures of a mutual group and, per member, the decrease obligation
/// of each call of a member, in call-site order. A call passes the callee
/// a part of the caller's measured argument, or that argument itself to a
/// member of lower rank. The reason LexLean cannot measure the group
/// otherwise.
pub(crate) fn group_obligations(
    members: &[GroupMember<'_>],
    binder_types: &dyn Fn(&str) -> Option<Vec<String>>,
) -> Lift<(Vec<String>, Vec<Vec<Obligation>>)> {
    let refused = |why: &str| Residue::Form(format!("mutual recursion with {why}"));
    let group: BTreeSet<String> = members.iter().map(|m| m.name.to_string()).collect();
    let by_name: BTreeMap<&str, &GroupMember<'_>> = members.iter().map(|m| (m.name, m)).collect();
    let mut found: Vec<Vec<Site>> = Vec::new();
    let mut same: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for m in members {
        let text: Vec<char> = m.body.chars().collect();
        let node = match nodes(&text, 0, None) {
            Some((top, _)) if top.len() == 1 => top.into_iter().next(),
            _ => None,
        };
        let mut calls = Vec::new();
        let mut allowed: Vec<String> = m.params.iter().map(|(p, _)| p.clone()).collect();
        node.as_ref()
            .and_then(|n| {
                sites(
                    n,
                    &group,
                    &mut Vec::new(),
                    &mut allowed,
                    &mut Vec::new(),
                    &mut calls,
                )
            })
            .ok_or_else(|| {
                refused(
                    "a call of a member whose argument or enclosing test names a let-bound local",
                )
            })?;
        for (callee, path, args) in &calls {
            let premises_free = path.iter().all(|step| {
                let text = match step {
                    Step::If(c, _) => c,
                    Step::Match(s, _) => s,
                };
                !group.iter().any(|g| {
                    text.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                        .any(|w| w == g)
                })
            });
            if !premises_free {
                return Err(refused(
                    "a call under a test of another member's result, which no prior theorem can state",
                ));
            }
            let target = by_name[callee.as_str()];
            let Some(arg) = args
                .get(target.measured)
                .filter(|_| args.len() == target.params.len())
            else {
                return Err(refused("a call of a member with another arity"));
            };
            match passed(arg, &m.params[m.measured].0, path) {
                Some(Passed::Same) => {
                    same.entry(m.name.to_string())
                        .or_default()
                        .insert(callee.clone());
                }
                Some(Passed::Part) => {}
                None => {
                    return Err(refused(
                        "a call of a member on a value that is not a part of the caller's argument",
                    ))
                }
            }
        }
        found.push(calls);
    }
    // A parameter every call passes on unchanged is one Lean fixes for the
    // whole group, and its decrease proofs cannot name it.
    let widest = members.iter().map(|m| m.params.len()).max().unwrap_or(0);
    let fixed = (0..widest).any(|j| {
        members.iter().zip(&found).all(|(m, calls)| {
            calls
                .iter()
                .all(|(_, _, args)| m.params.get(j).is_some_and(|(p, _)| args.get(j) == Some(p)))
        })
    });
    if fixed {
        return Err(refused("a parameter every call passes on unchanged, which Lean fixes for the group and its decrease proofs cannot name"));
    }
    let names: Vec<String> = members.iter().map(|m| m.name.to_string()).collect();
    let rank = ranks(&same, &names)
        .ok_or_else(|| refused("members that call each other on the same argument"))?;
    let copies = rank.values().max().map_or(1, |r| r + 1);
    let measure = |m: &GroupMember<'_>, term: &str| measure_of(m.size, term, copies, rank[m.name]);
    let measures: Vec<String> = members
        .iter()
        .map(|m| measure(m, &m.params[m.measured].0))
        .collect();
    let mut out = Vec::new();
    for (m, calls) in members.iter().zip(found) {
        let own = measure(m, &m.params[m.measured].0);
        let mut obligations = Vec::new();
        for (callee, path, args) in calls {
            let target = by_name[callee.as_str()];
            let call = measure(target, &args[target.measured]);
            obligations.push(
                obligation(m.params, &path, &call, &own, binder_types).ok_or_else(|| {
                    refused("a match around a call on a pattern of no known type")
                })?,
            );
        }
        out.push(obligations);
    }
    Ok((measures, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_under_a_constructor_branch_states_its_decrease() {
        let body =
            "(match selfValue | Expr.Flat value1 => true | Expr.If c t o => ((f c) && (f o)))";
        let types = |ctor: &str| match ctor {
            "Expr.Flat" => Some(vec!["String".to_string()]),
            "Expr.If" => Some(vec!["Expr".to_string(); 3]),
            _ => None,
        };
        let params = [("selfValue".to_string(), "Expr".to_string())];
        let out = obligations("f", &params, 0, "size_of_Expr", body, &types).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].1,
            "((selfValue = (ctor Expr.If c t o)) -> ((size_of_Expr c) < (size_of_Expr selfValue)))"
        );
        assert_eq!(out[1].0.len(), 4);
    }

    fn types(ctor: &str) -> Option<Vec<String>> {
        match ctor {
            "E.Leaf" => Some(Vec::new()),
            "E.Node" => Some(vec!["E".to_string(), "E".to_string()]),
            _ => None,
        }
    }

    #[test]
    fn a_member_passing_its_argument_on_ranks_above_its_callee() {
        let x = [("x".to_string(), "E".to_string())];
        let y = [
            ("y".to_string(), "E".to_string()),
            ("n".to_string(), "Nat".to_string()),
        ];
        let members = [
            GroupMember {
                name: "f",
                params: &x,
                measured: 0,
                size: "size_of_E",
                body: "(g x 0)",
            },
            GroupMember {
                name: "g",
                params: &y,
                measured: 0,
                size: "size_of_E",
                body: "(match y | E.Leaf => true | E.Node l r => (f l))",
            },
        ];
        let (measures, owed) = group_obligations(&members, &types).unwrap();
        assert_eq!(
            measures,
            [
                "(((size_of_E x) + (size_of_E x)) + 1)",
                "(((size_of_E y) + (size_of_E y)) + 0)"
            ]
        );
        assert_eq!(
            owed[0][0].1,
            "((((size_of_E x) + (size_of_E x)) + 0) < (((size_of_E x) + (size_of_E x)) + 1))"
        );
        assert_eq!(owed[1][0].0.len(), 4);
    }

    #[test]
    fn a_group_that_does_not_measure_is_refused() {
        let x = [("x".to_string(), "E".to_string())];
        let round = [
            GroupMember {
                name: "f",
                params: &x,
                measured: 0,
                size: "size_of_E",
                body: "(g x)",
            },
            GroupMember {
                name: "g",
                params: &x,
                measured: 0,
                size: "size_of_E",
                body: "(match x | E.Leaf => true | E.Node l r => (f x))",
            },
        ];
        assert!(group_obligations(&round, &types).is_err());
        let xn = [
            ("x".to_string(), "E".to_string()),
            ("n".to_string(), "Nat".to_string()),
        ];
        let fixed = [
            GroupMember {
                name: "f",
                params: &xn,
                measured: 0,
                size: "size_of_E",
                body: "(match x | E.Leaf => true | E.Node l r => (g l n))",
            },
            GroupMember {
                name: "g",
                params: &xn,
                measured: 0,
                size: "size_of_E",
                body: "(match x | E.Leaf => true | E.Node l r => (f r n))",
            },
        ];
        assert!(group_obligations(&fixed, &types).is_err());
    }

    #[test]
    fn a_call_on_a_let_bound_value_is_refused() {
        let body = "(let x : Expr := selfValue; (f x))";
        let params = [("selfValue".to_string(), "Expr".to_string())];
        assert!(obligations("f", &params, 0, "s", body, &|_| None).is_none());
    }
}
