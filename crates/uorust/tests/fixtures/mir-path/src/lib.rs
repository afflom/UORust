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
