//! Generated from `crates/uorust/mir/std.toml` by the test
//! `std_is_vendored_from_the_pinned_toolchain`. Do not edit: each assertion
//! runs std itself on a vector of a model the MIR path uses.

#[test]
fn std_agrees_with_every_vector_of_its_models() {
    assert_eq!(str::trim("  a b  "), "a b", "core::str::<impl str>::trim");
    assert_eq!(str::trim(""), "", "core::str::<impl str>::trim");
    assert_eq!(str::trim_start("  a "), "a ", "core::str::<impl str>::trim_start");
    assert_eq!(str::trim_end("  a "), "  a", "core::str::<impl str>::trim_end");
    assert_eq!(str::len("héllo"), 6, "core::str::<impl str>::len");
    assert!(str::starts_with("prism", "pri"), "core::str::<impl str>::starts_with");
    assert!(!str::starts_with("prism", "ism"), "core::str::<impl str>::starts_with");
    assert!(str::ends_with("prism", "ism"), "core::str::<impl str>::ends_with");
    assert!(!str::ends_with("prism", "pri"), "core::str::<impl str>::ends_with");
    assert!(str::contains("a::b", "::"), "core::str::<impl str>::contains");
    assert!(!str::contains("ab", "ba"), "core::str::<impl str>::contains");
    assert_eq!(str::find("a::b::c", "::"), Some(1), "core::str::<impl str>::find");
    assert_eq!(str::find("ab", "x"), None, "core::str::<impl str>::find");
    assert_eq!(str::rfind("a::b::c", "::"), Some(4), "core::str::<impl str>::rfind");
    assert_eq!(str::strip_prefix("--name", "--"), Some("name"), "core::str::<impl str>::strip_prefix");
    assert_eq!(str::strip_prefix("name", "--"), None, "core::str::<impl str>::strip_prefix");
    assert_eq!(str::strip_suffix("a.rs", ".rs"), Some("a"), "core::str::<impl str>::strip_suffix");
    assert_eq!(str::strip_suffix("a", ".rs"), None, "core::str::<impl str>::strip_suffix");
    assert_eq!(str::trim_start_matches("__x_", "_"), "x_", "core::str::<impl str>::trim_start_matches");
    assert_eq!(str::trim_end_matches("_x__", "_"), "_x", "core::str::<impl str>::trim_end_matches");
    assert_eq!(str::repeat("ab", 3), "ababab", "std::str::<impl str>::repeat");
    assert_eq!(str::repeat("x", 0), "", "std::str::<impl str>::repeat");
    assert_eq!(char::to_string(&'é'), "é", "<char as std::string::ToString>::to_string");
    assert_eq!(std::slice::from_ref(&7u32).to_vec(), vec![7u32], "std::slice::from_ref");
    assert_eq!(Some(String::from("a")).as_ref().cloned(), Some(String::from("a")), "std::option::Option::cloned");
    assert_eq!(None::<String>.as_ref().cloned(), None, "std::option::Option::cloned");
    assert_eq!(Some(3u8).as_ref().copied(), Some(3u8), "std::option::Option::copied");
    assert_eq!({ let mut v = vec![1u8]; v.push(2u8); v }, vec![1u8, 2], "std::vec::Vec::push");
    assert_eq!({ let mut s = String::from("ab"); s.push_str("cd"); s }, "abcd", "std::string::String::push_str");
    assert_eq!({ let mut s = String::from("ab"); s.push('é'); s }, "abé", "std::string::String::push");
    assert!(std::collections::BTreeSet::from(["a", "b"]).contains(&"b"), "std::collections::BTreeSet::contains");
    assert!(!std::collections::BTreeSet::from(["a"]).contains(&"c"), "std::collections::BTreeSet::contains");
    assert_eq!({ let mut s = std::collections::BTreeSet::from(["a", "c"]); let new = s.insert("b"); (new, s.into_iter().collect::<Vec<_>>()) }, (true, vec!["a", "b", "c"]), "std::collections::BTreeSet::insert");
    assert_eq!({ let mut s = std::collections::BTreeSet::from(["a"]); let new = s.insert("a"); (new, s.into_iter().collect::<Vec<_>>()) }, (false, vec!["a"]), "std::collections::BTreeSet::insert");
    assert_eq!(std::collections::BTreeSet::from(["a", "b"]).len(), 2, "std::collections::BTreeSet::len");
    assert!(std::collections::BTreeSet::from([] as [&str; 0]).is_empty(), "std::collections::BTreeSet::is_empty");
    assert_eq!(std::collections::BTreeMap::from([("a", 1u8)]).get(&"a").copied(), Some(1u8), "std::collections::BTreeMap::get");
    assert_eq!(std::collections::BTreeMap::from([("a", 1u8)]).get(&"b").copied(), None, "std::collections::BTreeMap::get");
    assert!(std::collections::BTreeMap::from([("a", 1u8)]).contains_key(&"a"), "std::collections::BTreeMap::contains_key");
    assert_eq!({ let mut m = std::collections::BTreeMap::from([("a", 1u8), ("c", 3)]); let old = m.insert("b", 2u8); (old, m.into_iter().collect::<Vec<_>>()) }, (None, vec![("a", 1u8), ("b", 2), ("c", 3)]), "std::collections::BTreeMap::insert");
    assert_eq!({ let mut m = std::collections::BTreeMap::from([("a", 1u8)]); let old = m.insert("a", 5u8); (old, m.into_iter().collect::<Vec<_>>()) }, (Some(1u8), vec![("a", 5u8)]), "std::collections::BTreeMap::insert");
    assert_eq!(std::collections::BTreeMap::from([("a", 1u8)]).len(), 1, "std::collections::BTreeMap::len");
    assert!(!std::collections::BTreeMap::from([("a", 1u8)]).is_empty(), "std::collections::BTreeMap::is_empty");
    assert_eq!(std::collections::BTreeSet::<u8>::new().len(), 0, "std::collections::BTreeSet::new");
    assert_eq!(std::collections::BTreeMap::<u8, u8>::new().len(), 0, "std::collections::BTreeMap::new");
    assert!(["a", "b"].contains(&"b"), "core::slice::<impl [T]>::contains");
    assert!(!["a"].contains(&"c"), "core::slice::<impl [T]>::contains");
    assert_eq!(["a", "b"].join("-"), "a-b", "std::slice::<impl [T]>::join");
    assert_eq!([1u8, 2].get(1).copied(), Some(2u8), "core::slice::<impl [T]>::get");
    assert_eq!([1u8, 2].get(2).copied(), None, "core::slice::<impl [T]>::get");
    assert_eq!([1u8, 2].iter().as_slice().to_vec(), vec![1u8, 2], "core::slice::<impl [T]>::iter");
    assert_eq!(IntoIterator::into_iter(&vec![1u8, 2]).as_slice().to_vec(), vec![1u8, 2], "<&std::vec::Vec as std::iter::IntoIterator>::into_iter");
    assert_eq!(<&[u8] as IntoIterator>::into_iter(&[1u8, 2]).as_slice().to_vec(), vec![1u8, 2], "<&[T] as std::iter::IntoIterator>::into_iter");
    assert_eq!(vec![1u8, 2].into_iter().as_slice().to_vec(), vec![1u8, 2], "<std::vec::Vec as std::iter::IntoIterator>::into_iter");
    assert_eq!(IntoIterator::into_iter(&std::collections::BTreeSet::from([2u8, 1])).copied().collect::<Vec<_>>(), vec![1u8, 2], "<&std::collections::BTreeSet as std::iter::IntoIterator>::into_iter");
    assert_eq!(std::collections::BTreeSet::from([2u8, 1]).iter().copied().collect::<Vec<_>>(), vec![1u8, 2], "std::collections::BTreeSet::iter");
    assert_eq!(std::collections::BTreeSet::from([2u8, 1]).into_iter().collect::<Vec<_>>(), vec![1u8, 2], "<std::collections::BTreeSet as std::iter::IntoIterator>::into_iter");
    assert_eq!({ let v = [1u8, 2]; let mut it = v.iter(); (it.next().copied(), it.as_slice().to_vec()) }, (Some(1u8), vec![2u8]), "<std::slice::Iter as std::iter::Iterator>::next");
    assert_eq!({ let v = [0u8; 0]; let mut it = v.iter(); (it.next().copied(), it.as_slice().to_vec()) }, (None, vec![]), "<std::slice::Iter as std::iter::Iterator>::next");
    assert_eq!({ let v = [1u8, 2]; let mut it = v.iter(); (it.next_back().copied(), it.as_slice().to_vec()) }, (Some(2u8), vec![1u8]), "<std::slice::Iter as std::iter::DoubleEndedIterator>::next_back");
    assert_eq!({ let v = [0u8; 0]; let mut it = v.iter(); (it.next_back().copied(), it.as_slice().to_vec()) }, (None, vec![]), "<std::slice::Iter as std::iter::DoubleEndedIterator>::next_back");
    assert_eq!({ let mut it = vec![1u8, 2].into_iter(); (it.next(), it.as_slice().to_vec()) }, (Some(1u8), vec![2u8]), "<std::vec::IntoIter as std::iter::Iterator>::next");
    assert_eq!({ let mut it = Vec::<u8>::new().into_iter(); (it.next(), it.as_slice().to_vec()) }, (None, vec![]), "<std::vec::IntoIter as std::iter::Iterator>::next");
    assert_eq!({ let s = std::collections::BTreeSet::from([2u8, 1]); let mut it = s.iter(); (it.next().copied(), it.copied().collect::<Vec<_>>()) }, (Some(1u8), vec![2u8]), "<std::collections::btree_set::Iter as std::iter::Iterator>::next");
    assert_eq!({ let mut it = std::collections::BTreeSet::from([2u8, 1]).into_iter(); (it.next(), it.collect::<Vec<_>>()) }, (Some(1u8), vec![2u8]), "<std::collections::btree_set::IntoIter as std::iter::Iterator>::next");
    assert_eq!([1u8, 2].iter().len(), 2, "<std::slice::Iter as std::iter::ExactSizeIterator>::len");
}
