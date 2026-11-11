//! A library that locks an old smallvec.

/// True: the library is linked.
pub fn linked() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_links() {
        assert!(super::linked());
    }
}
