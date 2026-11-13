//! Functions the MIR path of the lift translates.

/// The sum of the items while it stays below `cap`: a loop whose measure is
/// derived from its exit condition, an index whose bound is a theorem of the
/// assert before it, and an addition whose overflow is excluded by the path.
pub fn capped_sum(xs: &[u32], cap: u32) -> u32 {
    let mut total = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        let x = xs[i];
        if x < cap && total < cap - x {
            total += x;
        }
        i += 1;
    }
    total
}

/// The number of items equal to `needle`.
pub fn count(xs: &[u8], needle: u8) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < xs.len() {
        if xs[i] == needle {
            n += 1;
        }
        i += 1;
    }
    n
}

/// Bump a counter in place: a mutable borrow, outside a value model.
pub fn bump(n: &mut u32) {
    *n = n.wrapping_add(1);
}

/// The largest value present, absent ones counting as zero: std's own
/// `Option::unwrap_or`, lifted from its MIR at `u32`, inside a loop.
pub fn largest(xs: &[Option<u32>]) -> u32 {
    let mut best = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        let x = xs[i].unwrap_or(0);
        if x > best {
            best = x;
        }
        i += 1;
    }
    let floor: &'static u32 = &0;
    if best < *floor {
        return *floor;
    }
    best
}

/// The first line with nothing but whitespace: `str::trim`, whose std body
/// walks bytes with `get_unchecked`, by its model.
pub fn first_blank(lines: &[&str]) -> Option<usize> {
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].trim().is_empty() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// How many distinct names there are: a `BTreeSet` changed through mutable
/// borrows, each `insert` a backward function that returns the new set.
pub fn distinct(a: &str, b: &str) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(a.to_string());
    seen.insert(b.to_string());
    seen.len()
}

/// The first item at or above a floor, seven where none is given: std's own
/// `Option::unwrap_or`, lifted from its MIR at `u32`, and a default the
/// compiler promotes to a constant of its own.
pub fn above(xs: &[u32], floor: Option<u32>) -> Option<usize> {
    let seven: &'static u32 = &7;
    let floor = floor.unwrap_or(*seven);
    let mut i = 0usize;
    while i < xs.len() {
        if xs[i] >= floor {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Whether any item is above a floor: a `for` loop over a slice, whose
/// iterator is a cursor (the items and the two positions it has reached)
/// and whose `next` is `Some` exactly while the front is below the back, so
/// the loop's measure is derived from that guard as a `while` loop's is.
pub fn has_above(xs: &[u32], floor: u32) -> bool {
    for x in xs {
        if *x > floor {
            return true;
        }
    }
    false
}

/// The first item, capped: `?` on an `Option`, whose residual
/// `Option<Infallible>` holds only `None`.
pub fn first_capped(xs: &[u32], cap: u32) -> Option<u32> {
    let x = *xs.first()?;
    Some(x.min(cap))
}

/// The first rise between neighbours: a `for` loop whose state is an
/// option set on each pass, with an early return.
pub fn first_rise(xs: &[u32]) -> Option<u32> {
    let mut prev: Option<u32> = None;
    for x in xs {
        if let Some(p) = prev {
            if *x > p {
                return Some(*x - p);
            }
        }
        prev = Some(*x);
    }
    None
}

/// The sum of the bytes before the first `stop`, saturating: a `for` loop
/// left by `break`.
pub fn sum_until(xs: &[u8], stop: u8) -> u32 {
    let mut total = 0u32;
    for x in xs {
        if *x == stop {
            break;
        }
        total = total.saturating_add(u32::from(*x));
    }
    total
}

/// Whether the second name is in the set: `?` on an `Option` from a slice
/// and a loop over a `BTreeSet`'s members.
pub fn second_known(names: &[&str], known: &std::collections::BTreeSet<String>) -> Option<bool> {
    let second = names.get(1)?;
    for k in known {
        if k == second {
            return Some(true);
        }
    }
    Some(false)
}

/// Whether any item after the leading zeros is above the floor plus a
/// margin: a closure that captures the limit, passed to `Iterator::any`,
/// which slice::Iter defines as a loop on `next`, lifted from std's own MIR
/// with the closure lifted from its own body.
pub fn any_above(xs: &[u32], floor: u32, margin: u32) -> bool {
    let limit = floor.saturating_add(margin);
    let mut zeros = 0usize;
    while zeros < xs.len() {
        if xs[zeros] != 0 {
            break;
        }
        zeros += 1;
    }
    zeros < xs.len() && xs.iter().any(|x| *x > limit)
}

/// The first name that is not blank after a scan for an empty one:
/// `Iterator::find` with a closure.
pub fn first_named<'a>(names: &[&'a str]) -> Option<&'a str> {
    let mut i = 0usize;
    while i < names.len() {
        if names[i].is_empty() {
            return None;
        }
        i += 1;
    }
    names.iter().find(|n| !n.trim().is_empty()).copied()
}
