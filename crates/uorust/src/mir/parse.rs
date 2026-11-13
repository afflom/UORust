//! rustc's printing of MIR, read back into [`super::ir`].
//!
//! rustc prints each statement and terminator on a line of its own, in the
//! fixed notation of `rustc_middle::mir::pretty` for the pinned toolchain.
//! The reader follows that notation; a line in any other shape is kept as
//! the kind it names, so the lowering can make it a typed residue.

use super::ir::{
    IrAggregate, IrBlock, IrBlockId, IrBody, IrConstant, IrLocal, IrOperand, IrPlace, IrProjection,
    IrRvalue, IrStatement, IrTerminator,
};
use super::spec;

/// Every function body in the text of one dump file, or of many dump files
/// joined.
pub(crate) fn bodies(text: &str) -> Vec<IrBody> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let mut body = if let Some(head) = line.strip_prefix("fn ") {
            header(head)
        } else if let Some(head) = line.strip_prefix("const ") {
            match const_header(head) {
                Some(b) => b,
                None => continue,
            }
        } else {
            continue;
        };
        let mut current: Option<(IrBlockId, Vec<IrStatement>)> = None;
        let mut blocks: Vec<(IrBlockId, IrBlock)> = Vec::new();
        for line in lines.by_ref() {
            if line == "}" {
                break;
            }
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("debug ") {
                if let Some((name, value)) = rest.trim_end_matches(';').split_once(" => ") {
                    if let Some(local) = local_of(value.trim()) {
                        if local < body.names.len() && body.names[local].is_none() {
                            body.names[local] = Some(name.trim().to_string());
                        }
                    } else if let Some((k, ty)) = upvar_of(value.trim()) {
                        if body.upvars.len() <= k {
                            body.upvars.resize(k + 1, None);
                        }
                        body.upvars[k] = Some((name.trim().to_string(), ty));
                    }
                }
            } else if let Some(rest) = t.strip_prefix("let ") {
                let rest = rest.strip_prefix("mut ").unwrap_or(rest);
                if let Some((l, ty)) = rest.trim_end_matches(';').split_once(": ") {
                    if let Some(local) = local_of(l) {
                        if body.locals.len() <= local {
                            body.locals.resize(local + 1, String::new());
                            body.names.resize(local + 1, None);
                        }
                        body.locals[local] = ty.to_string();
                    }
                }
            } else if let Some(id) = t.strip_prefix("bb").and_then(|r| r.strip_suffix(": {")) {
                let id = id.split_whitespace().next().unwrap_or(id);
                current = id.parse().ok().map(|id| (id, Vec::new()));
            } else if t == "}" {
                // The end of a block, or of a scope.
            } else if let Some((id, statements)) = current.as_mut() {
                match terminator(t) {
                    Some(term) => {
                        blocks.push((
                            *id,
                            IrBlock {
                                statements: std::mem::take(statements),
                                terminator: term,
                            },
                        ));
                        current = None;
                    }
                    None => statements.push(statement(t)),
                }
            }
        }
        blocks.sort_by_key(|(id, _)| *id);
        if blocks.iter().enumerate().all(|(i, (id, _))| i == *id) {
            body.blocks = blocks.into_iter().map(|(_, b)| b).collect();
            out.push(body);
        }
    }
    out
}

/// The header after `fn `: the path, the parameters and their types.
/// `path: Type = {`, the head of a constant's body (a promoted constant or
/// a constant item): no parameters, its value the return place.
fn const_header(head: &str) -> Option<IrBody> {
    let head = head.strip_suffix(" = {")?;
    let mut depth = 0i64;
    let mut prev = ' ';
    for (i, c) in head.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if prev != '-' => depth -= 1,
            ':' if depth == 0 && head[i + 1..].starts_with(' ') => {
                let mut body = IrBody {
                    path: head[..i].to_string(),
                    ..IrBody::default()
                };
                body.locals.push(head[i + 2..].trim().to_string());
                body.names.push(None);
                return Some(body);
            }
            _ => {}
        }
        prev = c;
    }
    None
}

fn header(head: &str) -> IrBody {
    let open = head.find('(').unwrap_or(head.len());
    // `<impl at file:1:2: 3:4>::name(` holds no `(` before the parameters.
    let path = head[..open].to_string();
    let inside = head[open..]
        .strip_prefix('(')
        .and_then(|r| matching(r, '(', ')'))
        .unwrap_or_default();
    let params = split_top(inside, ',');
    let mut body = IrBody {
        path,
        arity: params.iter().filter(|p| !p.trim().is_empty()).count(),
        ..IrBody::default()
    };
    body.locals.resize(body.arity + 1, String::new());
    body.names.resize(body.arity + 1, None);
    for p in params {
        if let Some((l, ty)) = p.split_once(": ") {
            if let Some(local) = local_of(l.trim()) {
                if local < body.locals.len() {
                    body.locals[local] = ty.trim().to_string();
                }
            }
        }
    }
    body
}

/// The text inside a bracket opened just before `text`, up to its match.
fn matching(text: &str, open: char, close: char) -> Option<&str> {
    let mut depth = 1usize;
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if quoted {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// `text` split at `sep` where no bracket or string is open.
pub(crate) fn split_top(text: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i64, 0usize);
    let mut quoted = false;
    let mut escaped = false;
    let mut prev = ' ';
    for (i, c) in text.char_indices() {
        if quoted {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            prev = c;
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' | '[' | '{' | '<' => depth += 1,
            // `->` is an arrow, not a closing bracket.
            '>' if prev == '-' => {}
            ')' | ']' | '}' | '>' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(text[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
        prev = c;
    }
    let last = text[start..].trim();
    if !last.is_empty() || !out.is_empty() {
        out.push(last);
    }
    out
}

/// `_n` as a local.
fn local_of(text: &str) -> Option<IrLocal> {
    text.strip_prefix('_')?.parse().ok()
}

/// A closure's capture as debug info prints it: `((*_1).k: T)`,
/// `(*((*_1).k: T))` or `(_1.k: T)`, the field `k` of the environment `_1`
/// and its type.
fn upvar_of(value: &str) -> Option<(usize, String)> {
    let at = value
        .find("(*_1).")
        .map(|i| i + "(*_1).".len())
        .or_else(|| value.find("(_1.").map(|i| i + "(_1.".len()))?;
    let (k, rest) = value[at..].split_once(": ")?;
    let k: usize = k.parse().ok()?;
    // The type runs to the parenthesis that closes the projection.
    let mut depth = 0usize;
    for (i, c) in rest.char_indices() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' if depth == 0 => return Some((k, rest[..i].to_string())),
            ')' | '>' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// `bbn` as a block.
fn block_of(text: &str) -> Option<IrBlockId> {
    text.trim().strip_prefix("bb")?.parse().ok()
}

/// The block an edge list names under `label`: `[return: bb4, unwind ...]`.
fn edge(edges: &str, label: &str) -> Option<IrBlockId> {
    let inner = edges.trim().strip_prefix('[')?.strip_suffix(']')?;
    split_top(inner, ',')
        .into_iter()
        .find_map(|e| e.strip_prefix(label)?.strip_prefix(": ").and_then(block_of))
}

/// The kind a statement is, from the word it starts with.
fn statement_kind(t: &str) -> spec::MirStatementKind {
    use spec::MirStatementKind as S;
    let word: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    match word.as_str() {
        "StorageLive" => S::StorageLive,
        "StorageDead" => S::StorageDead,
        "FakeRead" => S::FakeRead,
        "PlaceMention" => S::PlaceMention,
        "AscribeUserType" => S::AscribeUserType,
        "Coverage" => S::Coverage,
        "ConstEvalCounter" => S::ConstEvalCounter,
        "nop" => S::Nop,
        "BackwardIncompatibleDropHint" => S::BackwardIncompatibleDropHint,
        "assume" | "copy_nonoverlapping" => S::Intrinsic,
        "discriminant" => S::SetDiscriminant,
        _ => S::Assign,
    }
}

fn statement(t: &str) -> IrStatement {
    use spec::MirStatementKind as S;
    let body = t.strip_suffix(';').unwrap_or(t);
    let kind = statement_kind(body);
    match kind {
        S::StorageLive
        | S::StorageDead
        | S::FakeRead
        | S::PlaceMention
        | S::AscribeUserType
        | S::Coverage
        | S::ConstEvalCounter
        | S::Nop
        | S::BackwardIncompatibleDropHint => IrStatement::Marker(kind),
        S::Intrinsic => match body
            .strip_prefix("assume(")
            .and_then(|r| r.strip_suffix(')'))
            .and_then(operand)
        {
            Some(op) => IrStatement::Assume(op),
            None => IrStatement::Other(kind, body.to_string()),
        },
        S::Assign => {
            let Some((lhs, rhs)) = body.split_once(" = ") else {
                return IrStatement::Other(kind, body.to_string());
            };
            match (place(lhs), rvalue(rhs)) {
                (Some(p), r) => IrStatement::Assign(p, r),
                (None, _) => IrStatement::Other(kind, body.to_string()),
            }
        }
        S::SetDiscriminant => IrStatement::Other(kind, body.to_string()),
    }
}

/// A place: `_1`, `(*_1)`, `(_1.0: T)`, `(_1 as V)`, `_1[_2]`, and those
/// nested.
pub(crate) fn place(text: &str) -> Option<IrPlace> {
    let text = text.trim();
    // An index, constant index or subslice step at the end.
    if let Some(stripped) = text.strip_suffix(']') {
        let open = stripped.rfind('[')?;
        let (base, step) = (&stripped[..open], &stripped[open + 1..]);
        let mut p = place(base)?;
        p.projection.push(match local_of(step) {
            Some(l) => IrProjection::Index(l),
            None if step.contains(" of ") => {
                let (n, _) = step.split_once(" of ")?;
                match n.trim().strip_prefix('-') {
                    Some(back) => IrProjection::ConstantIndex(back.parse().ok()?, true),
                    None => IrProjection::ConstantIndex(n.trim().parse().ok()?, false),
                }
            }
            None => IrProjection::Other(spec::MirProjectionElem::Subslice, step.to_string()),
        });
        return Some(p);
    }
    if let Some(l) = local_of(text) {
        return Some(IrPlace {
            local: l,
            projection: Vec::new(),
        });
    }
    let inner = text.strip_prefix('(')?.strip_suffix(')')?;
    if let Some(rest) = inner.strip_prefix('*') {
        let mut p = place(rest)?;
        p.projection.push(IrProjection::Deref);
        return Some(p);
    }
    // `(P.i: T)`: the last top-level `.` before the `: `.
    if let Some((left, ty)) = split_type(inner) {
        let dot = left.rfind('.')?;
        let (base, index) = (&left[..dot], &left[dot + 1..]);
        let mut p = place(base)?;
        p.projection
            .push(IrProjection::Field(index.parse().ok()?, ty.to_string()));
        return Some(p);
    }
    if let Some(at) = inner.rfind(" as ") {
        let (base, what) = (&inner[..at], &inner[at + 4..]);
        let mut p = place(base)?;
        p.projection.push(match what.strip_prefix("subtype ") {
            Some(t) => IrProjection::Other(spec::MirProjectionElem::OpaqueCast, t.to_string()),
            None => IrProjection::Downcast(what.to_string()),
        });
        return Some(p);
    }
    None
}

/// `P.i: T` split at the `: ` that is not inside a bracket of `P`.
fn split_type(inner: &str) -> Option<(&str, &str)> {
    let mut depth = 0i64;
    let bytes = inner.as_bytes();
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth -= 1,
            ':' if depth == 0 && bytes.get(i + 1) == Some(&b' ') => {
                return Some((&inner[..i], &inner[i + 2..]));
            }
            _ => {}
        }
    }
    None
}

/// An operand: `copy P`, `move P`, `const C`.
pub(crate) fn operand(text: &str) -> Option<IrOperand> {
    let t = text.trim();
    let t = t.strip_prefix("no_retag ").unwrap_or(t);
    if let Some(p) = t.strip_prefix("copy ") {
        return place(p).map(IrOperand::Copy);
    }
    if let Some(p) = t.strip_prefix("move ") {
        return place(p).map(IrOperand::Move);
    }
    if let Some(c) = t.strip_prefix("const ") {
        return Some(match c {
            "OverflowChecks" => IrOperand::Check(spec::MirRuntimeChecks::OverflowChecks),
            "UbChecks" => IrOperand::Check(spec::MirRuntimeChecks::UbChecks),
            "ContractChecks" => IrOperand::Check(spec::MirRuntimeChecks::ContractChecks),
            _ => IrOperand::Constant(constant(c)),
        });
    }
    None
}

fn constant(c: &str) -> IrConstant {
    match c {
        "true" => return IrConstant::Bool(true),
        "false" => return IrConstant::Bool(false),
        "()" => return IrConstant::Unit,
        _ => {}
    }
    if let Some(s) = c.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return match unescape(s) {
            Some(s) => IrConstant::Str(s),
            None => IrConstant::Other(c.to_string()),
        };
    }
    if let Some(s) = c.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        let mut chars = unescape(s)
            .unwrap_or_default()
            .chars()
            .collect::<Vec<char>>();
        if chars.len() == 1 {
            return IrConstant::Char(chars.remove(0));
        }
        return IrConstant::Other(c.to_string());
    }
    if let Some((digits, suffix)) = c.split_once('_') {
        let int = suffix.starts_with(['u', 'i'])
            && suffix[1..].chars().all(|d| d.is_ascii_alphanumeric());
        if let (true, Ok(n)) = (int, digits.parse::<i128>()) {
            return IrConstant::Int(n, suffix.to_string());
        }
    }
    let path = c
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_:<>, &'".contains(ch));
    if path && !c.is_empty() {
        IrConstant::Path(c.to_string())
    } else {
        IrConstant::Other(c.to_string())
    }
}

/// The text of a Rust string literal's body, unescaped.
fn unescape(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '0' => out.push('\0'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            'u' => {
                let hex: String = chars
                    .by_ref()
                    .skip_while(|c| *c == '{')
                    .take_while(|c| *c != '}')
                    .collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The name a call or rvalue starts with, up to its `(`.
fn head(text: &str) -> &str {
    let mut depth = 0i64;
    for (i, c) in text.char_indices() {
        match c {
            '<' | '{' | '[' => depth += 1,
            '>' | '}' | ']' => depth -= 1,
            '(' if depth == 0 => return &text[..i],
            _ => {}
        }
    }
    text
}

fn rvalue(text: &str) -> IrRvalue {
    use spec::MirRvalue as R;
    let t = text.trim();
    let other = |k: R| IrRvalue::Other(k, t.to_string());
    if let Some(rest) = t.strip_prefix("&raw ") {
        let _ = rest;
        return other(R::RawPtr);
    }
    if let Some(rest) = t.strip_prefix("&fake ") {
        let p = rest.split_once(' ').map_or(rest, |(_, p)| p);
        return place(p).map_or(other(R::Ref), |p| {
            IrRvalue::Ref(spec::MirBorrowKind::Fake, p)
        });
    }
    if let Some(rest) = t.strip_prefix("&mut ") {
        return place(rest).map_or(other(R::Ref), |p| {
            IrRvalue::Ref(spec::MirBorrowKind::Mut, p)
        });
    }
    if let Some(rest) = t.strip_prefix('&') {
        return place(rest).map_or(other(R::Ref), |p| {
            IrRvalue::Ref(spec::MirBorrowKind::Shared, p)
        });
    }
    if let Some(op) = operand(t) {
        return IrRvalue::Use(op);
    }
    // `move _1 as u64 (IntToInt)`.
    if t.ends_with(')') {
        if let Some(at) = t.find(" as ") {
            if let Some(open) = t.rfind(" (") {
                let kind_text = &t[open + 2..t.len() - 1];
                let word = head(kind_text);
                let kind = spec::MirCastKind::ALL
                    .iter()
                    .find(|k| k.path().rsplit("::").next() == Some(word));
                if let (Some(kind), Some(op)) = (kind, operand(&t[..at])) {
                    return IrRvalue::Cast(*kind, op, t[at + 4..open].to_string());
                }
                if kind.is_some() {
                    return other(R::Cast);
                }
            }
        }
    }
    let name = head(t);
    let args_text = t
        .get(name.len() + 1..)
        .and_then(|r| r.strip_suffix(')'))
        .filter(|_| t.len() > name.len());
    if let Some(args_text) = args_text {
        let args = split_top(args_text, ',');
        if let Some(op) = spec::MirBinOp::ALL
            .iter()
            .find(|k| k.path().rsplit("::").next() == Some(name))
        {
            return match args.as_slice() {
                [a, b] => match (operand(a), operand(b)) {
                    (Some(a), Some(b)) => IrRvalue::Binary(*op, a, b),
                    _ => other(R::BinaryOp),
                },
                _ => other(R::BinaryOp),
            };
        }
        if let Some(op) = spec::MirUnOp::ALL
            .iter()
            .find(|k| k.path().rsplit("::").next() == Some(name))
        {
            return match args.as_slice() {
                [a] => operand(a).map_or(other(R::UnaryOp), |a| IrRvalue::Unary(*op, a)),
                _ => other(R::UnaryOp),
            };
        }
        match name {
            "discriminant" => {
                return place(args_text).map_or(other(R::Discriminant), IrRvalue::Discriminant)
            }
            "CopyForDeref" => {
                return place(args_text).map_or(other(R::CopyForDeref), IrRvalue::CopyForDeref)
            }
            _ => {}
        }
    }
    // Aggregates: a closure, its type and then its captures,
    // `{closure@a.rs:1:2: 1:5} { x: move _3 }`.
    if t.starts_with("{closure@") {
        let Some(end) = t.find('}') else {
            return other(R::Aggregate);
        };
        let rest = t[end + 1..].trim();
        let mut ops = Vec::new();
        if let Some(inner) = rest.strip_prefix("{ ").and_then(|r| r.strip_suffix(" }")) {
            for f in split_top(inner, ',') {
                let Some((field, op)) = f.split_once(": ") else {
                    return other(R::Aggregate);
                };
                let Some(op) = operand(op) else {
                    return other(R::Aggregate);
                };
                ops.push((Some(field.trim().to_string()), op));
            }
        } else if !rest.is_empty() {
            return other(R::Aggregate);
        }
        return IrRvalue::Aggregate(IrAggregate::Closure(t[..=end].to_string()), ops);
    }
    if t.starts_with("{coroutine@") {
        return IrRvalue::Aggregate(
            IrAggregate::Other(spec::MirAggregateKind::Coroutine),
            Vec::new(),
        );
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        if split_top(inner, ';').len() == 2 {
            return other(R::Repeat);
        }
        return operands(inner).map_or(other(R::Aggregate), |ops| {
            IrRvalue::Aggregate(IrAggregate::Array, ops)
        });
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        return operands(inner).map_or(other(R::Aggregate), |ops| {
            IrRvalue::Aggregate(IrAggregate::Tuple, ops)
        });
    }
    // `path::V(a, b)`, `path { f: a }`, or a bare `path` of a unit variant.
    if let Some(args_text) = args_text {
        return operands(args_text).map_or(other(R::Aggregate), |ops| {
            IrRvalue::Aggregate(adt(name), ops)
        });
    }
    if let Some(open) = t.find(" { ") {
        if let Some(inner) = t[open + 3..].strip_suffix(" }") {
            let mut ops = Vec::new();
            for f in split_top(inner, ',') {
                let Some((field, op)) = f.split_once(": ") else {
                    return other(R::Aggregate);
                };
                let Some(op) = operand(op) else {
                    return other(R::Aggregate);
                };
                ops.push((Some(field.to_string()), op));
            }
            return IrRvalue::Aggregate(IrAggregate::Adt(t[..open].to_string(), None), ops);
        }
    }
    if t.chars()
        .all(|c| c.is_ascii_alphanumeric() || "_:<>, &'".contains(c))
    {
        return IrRvalue::Aggregate(adt(t), Vec::new());
    }
    other(R::Use)
}

/// An enum variant or a struct by its printed path: `a::E::V`,
/// `std::option::Option::<u8>::Some`, `a::S`.
fn adt(path: &str) -> IrAggregate {
    // The last segment is the variant when the one before it names a type
    // (an upper-case segment, possibly with generic arguments).
    let segments = split_path(path);
    match segments.as_slice() {
        [.., ty, v]
            if ty.starts_with(|c: char| c.is_ascii_uppercase())
                && v.starts_with(|c: char| c.is_ascii_uppercase()) =>
        {
            let ty_end = path.len() - v.len() - 2;
            IrAggregate::Adt(path[..ty_end].to_string(), Some((*v).to_string()))
        }
        _ => IrAggregate::Adt(path.to_string(), None),
    }
}

/// A path's segments at the top level, generic arguments kept with theirs
/// (`Option::<u8>` is one segment).
pub(crate) fn split_path(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i64, 0usize);
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => depth -= 1,
            b':' if depth == 0 && bytes.get(i + 1) == Some(&b':') => {
                // `::<` opens the generic arguments of this segment, except
                // `::<impl X>`, a segment of its own.
                if bytes.get(i + 2) != Some(&b'<') || path[i + 2..].starts_with("<impl ") {
                    out.push(&path[start..i]);
                    start = i + 2;
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&path[start..]);
    out
}

fn operands(text: &str) -> Option<Vec<(Option<String>, IrOperand)>> {
    if text.trim().is_empty() {
        return Some(Vec::new());
    }
    // A one-tuple is printed with a trailing comma: `(move _1,)`.
    split_top(text, ',')
        .into_iter()
        .filter(|o| !o.is_empty())
        .map(|o| operand(o).map(|o| (None, o)))
        .collect()
}

/// The assert kind its message names, as `AssertKind`'s `Display` words it.
fn assert_kind(message: &str) -> spec::MirAssertKind {
    use spec::MirAssertKind as A;
    let m = message.trim_start_matches('"');
    if m.starts_with("index out of bounds") {
        A::BoundsCheck
    } else if m.starts_with("attempt to divide") && m.contains("by zero") {
        A::DivisionByZero
    } else if m.starts_with("attempt to calculate the remainder") && m.contains("zero") {
        A::RemainderByZero
    } else if m.starts_with("attempt to negate") {
        A::OverflowNeg
    } else if m.starts_with("attempt to") {
        A::Overflow
    } else if m.starts_with("misaligned pointer") {
        A::MisalignedPointerDereference
    } else if m.starts_with("null pointer") {
        A::NullPointerDereference
    } else if m.contains("after panicking") {
        A::ResumedAfterPanic
    } else if m.contains("after completion") {
        A::ResumedAfterReturn
    } else if m.contains("after drop") {
        A::ResumedAfterDrop
    } else {
        A::InvalidEnumConstruction
    }
}

/// A terminator line, or `None` for a statement.
fn terminator(t: &str) -> Option<IrTerminator> {
    use spec::MirTerminatorKind as K;
    let body = t.strip_suffix(';')?;
    let other = |k: K| Some(IrTerminator::Other(k, body.to_string()));
    match body {
        "return" => return Some(IrTerminator::Return),
        "unreachable" => return Some(IrTerminator::Unreachable),
        "resume" => return other(K::UnwindResume),
        "coroutine_drop" => return other(K::CoroutineDrop),
        _ => {}
    }
    if body.starts_with("terminate") || body.starts_with("abort") {
        return other(K::UnwindTerminate);
    }
    if let Some(target) = body.strip_prefix("goto -> ") {
        return block_of(target).map(IrTerminator::Goto).or(other(K::Goto));
    }
    let (left, edges) = match body.rfind(" -> ") {
        Some(at) => (&body[..at], &body[at + 4..]),
        None => {
            return if body.starts_with("yield") {
                other(K::Yield)
            } else if body.starts_with("asm!") {
                other(K::InlineAsm)
            } else if body.starts_with("tailcall ") {
                other(K::TailCall)
            } else {
                None
            };
        }
    };
    // `-> bbN` or `-> unwind ...` for a single edge.
    let single = block_of(edges);
    if let Some(inner) = left
        .strip_prefix("switchInt(")
        .and_then(|r| r.strip_suffix(')'))
    {
        let list = edges.strip_prefix('[')?.strip_suffix(']')?;
        let mut arms = Vec::new();
        let mut otherwise = None;
        for e in split_top(list, ',') {
            let (v, b) = e.split_once(": ")?;
            let b = block_of(b)?;
            if v == "otherwise" {
                otherwise = Some(b);
            } else {
                arms.push((v.parse().ok()?, b));
            }
        }
        return match (operand(inner), otherwise) {
            (Some(op), Some(o)) => Some(IrTerminator::Switch(op, arms, o)),
            _ => other(K::SwitchInt),
        };
    }
    if left.starts_with("falseEdge") {
        return edge(edges, "real")
            .map(|b| IrTerminator::False(K::FalseEdge, b))
            .or(other(K::FalseEdge));
    }
    if left.starts_with("falseUnwind") {
        return edge(edges, "real")
            .map(|b| IrTerminator::False(K::FalseUnwind, b))
            .or(other(K::FalseUnwind));
    }
    if left.starts_with("drop(") {
        return edge(edges, "return")
            .or(single)
            .map(IrTerminator::Drop)
            .or(other(K::Drop));
    }
    if let Some(inner) = left
        .strip_prefix("assert(")
        .and_then(|r| r.strip_suffix(')'))
    {
        let parts = split_top(inner, ',');
        let (Some(cond), Some(message)) = (parts.first(), parts.get(1)) else {
            return other(K::Assert);
        };
        let (cond, expected) = match cond.strip_prefix('!') {
            Some(c) => (c, false),
            None => (*cond, true),
        };
        return match (operand(cond), edge(edges, "success").or(single)) {
            (Some(condition), Some(target)) => Some(IrTerminator::Assert {
                condition,
                expected,
                kind: assert_kind(message),
                target,
            }),
            _ => other(K::Assert),
        };
    }
    if left.starts_with("tailcall ") {
        return other(K::TailCall);
    }
    if left.starts_with("yield") {
        return other(K::Yield);
    }
    if left.starts_with("asm!") {
        return other(K::InlineAsm);
    }
    // A call: `dest = callee(args) -> [return: bbN, unwind ...]`.
    let (dest, call) = left.split_once(" = ")?;
    let callee = head(call);
    let args_text = call.get(callee.len() + 1..)?.strip_suffix(')')?;
    let destination = place(dest)?;
    let mut args = Vec::new();
    for a in split_top(args_text, ',') {
        if a.is_empty() {
            continue;
        }
        match operand(a) {
            Some(o) => args.push(o),
            // A function item passed by name is a constant of its type.
            None => args.push(IrOperand::Constant(IrConstant::Path(a.to_string()))),
        }
    }
    Some(IrTerminator::Call {
        destination,
        callee: callee.to_string(),
        args,
        target: edge(edges, "return").or(single),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUM: &str = "// MIR for `sum` before ElaborateBoxDerefs

fn sum(_1: &[u64], _2: (u8, bool)) -> u64 {
    debug xs => _1;
    let mut _0: u64;
    let mut _3: usize;
    let mut _4: bool;
    let mut _5: (usize, bool);
    let mut _6: usize;
    scope 1 {
        debug i => _3;
    }

    bb0: {
        StorageLive(_3);
        _3 = const 0_usize;
        goto -> bb1;
    }

    bb1: {
        _6 = PtrMetadata(copy _1);
        _4 = Lt(copy _3, move _6);
        switchInt(move _4) -> [0: bb3, otherwise: bb2];
    }

    bb2: {
        _5 = AddWithOverflow(copy _3, const 1_usize);
        assert(!move (_5.1: bool), \"attempt to compute `{} + {}`, which would overflow\", copy _3, const 1_usize) -> [success: bb4, unwind continue];
    }

    bb3: {
        _0 = core::num::<impl u64>::wrapping_add(copy (*_1)[_3], const 2_u64) -> [return: bb5, unwind continue];
    }

    bb4: {
        _3 = move (_5.0: usize);
        falseUnwind -> [real: bb1, unwind: bb6];
    }

    bb5: {
        return;
    }

    bb6 (cleanup): {
        resume;
    }
}
";

    #[test]
    fn a_dump_reads_into_blocks_locals_and_kinds() {
        let bodies = bodies(SUM);
        assert_eq!(bodies.len(), 1);
        let b = &bodies[0];
        assert_eq!(b.path, "sum");
        assert_eq!(b.arity, 2);
        assert_eq!(b.locals[1], "&[u64]");
        assert_eq!(b.locals[5], "(usize, bool)");
        assert_eq!(b.names[3].as_deref(), Some("i"));
        assert_eq!(b.blocks.len(), 7);
        assert_eq!(
            b.blocks[0].statements[0],
            IrStatement::Marker(spec::MirStatementKind::StorageLive)
        );
        assert!(matches!(
            &b.blocks[2].terminator,
            IrTerminator::Assert {
                expected: false,
                kind: spec::MirAssertKind::Overflow,
                target: 4,
                ..
            }
        ));
        assert!(matches!(
            &b.blocks[4].terminator,
            IrTerminator::False(spec::MirTerminatorKind::FalseUnwind, 1)
        ));
        assert!(matches!(
            &b.blocks[6].terminator,
            IrTerminator::Other(spec::MirTerminatorKind::UnwindResume, _)
        ));
        let IrTerminator::Call {
            callee,
            args,
            target,
            ..
        } = &b.blocks[3].terminator
        else {
            panic!("a call");
        };
        assert_eq!(callee, "core::num::<impl u64>::wrapping_add");
        assert_eq!(*target, Some(5));
        assert_eq!(
            args[0],
            IrOperand::Copy(IrPlace {
                local: 1,
                projection: vec![IrProjection::Deref, IrProjection::Index(3)]
            })
        );
    }

    #[test]
    fn places_nest_fields_downcasts_and_derefs() {
        assert_eq!(
            place("(((*_1) as Node).0: std::boxed::Box<T>)"),
            Some(IrPlace {
                local: 1,
                projection: vec![
                    IrProjection::Deref,
                    IrProjection::Downcast("Node".into()),
                    IrProjection::Field(0, "std::boxed::Box<T>".into())
                ]
            })
        );
        assert_eq!(
            place("_2[1 of 3]").map(|p| p.projection),
            Some(vec![IrProjection::ConstantIndex(1, false)])
        );
    }

    #[test]
    fn rvalues_read_as_their_kinds() {
        assert!(
            matches!(rvalue("move _1 as u64 (IntToInt)"), IrRvalue::Cast(spec::MirCastKind::IntToInt, _, t) if t == "u64")
        );
        assert!(
            matches!(rvalue("std::option::Option::<u8>::Some(move _3)"), IrRvalue::Aggregate(IrAggregate::Adt(t, Some(v)), _) if t == "std::option::Option::<u8>" && v == "Some")
        );
        assert!(
            matches!(rvalue("std::option::Option::<u8>::None"), IrRvalue::Aggregate(IrAggregate::Adt(_, Some(v)), ops) if v == "None" && ops.is_empty())
        );
        assert!(
            matches!(rvalue("a::S { v: copy _1 }"), IrRvalue::Aggregate(IrAggregate::Adt(t, None), ops) if t == "a::S" && ops.len() == 1)
        );
        assert!(
            matches!(rvalue("(copy _1, const true)"), IrRvalue::Aggregate(IrAggregate::Tuple, ops) if ops.len() == 2)
        );
        assert!(matches!(
            rvalue("[const 0_u8; 4]"),
            IrRvalue::Other(spec::MirRvalue::Repeat, _)
        ));
        assert!(matches!(
            rvalue("&raw const (*_1)"),
            IrRvalue::Other(spec::MirRvalue::RawPtr, _)
        ));
        assert!(matches!(
            rvalue("&mut _1"),
            IrRvalue::Ref(spec::MirBorrowKind::Mut, _)
        ));
        assert!(matches!(
            rvalue("discriminant((*_1))"),
            IrRvalue::Discriminant(_)
        ));
        assert!(
            matches!(rvalue("const \"a\\\"b\""), IrRvalue::Use(IrOperand::Constant(IrConstant::Str(s))) if s == "a\"b")
        );
        assert!(
            matches!(rvalue("const -3_i32"), IrRvalue::Use(IrOperand::Constant(IrConstant::Int(-3, s))) if s == "i32")
        );
    }

    #[test]
    fn paths_split_outside_generic_arguments() {
        assert_eq!(
            split_path("std::option::Option::<std::string::String>::Some"),
            ["std", "option", "Option::<std::string::String>", "Some"]
        );
        assert_eq!(
            split_path("core::num::<impl u64>::wrapping_add"),
            ["core", "num", "<impl u64>", "wrapping_add"]
        );
    }
}
