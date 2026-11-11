//! The configuration readers.

use thermo_core::config::parse_target;

/// A whole number parses; anything else is refused.
#[test]
fn parses_a_whole_number() {
    assert_eq!(parse_target(" 21 ").unwrap(), 21);
    assert!(parse_target("warm").is_err());
}
