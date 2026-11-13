//! The memory a body reaches through raw pointers, modeled as a value the
//! translation threads through the body, as it threads a `&mut` parameter.
//!
//! The model follows `core::ptr`'s documentation of the abstract machine:
//! memory is a set of allocations, and a pointer is an address together with
//! its provenance, the allocation it may access (`allocation`, `0` for
//! none: `ptr::null`, `ptr::without_provenance`, an integer transmuted to a
//! pointer). An access is valid when the pointer's provenance names a live
//! allocation, the address lies in it, aligned to the accessed type, and the
//! access does not run past its end ("valid for reads/writes", "properly
//! aligned"); a read also needs the bytes initialized. Every access the
//! translation makes carries the theorem that it is valid where the body
//! makes it, so nothing about memory is assumed.
//!
//! An allocation holds cells of one size, the size of the type it was made
//! for, which the target's data layout fixes (see [`super::layout`]); its
//! cells are addressed by their byte offset from its base. A read or write
//! at another size than its cells' is not valid in this model, so a body
//! that reads memory at another type than it wrote it is residue through its
//! theorem. An allocation is placed at the first address after every earlier
//! one that its alignment admits, past the memory's `top`, which the caller
//! chooses: a body's theorems hold of every placement above any top.
//!
//! The model's laws (a load after a store reads the stored value; a store
//! changes no other allocation, and no other cell of its own; an allocation
//! is valid to write, and leaves the others as they were; a freed one is
//! not valid) are theorems proved with the translation, at each cell size it
//! uses.

use std::collections::BTreeSet;

use crate::lex::{Decl, Expr};
use crate::lift::Ty;

/// The memory's struct.
pub(crate) const MEMORY: &str = "MirMemory";

/// The fields of [`MEMORY`]: the number of allocations made, the address
/// past the last one, and per allocation its base address, its length in
/// cells, its cell size, whether it is live, and per byte offset its cell's
/// value and whether that cell is initialized. A flag is `1` or `0`, so the
/// laws stay within linear arithmetic.
pub(crate) fn memory_fields() -> Vec<(String, Ty)> {
    let nat = || Ty::Nat(None);
    let per = |out: Ty| Ty::Fn(vec![nat()], Box::new(out));
    let cell = |out: Ty| Ty::Fn(vec![nat(), nat()], Box::new(out));
    vec![
        ("count".into(), nat()),
        ("top".into(), nat()),
        ("base".into(), per(nat())),
        ("length".into(), per(nat())),
        ("stride".into(), per(nat())),
        ("live".into(), per(nat())),
        ("cells".into(), cell(nat())),
        ("defined".into(), cell(nat())),
    ]
}

/// The memory's type.
pub(crate) fn memory_ty() -> Ty {
    Ty::Named(MEMORY.to_string())
}

/// What the memory's theorems elaborate with.
fn axioms() -> Vec<String> {
    vec!["Quot.sound".into(), "propext".into()]
}

fn definition(name: &str, parameters: &[(&str, &str)], result: &str, body: String) -> Decl {
    Decl::Definition {
        name: name.to_string(),
        parameters: parameters
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect(),
        result: result.to_string(),
        body: Expr::Flat(body),
        axioms: Vec::new(),
        recursive: None,
        termination: None,
        mutual: None,
    }
}

fn theorem(name: String, parameters: &[(&str, &str)], statement: String, uses: &[&str]) -> Decl {
    // A disjunction is decided classically, and so is a fresh allocation's
    // writability, whose rounded address splits on a decided `if`.
    let mut observed = axioms();
    if statement.contains("||") || name.starts_with("mirAllocatedIsWritable") {
        observed.insert(0, "Classical.choice".into());
    }
    let mut uses: Vec<&str> = uses.to_vec();
    uses.sort_unstable();
    uses.dedup();
    Decl::Theorem {
        name,
        parameters: parameters
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect(),
        statement,
        proof: format!("linear_arithmetic{{definitions: [{}]}}", uses.join(", ")),
        axioms: observed,
    }
}

/// `memory` with the fields `with` names replaced.
fn memory_with(with: &[(&str, String)]) -> String {
    let fields = [
        "count", "top", "base", "length", "stride", "live", "cells", "defined",
    ];
    let parts: Vec<String> = fields
        .iter()
        .map(|f| {
            let v = with
                .iter()
                .find(|(n, _)| n == f)
                .map_or_else(|| format!("(project memory {f})"), |(_, v)| v.clone());
            format!("{f} := {v}")
        })
        .collect();
    format!("(record {MEMORY} {{{}}})", parts.join(", "))
}

/// A field of the allocation a pointer's provenance names.
fn of(field: &str) -> String {
    format!("(apply (project memory {field}) (project pointer allocation))")
}

/// A per-allocation field with allocation `slot` set to `value`.
fn set_slot(field: &str, value: &str) -> String {
    let mut caps = vec!["memory", "slot"];
    if value.chars().next().is_some_and(char::is_alphabetic) {
        caps.push(value);
    }
    caps.sort_unstable();
    format!(
        "(fun [{}] (k : Nat) => (if (k =? slot) then {value} else (apply (project memory {field}) k)))",
        caps.join(", ")
    )
}

/// The model's definitions, which every translation that reaches memory
/// shares.
pub(crate) fn definitions() -> Vec<Decl> {
    let m = ("memory", MEMORY);
    let p = ("pointer", super::lower::POINTER);
    let nat = |n| (n, "Nat");
    let mut out = Vec::new();
    out.push(definition(
        "mirOffset",
        &[m, p],
        "Nat",
        format!(
            "(prim subtract (project pointer address) {} : Nat)",
            of("base")
        ),
    ));
    // Valid for a write of a cell of `stride` bytes.
    out.push(definition(
        "mirWritable",
        &[m, p, nat("stride")],
        "Bool",
        format!(
            "((not ((project pointer allocation) =? 0)) && (((project pointer allocation) <=? (project memory count)) && (({live} =? 1) && (({stride} =? stride) && (({base} <=? (project pointer address)) && (((prim remainder (mirOffset memory pointer) stride 0 : Nat) =? 0) && ((mirOffset memory pointer) <? (prim multiply {length} stride : Nat))))))))",
            live = of("live"),
            stride = of("stride"),
            base = of("base"),
            length = of("length"),
        ),
    ));
    out.push(definition(
        "mirReadable",
        &[m, p, nat("stride")],
        "Bool",
        "((mirWritable memory pointer stride) && ((apply (project memory defined) (project pointer allocation) (mirOffset memory pointer)) =? 1))".into(),
    ));
    out.push(definition(
        "mirLoad",
        &[m, p, nat("stride")],
        "Nat",
        "(apply (project memory cells) (project pointer allocation) (mirOffset memory pointer))"
            .into(),
    ));
    let store = |flag: &str| {
        let value = if flag == "1" {
            "value"
        } else {
            "(apply (project memory cells) k j)"
        };
        let caps = if flag == "1" {
            "i, memory, pointer, value"
        } else {
            "i, memory, pointer"
        };
        format!(
            "(let i : Nat := (mirOffset memory pointer); {})",
            memory_with(&[
                (
                    "cells",
                    format!("(fun [{caps}] (k : Nat) (j : Nat) => (if ((k =? (project pointer allocation)) && (j =? i)) then {value} else (apply (project memory cells) k j)))"),
                ),
                (
                    "defined",
                    format!("(fun [i, memory, pointer] (k : Nat) (j : Nat) => (if ((k =? (project pointer allocation)) && (j =? i)) then {flag} else (apply (project memory defined) k j)))"),
                ),
            ])
        )
    };
    out.push(definition(
        "mirStore",
        &[m, p, nat("stride"), nat("value")],
        MEMORY,
        store("1"),
    ));
    // The cell left uninitialized: a `MaybeUninit::uninit()` written.
    out.push(definition(
        "mirForget",
        &[m, p, nat("stride")],
        MEMORY,
        store("0"),
    ));
    out.push(definition(
        "mirLoadMaybe",
        &[m, p, nat("stride")],
        "(Option Nat)",
        "(if (mirReadable memory pointer stride) then (ctor Option.some [Nat] (mirLoad memory pointer stride)) else (ctor Option.none [Nat]))".into(),
    ));
    out.push(definition(
        "mirStoreMaybe",
        &[m, p, nat("stride"), ("value", "(Option Nat)")],
        MEMORY,
        "(match value | Option.none => (mirForget memory pointer stride) | Option.some held => (mirStore memory pointer stride held))".into(),
    ));
    out.push(definition(
        "mirFresh",
        &[m, nat("align")],
        "Nat",
        "(prim multiply (prim quotient ((project memory top) + align) align 0 : Nat) align : Nat)"
            .into(),
    ));
    out.push(definition(
        "mirAllocate",
        &[
            m,
            nat("stride"),
            nat("align"),
            nat("length"),
            ("cells", "(Fn (Nat) Nat)"),
            ("defined", "(Fn (Nat) Nat)"),
        ],
        MEMORY,
        format!(
            "(let slot : Nat := ((project memory count) + 1); (let start : Nat := (mirFresh memory align); {}))",
            memory_with(&[
                ("count", "slot".into()),
                ("top", "(start + (prim multiply length stride : Nat))".into()),
                ("base", set_slot("base", "start")),
                ("length", set_slot("length", "length")),
                ("stride", set_slot("stride", "stride")),
                ("live", set_slot("live", "1")),
                ("cells", "(fun [cells, memory, slot] (k : Nat) (j : Nat) => (if (k =? slot) then (apply cells j) else (apply (project memory cells) k j)))".into()),
                ("defined", "(fun [defined, memory, slot] (k : Nat) (j : Nat) => (if (k =? slot) then (apply defined j) else (apply (project memory defined) k j)))".into()),
            ])
        ),
    ));
    out.push(definition(
        "mirAllocated",
        &[m, nat("align"), nat("metadata")],
        super::lower::POINTER,
        format!(
            "(record {} {{allocation := ((project memory count) + 1), address := (mirFresh memory align), metadata := metadata}})",
            super::lower::POINTER
        ),
    ));
    out.push(definition(
        "mirFree",
        &[m, nat("slot")],
        MEMORY,
        memory_with(&[("live", set_slot("live", "0"))]),
    ));
    // `ptr.add(n)`'s pointer, `bytes` further on, with the same provenance.
    out.push(definition(
        "mirAdvance",
        &[p, nat("bytes")],
        super::lower::POINTER,
        format!(
            "(record {} {{allocation := (project pointer allocation), address := ((project pointer address) + bytes), metadata := (project pointer metadata)}})",
            super::lower::POINTER
        ),
    ));
    // In bounds of its allocation, or one past its end: what `offset`
    // requires of the pointer it computes.
    out.push(definition(
        "mirWithin",
        &[m, p],
        "Bool",
        format!(
            "((not ((project pointer allocation) =? 0)) && (((project pointer allocation) <=? (project memory count)) && (({live} =? 1) && (({base} <=? (project pointer address)) && ((mirOffset memory pointer) <=? (prim multiply {length} {stride} : Nat))))))",
            live = of("live"),
            base = of("base"),
            length = of("length"),
            stride = of("stride"),
        ),
    ));
    // The cells of a value: one value at every offset, or a list's items
    // at their offsets.
    out.push(definition(
        "mirCellsOf",
        &[nat("value")],
        "(Fn (Nat) Nat)",
        "(fun [value] (j : Nat) => value)".into(),
    ));
    out.push(definition(
        "mirCellsOfList",
        &[("values", "(List Nat)"), nat("stride")],
        "(Fn (Nat) Nat)",
        "(fun [stride, values] (j : Nat) => (match (prim index values (prim quotient j stride 0 : Nat) : (Option Nat)) | Option.none => 0 | Option.some item => item))".into(),
    ));
    out.push(definition(
        "mirDefinedAs",
        &[nat("flag")],
        "(Fn (Nat) Nat)",
        "(fun [flag] (j : Nat) => flag)".into(),
    ));
    // A `MaybeUninit`'s cell: its value where it holds one.
    out.push(definition(
        "mirCellsOfMaybe",
        &[("value", "(Option Nat)")],
        "(Fn (Nat) Nat)",
        "(fun [value] (j : Nat) => (match value | Option.none => 0 | Option.some held => held))"
            .into(),
    ));
    out.push(definition(
        "mirDefinedOfMaybe",
        &[("value", "(Option Nat)")],
        "(Fn (Nat) Nat)",
        "(fun [value] (j : Nat) => (match value | Option.none => 0 | Option.some held => 1))"
            .into(),
    ));
    out
}

/// The model's laws at cell size `s`.
pub(crate) fn laws(s: u64) -> Vec<Decl> {
    let m = ("memory", MEMORY);
    let p = ("pointer", super::lower::POINTER);
    let o = ("other", super::lower::POINTER);
    let v = ("value", "Nat");
    let base = [
        "mirOffset",
        "mirLoad",
        "mirStore",
        "mirWritable",
        "mirReadable",
    ];
    let with = |more: &[&'static str]| -> Vec<&'static str> {
        base.iter().copied().chain(more.iter().copied()).collect()
    };
    let alloc_params = [
        m,
        ("align", "Nat"),
        ("length", "Nat"),
        ("cells", "(Fn (Nat) Nat)"),
        ("defined", "(Fn (Nat) Nat)"),
    ];
    let mut out = vec![
        theorem(
            format!("mirLoadAfterStore{s}"),
            &[m, p, v],
            format!("((mirLoad (mirStore memory pointer {s} value) pointer {s}) = value)"),
            &with(&[]),
        ),
        theorem(
            format!("mirStoreKeepsOtherAllocations{s}"),
            &[m, p, o, v],
            format!("((((project pointer allocation) =? (project other allocation)) = false) -> ((mirLoad (mirStore memory pointer {s} value) other {s}) = (mirLoad memory other {s})))"),
            &with(&[]),
        ),
        theorem(
            format!("mirStoreKeepsOtherCells{s}"),
            &[m, p, ("address", "Nat"), v],
            {
                let at = format!("(record {} {{allocation := (project pointer allocation), address := address, metadata := 0}})", super::lower::POINTER);
                format!("(((mirWritable memory pointer {s}) = true) -> (((mirWritable memory {at} {s}) = true) -> ((((project pointer address) =? address) = false) -> ((mirLoad (mirStore memory pointer {s} value) {at} {s}) = (mirLoad memory {at} {s})))))")
            },
            &with(&[]),
        ),
        theorem(
            format!("mirStoreKeepsWritable{s}"),
            &[m, p, o, v],
            format!("((mirWritable (mirStore memory pointer {s} value) other {s}) = (mirWritable memory other {s}))"),
            &with(&[]),
        ),
        theorem(
            format!("mirStoreMakesReadable{s}"),
            &[m, p, v],
            // Stated as one disjunction, so the alignment test stays in
            // the goal, where `linear_arithmetic` splits it.
            format!("(((not (mirWritable memory pointer {s})) || (mirReadable (mirStore memory pointer {s} value) pointer {s})) = true)"),
            &with(&[]),
        ),
    ];
    let mut params = alloc_params.to_vec();
    params.push(("metadata", "Nat"));
    out.push(theorem(
        format!("mirAllocatedIsWritable{s}"),
        &params,
        format!("((0 < length) -> ((mirWritable (mirAllocate memory {s} align length cells defined) (mirAllocated memory align metadata) {s}) = true))"),
        &with(&["mirAllocate", "mirAllocated", "mirFresh"]),
    ));
    let mut params = alloc_params.to_vec();
    params.push(p);
    out.push(theorem(
        format!("mirAllocateKeepsOthers{s}"),
        &params,
        format!("(((project pointer allocation) <= (project memory count)) -> ((mirLoad (mirAllocate memory {s} align length cells defined) pointer {s}) = (mirLoad memory pointer {s})))"),
        &with(&["mirAllocate", "mirFresh"]),
    ));
    out.push(theorem(
        format!("mirFreeInvalidates{s}"),
        &[m, p],
        format!("(((mirWritable (mirFree memory (project pointer allocation)) pointer {s}) = true) -> (0 = 1))"),
        &with(&["mirFree"]),
    ));
    out
}

/// The model's declarations for a translation that uses cells of these
/// sizes: its definitions, then its laws at each size.
pub(crate) fn declarations(strides: &BTreeSet<u64>) -> Vec<Decl> {
    let mut out = definitions();
    for s in strides {
        out.extend(laws(*s));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_laws_name_their_cell_size() {
        let names: Vec<String> = laws(4).iter().map(|d| d.name().to_string()).collect();
        assert!(names.contains(&"mirLoadAfterStore4".to_string()));
        assert!(names.iter().all(|n| n.ends_with('4')));
        let all = declarations(&BTreeSet::from([1, 4]));
        assert_eq!(all.len(), definitions().len() + 2 * laws(1).len());
    }
}
