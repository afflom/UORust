#![forbid(unsafe_code)]
use core::cmp::Ordering;

use core::sync::atomic::{AtomicU64, Ordering as Memory};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Overflow;

pub type R<T> = Result<T, Overflow>;

static WORK: AtomicU64 = AtomicU64::new(0);

pub fn tick(units: u64) { WORK.fetch_add(units, Memory::Relaxed); }

pub fn work() -> u64 { WORK.load(Memory::Relaxed) }

pub trait Same {
    fn same(&self, other: &Self) -> bool;
}

pub trait Key {
    fn key(&self, other: &Self) -> Ordering;
}

impl Same for bool {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for bool {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for u8 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for u8 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for u16 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for u16 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for u32 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for u32 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for u64 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for u64 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for i8 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for i8 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for i16 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for i16 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for i32 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for i32 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for i64 {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl Key for i64 {
    fn key(&self, other: &Self) -> Ordering { self.cmp(other) }
}

impl Same for Ordering {
    fn same(&self, other: &Self) -> bool { self == other }
}

impl<A: Key, B: Key> Key for (A, B) {
    fn key(&self, other: &Self) -> Ordering { match self.0.key(&other.0) { Ordering::Equal => self.1.key(&other.1), decided => decided, } }
}

pub fn equal<T: Same>(a: T, b: T) -> bool { a.same(&b) }

pub fn compare<T: Key>(a: T, b: T) -> Ordering { a.key(&b) }

pub fn nat_add(p0: u64, p1: u64) -> R<u64> {
    match u64::checked_add(p0, p1) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<u64, Overflow>(Overflow)
        }
    }
}

pub fn nat_sub(p0: u64, p1: u64) -> u64 {
    u64::saturating_sub(p0, p1)
}

pub fn nat_mul(p0: u64, p1: u64) -> R<u64> {
    match u64::checked_mul(p0, p1) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<u64, Overflow>(Overflow)
        }
    }
}

pub fn nat_quot(p0: u64, p1: u64, p2: u64) -> u64 {
    u64::checked_div(p0, p1).unwrap_or(p2)
}

pub fn nat_rem(p0: u64, p1: u64, p2: u64) -> u64 {
    u64::checked_rem(p0, p1).unwrap_or(p2)
}

pub fn nat_eq(p0: u64, p1: u64) -> bool {
    core::cmp::PartialEq::eq(&p0, &p1)
}

pub fn nat_le(p0: u64, p1: u64) -> bool {
    core::cmp::PartialOrd::le(&p0, &p1)
}

pub fn nat_lt(p0: u64, p1: u64) -> bool {
    core::cmp::PartialOrd::lt(&p0, &p1)
}

pub fn nat_succ(p0: u64) -> R<u64> {
    match u64::checked_add(p0, 1u64) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<u64, Overflow>(Overflow)
        }
    }
}

pub fn int_add(p0: i64, p1: i64) -> R<i64> {
    match i64::checked_add(p0, p1) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<i64, Overflow>(Overflow)
        }
    }
}

pub fn int_sub(p0: i64, p1: i64) -> R<i64> {
    match i64::checked_sub(p0, p1) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<i64, Overflow>(Overflow)
        }
    }
}

pub fn int_mul(p0: i64, p1: i64) -> R<i64> {
    match i64::checked_mul(p0, p1) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<i64, Overflow>(Overflow)
        }
    }
}

pub fn int_neg(p0: i64) -> R<i64> {
    match i64::checked_neg(p0) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<i64, Overflow>(Overflow)
        }
    }
}

pub fn int_quot(p0: i64, p1: i64, p2: i64) -> R<i64> {
    { let value = { let (a, b, z): (i64, i64, i64) = (p0, p1, p2); if b == 0 { Some(z) } else { a.checked_div(b) } }; match value {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<i64, Overflow>(Overflow)
        }
    } }
}

pub fn int_rem(p0: i64, p1: i64, p2: i64) -> i64 {
    { let (a, b, z): (i64, i64, i64) = (p0, p1, p2); if b == 0 { z } else { a.wrapping_rem(b) } }
}

pub fn bool_not(p0: bool) -> bool {
    !p0
}

pub fn bool_and(p0: bool, p1: bool) -> bool {
    if p0 {
        p1
    } else {
        false
    }
}

pub fn bool_or(p0: bool, p1: bool) -> bool {
    if p0 {
        true
    } else {
        p1
    }
}

pub mod fixed_u8 {
    pub fn checked_add(p0: u8, p1: u8) -> Option<u8> {
        u8::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: u8, p1: u8) -> Option<u8> {
        u8::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: u8, p1: u8) -> Option<u8> {
        u8::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: u8, p1: u8) -> Option<u8> {
        u8::checked_div(p0, p1)
    }

    pub fn bit_and(p0: u8, p1: u8) -> u8 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: u8, p1: u8) -> u8 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: u8, p1: u8) -> u8 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: u8) -> u8 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: u8, p1: u32) -> Option<u8> {
        u8::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: u8, p1: u32) -> Option<u8> {
        u8::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<u8> {
        u8::try_from(p0).ok()
    }
}

pub mod fixed_u16 {
    pub fn checked_add(p0: u16, p1: u16) -> Option<u16> {
        u16::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: u16, p1: u16) -> Option<u16> {
        u16::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: u16, p1: u16) -> Option<u16> {
        u16::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: u16, p1: u16) -> Option<u16> {
        u16::checked_div(p0, p1)
    }

    pub fn bit_and(p0: u16, p1: u16) -> u16 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: u16, p1: u16) -> u16 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: u16, p1: u16) -> u16 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: u16) -> u16 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: u16, p1: u32) -> Option<u16> {
        u16::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: u16, p1: u32) -> Option<u16> {
        u16::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<u16> {
        u16::try_from(p0).ok()
    }
}

pub mod fixed_u32 {
    pub fn checked_add(p0: u32, p1: u32) -> Option<u32> {
        u32::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: u32, p1: u32) -> Option<u32> {
        u32::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: u32, p1: u32) -> Option<u32> {
        u32::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: u32, p1: u32) -> Option<u32> {
        u32::checked_div(p0, p1)
    }

    pub fn bit_and(p0: u32, p1: u32) -> u32 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: u32, p1: u32) -> u32 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: u32, p1: u32) -> u32 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: u32) -> u32 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: u32, p1: u32) -> Option<u32> {
        u32::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: u32, p1: u32) -> Option<u32> {
        u32::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<u32> {
        u32::try_from(p0).ok()
    }
}

pub mod fixed_u64 {
    pub fn checked_add(p0: u64, p1: u64) -> Option<u64> {
        u64::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: u64, p1: u64) -> Option<u64> {
        u64::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: u64, p1: u64) -> Option<u64> {
        u64::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: u64, p1: u64) -> Option<u64> {
        u64::checked_div(p0, p1)
    }

    pub fn bit_and(p0: u64, p1: u64) -> u64 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: u64, p1: u64) -> u64 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: u64, p1: u64) -> u64 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: u64) -> u64 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: u64, p1: u32) -> Option<u64> {
        u64::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: u64, p1: u32) -> Option<u64> {
        u64::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<u64> {
        u64::try_from(p0).ok()
    }
}

pub mod fixed_i8 {
    pub fn checked_add(p0: i8, p1: i8) -> Option<i8> {
        i8::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: i8, p1: i8) -> Option<i8> {
        i8::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: i8, p1: i8) -> Option<i8> {
        i8::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: i8, p1: i8) -> Option<i8> {
        i8::checked_div(p0, p1)
    }

    pub fn bit_and(p0: i8, p1: i8) -> i8 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: i8, p1: i8) -> i8 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: i8, p1: i8) -> i8 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: i8) -> i8 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: i8, p1: u32) -> Option<i8> {
        i8::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: i8, p1: u32) -> Option<i8> {
        i8::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<i8> {
        i8::try_from(p0).ok()
    }
}

pub fn checked_neg_i8(p0: i8) -> Option<i8> {
    i8::checked_neg(p0)
}

pub mod fixed_i16 {
    pub fn checked_add(p0: i16, p1: i16) -> Option<i16> {
        i16::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: i16, p1: i16) -> Option<i16> {
        i16::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: i16, p1: i16) -> Option<i16> {
        i16::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: i16, p1: i16) -> Option<i16> {
        i16::checked_div(p0, p1)
    }

    pub fn bit_and(p0: i16, p1: i16) -> i16 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: i16, p1: i16) -> i16 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: i16, p1: i16) -> i16 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: i16) -> i16 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: i16, p1: u32) -> Option<i16> {
        i16::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: i16, p1: u32) -> Option<i16> {
        i16::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<i16> {
        i16::try_from(p0).ok()
    }
}

pub fn checked_neg_i16(p0: i16) -> Option<i16> {
    i16::checked_neg(p0)
}

pub mod fixed_i32 {
    pub fn checked_add(p0: i32, p1: i32) -> Option<i32> {
        i32::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: i32, p1: i32) -> Option<i32> {
        i32::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: i32, p1: i32) -> Option<i32> {
        i32::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: i32, p1: i32) -> Option<i32> {
        i32::checked_div(p0, p1)
    }

    pub fn bit_and(p0: i32, p1: i32) -> i32 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: i32, p1: i32) -> i32 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: i32, p1: i32) -> i32 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: i32) -> i32 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: i32, p1: u32) -> Option<i32> {
        i32::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: i32, p1: u32) -> Option<i32> {
        i32::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<i32> {
        i32::try_from(p0).ok()
    }
}

pub fn checked_neg_i32(p0: i32) -> Option<i32> {
    i32::checked_neg(p0)
}

pub mod fixed_i64 {
    pub fn checked_add(p0: i64, p1: i64) -> Option<i64> {
        i64::checked_add(p0, p1)
    }

    pub fn checked_sub(p0: i64, p1: i64) -> Option<i64> {
        i64::checked_sub(p0, p1)
    }

    pub fn checked_mul(p0: i64, p1: i64) -> Option<i64> {
        i64::checked_mul(p0, p1)
    }

    pub fn checked_quot(p0: i64, p1: i64) -> Option<i64> {
        i64::checked_div(p0, p1)
    }

    pub fn bit_and(p0: i64, p1: i64) -> i64 {
        core::ops::BitAnd::bitand(p0, p1)
    }

    pub fn bit_or(p0: i64, p1: i64) -> i64 {
        core::ops::BitOr::bitor(p0, p1)
    }

    pub fn bit_xor(p0: i64, p1: i64) -> i64 {
        core::ops::BitXor::bitxor(p0, p1)
    }

    pub fn bit_not(p0: i64) -> i64 {
        core::ops::Not::not(p0)
    }

    pub fn shift_left(p0: i64, p1: u32) -> Option<i64> {
        i64::checked_shl(p0, p1)
    }

    pub fn shift_right(p0: i64, p1: u32) -> Option<i64> {
        i64::checked_shr(p0, p1)
    }

    pub fn convert(p0: i128) -> Option<i64> {
        i64::try_from(p0).ok()
    }
}

pub fn checked_neg_i64(p0: i64) -> Option<i64> {
    i64::checked_neg(p0)
}
use core::str;

use std::mem;

use std::rc::Rc;

#[derive(Clone)]
pub struct Str(Rc<str>);

impl Str {
    pub fn lit(text: &str) -> Str { Str(Rc::from(text)) }
    pub fn text(&self) -> &str { &self.0 }
    pub fn length(&self) -> u64 { chars(&self.0) }
    pub fn utf8_encode(&self) -> Bytes { tick(self.0.len() as u64); Bytes::lit(self.0.as_bytes()) }
    pub fn split_exact(&self, delimiter: &Str, maximum: u32) -> Option<List<Str>> { if delimiter.0.is_empty() { return None; }; chars(&self.0); let mut fields: Vec<Str> = Vec::new(); for piece in self.0.split(&*delimiter.0) { fields.push(Str::lit(piece)); }; if fields.len() as u128 > u128::from(maximum) { return None; }; Some(List::onto(fields, List::nil())) }
    pub fn join(texts: List<Str>, delimiter: &Str) -> Str { let mut out = String::new(); let mut first = true; let mut cursor = &texts; while let Repr::Cell(cell) = &cursor.0 { tick(1); if !first { chars(&delimiter.0); out.push_str(&delimiter.0); }; first = false; chars(&cell.head.0); out.push_str(&cell.head.0); cursor = &cell.tail; }; match &cursor.0 { Repr::Run { buffer, len } => { for text in buffer[..*len].iter().rev() { tick(1); if !first { chars(&delimiter.0); out.push_str(&delimiter.0); }; first = false; chars(&text.0); out.push_str(&text.0); } }, _ => { }, }; Str::lit(&out) }
    pub fn parse_int(text: &Str) -> Option<Option<i64>> { match canonical_decimal(&text.0) { None => Some(None), Some(Some(n)) => match i64::try_from(n) { Ok(value) => Some(Some(value)), Err(_) => None, }, Some(None) => None, } }
    pub fn format_u8(a: u8) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_u8(text: &Str) -> Option<u8> { match canonical_decimal(&text.0) { Some(Some(n)) => u8::try_from(n).ok(), _ => None, } }
    pub fn format_u16(a: u16) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_u16(text: &Str) -> Option<u16> { match canonical_decimal(&text.0) { Some(Some(n)) => u16::try_from(n).ok(), _ => None, } }
    pub fn format_u32(a: u32) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_u32(text: &Str) -> Option<u32> { match canonical_decimal(&text.0) { Some(Some(n)) => u32::try_from(n).ok(), _ => None, } }
    pub fn format_u64(a: u64) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_u64(text: &Str) -> Option<u64> { match canonical_decimal(&text.0) { Some(Some(n)) => u64::try_from(n).ok(), _ => None, } }
    pub fn format_i8(a: i8) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_i8(text: &Str) -> Option<i8> { match canonical_decimal(&text.0) { Some(Some(n)) => i8::try_from(n).ok(), _ => None, } }
    pub fn format_i16(a: i16) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_i16(text: &Str) -> Option<i16> { match canonical_decimal(&text.0) { Some(Some(n)) => i16::try_from(n).ok(), _ => None, } }
    pub fn format_i32(a: i32) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_i32(text: &Str) -> Option<i32> { match canonical_decimal(&text.0) { Some(Some(n)) => i32::try_from(n).ok(), _ => None, } }
    pub fn format_i64(a: i64) -> Str { let text = a.to_string(); tick(text.len() as u64); Str::lit(&text) }
    pub fn parse_i64(text: &Str) -> Option<i64> { match canonical_decimal(&text.0) { Some(Some(n)) => i64::try_from(n).ok(), _ => None, } }
}

#[derive(Clone)]
pub struct Bytes(Rc<[u8]>);

impl Bytes {
    pub fn lit(octets: &[u8]) -> Bytes { Bytes(Rc::from(octets)) }
    pub fn octets(&self) -> &[u8] { &self.0 }
    pub fn append(a: Bytes, b: Bytes) -> Bytes { tick((a.0.len() + b.0.len()) as u64); let mut out = Vec::with_capacity(a.0.len() + b.0.len()); out.extend_from_slice(&a.0); out.extend_from_slice(&b.0); Bytes(Rc::from(out)) }
    pub fn length(&self) -> u64 { self.0.len() as u64 }
    pub fn index(&self, i: u64) -> Option<u8> { match usize::try_from(i) { Ok(at) => self.0.get(at).copied(), Err(_) => None, } }
    pub fn slice(&self, start: u64, count: u64) -> Option<Bytes> { let end = u128::from(start) + u128::from(count); if end > self.0.len() as u128 { return None; }; tick(count); Some(Bytes::lit(&self.0[start as usize..end as usize])) }
    pub fn utf8_decode(&self) -> Option<Str> { tick(self.0.len() as u64); match str::from_utf8(&self.0) { Ok(text) => Some(Str::lit(text)), Err(_) => None, } }
    pub fn compare(&self, other: &Bytes) -> Ordering { tick(1 + self.0.len().min(other.0.len()) as u64); self.0.cmp(&other.0) }
}

pub struct Node<T> { head: T, tail: List<T> }

pub enum Repr<T> { Nil, Run { buffer: Rc<Vec<T>>, len: usize }, Cell(Rc<Node<T>>) }

pub struct List<T>(Repr<T>);

impl<T> Clone for List<T> {
    fn clone(&self) -> Self { List(match &self.0 { Repr::Nil => Repr::Nil, Repr::Run { buffer, len } => Repr::Run { buffer: buffer.clone(), len: *len }, Repr::Cell(cell) => Repr::Cell(cell.clone()), }) }
}

impl<T> List<T> {
    fn take(&mut self) -> Repr<T> { mem::replace(&mut self.0, Repr::Nil) }
}

impl<T> Drop for Node<T> {
    fn drop(&mut self) { let mut next = match self.tail.take() { Repr::Cell(cell) => Some(cell), _ => None, }; while let Some(cell) = next { match Rc::try_unwrap(cell) { Ok(mut node) => next = match node.tail.take() { Repr::Cell(cell) => Some(cell), _ => None, }, Err(_) => break, } } }
}

impl<T: Clone> List<T> {
    pub fn nil() -> Self { List(Repr::Nil) }
    pub fn cons(head: T, mut tail: List<T>) -> Self { List(match tail.take() { Repr::Nil => { let mut buffer = Vec::with_capacity(16); buffer.push(head); Repr::Run { buffer: Rc::new(buffer), len: 1 } }, Repr::Run { mut buffer, len } => { match Rc::get_mut(&mut buffer) { Some(cells) => { cells.truncate(len); cells.push(head); Repr::Run { buffer, len: len + 1 } }, None => Repr::Cell(Rc::new(Node { head, tail: List(Repr::Run { buffer, len }) })), } }, rest => Repr::Cell(Rc::new(Node { head, tail: List(rest) })), }) }
    pub fn uncons(&self) -> Option<(T, List<T>)> { match &self.0 { Repr::Nil => None, Repr::Run { buffer, len } => { let head = buffer[len - 1].clone(); Some((head, List(if *len == 1 { Repr::Nil } else { Repr::Run { buffer: buffer.clone(), len: len - 1 } }))) }, Repr::Cell(cell) => Some((cell.head.clone(), cell.tail.clone())), } }
    pub fn length(&self) -> u64 { let mut count = 0; let mut cursor = self; while let Repr::Cell(cell) = &cursor.0 { tick(1); count += 1; cursor = &cell.tail; }; match &cursor.0 { Repr::Run { buffer, len } => { for _ in buffer[..*len].iter().rev() { tick(1); count += 1; } }, _ => { }, }; count }
    pub fn append(a: List<T>, b: List<T>) -> List<T> { List::onto(a.items(), b) }
    pub fn index(&self, i: u64) -> Option<T> { let mut position = 0; let mut cursor = self; while let Repr::Cell(cell) = &cursor.0 { tick(1); if position == i { return Some(cell.head.clone()); }; position += 1; cursor = &cell.tail; }; match &cursor.0 { Repr::Run { buffer, len } => { for item in buffer[..*len].iter().rev() { tick(1); if position == i { return Some(item.clone()); }; position += 1; } }, _ => { }, }; None }
    pub fn slice(&self, start: u64, count: u64) -> Option<List<T>> { let end = u128::from(start) + u128::from(count); let mut part = Vec::new(); let mut position: u128 = 0; let mut cursor = self; while let Repr::Cell(cell) = &cursor.0 { tick(1); if position >= end { return Some(List::onto(part, List::nil())); }; if position >= u128::from(start) { part.push(cell.head.clone()); }; position += 1; cursor = &cell.tail; }; match &cursor.0 { Repr::Run { buffer, len } => { for item in buffer[..*len].iter().rev() { tick(1); if position >= end { return Some(List::onto(part, List::nil())); }; if position >= u128::from(start) { part.push(item.clone()); }; position += 1; } }, _ => { }, }; if position < end { None } else { Some(List::onto(part, List::nil())) } }
    fn items(&self) -> Vec<T> { let mut out = Vec::new(); let mut cursor = self; while let Repr::Cell(cell) = &cursor.0 { tick(1); out.push(cell.head.clone()); cursor = &cell.tail; }; match &cursor.0 { Repr::Run { buffer, len } => { for item in buffer[..*len].iter().rev() { tick(1); out.push(item.clone()); } }, _ => { }, }; out }
    fn onto(mut items: Vec<T>, tail: List<T>) -> List<T> { let mut out = tail; while let Some(item) = items.pop() { tick(1); out = List::cons(item, out); }; out }
}

pub fn chars(text: &str) -> u64 { let mut count = 0; for _ in text.chars() { tick(1); count += 1; }; count }

impl Same for Str {
    fn same(&self, other: &Self) -> bool { let mut left = self.0.chars(); let mut right = other.0.chars(); loop { tick(1); match (left.next(), right.next()) { (None, None) => return true, (a, b) if a != b => return false, _ => { }, } } }
}

impl Same for Bytes {
    fn same(&self, other: &Self) -> bool { tick(1 + self.0.len().min(other.0.len()) as u64); self.0 == other.0 }
}

impl Key for Str {
    fn key(&self, other: &Self) -> Ordering { let mut left = self.0.chars(); let mut right = other.0.chars(); loop { tick(1); match (left.next(), right.next()) { (None, None) => return Ordering::Equal, (None, Some(_)) => return Ordering::Less, (Some(_), None) => return Ordering::Greater, (Some(a), Some(b)) => match a.cmp(&b) { Ordering::Equal => { }, decided => return decided, }, } } }
}

pub fn canonical_decimal(text: &str) -> Option<Option<i128>> { let digits = match text.strip_prefix('-') { Some(rest) => rest, None => text, }; let mut canonical = !digits.is_empty() && (digits == "0" || !digits.starts_with('0')) && text != "-0"; for c in digits.chars() { tick(1); canonical = canonical && c.is_ascii_digit(); }; if !canonical { return None; }; Some(text.parse::<i128>().ok()) }

pub fn append_list<T: Clone>(p0: List<T>, p1: List<T>) -> List<T> {
    List::append(p0, p1)
}

pub fn length_list<T: Clone>(p0: List<T>) -> u64 {
    List::length(&p0)
}

pub fn index_list<T: Clone>(p0: List<T>, p1: u64) -> Option<T> {
    List::index(&p0, p1)
}

pub fn slice_list<T: Clone>(p0: List<T>, p1: u64, p2: u64) -> Option<List<T>> {
    List::slice(&p0, p1, p2)
}

pub fn append_bytes(p0: Bytes, p1: Bytes) -> Bytes {
    Bytes::append(p0, p1)
}

pub fn length_bytes(p0: Bytes) -> u64 {
    Bytes::length(&p0)
}

pub fn length_string(p0: Str) -> u64 {
    Str::length(&p0)
}

pub fn index_bytes(p0: Bytes, p1: u64) -> Option<u8> {
    Bytes::index(&p0, p1)
}

pub fn slice_bytes(p0: Bytes, p1: u64, p2: u64) -> Option<Bytes> {
    Bytes::slice(&p0, p1, p2)
}

pub fn utf8_encode(p0: Str) -> Bytes {
    Str::utf8_encode(&p0)
}

pub fn utf8_decode(p0: Bytes) -> Option<Str> {
    Bytes::utf8_decode(&p0)
}

pub fn compare_bytes(p0: Bytes, p1: Bytes) -> Ordering {
    Bytes::compare(&p0, &p1)
}

pub fn split_exact(p0: Str, p1: Str, p2: u32) -> Option<List<Str>> {
    Str::split_exact(&p0, &p1, p2)
}

pub fn join(p0: List<Str>, p1: Str) -> Str {
    Str::join(p0, &p1)
}

pub fn format_u8(p0: u8) -> Str {
    Str::format_u8(p0)
}

pub fn parse_u8(p0: Str) -> Option<u8> {
    Str::parse_u8(&p0)
}

pub fn format_u16(p0: u16) -> Str {
    Str::format_u16(p0)
}

pub fn parse_u16(p0: Str) -> Option<u16> {
    Str::parse_u16(&p0)
}

pub fn format_u32(p0: u32) -> Str {
    Str::format_u32(p0)
}

pub fn parse_u32(p0: Str) -> Option<u32> {
    Str::parse_u32(&p0)
}

pub fn format_u64(p0: u64) -> Str {
    Str::format_u64(p0)
}

pub fn parse_u64(p0: Str) -> Option<u64> {
    Str::parse_u64(&p0)
}

pub fn format_i8(p0: i8) -> Str {
    Str::format_i8(p0)
}

pub fn parse_i8(p0: Str) -> Option<i8> {
    Str::parse_i8(&p0)
}

pub fn format_i16(p0: i16) -> Str {
    Str::format_i16(p0)
}

pub fn parse_i16(p0: Str) -> Option<i16> {
    Str::parse_i16(&p0)
}

pub fn format_i32(p0: i32) -> Str {
    Str::format_i32(p0)
}

pub fn parse_i32(p0: Str) -> Option<i32> {
    Str::parse_i32(&p0)
}

pub fn format_i64(p0: i64) -> Str {
    Str::format_i64(p0)
}

pub fn parse_i64(p0: Str) -> Option<i64> {
    Str::parse_i64(&p0)
}

pub fn parse_int(p0: Str) -> R<Option<i64>> {
    match Str::parse_int(&p0) {
        Some(v0) => {
            Ok(v0)
        }
        None => {
            Err::<Option<i64>, Overflow>(Overflow)
        }
    }
}

pub fn format_int(p0: i64) -> Str {
    Str::format_i64(p0)
}

#[derive(Clone)]
pub enum Adt0 {
    C0(List<(Bytes, Bytes)>, List<(Str, bool)>, List<(Str, Str)>, List<(Str, bool)>),
}

#[derive(Clone)]
pub enum Adt1 {
    C0(Adt2, Str, Adt4),
}

#[derive(Clone)]
pub enum Adt2 {
    C0(Str, Adt3, Str, Str, List<Str>, List<Str>),
}

#[derive(Clone)]
pub enum Adt3 {
    C0,
    C1,
}

#[derive(Clone)]
pub enum Adt4 {
    C0,
    C1,
    C2,
    C3,
}

#[derive(Clone)]
pub enum Adt5 {
    C0,
    C1,
    C2,
    C3,
    C4,
    C5,
    C6,
    C7,
    C8,
    C9,
    C10,
    C11,
    C12,
    C13,
    C14,
    C15,
}

#[derive(Clone)]
pub enum Adt6 {
    C0(u32, u32, u32, u32, u32, u32, u32, u32),
}

#[derive(Clone)]
pub enum Adt7 {
    C0(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32),
}

#[derive(Clone)]
pub enum Adt8 {
    C0(u32, u32, u32, u32, u32, u32, u32, u32, Adt7, u64),
}

#[derive(Clone)]
pub enum Fn0 {
    F6,
}

impl Fn0 {
    pub fn apply(&self, p0: Adt0, p1: Bytes) -> R<Adt0> {
        match self {
            Fn0::F6 => f6(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn1 {
    F12(List<(Bytes, Bytes)>),
    F123,
}

impl Fn1 {
    pub fn apply(&self, p0: List<Bytes>, p1: Adt1) -> R<List<Bytes>> {
        match self {
            Fn1::F12(k0) => f12(k0.clone(), p0, p1),
            Fn1::F123 => Ok(f123(p0, p1)),
        }
    }
}

#[derive(Clone)]
pub enum Fn2 {
    F14,
}

impl Fn2 {
    pub fn apply(&self, p0: Bytes, p1: (Str, u64)) -> Bytes {
        match self {
            Fn2::F14 => f14(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn3 {
    F20(List<(Str, bool)>, List<(Bytes, Bytes)>),
}

impl Fn3 {
    pub fn apply(&self, p0: (List<Adt1>, List<(List<Str>, Str)>), p1: Adt2) -> R<(List<Adt1>, List<(List<Str>, Str)>)> {
        match self {
            Fn3::F20(k0, k1) => f20(k0.clone(), k1.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn4 {
    F24(Bytes),
}

impl Fn4 {
    pub fn apply(&self, p0: (u64, (u64, List<Bytes>))) -> R<Option<(u64, (u64, List<Bytes>))>> {
        match self {
            Fn4::F24(k0) => f24(k0.clone(), p0),
        }
    }
}

#[derive(Clone)]
pub enum Fn5 {
    F28,
}

impl Fn5 {
    pub fn apply(&self, p0: List<(Bytes, Bytes)>, p1: (Bytes, Bytes)) -> List<(Bytes, Bytes)> {
        match self {
            Fn5::F28 => f28(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn6 {
    F30,
}

impl Fn6 {
    pub fn apply(&self, p0: List<(Str, Str)>, p1: (Str, Str)) -> List<(Str, Str)> {
        match self {
            Fn6::F30 => f30(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn7 {
    F34(List<(Str, Str)>),
}

impl Fn7 {
    pub fn apply(&self, p0: List<(Str, u64)>, p1: Adt1) -> List<(Str, u64)> {
        match self {
            Fn7::F34(k0) => f34(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn8 {
    F37,
    F80(Str, Str),
    F122(Str),
}

impl Fn8 {
    pub fn apply(&self, p0: List<Bytes>, p1: Str) -> List<Bytes> {
        match self {
            Fn8::F37 => f37(p0, p1),
            Fn8::F80(k0, k1) => f80(k0.clone(), k1.clone(), p0, p1),
            Fn8::F122(k0) => f122(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn9 {
    F39,
}

impl Fn9 {
    pub fn apply(&self, p0: bool, p1: Adt1) -> bool {
        match self {
            Fn9::F39 => f39(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn10 {
    F49,
}

impl Fn10 {
    pub fn apply(&self, p0: List<Bytes>, p1: Bytes) -> List<Bytes> {
        match self {
            Fn10::F49 => f49(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn11 {
    F51,
}

impl Fn11 {
    pub fn apply(&self, p0: List<Bytes>) -> Option<List<Bytes>> {
        match self {
            Fn11::F51 => f51(p0),
        }
    }
}

#[derive(Clone)]
pub enum Fn12 {
    F65,
}

impl Fn12 {
    pub fn apply(&self, p0: bool, p1: Adt4) -> bool {
        match self {
            Fn12::F65 => f65(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn13 {
    F67,
}

impl Fn13 {
    pub fn apply(&self, p0: List<Adt4>, p1: Adt1) -> List<Adt4> {
        match self {
            Fn13::F67 => f67(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn14 {
    F79(List<Str>, Str),
    F108(List<Str>),
}

impl Fn14 {
    pub fn apply(&self, p0: List<Bytes>, p1: (Bytes, Bytes)) -> R<List<Bytes>> {
        match self {
            Fn14::F79(k0, k1) => f79(k0.clone(), k1.clone(), p0, p1),
            Fn14::F108(k0) => f108(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn15 {
    F83(Str),
}

impl Fn15 {
    pub fn apply(&self, p0: Str, p1: (Str, Str)) -> Str {
        match self {
            Fn15::F83(k0) => f83(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn16 {
    F85(Bytes),
}

impl Fn16 {
    pub fn apply(&self, p0: Option<Bytes>, p1: Bytes) -> Option<Bytes> {
        match self {
            Fn16::F85(k0) => f85(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn17 {
    F88(List<Str>),
}

impl Fn17 {
    pub fn apply(&self, p0: Option<Str>, p1: (List<Str>, Str)) -> Option<Str> {
        match self {
            Fn17::F88(k0) => f88(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn18 {
    F92(List<Adt1>),
}

impl Fn18 {
    pub fn apply(&self, p0: List<Adt1>, p1: Str) -> List<Adt1> {
        match self {
            Fn18::F92(k0) => f92(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn19 {
    F95(List<Adt1>),
    F104(Str),
}

impl Fn19 {
    pub fn apply(&self, p0: bool, p1: Str) -> bool {
        match self {
            Fn19::F95(k0) => f95(k0.clone(), p0, p1),
            Fn19::F104(k0) => f104(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn20 {
    F98(Bytes, u64, Bytes, u64),
}

impl Fn20 {
    pub fn apply(&self, p0: u64) -> R<Option<u64>> {
        match self {
            Fn20::F98(k0, k1, k2, k3) => f98(k0.clone(), *k1, k2.clone(), *k3, p0),
        }
    }
}

#[derive(Clone)]
pub enum Fn21 {
    F102,
}

impl Fn21 {
    pub fn apply(&self, p0: (Option<Bytes>, List<Bytes>), p1: Bytes) -> (Option<Bytes>, List<Bytes>) {
        match self {
            Fn21::F102 => f102(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn22 {
    F113(Str),
}

impl Fn22 {
    pub fn apply(&self, p0: Option<bool>, p1: (Str, bool)) -> Option<bool> {
        match self {
            Fn22::F113(k0) => f113(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn23 {
    F118(Bytes, u64, u64),
}

impl Fn23 {
    pub fn apply(&self, p0: (u64, Adt6)) -> R<(u64, Adt6)> {
        match self {
            Fn23::F118(k0, k1, k2) => f118(k0.clone(), *k1, *k2, p0),
        }
    }
}

#[derive(Clone)]
pub enum Fn24 {
    F121(Str),
}

impl Fn24 {
    pub fn apply(&self, p0: Option<Adt1>, p1: Adt1) -> Option<Adt1> {
        match self {
            Fn24::F121(k0) => f121(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn25 {
    F125(bool, Bytes),
}

impl Fn25 {
    pub fn apply(&self, p0: u64, p1: Str) -> R<u64> {
        match self {
            Fn25::F125(k0, k1) => f125(*k0, k1.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn26 {
    F133,
}

impl Fn26 {
    pub fn apply(&self, p0: Adt8) -> R<Adt8> {
        match self {
            Fn26::F133 => f133(p0),
        }
    }
}

#[derive(Clone)]
pub enum Fn27 {
    F151,
}

impl Fn27 {
    pub fn apply(&self, p0: (u64, (u32, u32))) -> (u64, (u32, u32)) {
        match self {
            Fn27::F151 => f151(p0),
        }
    }
}

pub fn f0(v0: Bytes) -> R<Bytes> {
    let v1: Adt0 = f1(v0)?;
    f2(f3(f4(), match v1.clone() {
        Adt0::C0(h1, _, _, _) => {
            h1
        }
    }, match v1.clone() {
        Adt0::C0(_, h2, _, _) => {
            h2
        }
    })?, match v1.clone() {
        Adt0::C0(_, _, h3, _) => {
            h3
        }
    }, match v1 {
        Adt0::C0(h4, _, _, _) => {
            h4
        }
    })
}

pub fn f1(v0: Bytes) -> R<Adt0> {
    let v1: Adt0 = f5(Fn0::F6, Adt0::C0(List::<(Bytes, Bytes)>::nil(), List::<(Str, bool)>::nil(), List::<(Str, Str)>::nil(), List::<(Str, bool)>::nil()), f7(v0)?)?;
    Ok(Adt0::C0(f8(match v1.clone() {
        Adt0::C0(h5, _, _, _) => {
            h5
        }
    }), match v1.clone() {
        Adt0::C0(_, h6, _, _) => {
            h6
        }
    }, f9(match v1.clone() {
        Adt0::C0(_, _, h7, _) => {
            h7
        }
    }), match v1 {
        Adt0::C0(_, _, _, h8) => {
            h8
        }
    }))
}

pub fn f2(v0: List<Adt1>, v1: List<(Str, Str)>, v2: List<(Bytes, Bytes)>) -> R<Bytes> {
    let v3: Bytes = f10(f11(Fn1::F12(v2), List::<Bytes>::nil(), v0.clone())?);
    let v4: Bytes = f13(Fn2::F14, Bytes::lit(&[]), f15(v0.clone(), v1));
    let a13 = {
        let a11 = {
            let a9 = v3;
            let a10 = v4;
            append_bytes(a9, a10)
        };
        let a12 = f16(List::cons(Str::lit("cuttable"), List::cons(if f17(v0.clone()) {
            Str::lit("yes")
        } else {
            Str::lit("no")
        }, List::<Str>::nil())));
        append_bytes(a11, a12)
    };
    let a14 = f16(List::cons(Str::lit("releasable"), List::cons(if f18(v0) {
        Str::lit("yes")
    } else {
        Str::lit("no")
    }, List::<Str>::nil())));
    Ok(append_bytes(a13, a14))
}

pub fn f3(v0: List<Adt2>, v1: List<(Bytes, Bytes)>, v2: List<(Str, bool)>) -> R<List<Adt1>> {
    let (h15, _) = f19(Fn3::F20(v2, v1), (List::<Adt1>::nil(), List::<(List<Str>, Str)>::nil()), v0)?;
    Ok(h15)
}

pub fn f4() -> List<Adt2> {
    List::cons(f21(Str::lit("factory-model"), Adt3::C0, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'bash prism-factory/check.sh'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("prism-factory/"), List::cons(Str::lit("Justfile"), List::cons(Str::lit("rust-toolchain.toml"), List::<Str>::nil()))))), List::<Str>::nil()), List::cons(f21(Str::lit("factory-upstream"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/upstream.sh"), List::cons(Str::lit("prism-factory/"), List::<Str>::nil()), List::<Str>::nil()), List::cons(f21(Str::lit("fmt-check"), Adt3::C0, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo fmt --all -- --check'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("rustfmt.toml"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("model"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo run -q --locked -p xtask -- validate'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit(".devcontainer/"), List::cons(Str::lit(".github/"), List::cons(Str::lit("AGENTS.md"), List::cons(Str::lit("CONFORMANCE.md"), List::cons(Str::lit("README.md"), List::cons(Str::lit("VERIFICATION.md"), List::cons(Str::lit("template.lock"), List::cons(Str::lit("template-contract.json"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("model/"), List::cons(Str::lit("LANES.md"), List::cons(Str::lit("TEMPLATE-CONTRACT.md"), List::cons(Str::lit("TEMPLATE-VERIFICATION.md"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("template-check"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh '(prismpm template check) && (prismpm lock check)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit(".devcontainer/"), List::cons(Str::lit(".github/"), List::cons(Str::lit("AGENTS.md"), List::cons(Str::lit("CONFORMANCE.md"), List::cons(Str::lit("README.md"), List::cons(Str::lit("VERIFICATION.md"), List::cons(Str::lit("template.lock"), List::cons(Str::lit("template-contract.json"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("model/"), List::cons(Str::lit("Justfile"), List::<Str>::nil())))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("self-model-check"), Adt3::C0, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh scripts/self-model/check.sh"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("model/"), List::cons(Str::lit("app/"), List::cons(Str::lit("brand/"), List::cons(Str::lit("scripts/self-model/"), List::cons(Str::lit("scripts/experience/"), List::cons(Str::lit("scripts/mir/ledger.py"), List::cons(Str::lit("crates/uorust/src/"), List::cons(Str::lit("crates/uorust/core/frontier.toml"), List::cons(Str::lit("crates/uorust/std/frontier.toml"), List::cons(Str::lit("crates/uorust/crates/"), List::cons(Str::lit("crates/certified/manifest.toml"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("site/app.json"), List::cons(Str::lit(".github/workflows/"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("README.md"), List::<Str>::nil())))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("lint"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo clippy --workspace --all-targets --all-features --locked -- -D warnings'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("clippy.toml"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("test"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo test --workspace --locked'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::cons(Str::lit("model/"), List::cons(Str::lit("features/"), List::cons(Str::lit("app/"), List::cons(Str::lit("Justfile"), List::cons(Str::lit("CONFORMANCE.md"), List::cons(Str::lit("README.md"), List::cons(Str::lit("VERIFICATION.md"), List::cons(Str::lit("ANALYSIS.md"), List::cons(Str::lit("scripts/ci/"), List::cons(Str::lit("scripts/hooks/passes.py"), List::cons(Str::lit("scripts/release/"), List::cons(Str::lit("scripts/self-model/"), List::cons(Str::lit("bootstrap/acceptance-result.test.mjs"), List::cons(Str::lit("site/acceptance-result.mjs"), List::cons(Str::lit("site/guest-worker.mjs"), List::cons(Str::lit("site/spec.mjs"), List::cons(Str::lit("prism-factory/"), List::cons(Str::lit("brand/foundation/src/index.css"), List::cons(Str::lit("@base:frontier.toml"), List::cons(Str::lit("@base:crates/certified/manifest.toml"), List::<Str>::nil())))))))))))))))))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("deny"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo deny --all-features check'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("deny.toml"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("app"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh '(cd app && lexlean fmt --check && lexlean lock --check) && (bash -euo pipefail -c '\"'\"'case \"${PRISM_PROOF_EVENT:-}\" in pull_request:*|merge_group:*|push:refs/heads/main) if cargo run -q --locked -p xtask -- proof-reuse; then exit 0; fi ;; esac; rm -rf .prism-proof; prismpm --project app verify | tee .prism-proof.log; attestation=$(sed -n \"s/^verified attestation: \\([0-9a-f]\\{64\\}\\)$/\\1/p\" .prism-proof.log); rm -f .prism-proof.log; cargo run -q --locked -p xtask -- proof-keep \"$attestation\"'\"'\"')'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("app/"), List::cons(Str::lit("Justfile"), List::<Str>::nil()))))))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("uorust"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh '(cargo run -q --locked -p uorust -- check crates/uorust/tests/fixtures/thermostat --out crates/uorust/tests/fixtures/thermostat-prism --name thermostat --last-modified 2026-11-11T00:00:00Z) && (cd crates/uorust/tests/fixtures/thermostat-prism && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check) && (rm -rf target/uorust-self) && (cargo run -q --locked -p uorust -- mir . --out target/mir) && (cargo run -q --locked -p uorust -- project . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --mir target/mir --verdicts lift-verdicts.toml) && (cargo run -q --locked -p uorust -- check . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --register frontier.toml --mir target/mir --verdicts lift-verdicts.toml) && (cd target/uorust-self && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check) && (rm -rf target/uorust-advised) && (cargo run -q --locked -p uorust -- project crates/uorust/tests/fixtures/advised --out target/uorust-advised --name advised --last-modified 2026-11-11T00:00:00Z --advisories crates/uorust/tests/fixtures/advised/advisory-db) && (cd target/uorust-advised && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::<Str>::nil())))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("vv"), Adt3::C0, Str::lit("host"), Str::lit("echo \"vv: the acceptance gate passed\""), List::cons(Str::lit("Justfile"), List::<Str>::nil()), List::cons(Str::lit("fmt-check"), List::cons(Str::lit("model"), List::cons(Str::lit("template-check"), List::cons(Str::lit("self-model-check"), List::cons(Str::lit("lint"), List::cons(Str::lit("test"), List::cons(Str::lit("deny"), List::cons(Str::lit("app"), List::cons(Str::lit("uorust"), List::<Str>::nil())))))))))), List::cons(f21(Str::lit("prove-fixture"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/written.sh crates/uorust/tests/fixtures/mir-path/lift-verdicts.toml -- bash prism-factory/sdk.sh '(cargo run -q --locked -p uorust -- mir crates/uorust/tests/fixtures/mir-path --out target/mir-fixture) && (cargo run -q --locked -p uorust -- prove crates/uorust/tests/fixtures/mir-path --out target/mir-fixture-prove --scratch target/mir-fixture-scratch --name walk --last-modified 2026-11-11T00:00:00Z --mir target/mir-fixture --verdicts crates/uorust/tests/fixtures/mir-path/lift-verdicts.toml)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("crates/uorust/tests/fixtures/mir-path/"), List::cons(Str::lit("prism-factory/written.sh"), List::<Str>::nil()))))))))))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("prove"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/written.sh lift-verdicts.toml -- bash prism-factory/sdk.sh '(cargo run -q --locked -p uorust -- mir . --out target/mir) && (cargo run -q --locked -p uorust -- prove . --out target/uorust-prove --scratch target/mir-prove --name uorust --last-modified 2026-11-11T00:00:00Z --mir target/mir --verdicts lift-verdicts.toml)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::cons(Str::lit("prism-factory/written.sh"), List::<Str>::nil()))))))))))))), List::<Str>::nil()), List::cons(f21(Str::lit("certified"), Adt3::C1, Str::lit("host"), Str::lit("bash prism-factory/written.sh crates/certified/ crates/uorust/src/certified.rs -- bash prism-factory/sdk.sh '(just certified-project) && (just certified-export \"\" ) && (just certified-sync)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::cons(Str::lit("Justfile"), List::cons(Str::lit("prism-factory/written.sh"), List::<Str>::nil())))))))))))))), List::cons(Str::lit("prove"), List::<Str>::nil())), List::<Adt2>::nil())))))))))))))))
}

pub fn f5(v0: Fn0, p19: Adt0, p20: List<Bytes>) -> R<Adt0> {
    let m18 = (p19, p20);
    {
        let mut m18 = m18;
        loop {
            let step = {
                let (v1, v2) = m18;
                let m16 = v2;
                match m16.uncons() {
                    None => {
                        Err::<(Adt0, List<Bytes>), R<Adt0>>(Ok(v1))
                    }
                    Some((v3, v4)) => {
                        Ok::<(Adt0, List<Bytes>), R<Adt0>>(({
                            let c17: Fn0 = v0.clone();
                            c17.apply(v1, v3)?
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m18 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f6(v0: Adt0, v1: Bytes) -> R<Adt0> {
    f22(v0, v1)
}

pub fn f7(v0: Bytes) -> R<List<Bytes>> {
    let v1: u64 = {
        let a21 = v0.clone();
        length_bytes(a21)
    };
    let v2: (u64, (u64, List<Bytes>)) = {
        let (h22, _) = f23(Fn4::F24(v0.clone()), v1, (0u64, (0u64, List::<Bytes>::nil())))?;
        h22
    };
    let v3: u64 = {
        let (h23, _) = {
            let (_, h24) = v2.clone();
            h24
        };
        h23
    };
    let v4: List<Bytes> = {
        let (_, h25) = {
            let (_, h26) = v2;
            h26
        };
        h25
    };
    let a27 = v3;
    let a28 = v1;
    if nat_lt(a27, a28) {
        Ok(f25(List::cons(f26(v0, v3, {
            let a29 = v1;
            let a30 = v3;
            nat_sub(a29, a30)
        }), v4)))
    } else {
        Ok(f25(v4))
    }
}

pub fn f8(v0: List<(Bytes, Bytes)>) -> List<(Bytes, Bytes)> {
    f27(Fn5::F28, List::<(Bytes, Bytes)>::nil(), v0)
}

pub fn f9(v0: List<(Str, Str)>) -> List<(Str, Str)> {
    f29(Fn6::F30, List::<(Str, Str)>::nil(), v0)
}

pub fn f10(v0: List<Bytes>) -> Bytes {
    f31(f25(v0))
}

pub fn f11(v0: Fn1, p34: List<Bytes>, p35: List<Adt1>) -> R<List<Bytes>> {
    let m33 = (p34, p35);
    {
        let mut m33 = m33;
        loop {
            let step = {
                let (v1, v2) = m33;
                let m31 = v2;
                match m31.uncons() {
                    None => {
                        Err::<(List<Bytes>, List<Adt1>), R<List<Bytes>>>(Ok(v1))
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<Bytes>, List<Adt1>), R<List<Bytes>>>(({
                            let c32: Fn1 = v0.clone();
                            c32.apply(v1, v3)?
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m33 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f12(v0: List<(Bytes, Bytes)>, v1: List<Bytes>, v2: Adt1) -> R<List<Bytes>> {
    Ok(List::cons(f32(v2, v0)?, v1))
}

pub fn f13(v0: Fn2, p39: Bytes, p40: List<(Str, u64)>) -> Bytes {
    let m38 = (p39, p40);
    {
        let mut m38 = m38;
        loop {
            let step = {
                let (v1, v2) = m38;
                let m36 = v2;
                match m36.uncons() {
                    None => {
                        Err::<(Bytes, List<(Str, u64)>), Bytes>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Bytes, List<(Str, u64)>), Bytes>(({
                            let c37: Fn2 = v0.clone();
                            c37.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m38 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f14(v0: Bytes, v1: (Str, u64)) -> Bytes {
    let a42 = v0;
    let a43 = f16(List::cons(Str::lit("schedule"), List::cons({
        let (h41, _) = v1;
        h41
    }, List::<Str>::nil())));
    append_bytes(a42, a43)
}

pub fn f15(v0: List<Adt1>, v1: List<(Str, Str)>) -> List<(Str, u64)> {
    f33(Fn7::F34(v1), List::<(Str, u64)>::nil(), v0)
}

pub fn f16(v0: List<Str>) -> Bytes {
    f35(f36(Fn8::F37, List::<Bytes>::nil(), v0))
}

pub fn f17(v0: List<Adt1>) -> bool {
    f38(Fn9::F39, true, v0)
}

pub fn f18(v0: List<Adt1>) -> bool {
    f40(f41(v0))
}

pub fn f19(v0: Fn3, p47: (List<Adt1>, List<(List<Str>, Str)>), p48: List<Adt2>) -> R<(List<Adt1>, List<(List<Str>, Str)>)> {
    let m46 = (p47, p48);
    {
        let mut m46 = m46;
        loop {
            let step = {
                let (v1, v2) = m46;
                let m44 = v2;
                match m44.uncons() {
                    None => {
                        Err::<((List<Adt1>, List<(List<Str>, Str)>), List<Adt2>), R<(List<Adt1>, List<(List<Str>, Str)>)>>(Ok(v1))
                    }
                    Some((v3, v4)) => {
                        Ok::<((List<Adt1>, List<(List<Str>, Str)>), List<Adt2>), R<(List<Adt1>, List<(List<Str>, Str)>)>>(({
                            let c45: Fn3 = v0.clone();
                            c45.apply(v1, v3)?
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m46 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f20(v0: List<(Str, bool)>, v1: List<(Bytes, Bytes)>, v2: (List<Adt1>, List<(List<Str>, Str)>), v3: Adt2) -> R<(List<Adt1>, List<(List<Str>, Str)>)> {
    f42(v1, v0, v2, v3)
}

pub fn f21(v0: Str, v1: Adt3, v2: Str, v3: Str, v4: List<Str>, v5: List<Str>) -> Adt2 {
    Adt2::C0(v0, v1, v2, v3, v4, v5)
}

pub fn f22(v0: Adt0, v1: Bytes) -> R<Adt0> {
    let v2: (Bytes, Bytes) = f43(v1)?;
    let v3: Bytes = {
        let (h49, _) = v2;
        h49
    };
    let v4: (Bytes, Bytes) = f43({
        let (_, h50) = v2;
        h50
    })?;
    let a51 = v3.clone();
    let a52 = f44(Str::lit("tree"));
    if equal(a51, a52) {
        Ok(Adt0::C0(List::cons(({
            let (h53, _) = v4;
            h53
        }, {
            let (_, h54) = v4;
            h54
        }), match v0.clone() {
            Adt0::C0(h55, _, _, _) => {
                h55
            }
        }), match v0.clone() {
            Adt0::C0(_, h56, _, _) => {
                h56
            }
        }, match v0.clone() {
            Adt0::C0(_, _, h57, _) => {
                h57
            }
        }, match v0 {
            Adt0::C0(_, _, _, h58) => {
                h58
            }
        }))
    } else {
        let a59 = v3.clone();
        let a60 = f44(Str::lit("receipt"));
        if equal(a59, a60) {
            Ok(Adt0::C0(match v0.clone() {
                Adt0::C0(h61, _, _, _) => {
                    h61
                }
            }, List::cons((f45({
                let (h62, _) = v4;
                h62
            }), f46({
                let (_, h63) = v4;
                h63
            })), match v0.clone() {
                Adt0::C0(_, h64, _, _) => {
                    h64
                }
            }), match v0.clone() {
                Adt0::C0(_, _, h65, _) => {
                    h65
                }
            }, match v0 {
                Adt0::C0(_, _, _, h66) => {
                    h66
                }
            }))
        } else {
            let a67 = v3.clone();
            let a68 = f44(Str::lit("weight"));
            if equal(a67, a68) {
                Ok(Adt0::C0(match v0.clone() {
                    Adt0::C0(h69, _, _, _) => {
                        h69
                    }
                }, match v0.clone() {
                    Adt0::C0(_, h70, _, _) => {
                        h70
                    }
                }, List::cons((f45({
                    let (h71, _) = v4;
                    h71
                }), f45({
                    let (_, h72) = v4;
                    h72
                })), match v0.clone() {
                    Adt0::C0(_, _, h73, _) => {
                        h73
                    }
                }), match v0 {
                    Adt0::C0(_, _, _, h74) => {
                        h74
                    }
                }))
            } else {
                let a75 = v3;
                let a76 = f44(Str::lit("outcome"));
                if equal(a75, a76) {
                    Ok(Adt0::C0(match v0.clone() {
                        Adt0::C0(h77, _, _, _) => {
                            h77
                        }
                    }, match v0.clone() {
                        Adt0::C0(_, h78, _, _) => {
                            h78
                        }
                    }, match v0.clone() {
                        Adt0::C0(_, _, h79, _) => {
                            h79
                        }
                    }, List::cons((f45({
                        let (h80, _) = v4;
                        h80
                    }), f46({
                        let (_, h81) = v4;
                        h81
                    })), match v0 {
                        Adt0::C0(_, _, _, h82) => {
                            h82
                        }
                    })))
                } else {
                    Ok(v0)
                }
            }
        }
    }
}

pub fn f23(v0: Fn4, p87: u64, p88: (u64, (u64, List<Bytes>))) -> R<((u64, (u64, List<Bytes>)), bool)> {
    let m86 = (p87, p88);
    {
        let mut m86 = m86;
        loop {
            let step = {
                let (v1, v2) = m86;
                let m83 = v1;
                if m83 == 0 {
                    Err::<(u64, (u64, (u64, List<Bytes>))), R<((u64, (u64, List<Bytes>)), bool)>>(Ok((v2, false)))
                } else {
                    let v3 = m83 - 1;
                    let m85 = {
                        let c84: Fn4 = v0.clone();
                        c84.apply(v2.clone())?
                    };
                    match m85 {
                        None => {
                            Err::<(u64, (u64, (u64, List<Bytes>))), R<((u64, (u64, List<Bytes>)), bool)>>(Ok((v2, true)))
                        }
                        Some(v4) => {
                            Ok::<(u64, (u64, (u64, List<Bytes>))), R<((u64, (u64, List<Bytes>)), bool)>>((v3, v4))
                        }
                    }
                }
            };
            match step {
                Ok(next) => { m86 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f24(v0: Bytes, v1: (u64, (u64, List<Bytes>))) -> R<Option<(u64, (u64, List<Bytes>))>> {
    f47(v0, v1)
}

pub fn f25(v0: List<Bytes>) -> List<Bytes> {
    f48(Fn10::F49, List::<Bytes>::nil(), v0)
}

pub fn f26(v0: Bytes, v1: u64, v2: u64) -> Bytes {
    let m92 = {
        let a89 = v0;
        let a90 = v1;
        let a91 = v2;
        slice_bytes(a89, a90, a91)
    };
    match m92 {
        None => {
            Bytes::lit(&[])
        }
        Some(v3) => {
            v3
        }
    }
}

pub fn f27(v0: Fn5, p96: List<(Bytes, Bytes)>, p97: List<(Bytes, Bytes)>) -> List<(Bytes, Bytes)> {
    let m95 = (p96, p97);
    {
        let mut m95 = m95;
        loop {
            let step = {
                let (v1, v2) = m95;
                let m93 = v2;
                match m93.uncons() {
                    None => {
                        Err::<(List<(Bytes, Bytes)>, List<(Bytes, Bytes)>), List<(Bytes, Bytes)>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<(Bytes, Bytes)>, List<(Bytes, Bytes)>), List<(Bytes, Bytes)>>(({
                            let c94: Fn5 = v0.clone();
                            c94.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m95 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f28(v0: List<(Bytes, Bytes)>, v1: (Bytes, Bytes)) -> List<(Bytes, Bytes)> {
    List::cons(v1, v0)
}

pub fn f29(v0: Fn6, p101: List<(Str, Str)>, p102: List<(Str, Str)>) -> List<(Str, Str)> {
    let m100 = (p101, p102);
    {
        let mut m100 = m100;
        loop {
            let step = {
                let (v1, v2) = m100;
                let m98 = v2;
                match m98.uncons() {
                    None => {
                        Err::<(List<(Str, Str)>, List<(Str, Str)>), List<(Str, Str)>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<(Str, Str)>, List<(Str, Str)>), List<(Str, Str)>>(({
                            let c99: Fn6 = v0.clone();
                            c99.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m100 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f30(v0: List<(Str, Str)>, v1: (Str, Str)) -> List<(Str, Str)> {
    List::cons(v1, v0)
}

pub fn f31(v0: List<Bytes>) -> Bytes {
    let m105 = {
        let (h103, _) = f50(Fn11::F51, {
            let a104 = v0.clone();
            length_list(a104)
        }, v0);
        h103
    };
    match m105.uncons() {
        None => {
            Bytes::lit(&[])
        }
        Some((v1, _)) => {
            v1
        }
    }
}

pub fn f32(v0: Adt1, v1: List<(Bytes, Bytes)>) -> R<Bytes> {
    let v2: Adt2 = match v0.clone() {
        Adt1::C0(h106, _, _) => {
            h106
        }
    };
    let v3: Str = match v2.clone() {
        Adt2::C0(h107, _, _, _, _, _) => {
            h107
        }
    };
    let v4: Bytes = f16(List::cons(Str::lit("obligation"), List::cons(v3.clone(), List::cons(f52(match v0.clone() {
        Adt1::C0(_, _, h108) => {
            h108
        }
    }), List::cons(match v0.clone() {
        Adt1::C0(_, h109, _) => {
            h109
        }
    }, List::<Str>::nil())))));
    let v5: Bytes = {
        let a115 = {
            let a112 = f16(List::cons(Str::lit("kind"), List::cons(v3.clone(), List::cons(f53(match v2.clone() {
                Adt2::C0(_, h110, _, _, _, _) => {
                    h110
                }
            }), List::<Str>::nil()))));
            let a113 = f16(List::cons(Str::lit("environment"), List::cons(v3.clone(), List::cons(match v2.clone() {
                Adt2::C0(_, _, h111, _, _, _) => {
                    h111
                }
            }, List::<Str>::nil()))));
            append_bytes(a112, a113)
        };
        let a116 = f16(List::cons(Str::lit("run"), List::cons(v3.clone(), List::cons(match v2.clone() {
            Adt2::C0(_, _, _, h114, _, _) => {
                h114
            }
        }, List::<Str>::nil()))));
        append_bytes(a115, a116)
    };
    let v6: Bytes = if f54(v0) {
        f55(v3.clone(), match v2.clone() {
            Adt2::C0(_, _, _, _, h117, _) => {
                h117
            }
        }, v1)?
    } else {
        Bytes::lit(&[])
    };
    let a126 = {
        let a118 = v4;
        let a119 = v5;
        append_bytes(a118, a119)
    };
    let a127 = {
        let a124 = {
            let a122 = f56(Str::lit("input"), v3.clone(), match v2.clone() {
                Adt2::C0(_, _, _, _, h120, _) => {
                    h120
                }
            });
            let a123 = f56(Str::lit("needs"), v3, match v2 {
                Adt2::C0(_, _, _, _, _, h121) => {
                    h121
                }
            });
            append_bytes(a122, a123)
        };
        let a125 = v6;
        append_bytes(a124, a125)
    };
    Ok(append_bytes(a126, a127))
}

pub fn f33(v0: Fn7, p131: List<(Str, u64)>, p132: List<Adt1>) -> List<(Str, u64)> {
    let m130 = (p131, p132);
    {
        let mut m130 = m130;
        loop {
            let step = {
                let (v1, v2) = m130;
                let m128 = v2;
                match m128.uncons() {
                    None => {
                        Err::<(List<(Str, u64)>, List<Adt1>), List<(Str, u64)>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<(Str, u64)>, List<Adt1>), List<(Str, u64)>>(({
                            let c129: Fn7 = v0.clone();
                            c129.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m130 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f34(v0: List<(Str, Str)>, v1: List<(Str, u64)>, v2: Adt1) -> List<(Str, u64)> {
    if f57(match v2.clone() {
        Adt1::C0(_, _, h133) => {
            h133
        }
    }) {
        f58(({
            let m136 = match v2.clone() {
                Adt1::C0(h135, _, _) => {
                    h135
                }
            };
            match m136 {
                Adt2::C0(h134, _, _, _, _, _) => {
                    h134
                }
            }
        }, f59(f60({
            let m139 = match v2 {
                Adt1::C0(h138, _, _) => {
                    h138
                }
            };
            match m139 {
                Adt2::C0(h137, _, _, _, _, _) => {
                    h137
                }
            }
        }, v0))), v1)
    } else {
        v1
    }
}

pub fn f35(v0: List<Bytes>) -> Bytes {
    let a140 = f61(v0, Bytes::lit(&[0x20]));
    let a141 = Bytes::lit(&[0x0a]);
    append_bytes(a140, a141)
}

pub fn f36(v0: Fn8, p145: List<Bytes>, p146: List<Str>) -> List<Bytes> {
    let m144 = (p145, p146);
    {
        let mut m144 = m144;
        loop {
            let step = {
                let (v1, v2) = m144;
                let m142 = v2;
                match m142.uncons() {
                    None => {
                        Err::<(List<Bytes>, List<Str>), List<Bytes>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<Bytes>, List<Str>), List<Bytes>>(({
                            let c143: Fn8 = v0.clone();
                            c143.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m144 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f37(v0: List<Bytes>, v1: Str) -> List<Bytes> {
    let a147 = v0;
    let a148 = List::cons(f44(v1), List::<Bytes>::nil());
    append_list(a147, a148)
}

pub fn f38(v0: Fn9, p152: bool, p153: List<Adt1>) -> bool {
    let m151 = (p152, p153);
    {
        let mut m151 = m151;
        loop {
            let step = {
                let (v1, v2) = m151;
                let m149 = v2;
                match m149.uncons() {
                    None => {
                        Err::<(bool, List<Adt1>), bool>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(bool, List<Adt1>), bool>(({
                            let c150: Fn9 = v0.clone();
                            c150.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m151 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f39(v0: bool, v1: Adt1) -> bool {
    if v0 {
        let a155 = f62(match v1.clone() {
            Adt1::C0(h154, _, _) => {
                h154
            }
        });
        if bool_not(a155) {
            true
        } else {
            f63(match v1 {
                Adt1::C0(_, _, h156) => {
                    h156
                }
            })
        }
    } else {
        false
    }
}

pub fn f40(v0: List<Adt4>) -> bool {
    f64(Fn12::F65, true, v0)
}

pub fn f41(v0: List<Adt1>) -> List<Adt4> {
    f66(Fn13::F67, List::<Adt4>::nil(), v0)
}

pub fn f42(v0: List<(Bytes, Bytes)>, v1: List<(Str, bool)>, v2: (List<Adt1>, List<(List<Str>, Str)>), v3: Adt2) -> R<(List<Adt1>, List<(List<Str>, Str)>)> {
    let v4: List<Adt1> = {
        let (h157, _) = v2;
        h157
    };
    let v5: List<(List<Str>, Str)> = {
        let (_, h158) = v2;
        h158
    };
    let v6: List<Str> = match v3.clone() {
        Adt2::C0(_, _, _, _, h159, _) => {
            h159
        }
    };
    let v8: Str = {
        let m160 = f68(v6.clone(), v5.clone());
        match m160 {
            None => {
                f69(v6.clone(), v0)?
            }
            Some(v7) => {
                v7
            }
        }
    };
    let v9: List<Adt1> = f70(match v3.clone() {
        Adt2::C0(_, _, _, _, _, h161) => {
            h161
        }
    }, v4.clone());
    let v10: Str = f71(v3.clone(), v8.clone(), v9.clone())?;
    let v11: List<Adt4> = if f72(match v3.clone() {
        Adt2::C0(_, _, _, _, _, h162) => {
            h162
        }
    }, v4.clone()) {
        f41(v9)
    } else {
        List::cons(Adt4::C3, List::<Adt4>::nil())
    };
    Ok(({
        let a164 = v4;
        let a165 = List::cons(Adt1::C0(v3.clone(), v10.clone(), f73(match v3 {
            Adt2::C0(_, h163, _, _, _, _) => {
                h163
            }
        }, v10, v1, v11)), List::<Adt1>::nil());
        append_list(a164, a165)
    }, List::cons((v6, v8), v5)))
}

pub fn f43(v0: Bytes) -> R<(Bytes, Bytes)> {
    let m166 = f74(v0.clone(), Bytes::lit(&[0x20]))?;
    match m166 {
        None => {
            Ok((v0, Bytes::lit(&[])))
        }
        Some(v1) => {
            Ok((f26(v0.clone(), 0u64, v1), f26(v0.clone(), {
                let a167 = v1;
                let a168 = 1u64;
                nat_add(a167, a168)?
            }, {
                let a172 = {
                    let a169 = v0;
                    length_bytes(a169)
                };
                let a173 = {
                    let a170 = v1;
                    let a171 = 1u64;
                    nat_add(a170, a171)?
                };
                nat_sub(a172, a173)
            })))
        }
    }
}

pub fn f44(v0: Str) -> Bytes {
    let a174 = v0;
    utf8_encode(a174)
}

pub fn f45(v0: Bytes) -> Str {
    let m176 = {
        let a175 = v0;
        utf8_decode(a175)
    };
    match m176 {
        None => {
            Str::lit("")
        }
        Some(v1) => {
            v1
        }
    }
}

pub fn f46(v0: Bytes) -> bool {
    let a177 = v0;
    let a178 = f44(Str::lit("pass"));
    equal(a177, a178)
}

pub fn f47(v0: Bytes, v1: (u64, (u64, List<Bytes>))) -> R<Option<(u64, (u64, List<Bytes>))>> {
    let v2: u64 = {
        let (h179, _) = v1.clone();
        h179
    };
    let v3: u64 = {
        let (h180, _) = {
            let (_, h181) = v1.clone();
            h181
        };
        h180
    };
    let v4: List<Bytes> = {
        let (_, h182) = {
            let (_, h183) = v1;
            h183
        };
        h182
    };
    let m184 = f75(v0.clone(), v2);
    match m184 {
        None => {
            Ok(None::<(u64, (u64, List<Bytes>))>)
        }
        Some(v5) => {
            let a185 = v5;
            let a186 = 10u8;
            if equal(a185, a186) {
                Ok(Some(({
                    let a187 = v2;
                    let a188 = 1u64;
                    nat_add(a187, a188)?
                }, ({
                    let a189 = v2;
                    let a190 = 1u64;
                    nat_add(a189, a190)?
                }, List::cons(f76(f26(v0, v3, {
                    let a191 = v2;
                    let a192 = v3;
                    nat_sub(a191, a192)
                })), v4)))))
            } else {
                Ok(Some(({
                    let a193 = v2;
                    let a194 = 1u64;
                    nat_add(a193, a194)?
                }, (v3, v4))))
            }
        }
    }
}

pub fn f48(v0: Fn10, p198: List<Bytes>, p199: List<Bytes>) -> List<Bytes> {
    let m197 = (p198, p199);
    {
        let mut m197 = m197;
        loop {
            let step = {
                let (v1, v2) = m197;
                let m195 = v2;
                match m195.uncons() {
                    None => {
                        Err::<(List<Bytes>, List<Bytes>), List<Bytes>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<Bytes>, List<Bytes>), List<Bytes>>(({
                            let c196: Fn10 = v0.clone();
                            c196.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m197 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f49(v0: List<Bytes>, v1: Bytes) -> List<Bytes> {
    List::cons(v1, v0)
}

pub fn f50(v0: Fn11, p204: u64, p205: List<Bytes>) -> (List<Bytes>, bool) {
    let m203 = (p204, p205);
    {
        let mut m203 = m203;
        loop {
            let step = {
                let (v1, v2) = m203;
                let m200 = v1;
                if m200 == 0 {
                    Err::<(u64, List<Bytes>), (List<Bytes>, bool)>((v2, false))
                } else {
                    let v3 = m200 - 1;
                    let m202 = {
                        let c201: Fn11 = v0.clone();
                        c201.apply(v2.clone())
                    };
                    match m202 {
                        None => {
                            Err::<(u64, List<Bytes>), (List<Bytes>, bool)>((v2, true))
                        }
                        Some(v4) => {
                            Ok::<(u64, List<Bytes>), (List<Bytes>, bool)>((v3, v4))
                        }
                    }
                }
            };
            match step {
                Ok(next) => { m203 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f51(v0: List<Bytes>) -> Option<List<Bytes>> {
    f77(v0)
}

pub fn f52(v0: Adt4) -> Str {
    let m206 = v0;
    match m206 {
        Adt4::C0 => {
            Str::lit("proved")
        }
        Adt4::C1 => {
            Str::lit("observe")
        }
        Adt4::C2 => {
            Str::lit("refuted")
        }
        Adt4::C3 => {
            Str::lit("blocked")
        }
    }
}

pub fn f53(v0: Adt3) -> Str {
    let m207 = v0;
    match m207 {
        Adt3::C0 => {
            Str::lit("hermetic")
        }
        Adt3::C1 => {
            Str::lit("fetch")
        }
    }
}

pub fn f54(v0: Adt1) -> bool {
    if f57(match v0.clone() {
        Adt1::C0(_, _, h208) => {
            h208
        }
    }) {
        let a212 = {
            let m211 = match v0 {
                Adt1::C0(h210, _, _) => {
                    h210
                }
            };
            match m211 {
                Adt2::C0(_, _, h209, _, _, _) => {
                    h209
                }
            }
        };
        let a213 = Str::lit("host");
        equal(a212, a213)
    } else {
        false
    }
}

pub fn f55(v0: Str, v1: List<Str>, v2: List<(Bytes, Bytes)>) -> R<Bytes> {
    Ok(f10(f78(Fn14::F79(v1, v0), List::<Bytes>::nil(), v2)?))
}

pub fn f56(v0: Str, v1: Str, v2: List<Str>) -> Bytes {
    f10(f36(Fn8::F80(v1, v0), List::<Bytes>::nil(), v2))
}

pub fn f57(v0: Adt4) -> bool {
    let m214 = v0;
    match m214 {
        Adt4::C0 => {
            false
        }
        Adt4::C1 => {
            true
        }
        Adt4::C2 => {
            false
        }
        Adt4::C3 => {
            false
        }
    }
}

pub fn f58(v0: (Str, u64), v1: List<(Str, u64)>) -> List<(Str, u64)> {
    let m215 = v1;
    match m215.uncons() {
        None => {
            List::cons(v0, List::<(Str, u64)>::nil())
        }
        Some((v2, v3)) => {
            let a218 = {
                let (_, h216) = v2.clone();
                h216
            };
            let a219 = {
                let (_, h217) = v0.clone();
                h217
            };
            if nat_lt(a218, a219) {
                List::cons(v0, List::cons(v2, v3))
            } else {
                List::cons(v2, f58(v0, v3))
            }
        }
    }
}

pub fn f59(v0: Adt5) -> u64 {
    let m220 = v0;
    match m220 {
        Adt5::C0 => {
            1u64
        }
        Adt5::C1 => {
            2u64
        }
        Adt5::C2 => {
            3u64
        }
        Adt5::C3 => {
            4u64
        }
        Adt5::C4 => {
            5u64
        }
        Adt5::C5 => {
            6u64
        }
        Adt5::C6 => {
            7u64
        }
        Adt5::C7 => {
            8u64
        }
        Adt5::C8 => {
            9u64
        }
        Adt5::C9 => {
            10u64
        }
        Adt5::C10 => {
            11u64
        }
        Adt5::C11 => {
            12u64
        }
        Adt5::C12 => {
            13u64
        }
        Adt5::C13 => {
            14u64
        }
        Adt5::C14 => {
            15u64
        }
        Adt5::C15 => {
            16u64
        }
    }
}

pub fn f60(v0: Str, v1: List<(Str, Str)>) -> Adt5 {
    f81(f82(Fn15::F83(v0), Str::lit("4"), v1))
}

pub fn f61(v0: List<Bytes>, v1: Bytes) -> Bytes {
    let m221 = f84(Fn16::F85(v1), None::<Bytes>, v0);
    match m221 {
        None => {
            Bytes::lit(&[])
        }
        Some(v2) => {
            v2
        }
    }
}

pub fn f62(v0: Adt2) -> bool {
    let a223 = f86(Str::lit("@mode"), match v0.clone() {
        Adt2::C0(_, _, _, _, h222, _) => {
            h222
        }
    });
    if bool_not(a223) {
        let a225 = f86(Str::lit("@run"), match v0 {
            Adt2::C0(_, _, _, _, h224, _) => {
                h224
            }
        });
        bool_not(a225)
    } else {
        false
    }
}

pub fn f63(v0: Adt4) -> bool {
    let m226 = v0;
    match m226 {
        Adt4::C0 => {
            true
        }
        Adt4::C1 => {
            false
        }
        Adt4::C2 => {
            false
        }
        Adt4::C3 => {
            false
        }
    }
}

pub fn f64(v0: Fn12, p230: bool, p231: List<Adt4>) -> bool {
    let m229 = (p230, p231);
    {
        let mut m229 = m229;
        loop {
            let step = {
                let (v1, v2) = m229;
                let m227 = v2;
                match m227.uncons() {
                    None => {
                        Err::<(bool, List<Adt4>), bool>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(bool, List<Adt4>), bool>(({
                            let c228: Fn12 = v0.clone();
                            c228.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m229 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f65(v0: bool, v1: Adt4) -> bool {
    if v0 {
        f63(v1)
    } else {
        false
    }
}

pub fn f66(v0: Fn13, p235: List<Adt4>, p236: List<Adt1>) -> List<Adt4> {
    let m234 = (p235, p236);
    {
        let mut m234 = m234;
        loop {
            let step = {
                let (v1, v2) = m234;
                let m232 = v2;
                match m232.uncons() {
                    None => {
                        Err::<(List<Adt4>, List<Adt1>), List<Adt4>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<Adt4>, List<Adt1>), List<Adt4>>(({
                            let c233: Fn13 = v0.clone();
                            c233.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m234 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f67(v0: List<Adt4>, v1: Adt1) -> List<Adt4> {
    let a238 = v0;
    let a239 = List::cons(match v1 {
        Adt1::C0(_, _, h237) => {
            h237
        }
    }, List::<Adt4>::nil());
    append_list(a238, a239)
}

pub fn f68(v0: List<Str>, v1: List<(List<Str>, Str)>) -> Option<Str> {
    f87(Fn17::F88(v0), None::<Str>, v1)
}

pub fn f69(v0: List<Str>, v1: List<(Bytes, Bytes)>) -> R<Str> {
    Ok(f45(f89(f90(v0, v1)?)?))
}

pub fn f70(v0: List<Str>, v1: List<Adt1>) -> List<Adt1> {
    f91(Fn18::F92(v1), List::<Adt1>::nil(), v0)
}

pub fn f71(v0: Adt2, v1: Str, v2: List<Adt1>) -> R<Str> {
    Ok(f45(f89(f93(v0, v1, v2)?)?))
}

pub fn f72(v0: List<Str>, v1: List<Adt1>) -> bool {
    f94(Fn19::F95(v1), true, v0)
}

pub fn f73(v0: Adt3, v1: Str, v2: List<(Str, bool)>, v3: List<Adt4>) -> Adt4 {
    if f40(v3) {
        let m240 = f96(v1, v2);
        match m240 {
            None => {
                Adt4::C1
            }
            Some(v4) => {
                if v4 {
                    Adt4::C0
                } else {
                    let m241 = v0;
                    match m241 {
                        Adt3::C0 => {
                            Adt4::C2
                        }
                        Adt3::C1 => {
                            Adt4::C1
                        }
                    }
                }
            }
        }
    } else {
        Adt4::C3
    }
}

pub fn f74(v0: Bytes, v1: Bytes) -> R<Option<u64>> {
    let v2: u64 = {
        let a242 = v0.clone();
        length_bytes(a242)
    };
    let v3: u64 = {
        let a243 = v1.clone();
        length_bytes(a243)
    };
    let v4: u64 = {
        let (h244, _) = f97(Fn20::F98(v1.clone(), v3, v0.clone(), v2), {
            let a245 = v2;
            let a246 = 1u64;
            nat_add(a245, a246)?
        }, 0u64)?;
        h244
    };
    if f99(v0, v1, v4) {
        Ok(Some(v4))
    } else {
        Ok(None::<u64>)
    }
}

pub fn f75(v0: Bytes, v1: u64) -> Option<u8> {
    let a247 = v0;
    let a248 = v1;
    index_bytes(a247, a248)
}

pub fn f76(v0: Bytes) -> Bytes {
    if f100(v0.clone(), Bytes::lit(&[0x0d])) {
        f26(v0.clone(), 0u64, {
            let a250 = {
                let a249 = v0;
                length_bytes(a249)
            };
            let a251 = 1u64;
            nat_sub(a250, a251)
        })
    } else {
        v0
    }
}

pub fn f77(v0: List<Bytes>) -> Option<List<Bytes>> {
    let m252 = v0.clone();
    match m252.uncons() {
        None => {
            None::<List<Bytes>>
        }
        Some((_, v2)) => {
            let m253 = v2;
            match m253.uncons() {
                None => {
                    None::<List<Bytes>>
                }
                Some((_, _)) => {
                    let v5: (Option<Bytes>, List<Bytes>) = f101(Fn21::F102, (None::<Bytes>, List::<Bytes>::nil()), v0);
                    let m255 = {
                        let (h254, _) = v5;
                        h254
                    };
                    match m255 {
                        None => {
                            Some(f25({
                                let (_, h257) = v5;
                                h257
                            }))
                        }
                        Some(v6) => {
                            Some(f25(List::cons(v6, {
                                let (_, h256) = v5;
                                h256
                            })))
                        }
                    }
                }
            }
        }
    }
}

pub fn f78(v0: Fn14, p261: List<Bytes>, p262: List<(Bytes, Bytes)>) -> R<List<Bytes>> {
    let m260 = (p261, p262);
    {
        let mut m260 = m260;
        loop {
            let step = {
                let (v1, v2) = m260;
                let m258 = v2;
                match m258.uncons() {
                    None => {
                        Err::<(List<Bytes>, List<(Bytes, Bytes)>), R<List<Bytes>>>(Ok(v1))
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<Bytes>, List<(Bytes, Bytes)>), R<List<Bytes>>>(({
                            let c259: Fn14 = v0.clone();
                            c259.apply(v1, v3)?
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m260 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f79(v0: List<Str>, v1: Str, v2: List<Bytes>, v3: (Bytes, Bytes)) -> R<List<Bytes>> {
    if f103(v0, {
        let (_, h263) = v3.clone();
        h263
    })? {
        Ok(List::cons(f35(List::cons(f44(Str::lit("file")), List::cons(f44(v1), List::cons({
            let (_, h264) = v3;
            h264
        }, List::<Bytes>::nil())))), v2))
    } else {
        Ok(v2)
    }
}

pub fn f80(v0: Str, v1: Str, v2: List<Bytes>, v3: Str) -> List<Bytes> {
    List::cons(f16(List::cons(v1, List::cons(v0, List::cons(v3, List::<Str>::nil())))), v2)
}

pub fn f81(v0: Str) -> Adt5 {
    let a265 = v0.clone();
    let a266 = Str::lit("1");
    if equal(a265, a266) {
        Adt5::C0
    } else {
        let a267 = v0.clone();
        let a268 = Str::lit("2");
        if equal(a267, a268) {
            Adt5::C1
        } else {
            let a269 = v0.clone();
            let a270 = Str::lit("3");
            if equal(a269, a270) {
                Adt5::C2
            } else {
                let a271 = v0.clone();
                let a272 = Str::lit("4");
                if equal(a271, a272) {
                    Adt5::C3
                } else {
                    let a273 = v0.clone();
                    let a274 = Str::lit("5");
                    if equal(a273, a274) {
                        Adt5::C4
                    } else {
                        let a275 = v0.clone();
                        let a276 = Str::lit("6");
                        if equal(a275, a276) {
                            Adt5::C5
                        } else {
                            let a277 = v0.clone();
                            let a278 = Str::lit("7");
                            if equal(a277, a278) {
                                Adt5::C6
                            } else {
                                let a279 = v0.clone();
                                let a280 = Str::lit("8");
                                if equal(a279, a280) {
                                    Adt5::C7
                                } else {
                                    let a281 = v0.clone();
                                    let a282 = Str::lit("9");
                                    if equal(a281, a282) {
                                        Adt5::C8
                                    } else {
                                        let a283 = v0.clone();
                                        let a284 = Str::lit("10");
                                        if equal(a283, a284) {
                                            Adt5::C9
                                        } else {
                                            let a285 = v0.clone();
                                            let a286 = Str::lit("11");
                                            if equal(a285, a286) {
                                                Adt5::C10
                                            } else {
                                                let a287 = v0.clone();
                                                let a288 = Str::lit("12");
                                                if equal(a287, a288) {
                                                    Adt5::C11
                                                } else {
                                                    let a289 = v0.clone();
                                                    let a290 = Str::lit("13");
                                                    if equal(a289, a290) {
                                                        Adt5::C12
                                                    } else {
                                                        let a291 = v0.clone();
                                                        let a292 = Str::lit("14");
                                                        if equal(a291, a292) {
                                                            Adt5::C13
                                                        } else {
                                                            let a293 = v0.clone();
                                                            let a294 = Str::lit("15");
                                                            if equal(a293, a294) {
                                                                Adt5::C14
                                                            } else {
                                                                let a295 = v0;
                                                                let a296 = Str::lit("16");
                                                                if equal(a295, a296) {
                                                                    Adt5::C15
                                                                } else {
                                                                    Adt5::C3
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn f82(v0: Fn15, p300: Str, p301: List<(Str, Str)>) -> Str {
    let m299 = (p300, p301);
    {
        let mut m299 = m299;
        loop {
            let step = {
                let (v1, v2) = m299;
                let m297 = v2;
                match m297.uncons() {
                    None => {
                        Err::<(Str, List<(Str, Str)>), Str>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Str, List<(Str, Str)>), Str>(({
                            let c298: Fn15 = v0.clone();
                            c298.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m299 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f83(v0: Str, v1: Str, v2: (Str, Str)) -> Str {
    let a303 = {
        let (h302, _) = v2;
        h302
    };
    let a304 = v0;
    if equal(a303, a304) {
        let (_, h305) = v2;
        h305
    } else {
        v1
    }
}

pub fn f84(v0: Fn16, p309: Option<Bytes>, p310: List<Bytes>) -> Option<Bytes> {
    let m308 = (p309, p310);
    {
        let mut m308 = m308;
        loop {
            let step = {
                let (v1, v2) = m308;
                let m306 = v2;
                match m306.uncons() {
                    None => {
                        Err::<(Option<Bytes>, List<Bytes>), Option<Bytes>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Option<Bytes>, List<Bytes>), Option<Bytes>>(({
                            let c307: Fn16 = v0.clone();
                            c307.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m308 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f85(v0: Bytes, v1: Option<Bytes>, v2: Bytes) -> Option<Bytes> {
    let m311 = v1;
    match m311 {
        None => {
            Some(v2)
        }
        Some(v3) => {
            Some({
                let a314 = {
                    let a312 = v3;
                    let a313 = v0;
                    append_bytes(a312, a313)
                };
                let a315 = v2;
                append_bytes(a314, a315)
            })
        }
    }
}

pub fn f86(v0: Str, v1: List<Str>) -> bool {
    f94(Fn19::F104(v0), false, v1)
}

pub fn f87(v0: Fn17, p319: Option<Str>, p320: List<(List<Str>, Str)>) -> Option<Str> {
    let m318 = (p319, p320);
    {
        let mut m318 = m318;
        loop {
            let step = {
                let (v1, v2) = m318;
                let m316 = v2;
                match m316.uncons() {
                    None => {
                        Err::<(Option<Str>, List<(List<Str>, Str)>), Option<Str>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Option<Str>, List<(List<Str>, Str)>), Option<Str>>(({
                            let c317: Fn17 = v0.clone();
                            c317.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m318 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f88(v0: List<Str>, v1: Option<Str>, v2: (List<Str>, Str)) -> Option<Str> {
    if f105({
        let (h321, _) = v2;
        h321
    }, v0) {
        Some({
            let (_, h322) = v2;
            h322
        })
    } else {
        v1
    }
}

pub fn f89(v0: Bytes) -> R<Bytes> {
    Ok(f106(f107(v0)?))
}

pub fn f90(v0: List<Str>, v1: List<(Bytes, Bytes)>) -> R<Bytes> {
    Ok(f10(f78(Fn14::F108(v0), List::<Bytes>::nil(), v1)?))
}

pub fn f91(v0: Fn18, p326: List<Adt1>, p327: List<Str>) -> List<Adt1> {
    let m325 = (p326, p327);
    {
        let mut m325 = m325;
        loop {
            let step = {
                let (v1, v2) = m325;
                let m323 = v2;
                match m323.uncons() {
                    None => {
                        Err::<(List<Adt1>, List<Str>), List<Adt1>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<Adt1>, List<Str>), List<Adt1>>(({
                            let c324: Fn18 = v0.clone();
                            c324.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m325 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f92(v0: List<Adt1>, v1: List<Adt1>, v2: Str) -> List<Adt1> {
    let m328 = f109(v2, v0);
    match m328 {
        None => {
            v1
        }
        Some(v3) => {
            let a329 = v1;
            let a330 = List::cons(v3, List::<Adt1>::nil());
            append_list(a329, a330)
        }
    }
}

pub fn f93(v0: Adt2, v1: Str, v2: List<Adt1>) -> R<Bytes> {
    Ok(f31(List::cons(f35(List::cons(f44(Str::lit("prism-factory/obligation/1")), List::<Bytes>::nil())), List::cons(f35(List::cons(f44(Str::lit("name")), List::cons(f44(match v0.clone() {
        Adt2::C0(h331, _, _, _, _, _) => {
            h331
        }
    }), List::<Bytes>::nil()))), List::cons(f35(List::cons(f44(Str::lit("kind")), List::cons(f44(f53(match v0.clone() {
        Adt2::C0(_, h332, _, _, _, _) => {
            h332
        }
    })), List::<Bytes>::nil()))), List::cons(f35(List::cons(f44(Str::lit("environment")), List::cons(f44(match v0.clone() {
        Adt2::C0(_, _, h333, _, _, _) => {
            h333
        }
    }), List::<Bytes>::nil()))), List::cons(f35(List::cons(f44(Str::lit("run")), List::cons(f44(match v0.clone() {
        Adt2::C0(_, _, _, h334, _, _) => {
            h334
        }
    }), List::<Bytes>::nil()))), List::cons(f110(Str::lit("input"), match v0 {
        Adt2::C0(_, _, _, _, h335, _) => {
            h335
        }
    }), List::cons(f111(v2)?, List::cons(f35(List::cons(f44(Str::lit("closure")), List::cons(f44(v1), List::<Bytes>::nil()))), List::<Bytes>::nil()))))))))))
}

pub fn f94(v0: Fn19, p339: bool, p340: List<Str>) -> bool {
    let m338 = (p339, p340);
    {
        let mut m338 = m338;
        loop {
            let step = {
                let (v1, v2) = m338;
                let m336 = v2;
                match m336.uncons() {
                    None => {
                        Err::<(bool, List<Str>), bool>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(bool, List<Str>), bool>(({
                            let c337: Fn19 = v0.clone();
                            c337.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m338 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f95(v0: List<Adt1>, v1: bool, v2: Str) -> bool {
    let m341 = f109(v2, v0);
    match m341 {
        None => {
            false
        }
        Some(_) => {
            v1
        }
    }
}

pub fn f96(v0: Str, v1: List<(Str, bool)>) -> Option<bool> {
    f112(Fn22::F113(v0), None::<bool>, v1)
}

pub fn f97(v0: Fn20, p346: u64, p347: u64) -> R<(u64, bool)> {
    let m345 = (p346, p347);
    {
        let mut m345 = m345;
        loop {
            let step = {
                let (v1, v2) = m345;
                let m342 = v1;
                if m342 == 0 {
                    Err::<(u64, u64), R<(u64, bool)>>(Ok((v2, false)))
                } else {
                    let v3 = m342 - 1;
                    let m344 = {
                        let c343: Fn20 = v0.clone();
                        c343.apply(v2)?
                    };
                    match m344 {
                        None => {
                            Err::<(u64, u64), R<(u64, bool)>>(Ok((v2, true)))
                        }
                        Some(v4) => {
                            Ok::<(u64, u64), R<(u64, bool)>>((v3, v4))
                        }
                    }
                }
            };
            match step {
                Ok(next) => { m345 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f98(v0: Bytes, v1: u64, v2: Bytes, v3: u64, v4: u64) -> R<Option<u64>> {
    if f99(v2, v0, v4) {
        Ok(None::<u64>)
    } else {
        let a350 = {
            let a348 = v4;
            let a349 = v1;
            nat_add(a348, a349)?
        };
        let a351 = v3;
        if nat_lt(a350, a351) {
            Ok(Some({
                let a352 = v4;
                let a353 = 1u64;
                nat_add(a352, a353)?
            }))
        } else {
            Ok(None::<u64>)
        }
    }
}

pub fn f99(v0: Bytes, v1: Bytes, v2: u64) -> bool {
    let m358 = {
        let a355 = v0;
        let a356 = v2;
        let a357 = {
            let a354 = v1.clone();
            length_bytes(a354)
        };
        slice_bytes(a355, a356, a357)
    };
    match m358 {
        None => {
            false
        }
        Some(v3) => {
            let a359 = v3;
            let a360 = v1;
            equal(a359, a360)
        }
    }
}

pub fn f100(v0: Bytes, v1: Bytes) -> bool {
    let v2: u64 = {
        let a361 = v0.clone();
        length_bytes(a361)
    };
    let v3: u64 = {
        let a362 = v1.clone();
        length_bytes(a362)
    };
    let a363 = v3;
    let a364 = v2;
    if nat_le(a363, a364) {
        let m370 = {
            let a367 = v0;
            let a368 = {
                let a365 = v2;
                let a366 = v3;
                nat_sub(a365, a366)
            };
            let a369 = v3;
            slice_bytes(a367, a368, a369)
        };
        match m370 {
            None => {
                false
            }
            Some(v4) => {
                let a371 = v4;
                let a372 = v1;
                equal(a371, a372)
            }
        }
    } else {
        false
    }
}

pub fn f101(v0: Fn21, p376: (Option<Bytes>, List<Bytes>), p377: List<Bytes>) -> (Option<Bytes>, List<Bytes>) {
    let m375 = (p376, p377);
    {
        let mut m375 = m375;
        loop {
            let step = {
                let (v1, v2) = m375;
                let m373 = v2;
                match m373.uncons() {
                    None => {
                        Err::<((Option<Bytes>, List<Bytes>), List<Bytes>), (Option<Bytes>, List<Bytes>)>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<((Option<Bytes>, List<Bytes>), List<Bytes>), (Option<Bytes>, List<Bytes>)>(({
                            let c374: Fn21 = v0.clone();
                            c374.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m375 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f102(v0: (Option<Bytes>, List<Bytes>), v1: Bytes) -> (Option<Bytes>, List<Bytes>) {
    f114(v0, v1)
}

pub fn f103(v0: List<Str>, v1: Bytes) -> R<bool> {
    let a378 = f115(v0.clone(), v1.clone(), true)?;
    let a379 = f115(v0, v1, false)?;
    Ok(nat_lt(a378, a379))
}

pub fn f104(v0: Str, v1: bool, v2: Str) -> bool {
    if v1 {
        true
    } else {
        let a380 = v2;
        let a381 = v0;
        equal(a380, a381)
    }
}

pub fn f105(p388: List<Str>, p389: List<Str>) -> bool {
    let m387 = (p388, p389);
    {
        let mut m387 = m387;
        loop {
            let step = {
                let (v0, v1) = m387;
                let m382 = v0;
                match m382.uncons() {
                    None => {
                        let m383 = v1;
                        match m383.uncons() {
                            None => {
                                Err::<(List<Str>, List<Str>), bool>(true)
                            }
                            Some((_, _)) => {
                                Err::<(List<Str>, List<Str>), bool>(false)
                            }
                        }
                    }
                    Some((v4, v5)) => {
                        let m384 = v1;
                        match m384.uncons() {
                            None => {
                                Err::<(List<Str>, List<Str>), bool>(false)
                            }
                            Some((v6, v7)) => {
                                let a385 = v4;
                                let a386 = v6;
                                if equal(a385, a386) {
                                    Ok::<(List<Str>, List<Str>), bool>((v5, v7))
                                } else {
                                    Err::<(List<Str>, List<Str>), bool>(false)
                                }
                            }
                        }
                    }
                }
            };
            match step {
                Ok(next) => { m387 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f106(v0: Adt6) -> Bytes {
    let a410 = f116(match v0.clone() {
        Adt6::C0(h390, _, _, _, _, _, _, _) => {
            h390
        }
    });
    let a411 = {
        let a408 = f116(match v0.clone() {
            Adt6::C0(_, h391, _, _, _, _, _, _) => {
                h391
            }
        });
        let a409 = {
            let a406 = f116(match v0.clone() {
                Adt6::C0(_, _, h392, _, _, _, _, _) => {
                    h392
                }
            });
            let a407 = {
                let a404 = f116(match v0.clone() {
                    Adt6::C0(_, _, _, h393, _, _, _, _) => {
                        h393
                    }
                });
                let a405 = {
                    let a402 = f116(match v0.clone() {
                        Adt6::C0(_, _, _, _, h394, _, _, _) => {
                            h394
                        }
                    });
                    let a403 = {
                        let a400 = f116(match v0.clone() {
                            Adt6::C0(_, _, _, _, _, h395, _, _) => {
                                h395
                            }
                        });
                        let a401 = {
                            let a398 = f116(match v0.clone() {
                                Adt6::C0(_, _, _, _, _, _, h396, _) => {
                                    h396
                                }
                            });
                            let a399 = f116(match v0 {
                                Adt6::C0(_, _, _, _, _, _, _, h397) => {
                                    h397
                                }
                            });
                            append_bytes(a398, a399)
                        };
                        append_bytes(a400, a401)
                    };
                    append_bytes(a402, a403)
                };
                append_bytes(a404, a405)
            };
            append_bytes(a406, a407)
        };
        append_bytes(a408, a409)
    };
    append_bytes(a410, a411)
}

pub fn f107(v0: Bytes) -> R<Adt6> {
    let v1: u64 = {
        let a412 = v0.clone();
        length_bytes(a412)
    };
    let v2: u64 = {
        let a418 = {
            let a415 = {
                let a413 = v1;
                let a414 = 72u64;
                nat_add(a413, a414)?
            };
            let a416 = 64u64;
            let a417 = 0u64;
            nat_quot(a415, a416, a417)
        };
        let a419 = 64u64;
        nat_mul(a418, a419)?
    };
    let (_, h420) = f117(Fn23::F118(v0, v1, v2), {
        let a421 = v2;
        let a422 = 64u64;
        let a423 = 0u64;
        nat_quot(a421, a422, a423)
    }, (0u64, f119()))?;
    Ok(h420)
}

pub fn f108(v0: List<Str>, v1: List<Bytes>, v2: (Bytes, Bytes)) -> R<List<Bytes>> {
    if f103(v0, {
        let (_, h424) = v2.clone();
        h424
    })? {
        Ok(List::cons(f35(List::cons(f44(Str::lit("file")), List::cons({
            let (h425, _) = v2;
            h425
        }, List::cons({
            let (_, h426) = v2;
            h426
        }, List::<Bytes>::nil())))), v1))
    } else {
        Ok(v1)
    }
}

pub fn f109(v0: Str, v1: List<Adt1>) -> Option<Adt1> {
    f120(Fn24::F121(v0), None::<Adt1>, v1)
}

pub fn f110(v0: Str, v1: List<Str>) -> Bytes {
    f10(f36(Fn8::F122(v0), List::<Bytes>::nil(), v1))
}

pub fn f111(v0: List<Adt1>) -> R<Bytes> {
    Ok(f10(f11(Fn1::F123, List::<Bytes>::nil(), v0)?))
}

pub fn f112(v0: Fn22, p430: Option<bool>, p431: List<(Str, bool)>) -> Option<bool> {
    let m429 = (p430, p431);
    {
        let mut m429 = m429;
        loop {
            let step = {
                let (v1, v2) = m429;
                let m427 = v2;
                match m427.uncons() {
                    None => {
                        Err::<(Option<bool>, List<(Str, bool)>), Option<bool>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Option<bool>, List<(Str, bool)>), Option<bool>>(({
                            let c428: Fn22 = v0.clone();
                            c428.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m429 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f113(v0: Str, v1: Option<bool>, v2: (Str, bool)) -> Option<bool> {
    let a433 = {
        let (h432, _) = v2;
        h432
    };
    let a434 = v0;
    if equal(a433, a434) {
        let m435 = v1;
        match m435 {
            None => {
                Some({
                    let (_, h437) = v2;
                    h437
                })
            }
            Some(v3) => {
                Some(if v3 {
                    true
                } else {
                    let (_, h436) = v2;
                    h436
                })
            }
        }
    } else {
        v1
    }
}

pub fn f114(v0: (Option<Bytes>, List<Bytes>), v1: Bytes) -> (Option<Bytes>, List<Bytes>) {
    let m439 = {
        let (h438, _) = v0;
        h438
    };
    match m439 {
        None => {
            (Some(v1), {
                let (_, h440) = v0;
                h440
            })
        }
        Some(v2) => {
            (None::<Bytes>, List::cons({
                let a441 = v2;
                let a442 = v1;
                append_bytes(a441, a442)
            }, {
                let (_, h443) = v0;
                h443
            }))
        }
    }
}

pub fn f115(v0: List<Str>, v1: Bytes, v2: bool) -> R<u64> {
    f124(Fn25::F125(v2, v1), 0u64, v0)
}

pub fn f116(v0: u32) -> Bytes {
    let a472 = f126({
        let a444 = f127(v0, 28u32);
        let a445 = 15u32;
        fixed_u32::bit_and(a444, a445)
    });
    let a473 = {
        let a470 = f126({
            let a446 = f127(v0, 24u32);
            let a447 = 15u32;
            fixed_u32::bit_and(a446, a447)
        });
        let a471 = {
            let a468 = f126({
                let a448 = f127(v0, 20u32);
                let a449 = 15u32;
                fixed_u32::bit_and(a448, a449)
            });
            let a469 = {
                let a466 = f126({
                    let a450 = f127(v0, 16u32);
                    let a451 = 15u32;
                    fixed_u32::bit_and(a450, a451)
                });
                let a467 = {
                    let a464 = f126({
                        let a452 = f127(v0, 12u32);
                        let a453 = 15u32;
                        fixed_u32::bit_and(a452, a453)
                    });
                    let a465 = {
                        let a462 = f126({
                            let a454 = f127(v0, 8u32);
                            let a455 = 15u32;
                            fixed_u32::bit_and(a454, a455)
                        });
                        let a463 = {
                            let a460 = f126({
                                let a456 = f127(v0, 4u32);
                                let a457 = 15u32;
                                fixed_u32::bit_and(a456, a457)
                            });
                            let a461 = f126({
                                let a458 = v0;
                                let a459 = 15u32;
                                fixed_u32::bit_and(a458, a459)
                            });
                            append_bytes(a460, a461)
                        };
                        append_bytes(a462, a463)
                    };
                    append_bytes(a464, a465)
                };
                append_bytes(a466, a467)
            };
            append_bytes(a468, a469)
        };
        append_bytes(a470, a471)
    };
    append_bytes(a472, a473)
}

pub fn f117(v0: Fn23, p477: u64, p478: (u64, Adt6)) -> R<(u64, Adt6)> {
    let m476 = (p477, p478);
    {
        let mut m476 = m476;
        loop {
            let step = {
                let (v1, v2) = m476;
                let m474 = v1;
                if m474 == 0 {
                    Err::<(u64, (u64, Adt6)), R<(u64, Adt6)>>(Ok(v2))
                } else {
                    let v3 = m474 - 1;
                    Ok::<(u64, (u64, Adt6)), R<(u64, Adt6)>>((v3, {
                        let c475: Fn23 = v0.clone();
                        c475.apply(v2)?
                    }))
                }
            };
            match step {
                Ok(next) => { m476 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f118(v0: Bytes, v1: u64, v2: u64, v3: (u64, Adt6)) -> R<(u64, Adt6)> {
    Ok(({
        let a480 = {
            let (h479, _) = v3.clone();
            h479
        };
        let a481 = 64u64;
        nat_add(a480, a481)?
    }, f128({
        let (_, h482) = v3;
        h482
    }, f129(v0, v1, v2, {
        let (h483, _) = v3;
        h483
    })?)?))
}

pub fn f119() -> Adt6 {
    Adt6::C0(1779033703u32, 3144134277u32, 1013904242u32, 2773480762u32, 1359893119u32, 2600822924u32, 528734635u32, 1541459225u32)
}

pub fn f120(v0: Fn24, p487: Option<Adt1>, p488: List<Adt1>) -> Option<Adt1> {
    let m486 = (p487, p488);
    {
        let mut m486 = m486;
        loop {
            let step = {
                let (v1, v2) = m486;
                let m484 = v2;
                match m484.uncons() {
                    None => {
                        Err::<(Option<Adt1>, List<Adt1>), Option<Adt1>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Option<Adt1>, List<Adt1>), Option<Adt1>>(({
                            let c485: Fn24 = v0.clone();
                            c485.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m486 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f121(v0: Str, v1: Option<Adt1>, v2: Adt1) -> Option<Adt1> {
    let a492 = {
        let m491 = match v2.clone() {
            Adt1::C0(h490, _, _) => {
                h490
            }
        };
        match m491 {
            Adt2::C0(h489, _, _, _, _, _) => {
                h489
            }
        }
    };
    let a493 = v0;
    if equal(a492, a493) {
        Some(v2)
    } else {
        v1
    }
}

pub fn f122(v0: Str, v1: List<Bytes>, v2: Str) -> List<Bytes> {
    List::cons(f35(List::cons(f44(v0), List::cons(f44(v2), List::<Bytes>::nil()))), v1)
}

pub fn f123(v0: List<Bytes>, v1: Adt1) -> List<Bytes> {
    List::cons(f35(List::cons(f44(Str::lit("needs")), List::cons(f44({
        let m496 = match v1.clone() {
            Adt1::C0(h495, _, _) => {
                h495
            }
        };
        match m496 {
            Adt2::C0(h494, _, _, _, _, _) => {
                h494
            }
        }
    }), List::cons(f44(match v1 {
        Adt1::C0(_, h497, _) => {
            h497
        }
    }), List::<Bytes>::nil())))), v0)
}

pub fn f124(v0: Fn25, p501: u64, p502: List<Str>) -> R<u64> {
    let m500 = (p501, p502);
    {
        let mut m500 = m500;
        loop {
            let step = {
                let (v1, v2) = m500;
                let m498 = v2;
                match m498.uncons() {
                    None => {
                        Err::<(u64, List<Str>), R<u64>>(Ok(v1))
                    }
                    Some((v3, v4)) => {
                        Ok::<(u64, List<Str>), R<u64>>(({
                            let c499: Fn25 = v0.clone();
                            c499.apply(v1, v3)?
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m500 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f125(v0: bool, v1: Bytes, v2: u64, v3: Str) -> R<u64> {
    let a503 = f130(v3.clone(), Bytes::lit(&[0x21]));
    let a504 = v0;
    if equal(a503, a504) {
        let v4: u64 = f131(v3, v1)?;
        let a505 = v2;
        let a506 = v4;
        if nat_lt(a505, a506) {
            Ok(v4)
        } else {
            Ok(v2)
        }
    } else {
        Ok(v2)
    }
}

pub fn f126(v0: u32) -> Bytes {
    let a509 = {
        let a507 = v0;
        let a508 = 8u32;
        fixed_u32::bit_and(a507, a508)
    };
    let a510 = 0u32;
    if equal(a509, a510) {
        let a513 = {
            let a511 = v0;
            let a512 = 4u32;
            fixed_u32::bit_and(a511, a512)
        };
        let a514 = 0u32;
        if equal(a513, a514) {
            let a517 = {
                let a515 = v0;
                let a516 = 2u32;
                fixed_u32::bit_and(a515, a516)
            };
            let a518 = 0u32;
            if equal(a517, a518) {
                let a521 = {
                    let a519 = v0;
                    let a520 = 1u32;
                    fixed_u32::bit_and(a519, a520)
                };
                let a522 = 0u32;
                if equal(a521, a522) {
                    Bytes::lit(&[0x30])
                } else {
                    Bytes::lit(&[0x31])
                }
            } else {
                let a525 = {
                    let a523 = v0;
                    let a524 = 1u32;
                    fixed_u32::bit_and(a523, a524)
                };
                let a526 = 0u32;
                if equal(a525, a526) {
                    Bytes::lit(&[0x32])
                } else {
                    Bytes::lit(&[0x33])
                }
            }
        } else {
            let a529 = {
                let a527 = v0;
                let a528 = 2u32;
                fixed_u32::bit_and(a527, a528)
            };
            let a530 = 0u32;
            if equal(a529, a530) {
                let a533 = {
                    let a531 = v0;
                    let a532 = 1u32;
                    fixed_u32::bit_and(a531, a532)
                };
                let a534 = 0u32;
                if equal(a533, a534) {
                    Bytes::lit(&[0x34])
                } else {
                    Bytes::lit(&[0x35])
                }
            } else {
                let a537 = {
                    let a535 = v0;
                    let a536 = 1u32;
                    fixed_u32::bit_and(a535, a536)
                };
                let a538 = 0u32;
                if equal(a537, a538) {
                    Bytes::lit(&[0x36])
                } else {
                    Bytes::lit(&[0x37])
                }
            }
        }
    } else {
        let a541 = {
            let a539 = v0;
            let a540 = 4u32;
            fixed_u32::bit_and(a539, a540)
        };
        let a542 = 0u32;
        if equal(a541, a542) {
            let a545 = {
                let a543 = v0;
                let a544 = 2u32;
                fixed_u32::bit_and(a543, a544)
            };
            let a546 = 0u32;
            if equal(a545, a546) {
                let a549 = {
                    let a547 = v0;
                    let a548 = 1u32;
                    fixed_u32::bit_and(a547, a548)
                };
                let a550 = 0u32;
                if equal(a549, a550) {
                    Bytes::lit(&[0x38])
                } else {
                    Bytes::lit(&[0x39])
                }
            } else {
                let a553 = {
                    let a551 = v0;
                    let a552 = 1u32;
                    fixed_u32::bit_and(a551, a552)
                };
                let a554 = 0u32;
                if equal(a553, a554) {
                    Bytes::lit(&[0x61])
                } else {
                    Bytes::lit(&[0x62])
                }
            }
        } else {
            let a557 = {
                let a555 = v0;
                let a556 = 2u32;
                fixed_u32::bit_and(a555, a556)
            };
            let a558 = 0u32;
            if equal(a557, a558) {
                let a561 = {
                    let a559 = v0;
                    let a560 = 1u32;
                    fixed_u32::bit_and(a559, a560)
                };
                let a562 = 0u32;
                if equal(a561, a562) {
                    Bytes::lit(&[0x63])
                } else {
                    Bytes::lit(&[0x64])
                }
            } else {
                let a565 = {
                    let a563 = v0;
                    let a564 = 1u32;
                    fixed_u32::bit_and(a563, a564)
                };
                let a566 = 0u32;
                if equal(a565, a566) {
                    Bytes::lit(&[0x65])
                } else {
                    Bytes::lit(&[0x66])
                }
            }
        }
    }
}

pub fn f127(v0: u32, v1: u32) -> u32 {
    let m569 = {
        let a567 = v0;
        let a568 = v1;
        fixed_u32::shift_right(a567, a568)
    };
    match m569 {
        None => {
            0u32
        }
        Some(v2) => {
            v2
        }
    }
}

pub fn f128(v0: Adt6, v1: Adt7) -> R<Adt6> {
    let v2: Adt8 = f132(Fn26::F133, 64u64, Adt8::C0(match v0.clone() {
        Adt6::C0(h570, _, _, _, _, _, _, _) => {
            h570
        }
    }, match v0.clone() {
        Adt6::C0(_, h571, _, _, _, _, _, _) => {
            h571
        }
    }, match v0.clone() {
        Adt6::C0(_, _, h572, _, _, _, _, _) => {
            h572
        }
    }, match v0.clone() {
        Adt6::C0(_, _, _, h573, _, _, _, _) => {
            h573
        }
    }, match v0.clone() {
        Adt6::C0(_, _, _, _, h574, _, _, _) => {
            h574
        }
    }, match v0.clone() {
        Adt6::C0(_, _, _, _, _, h575, _, _) => {
            h575
        }
    }, match v0.clone() {
        Adt6::C0(_, _, _, _, _, _, h576, _) => {
            h576
        }
    }, match v0.clone() {
        Adt6::C0(_, _, _, _, _, _, _, h577) => {
            h577
        }
    }, v1, 0u64))?;
    Ok(Adt6::C0(f134(match v0.clone() {
        Adt6::C0(h578, _, _, _, _, _, _, _) => {
            h578
        }
    }, match v2.clone() {
        Adt8::C0(h579, _, _, _, _, _, _, _, _, _) => {
            h579
        }
    }), f134(match v0.clone() {
        Adt6::C0(_, h580, _, _, _, _, _, _) => {
            h580
        }
    }, match v2.clone() {
        Adt8::C0(_, h581, _, _, _, _, _, _, _, _) => {
            h581
        }
    }), f134(match v0.clone() {
        Adt6::C0(_, _, h582, _, _, _, _, _) => {
            h582
        }
    }, match v2.clone() {
        Adt8::C0(_, _, h583, _, _, _, _, _, _, _) => {
            h583
        }
    }), f134(match v0.clone() {
        Adt6::C0(_, _, _, h584, _, _, _, _) => {
            h584
        }
    }, match v2.clone() {
        Adt8::C0(_, _, _, h585, _, _, _, _, _, _) => {
            h585
        }
    }), f134(match v0.clone() {
        Adt6::C0(_, _, _, _, h586, _, _, _) => {
            h586
        }
    }, match v2.clone() {
        Adt8::C0(_, _, _, _, h587, _, _, _, _, _) => {
            h587
        }
    }), f134(match v0.clone() {
        Adt6::C0(_, _, _, _, _, h588, _, _) => {
            h588
        }
    }, match v2.clone() {
        Adt8::C0(_, _, _, _, _, h589, _, _, _, _) => {
            h589
        }
    }), f134(match v0.clone() {
        Adt6::C0(_, _, _, _, _, _, h590, _) => {
            h590
        }
    }, match v2.clone() {
        Adt8::C0(_, _, _, _, _, _, h591, _, _, _) => {
            h591
        }
    }), f134(match v0 {
        Adt6::C0(_, _, _, _, _, _, _, h592) => {
            h592
        }
    }, match v2 {
        Adt8::C0(_, _, _, _, _, _, _, h593, _, _) => {
            h593
        }
    })))
}

pub fn f129(v0: Bytes, v1: u64, v2: u64, v3: u64) -> R<Adt7> {
    Ok(Adt7::C0(f135(v0.clone(), v1, v2, v3)?, f135(v0.clone(), v1, v2, {
        let a594 = v3;
        let a595 = 4u64;
        nat_add(a594, a595)?
    })?, f135(v0.clone(), v1, v2, {
        let a596 = v3;
        let a597 = 8u64;
        nat_add(a596, a597)?
    })?, f135(v0.clone(), v1, v2, {
        let a598 = v3;
        let a599 = 12u64;
        nat_add(a598, a599)?
    })?, f135(v0.clone(), v1, v2, {
        let a600 = v3;
        let a601 = 16u64;
        nat_add(a600, a601)?
    })?, f135(v0.clone(), v1, v2, {
        let a602 = v3;
        let a603 = 20u64;
        nat_add(a602, a603)?
    })?, f135(v0.clone(), v1, v2, {
        let a604 = v3;
        let a605 = 24u64;
        nat_add(a604, a605)?
    })?, f135(v0.clone(), v1, v2, {
        let a606 = v3;
        let a607 = 28u64;
        nat_add(a606, a607)?
    })?, f135(v0.clone(), v1, v2, {
        let a608 = v3;
        let a609 = 32u64;
        nat_add(a608, a609)?
    })?, f135(v0.clone(), v1, v2, {
        let a610 = v3;
        let a611 = 36u64;
        nat_add(a610, a611)?
    })?, f135(v0.clone(), v1, v2, {
        let a612 = v3;
        let a613 = 40u64;
        nat_add(a612, a613)?
    })?, f135(v0.clone(), v1, v2, {
        let a614 = v3;
        let a615 = 44u64;
        nat_add(a614, a615)?
    })?, f135(v0.clone(), v1, v2, {
        let a616 = v3;
        let a617 = 48u64;
        nat_add(a616, a617)?
    })?, f135(v0.clone(), v1, v2, {
        let a618 = v3;
        let a619 = 52u64;
        nat_add(a618, a619)?
    })?, f135(v0.clone(), v1, v2, {
        let a620 = v3;
        let a621 = 56u64;
        nat_add(a620, a621)?
    })?, f135(v0, v1, v2, {
        let a622 = v3;
        let a623 = 60u64;
        nat_add(a622, a623)?
    })?))
}

pub fn f130(v0: Str, v1: Bytes) -> bool {
    f136(f44(v0), v1)
}

pub fn f131(v0: Str, v1: Bytes) -> R<u64> {
    let v2: Bytes = if f130(v0.clone(), Bytes::lit(&[0x21])) {
        f137(v0)
    } else {
        f44(v0)
    };
    if f136(v1.clone(), v2.clone()) {
        if f136(v1, Bytes::lit(&[0x40])) {
            if f136(v2.clone(), Bytes::lit(&[0x40])) {
                let a625 = {
                    let a624 = v2;
                    length_bytes(a624)
                };
                let a626 = 1u64;
                nat_add(a625, a626)
            } else {
                Ok(0u64)
            }
        } else {
            let a628 = {
                let a627 = v2;
                length_bytes(a627)
            };
            let a629 = 1u64;
            nat_add(a628, a629)
        }
    } else {
        Ok(0u64)
    }
}

pub fn f132(v0: Fn26, p633: u64, p634: Adt8) -> R<Adt8> {
    let m632 = (p633, p634);
    {
        let mut m632 = m632;
        loop {
            let step = {
                let (v1, v2) = m632;
                let m630 = v1;
                if m630 == 0 {
                    Err::<(u64, Adt8), R<Adt8>>(Ok(v2))
                } else {
                    let v3 = m630 - 1;
                    Ok::<(u64, Adt8), R<Adt8>>((v3, {
                        let c631: Fn26 = v0.clone();
                        c631.apply(v2)?
                    }))
                }
            };
            match step {
                Ok(next) => { m632 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f133(v0: Adt8) -> R<Adt8> {
    f138(v0)
}

pub fn f134(v0: u32, v1: u32) -> u32 {
    let m637 = {
        let a635 = f139(v0);
        let a636 = f139(v1);
        fixed_u64::checked_add(a635, a636)
    };
    match m637 {
        None => {
            0u32
        }
        Some(v2) => {
            f140(v2)
        }
    }
}

pub fn f135(v0: Bytes, v1: u64, v2: u64, v3: u64) -> R<u32> {
    let a640 = {
        let a638 = v3;
        let a639 = 8u64;
        nat_add(a638, a639)?
    };
    let a641 = v2;
    if nat_eq(a640, a641) {
        Ok(f141({
            let a644 = {
                let a642 = v1;
                let a643 = 8u64;
                nat_mul(a642, a643)?
            };
            let a645 = 4294967296u64;
            let a646 = 0u64;
            nat_quot(a644, a645, a646)
        }))
    } else {
        let a649 = {
            let a647 = v3;
            let a648 = 4u64;
            nat_add(a647, a648)?
        };
        let a650 = v2;
        if nat_eq(a649, a650) {
            Ok(f141({
                let a653 = {
                    let a651 = v1;
                    let a652 = 8u64;
                    nat_mul(a651, a652)?
                };
                let a654 = 4294967296u64;
                let a655 = 0u64;
                nat_rem(a653, a654, a655)
            }))
        } else {
            Ok(f142(f143(v0.clone(), v1, v3), f143(v0.clone(), v1, {
                let a656 = v3;
                let a657 = 1u64;
                nat_add(a656, a657)?
            }), f143(v0.clone(), v1, {
                let a658 = v3;
                let a659 = 2u64;
                nat_add(a658, a659)?
            }), f143(v0, v1, {
                let a660 = v3;
                let a661 = 3u64;
                nat_add(a660, a661)?
            })))
        }
    }
}

pub fn f136(v0: Bytes, v1: Bytes) -> bool {
    let m666 = {
        let a663 = v0;
        let a664 = 0u64;
        let a665 = {
            let a662 = v1.clone();
            length_bytes(a662)
        };
        slice_bytes(a663, a664, a665)
    };
    match m666 {
        None => {
            false
        }
        Some(v2) => {
            let a667 = v2;
            let a668 = v1;
            equal(a667, a668)
        }
    }
}

pub fn f137(v0: Str) -> Bytes {
    let v1: Bytes = f44(v0);
    f26(v1.clone(), 1u64, {
        let a670 = {
            let a669 = v1;
            length_bytes(a669)
        };
        let a671 = 1u64;
        nat_sub(a670, a671)
    })
}

pub fn f138(v0: Adt8) -> R<Adt8> {
    let v1: u32 = f144(match v0.clone() {
        Adt8::C0(_, _, _, _, _, _, _, _, _, h672) => {
            h672
        }
    });
    let v2: u32 = f134(match v0.clone() {
        Adt8::C0(_, _, _, _, _, _, _, h673, _, _) => {
            h673
        }
    }, f134(f145(match v0.clone() {
        Adt8::C0(_, _, _, _, h674, _, _, _, _, _) => {
            h674
        }
    }), f134(f146(match v0.clone() {
        Adt8::C0(_, _, _, _, h675, _, _, _, _, _) => {
            h675
        }
    }, match v0.clone() {
        Adt8::C0(_, _, _, _, _, h676, _, _, _, _) => {
            h676
        }
    }, match v0.clone() {
        Adt8::C0(_, _, _, _, _, _, h677, _, _, _) => {
            h677
        }
    }), f134(v1, {
        let m680 = match v0.clone() {
            Adt8::C0(_, _, _, _, _, _, _, _, h679, _) => {
                h679
            }
        };
        match m680 {
            Adt7::C0(h678, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => {
                h678
            }
        }
    }))));
    let v3: u32 = f134(f147(match v0.clone() {
        Adt8::C0(h681, _, _, _, _, _, _, _, _, _) => {
            h681
        }
    }), f148(match v0.clone() {
        Adt8::C0(h682, _, _, _, _, _, _, _, _, _) => {
            h682
        }
    }, match v0.clone() {
        Adt8::C0(_, h683, _, _, _, _, _, _, _, _) => {
            h683
        }
    }, match v0.clone() {
        Adt8::C0(_, _, h684, _, _, _, _, _, _, _) => {
            h684
        }
    }));
    Ok(Adt8::C0(f134(v2, v3), match v0.clone() {
        Adt8::C0(h685, _, _, _, _, _, _, _, _, _) => {
            h685
        }
    }, match v0.clone() {
        Adt8::C0(_, h686, _, _, _, _, _, _, _, _) => {
            h686
        }
    }, match v0.clone() {
        Adt8::C0(_, _, h687, _, _, _, _, _, _, _) => {
            h687
        }
    }, f134(match v0.clone() {
        Adt8::C0(_, _, _, h688, _, _, _, _, _, _) => {
            h688
        }
    }, v2), match v0.clone() {
        Adt8::C0(_, _, _, _, h689, _, _, _, _, _) => {
            h689
        }
    }, match v0.clone() {
        Adt8::C0(_, _, _, _, _, h690, _, _, _, _) => {
            h690
        }
    }, match v0.clone() {
        Adt8::C0(_, _, _, _, _, _, h691, _, _, _) => {
            h691
        }
    }, f149(match v0.clone() {
        Adt8::C0(_, _, _, _, _, _, _, _, h692, _) => {
            h692
        }
    }), {
        let a694 = match v0 {
            Adt8::C0(_, _, _, _, _, _, _, _, _, h693) => {
                h693
            }
        };
        let a695 = 1u64;
        nat_add(a694, a695)?
    }))
}

pub fn f139(v0: u32) -> u64 {
    let m697 = {
        let a696 = v0;
        fixed_u64::convert(i128::from(a696))
    };
    match m697 {
        None => {
            0u64
        }
        Some(v1) => {
            v1
        }
    }
}

pub fn f140(v0: u64) -> u32 {
    let m701 = {
        let a700 = {
            let a698 = v0;
            let a699 = 4294967295u64;
            fixed_u64::bit_and(a698, a699)
        };
        fixed_u32::convert(i128::from(a700))
    };
    match m701 {
        None => {
            0u32
        }
        Some(v1) => {
            v1
        }
    }
}

pub fn f141(v0: u64) -> u32 {
    let (_, h702) = {
        let (_, h703) = f150(Fn27::F151, 32u64, (v0, (1u32, 0u32)));
        h703
    };
    h702
}

pub fn f142(v0: u32, v1: u32, v2: u32, v3: u32) -> u32 {
    let a708 = f152(v0, 24u32);
    let a709 = {
        let a706 = f152(v1, 16u32);
        let a707 = {
            let a704 = f152(v2, 8u32);
            let a705 = v3;
            fixed_u32::bit_or(a704, a705)
        };
        fixed_u32::bit_or(a706, a707)
    };
    fixed_u32::bit_or(a708, a709)
}

pub fn f143(v0: Bytes, v1: u64, v2: u64) -> u32 {
    let a710 = v2;
    let a711 = v1;
    if nat_lt(a710, a711) {
        let m714 = {
            let a712 = v0;
            let a713 = v2;
            index_bytes(a712, a713)
        };
        match m714 {
            None => {
                0u32
            }
            Some(v3) => {
                let m716 = {
                    let a715 = v3;
                    fixed_u32::convert(i128::from(a715))
                };
                match m716 {
                    None => {
                        0u32
                    }
                    Some(v4) => {
                        v4
                    }
                }
            }
        }
    } else {
        let a717 = v2;
        let a718 = v1;
        if nat_eq(a717, a718) {
            128u32
        } else {
            0u32
        }
    }
}

pub fn f144(v0: u64) -> u32 {
    let a719 = v0;
    let a720 = 32u64;
    if nat_lt(a719, a720) {
        let a721 = v0;
        let a722 = 16u64;
        if nat_lt(a721, a722) {
            let a723 = v0;
            let a724 = 8u64;
            if nat_lt(a723, a724) {
                let a725 = v0;
                let a726 = 4u64;
                if nat_lt(a725, a726) {
                    let a727 = v0;
                    let a728 = 2u64;
                    if nat_lt(a727, a728) {
                        let a729 = v0;
                        let a730 = 1u64;
                        if nat_lt(a729, a730) {
                            1116352408u32
                        } else {
                            1899447441u32
                        }
                    } else {
                        let a731 = v0;
                        let a732 = 3u64;
                        if nat_lt(a731, a732) {
                            3049323471u32
                        } else {
                            3921009573u32
                        }
                    }
                } else {
                    let a733 = v0;
                    let a734 = 6u64;
                    if nat_lt(a733, a734) {
                        let a735 = v0;
                        let a736 = 5u64;
                        if nat_lt(a735, a736) {
                            961987163u32
                        } else {
                            1508970993u32
                        }
                    } else {
                        let a737 = v0;
                        let a738 = 7u64;
                        if nat_lt(a737, a738) {
                            2453635748u32
                        } else {
                            2870763221u32
                        }
                    }
                }
            } else {
                let a739 = v0;
                let a740 = 12u64;
                if nat_lt(a739, a740) {
                    let a741 = v0;
                    let a742 = 10u64;
                    if nat_lt(a741, a742) {
                        let a743 = v0;
                        let a744 = 9u64;
                        if nat_lt(a743, a744) {
                            3624381080u32
                        } else {
                            310598401u32
                        }
                    } else {
                        let a745 = v0;
                        let a746 = 11u64;
                        if nat_lt(a745, a746) {
                            607225278u32
                        } else {
                            1426881987u32
                        }
                    }
                } else {
                    let a747 = v0;
                    let a748 = 14u64;
                    if nat_lt(a747, a748) {
                        let a749 = v0;
                        let a750 = 13u64;
                        if nat_lt(a749, a750) {
                            1925078388u32
                        } else {
                            2162078206u32
                        }
                    } else {
                        let a751 = v0;
                        let a752 = 15u64;
                        if nat_lt(a751, a752) {
                            2614888103u32
                        } else {
                            3248222580u32
                        }
                    }
                }
            }
        } else {
            let a753 = v0;
            let a754 = 24u64;
            if nat_lt(a753, a754) {
                let a755 = v0;
                let a756 = 20u64;
                if nat_lt(a755, a756) {
                    let a757 = v0;
                    let a758 = 18u64;
                    if nat_lt(a757, a758) {
                        let a759 = v0;
                        let a760 = 17u64;
                        if nat_lt(a759, a760) {
                            3835390401u32
                        } else {
                            4022224774u32
                        }
                    } else {
                        let a761 = v0;
                        let a762 = 19u64;
                        if nat_lt(a761, a762) {
                            264347078u32
                        } else {
                            604807628u32
                        }
                    }
                } else {
                    let a763 = v0;
                    let a764 = 22u64;
                    if nat_lt(a763, a764) {
                        let a765 = v0;
                        let a766 = 21u64;
                        if nat_lt(a765, a766) {
                            770255983u32
                        } else {
                            1249150122u32
                        }
                    } else {
                        let a767 = v0;
                        let a768 = 23u64;
                        if nat_lt(a767, a768) {
                            1555081692u32
                        } else {
                            1996064986u32
                        }
                    }
                }
            } else {
                let a769 = v0;
                let a770 = 28u64;
                if nat_lt(a769, a770) {
                    let a771 = v0;
                    let a772 = 26u64;
                    if nat_lt(a771, a772) {
                        let a773 = v0;
                        let a774 = 25u64;
                        if nat_lt(a773, a774) {
                            2554220882u32
                        } else {
                            2821834349u32
                        }
                    } else {
                        let a775 = v0;
                        let a776 = 27u64;
                        if nat_lt(a775, a776) {
                            2952996808u32
                        } else {
                            3210313671u32
                        }
                    }
                } else {
                    let a777 = v0;
                    let a778 = 30u64;
                    if nat_lt(a777, a778) {
                        let a779 = v0;
                        let a780 = 29u64;
                        if nat_lt(a779, a780) {
                            3336571891u32
                        } else {
                            3584528711u32
                        }
                    } else {
                        let a781 = v0;
                        let a782 = 31u64;
                        if nat_lt(a781, a782) {
                            113926993u32
                        } else {
                            338241895u32
                        }
                    }
                }
            }
        }
    } else {
        let a783 = v0;
        let a784 = 48u64;
        if nat_lt(a783, a784) {
            let a785 = v0;
            let a786 = 40u64;
            if nat_lt(a785, a786) {
                let a787 = v0;
                let a788 = 36u64;
                if nat_lt(a787, a788) {
                    let a789 = v0;
                    let a790 = 34u64;
                    if nat_lt(a789, a790) {
                        let a791 = v0;
                        let a792 = 33u64;
                        if nat_lt(a791, a792) {
                            666307205u32
                        } else {
                            773529912u32
                        }
                    } else {
                        let a793 = v0;
                        let a794 = 35u64;
                        if nat_lt(a793, a794) {
                            1294757372u32
                        } else {
                            1396182291u32
                        }
                    }
                } else {
                    let a795 = v0;
                    let a796 = 38u64;
                    if nat_lt(a795, a796) {
                        let a797 = v0;
                        let a798 = 37u64;
                        if nat_lt(a797, a798) {
                            1695183700u32
                        } else {
                            1986661051u32
                        }
                    } else {
                        let a799 = v0;
                        let a800 = 39u64;
                        if nat_lt(a799, a800) {
                            2177026350u32
                        } else {
                            2456956037u32
                        }
                    }
                }
            } else {
                let a801 = v0;
                let a802 = 44u64;
                if nat_lt(a801, a802) {
                    let a803 = v0;
                    let a804 = 42u64;
                    if nat_lt(a803, a804) {
                        let a805 = v0;
                        let a806 = 41u64;
                        if nat_lt(a805, a806) {
                            2730485921u32
                        } else {
                            2820302411u32
                        }
                    } else {
                        let a807 = v0;
                        let a808 = 43u64;
                        if nat_lt(a807, a808) {
                            3259730800u32
                        } else {
                            3345764771u32
                        }
                    }
                } else {
                    let a809 = v0;
                    let a810 = 46u64;
                    if nat_lt(a809, a810) {
                        let a811 = v0;
                        let a812 = 45u64;
                        if nat_lt(a811, a812) {
                            3516065817u32
                        } else {
                            3600352804u32
                        }
                    } else {
                        let a813 = v0;
                        let a814 = 47u64;
                        if nat_lt(a813, a814) {
                            4094571909u32
                        } else {
                            275423344u32
                        }
                    }
                }
            }
        } else {
            let a815 = v0;
            let a816 = 56u64;
            if nat_lt(a815, a816) {
                let a817 = v0;
                let a818 = 52u64;
                if nat_lt(a817, a818) {
                    let a819 = v0;
                    let a820 = 50u64;
                    if nat_lt(a819, a820) {
                        let a821 = v0;
                        let a822 = 49u64;
                        if nat_lt(a821, a822) {
                            430227734u32
                        } else {
                            506948616u32
                        }
                    } else {
                        let a823 = v0;
                        let a824 = 51u64;
                        if nat_lt(a823, a824) {
                            659060556u32
                        } else {
                            883997877u32
                        }
                    }
                } else {
                    let a825 = v0;
                    let a826 = 54u64;
                    if nat_lt(a825, a826) {
                        let a827 = v0;
                        let a828 = 53u64;
                        if nat_lt(a827, a828) {
                            958139571u32
                        } else {
                            1322822218u32
                        }
                    } else {
                        let a829 = v0;
                        let a830 = 55u64;
                        if nat_lt(a829, a830) {
                            1537002063u32
                        } else {
                            1747873779u32
                        }
                    }
                }
            } else {
                let a831 = v0;
                let a832 = 60u64;
                if nat_lt(a831, a832) {
                    let a833 = v0;
                    let a834 = 58u64;
                    if nat_lt(a833, a834) {
                        let a835 = v0;
                        let a836 = 57u64;
                        if nat_lt(a835, a836) {
                            1955562222u32
                        } else {
                            2024104815u32
                        }
                    } else {
                        let a837 = v0;
                        let a838 = 59u64;
                        if nat_lt(a837, a838) {
                            2227730452u32
                        } else {
                            2361852424u32
                        }
                    }
                } else {
                    let a839 = v0;
                    let a840 = 62u64;
                    if nat_lt(a839, a840) {
                        let a841 = v0;
                        let a842 = 61u64;
                        if nat_lt(a841, a842) {
                            2428436474u32
                        } else {
                            2756734187u32
                        }
                    } else {
                        let a843 = v0;
                        let a844 = 63u64;
                        if nat_lt(a843, a844) {
                            3204031479u32
                        } else {
                            3329325298u32
                        }
                    }
                }
            }
        }
    }
}

pub fn f145(v0: u32) -> u32 {
    let a847 = {
        let a845 = f153(v0, 6u32, 26u32);
        let a846 = f153(v0, 11u32, 21u32);
        fixed_u32::bit_xor(a845, a846)
    };
    let a848 = f153(v0, 25u32, 7u32);
    fixed_u32::bit_xor(a847, a848)
}

pub fn f146(v0: u32, v1: u32, v2: u32) -> u32 {
    let a854 = {
        let a849 = v0;
        let a850 = v1;
        fixed_u32::bit_and(a849, a850)
    };
    let a855 = {
        let a852 = {
            let a851 = v0;
            fixed_u32::bit_not(a851)
        };
        let a853 = v2;
        fixed_u32::bit_and(a852, a853)
    };
    fixed_u32::bit_xor(a854, a855)
}

pub fn f147(v0: u32) -> u32 {
    let a858 = {
        let a856 = f153(v0, 2u32, 30u32);
        let a857 = f153(v0, 13u32, 19u32);
        fixed_u32::bit_xor(a856, a857)
    };
    let a859 = f153(v0, 22u32, 10u32);
    fixed_u32::bit_xor(a858, a859)
}

pub fn f148(v0: u32, v1: u32, v2: u32) -> u32 {
    let a868 = {
        let a864 = {
            let a860 = v0;
            let a861 = v1;
            fixed_u32::bit_and(a860, a861)
        };
        let a865 = {
            let a862 = v0;
            let a863 = v2;
            fixed_u32::bit_and(a862, a863)
        };
        fixed_u32::bit_xor(a864, a865)
    };
    let a869 = {
        let a866 = v1;
        let a867 = v2;
        fixed_u32::bit_and(a866, a867)
    };
    fixed_u32::bit_xor(a868, a869)
}

pub fn f149(v0: Adt7) -> Adt7 {
    Adt7::C0(match v0.clone() {
        Adt7::C0(_, h870, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => {
            h870
        }
    }, match v0.clone() {
        Adt7::C0(_, _, h871, _, _, _, _, _, _, _, _, _, _, _, _, _) => {
            h871
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, h872, _, _, _, _, _, _, _, _, _, _, _, _) => {
            h872
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, h873, _, _, _, _, _, _, _, _, _, _, _) => {
            h873
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, h874, _, _, _, _, _, _, _, _, _, _) => {
            h874
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, h875, _, _, _, _, _, _, _, _, _) => {
            h875
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, h876, _, _, _, _, _, _, _, _) => {
            h876
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, h877, _, _, _, _, _, _, _) => {
            h877
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, h878, _, _, _, _, _, _) => {
            h878
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, _, h879, _, _, _, _, _) => {
            h879
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, _, _, h880, _, _, _, _) => {
            h880
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, _, _, _, h881, _, _, _) => {
            h881
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, _, _, _, _, h882, _, _) => {
            h882
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, _, _, _, _, _, h883, _) => {
            h883
        }
    }, match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, h884) => {
            h884
        }
    }, f134(f154(match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, _, _, _, _, _, h885, _) => {
            h885
        }
    }), f134(match v0.clone() {
        Adt7::C0(_, _, _, _, _, _, _, _, _, h886, _, _, _, _, _, _) => {
            h886
        }
    }, f134(f155(match v0.clone() {
        Adt7::C0(_, h887, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => {
            h887
        }
    }), match v0 {
        Adt7::C0(h888, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => {
            h888
        }
    }))))
}

pub fn f150(v0: Fn27, p892: u64, p893: (u64, (u32, u32))) -> (u64, (u32, u32)) {
    let m891 = (p892, p893);
    {
        let mut m891 = m891;
        loop {
            let step = {
                let (v1, v2) = m891;
                let m889 = v1;
                if m889 == 0 {
                    Err::<(u64, (u64, (u32, u32))), (u64, (u32, u32))>(v2)
                } else {
                    let v3 = m889 - 1;
                    Ok::<(u64, (u64, (u32, u32))), (u64, (u32, u32))>((v3, {
                        let c890: Fn27 = v0.clone();
                        c890.apply(v2)
                    }))
                }
            };
            match step {
                Ok(next) => { m891 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f151(v0: (u64, (u32, u32))) -> (u64, (u32, u32)) {
    let v1: u64 = {
        let (h894, _) = v0;
        h894
    };
    let v2: u32 = {
        let (h895, _) = {
            let (_, h896) = v0;
            h896
        };
        h895
    };
    let v3: u32 = {
        let (_, h897) = {
            let (_, h898) = v0;
            h898
        };
        h897
    };
    ({
        let a899 = v1;
        let a900 = 2u64;
        let a901 = 0u64;
        nat_quot(a899, a900, a901)
    }, (f152(v2, 1u32), {
        let a905 = {
            let a902 = v1;
            let a903 = 2u64;
            let a904 = 0u64;
            nat_rem(a902, a903, a904)
        };
        let a906 = 1u64;
        if nat_eq(a905, a906) {
            let a907 = v3;
            let a908 = v2;
            fixed_u32::bit_or(a907, a908)
        } else {
            v3
        }
    }))
}

pub fn f152(v0: u32, v1: u32) -> u32 {
    let m911 = {
        let a909 = v0;
        let a910 = v1;
        fixed_u32::shift_left(a909, a910)
    };
    match m911 {
        None => {
            0u32
        }
        Some(v2) => {
            v2
        }
    }
}

pub fn f153(v0: u32, v1: u32, v2: u32) -> u32 {
    let a912 = f127(v0, v1);
    let a913 = f152(v0, v2);
    fixed_u32::bit_or(a912, a913)
}

pub fn f154(v0: u32) -> u32 {
    let a916 = {
        let a914 = f153(v0, 17u32, 15u32);
        let a915 = f153(v0, 19u32, 13u32);
        fixed_u32::bit_xor(a914, a915)
    };
    let a917 = f127(v0, 10u32);
    fixed_u32::bit_xor(a916, a917)
}

pub fn f155(v0: u32) -> u32 {
    let a920 = {
        let a918 = f153(v0, 7u32, 25u32);
        let a919 = f153(v0, 18u32, 14u32);
        fixed_u32::bit_xor(a918, a919)
    };
    let a921 = f127(v0, 3u32);
    fixed_u32::bit_xor(a920, a921)
}

pub fn run(p0: Bytes) -> R<Bytes> {
    f0(p0)
}
