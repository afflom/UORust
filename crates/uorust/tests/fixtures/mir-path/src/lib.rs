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
