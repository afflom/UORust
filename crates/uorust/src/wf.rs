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

/// The enclosing branches and the argument texts of one recursive call.
type Site = (Vec<Step>, Vec<String>);

/// Every word a node holds.
fn words(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Word(w) => out.push(w.clone()),
        Node::Form(inner, _) => inner.iter().for_each(|n| words(n, out)),
        Node::Group(text) => out.extend(
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .map(String::from),
        ),
    }
}

fn mentions(node: &Node, name: &str) -> bool {
    let mut ws = Vec::new();
    words(node, &mut ws);
    ws.iter().any(|w| w == name)
}

/// The recursive calls of `name` in `node`, in pre-order, or `None` where
/// LexLean refuses the recursion: a call inside a lambda or another call's
/// argument, or an argument or an enclosing test naming a local that is not
/// a parameter or an enclosing match binder.
fn sites(
    node: &Node,
    name: &str,
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
        Some(f) if f == name => {
            let args = &inner[1..];
            if args
                .iter()
                .any(|a| mentions(a, name) || !local_free(a, lets))
            {
                return None;
            }
            out.push((
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
            sites(c, name, path, allowed, lets, out)?;
            let encloses = mentions(a, name) || mentions(b, name);
            if encloses && !local_free(c, lets) {
                return None;
            }
            for (branch, polarity) in [(a, true), (b, false)] {
                path.push(Step::If(c.text().to_string(), polarity));
                sites(branch, name, path, allowed, lets, out)?;
                path.pop();
            }
        }
        Some("match") => {
            let scrutinee = inner.get(1)?;
            sites(scrutinee, name, path, allowed, lets, out)?;
            let encloses = inner[2..].iter().any(|n| mentions(n, name));
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
                sites(body, name, path, allowed, lets, out)?;
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
                sites(n, name, path, allowed, lets, out)?;
            }
            lets.push(bound);
            for n in &inner[semi + 1..] {
                sites(n, name, path, allowed, lets, out)?;
            }
            lets.pop();
        }
        Some("fun") => {
            if mentions(node, name) {
                return None;
            }
        }
        _ => {
            for n in inner {
                sites(n, name, path, allowed, lets, out)?;
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
        name,
        &mut Vec::new(),
        &mut allowed,
        &mut Vec::new(),
        &mut found,
    )?;
    let measure = format!("({size} {})", params[arg].0);
    let mut out = Vec::new();
    for (path, args) in found {
        if args.len() != params.len() {
            return None;
        }
        let mut binders = Vec::new();
        let mut premises = Vec::new();
        for step in path {
            match step {
                Step::If(c, polarity) => premises.push(format!("({c} = {polarity})")),
                Step::Match(s, pattern) => {
                    let ctor = &pattern[0];
                    let tys = binder_types(ctor)?;
                    if tys.len() + 1 != pattern.len() {
                        return None;
                    }
                    for (b, t) in pattern[1..].iter().zip(tys) {
                        binders.push((b.clone(), t));
                    }
                    let value: String = std::iter::once(format!("(ctor {ctor}"))
                        .chain(pattern[1..].iter().map(|b| format!(" {b}")))
                        .collect();
                    premises.push(format!("({s} = {value}))"));
                }
            }
        }
        let mut statement = format!("(({size} {}) < {measure})", args[arg]);
        for p in premises.into_iter().rev() {
            statement = format!("({p} -> {statement})");
        }
        let mut all = params.to_vec();
        all.extend(binders);
        out.push((all, statement));
    }
    (!out.is_empty()).then_some(out)
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

    #[test]
    fn a_call_on_a_let_bound_value_is_refused() {
        let body = "(let x : Expr := selfValue; (f x))";
        let params = [("selfValue".to_string(), "Expr".to_string())];
        assert!(obligations("f", &params, 0, "s", body, &|_| None).is_none());
    }
}
