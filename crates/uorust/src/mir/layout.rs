//! The layout of a value in memory, from the target's data layout.
//!
//! rustc gives every target a data layout, the LLVM data layout string its
//! target specification states (`rustc --print target-spec-json`, its
//! `data-layout`). It fixes the byte order, a pointer's size and alignment,
//! and each integer and float width's ABI alignment. A primitive's size is
//! its width in bytes, rounded up to its alignment (the Reference, "Type
//! layout": a value's size is a multiple of its alignment). A specification
//! the string leaves out takes the default LLVM's LangRef documents for it
//! ("Data Layout": `i1:8`, `i8:8`, `i16:16`, `i32:32`, `i64:32:64`,
//! `f16:16`, `f32:32`, `f64:64`, `f128:128`, `p:64:64`). A width neither
//! the string nor the defaults state has no layout here, so nothing about
//! it is assumed.
//!
//! The dumps record the data layout of the target they were built for (see
//! `scripts/mir/dump-lib.sh`); a layout is only used where every target the
//! translation answers for agrees on it.

use std::collections::BTreeMap;

use crate::lift::Ty;

/// One target's data layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DataLayout {
    /// The target triple.
    pub(crate) target: String,
    /// Whether the target is big-endian (`E`) rather than little-endian.
    pub(crate) big_endian: bool,
    /// The pointer of address space 0: its size and ABI alignment, in bits.
    pub(crate) pointer: (u32, u32),
    /// Integer widths' ABI alignments, in bits.
    pub(crate) ints: BTreeMap<u32, u32>,
    /// Float widths' ABI alignments, in bits.
    pub(crate) floats: BTreeMap<u32, u32>,
}

/// Why a data layout, or a layout from it, is not had.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A field of the string that should be a number of bits is not one.
    NotBits { field: String, text: String },
    /// A pointer specification without its size.
    PointerWithoutSize { text: String },
    /// An integer or float specification without its alignment.
    WidthWithoutAlignment { text: String },
    /// A `target.toml` that records a data layout but not its triple.
    WithoutTriple,
    /// No data layout is recorded for the target.
    NoTarget { ty: String },
    /// A target's data layout states nothing of the type.
    Unstated { ty: String, target: String },
    /// The targets' layouts of the type differ: each target's, in order.
    Differs {
        ty: String,
        each: Vec<(String, Layout)>,
    },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NotBits { field, text } => {
                write!(f, "`{field}` is not a number of bits in `{text}`")
            }
            Refusal::PointerWithoutSize { text } => {
                write!(f, "a pointer without a size in `{text}`")
            }
            Refusal::WidthWithoutAlignment { text } => {
                write!(f, "a width without an alignment in `{text}`")
            }
            Refusal::WithoutTriple => write!(f, "a data layout without its triple"),
            Refusal::NoTarget { ty } => write!(
                f,
                "the layout of {ty}, with no data layout recorded for the target"
            ),
            Refusal::Unstated { ty, target } => write!(
                f,
                "the layout of {ty}, which the data layout of {target} does not state"
            ),
            Refusal::Differs { ty, each } => write!(
                f,
                "the layout of {ty}, which differs by target: {}",
                each.iter()
                    .map(|(target, m)| format!("size {} align {} on {target}", m.size, m.align))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// A value's size and alignment, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Layout {
    pub(crate) size: u64,
    pub(crate) align: u64,
}

impl DataLayout {
    /// Parse an LLVM data layout string, the defaults LangRef documents
    /// filled in where it states nothing.
    pub(crate) fn parse(target: &str, text: &str) -> Result<Self, Refusal> {
        let mut dl = DataLayout {
            target: target.to_string(),
            big_endian: false,
            pointer: (64, 64),
            ints: BTreeMap::from([(1, 8), (8, 8), (16, 16), (32, 32), (64, 32)]),
            floats: BTreeMap::from([(16, 16), (32, 32), (64, 64), (128, 128)]),
        };
        let bits = |s: &str| -> Result<u32, Refusal> {
            s.parse::<u32>().map_err(|_| Refusal::NotBits {
                field: s.to_string(),
                text: text.to_string(),
            })
        };
        for spec in text.split('-').filter(|s| !s.is_empty()) {
            let (head, rest) = spec.split_at(1);
            let fields: Vec<&str> = rest.split(':').collect();
            match head {
                "e" if rest.is_empty() => dl.big_endian = false,
                "E" if rest.is_empty() => dl.big_endian = true,
                // `p[n]:size:abi[:pref[:idx]]`: only address space 0.
                "p" => {
                    let space = fields[0];
                    if space.is_empty() || space == "0" {
                        let size =
                            bits(fields.get(1).ok_or_else(|| Refusal::PointerWithoutSize {
                                text: text.to_string(),
                            })?)?;
                        let abi = fields.get(2).map_or(Ok(size), |a| bits(a))?;
                        dl.pointer = (size, abi);
                    }
                }
                "i" | "f" => {
                    let width = bits(fields[0])?;
                    let abi =
                        bits(
                            fields
                                .get(1)
                                .ok_or_else(|| Refusal::WidthWithoutAlignment {
                                    text: text.to_string(),
                                })?,
                        )?;
                    let table = if head == "i" {
                        &mut dl.ints
                    } else {
                        &mut dl.floats
                    };
                    table.insert(width, abi);
                }
                _ => {}
            }
        }
        Ok(dl)
    }

    /// The layout of a primitive of `width` bits aligned to `align` bits.
    fn primitive(width: u32, align: u32) -> Layout {
        let align = u64::from(align.max(8) / 8);
        let bytes = u64::from(width.div_ceil(8));
        Layout {
            size: bytes.div_ceil(align) * align,
            align,
        }
    }

    /// The layout of a value of `t`, where this data layout fixes it: a
    /// primitive (`bool` is an `i8`, `char` an `i32`, a thin raw pointer
    /// the pointer), or the unit.
    pub(crate) fn of(&self, t: &Ty) -> Option<Layout> {
        let int = |w: u32| self.ints.get(&w).map(|a| Self::primitive(w, *a));
        match t {
            Ty::Unit => Some(Layout { size: 0, align: 1 }),
            Ty::Bool => int(8),
            Ty::Char => int(32),
            Ty::Nat(Some(w)) | Ty::Int(w) => int(*w),
            Ty::Nat(None) => Some(Self::primitive(self.pointer.0, self.pointer.1)),
            Ty::Float(f) => {
                let w = 1 + f.exp + f.frac;
                self.floats.get(&w).map(|a| Self::primitive(w, *a))
            }
            Ty::Named(n) if n == super::lower::POINTER => {
                Some(Self::primitive(self.pointer.0, self.pointer.1))
            }
            _ => None,
        }
    }
}

/// The layout of `t` on every target of `layouts`, where they agree; the
/// refusal otherwise, naming `t` and the targets.
pub(crate) fn agreed(t: &Ty, layouts: &[DataLayout]) -> Result<Layout, Refusal> {
    let ty = || t.render();
    let mut each = Vec::new();
    for dl in layouts {
        let l = dl.of(t).ok_or_else(|| Refusal::Unstated {
            ty: ty(),
            target: dl.target.clone(),
        })?;
        each.push((dl.target.clone(), l));
    }
    let Some((_, l)) = each.first().cloned() else {
        return Err(Refusal::NoTarget { ty: ty() });
    };
    if each.iter().any(|(_, m)| *m != l) {
        return Err(Refusal::Differs { ty: ty(), each });
    }
    Ok(l)
}

/// The data layouts a `target.toml` beside the dumps records: its
/// `data_layout` for its `triple`, or none when it records none.
pub(crate) fn from_target_toml(doc: &toml::Table) -> Result<Vec<DataLayout>, Refusal> {
    let Some(text) = doc.get("data_layout").and_then(toml::Value::as_str) else {
        return Ok(Vec::new());
    };
    let triple = doc
        .get("triple")
        .and_then(toml::Value::as_str)
        .ok_or(Refusal::WithoutTriple)?;
    Ok(vec![DataLayout::parse(triple, text)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    const X86_64: &str =
        "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128";
    const THUMB: &str = "e-m:e-p:32:32-Fi8-i64:64-v128:64:128-a:0:32-n32-S64";

    #[test]
    fn primitives_take_the_data_layouts_alignment() {
        let x = DataLayout::parse("x86_64-unknown-linux-gnu", X86_64).unwrap();
        assert_eq!(x.pointer, (64, 64));
        assert_eq!(x.of(&Ty::Nat(Some(32))), Some(Layout { size: 4, align: 4 }));
        assert_eq!(x.of(&Ty::Nat(Some(64))), Some(Layout { size: 8, align: 8 }));
        assert_eq!(
            x.of(&Ty::Nat(Some(128))),
            Some(Layout {
                size: 16,
                align: 16
            })
        );
        assert_eq!(x.of(&Ty::Nat(None)), Some(Layout { size: 8, align: 8 }));
        assert_eq!(x.of(&Ty::Bool), Some(Layout { size: 1, align: 1 }));
        let t = DataLayout::parse("thumbv7em-none-eabihf", THUMB).unwrap();
        assert_eq!(t.of(&Ty::Nat(None)), Some(Layout { size: 4, align: 4 }));
        // `i128` is neither stated nor a LangRef default.
        assert_eq!(t.of(&Ty::Nat(Some(128))), None);
    }

    #[test]
    fn a_layout_is_used_only_where_the_targets_agree() {
        let both = [
            DataLayout::parse("x86_64-unknown-linux-gnu", X86_64).unwrap(),
            DataLayout::parse("thumbv7em-none-eabihf", THUMB).unwrap(),
        ];
        assert_eq!(
            agreed(&Ty::Nat(Some(32)), &both),
            Ok(Layout { size: 4, align: 4 })
        );
        let differs = agreed(&Ty::Nat(None), &both).unwrap_err();
        assert!(matches!(&differs, Refusal::Differs { each, .. } if each.len() == 2));
        assert!(differs.to_string().contains("differs by target"));
        assert!(matches!(
            agreed(&Ty::Nat(Some(8)), &[]),
            Err(Refusal::NoTarget { .. })
        ));
        assert!(matches!(
            agreed(&Ty::Nat(Some(128)), &both),
            Err(Refusal::Unstated { target, .. }) if target == "thumbv7em-none-eabihf"
        ));
    }

    #[test]
    fn a_malformed_data_layout_is_refused() {
        assert!(matches!(
            DataLayout::parse("t", "e-p:x:64"),
            Err(Refusal::NotBits { field, .. }) if field == "x"
        ));
        assert!(matches!(
            DataLayout::parse("t", "e-p"),
            Err(Refusal::PointerWithoutSize { .. })
        ));
        assert!(matches!(
            DataLayout::parse("t", "e-i64"),
            Err(Refusal::WidthWithoutAlignment { .. })
        ));
        let doc: toml::Table = toml::from_str("data_layout = \"e\"").unwrap();
        assert_eq!(from_target_toml(&doc), Err(Refusal::WithoutTriple));
        let none: toml::Table = toml::from_str("triple = \"t\"").unwrap();
        assert_eq!(from_target_toml(&none), Ok(Vec::new()));
    }
}
