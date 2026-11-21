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
    C0(Str, Adt2, Str, Str, List<Str>, List<Str>),
}

#[derive(Clone)]
pub enum Adt2 {
    C0,
    C1,
}

#[derive(Clone)]
pub enum Adt3 {
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
pub enum Fn0 {
    F3(Adt0),
}

impl Fn0 {
    pub fn apply(&self, p0: Bytes, p1: Adt1) -> Bytes {
        match self {
            Fn0::F3(k0) => f3(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn1 {
    F6,
}

impl Fn1 {
    pub fn apply(&self, p0: Adt0, p1: Bytes) -> R<Adt0> {
        match self {
            Fn1::F6 => f6(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn2 {
    F18(Bytes),
}

impl Fn2 {
    pub fn apply(&self, p0: (u64, (u64, List<Bytes>))) -> R<Option<(u64, (u64, List<Bytes>))>> {
        match self {
            Fn2::F18(k0) => f18(k0.clone(), p0),
        }
    }
}

#[derive(Clone)]
pub enum Fn3 {
    F22,
}

impl Fn3 {
    pub fn apply(&self, p0: List<(Bytes, Bytes)>, p1: (Bytes, Bytes)) -> List<(Bytes, Bytes)> {
        match self {
            Fn3::F22 => f22(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn4 {
    F24,
}

impl Fn4 {
    pub fn apply(&self, p0: List<(Str, Str)>, p1: (Str, Str)) -> List<(Str, Str)> {
        match self {
            Fn4::F24 => f24(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn5 {
    F27(Str),
}

impl Fn5 {
    pub fn apply(&self, p0: Str, p1: (Str, Str)) -> Str {
        match self {
            Fn5::F27(k0) => f27(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn6 {
    F29(Str),
}

impl Fn6 {
    pub fn apply(&self, p0: Option<bool>, p1: (Str, bool)) -> Option<bool> {
        match self {
            Fn6::F29(k0) => f29(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn7 {
    F34,
}

impl Fn7 {
    pub fn apply(&self, p0: List<Bytes>, p1: Str) -> List<Bytes> {
        match self {
            Fn7::F34 => f34(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn8 {
    F41,
}

impl Fn8 {
    pub fn apply(&self, p0: List<Bytes>, p1: Bytes) -> List<Bytes> {
        match self {
            Fn8::F41 => f41(p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn9 {
    F47(Bytes),
}

impl Fn9 {
    pub fn apply(&self, p0: Option<Bytes>, p1: Bytes) -> Option<Bytes> {
        match self {
            Fn9::F47(k0) => f47(k0.clone(), p0, p1),
        }
    }
}

#[derive(Clone)]
pub enum Fn10 {
    F49(Bytes, u64, Bytes, u64),
}

impl Fn10 {
    pub fn apply(&self, p0: u64) -> R<Option<u64>> {
        match self {
            Fn10::F49(k0, k1, k2, k3) => f49(k0.clone(), *k1, k2.clone(), *k3, p0),
        }
    }
}

pub fn f0(v0: Bytes) -> R<Bytes> {
    let v1: Adt0 = f1(v0)?;
    Ok(f2(Fn0::F3(v1), Bytes::lit(&[]), f4()))
}

pub fn f1(v0: Bytes) -> R<Adt0> {
    let v1: Adt0 = f5(Fn1::F6, Adt0::C0(List::<(Bytes, Bytes)>::nil(), List::<(Str, bool)>::nil(), List::<(Str, Str)>::nil(), List::<(Str, bool)>::nil()), f7(v0)?)?;
    Ok(Adt0::C0(f8(match v1.clone() {
        Adt0::C0(h1, _, _, _) => {
            h1
        }
    }), match v1.clone() {
        Adt0::C0(_, h2, _, _) => {
            h2
        }
    }, f9(match v1.clone() {
        Adt0::C0(_, _, h3, _) => {
            h3
        }
    }), match v1 {
        Adt0::C0(_, _, _, h4) => {
            h4
        }
    }))
}

pub fn f2(v0: Fn0, p8: Bytes, p9: List<Adt1>) -> Bytes {
    let m7 = (p8, p9);
    {
        let mut m7 = m7;
        loop {
            let step = {
                let (v1, v2) = m7;
                let m5 = v2;
                match m5.uncons() {
                    None => {
                        Err::<(Bytes, List<Adt1>), Bytes>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Bytes, List<Adt1>), Bytes>(({
                            let c6: Fn0 = v0.clone();
                            c6.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m7 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f3(v0: Adt0, v1: Bytes, v2: Adt1) -> Bytes {
    let v3: Adt3 = f10(match v2.clone() {
        Adt1::C0(h10, _, _, _, _, _) => {
            h10
        }
    }, match v0.clone() {
        Adt0::C0(_, _, h11, _) => {
            h11
        }
    });
    let v5: Adt3 = {
        let m14 = f11(match v2.clone() {
            Adt1::C0(h12, _, _, _, _, _) => {
                h12
            }
        }, match v0 {
            Adt0::C0(_, _, _, h13) => {
                h13
            }
        });
        match m14 {
            None => {
                v3
            }
            Some(v4) => {
                f12(v3, v4)
            }
        }
    };
    let a16 = v1;
    let a17 = f13(List::cons(Str::lit("weight"), List::cons(match v2 {
        Adt1::C0(h15, _, _, _, _, _) => {
            h15
        }
    }, List::cons(f14(v5), List::<Str>::nil()))));
    append_bytes(a16, a17)
}

pub fn f4() -> List<Adt1> {
    List::cons(f15(Str::lit("factory-model"), Adt2::C0, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'bash prism-factory/check.sh'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("prism-factory/"), List::cons(Str::lit("Justfile"), List::cons(Str::lit("rust-toolchain.toml"), List::<Str>::nil()))))), List::<Str>::nil()), List::cons(f15(Str::lit("factory-upstream"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/upstream.sh"), List::cons(Str::lit("prism-factory/"), List::<Str>::nil()), List::<Str>::nil()), List::cons(f15(Str::lit("fmt-check"), Adt2::C0, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo fmt --all -- --check'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("rustfmt.toml"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("model"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo run -q --locked -p xtask -- validate'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit(".devcontainer/"), List::cons(Str::lit(".github/"), List::cons(Str::lit("AGENTS.md"), List::cons(Str::lit("CONFORMANCE.md"), List::cons(Str::lit("README.md"), List::cons(Str::lit("VERIFICATION.md"), List::cons(Str::lit("template.lock"), List::cons(Str::lit("template-contract.json"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("model/"), List::cons(Str::lit("LANES.md"), List::cons(Str::lit("TEMPLATE-CONTRACT.md"), List::cons(Str::lit("TEMPLATE-VERIFICATION.md"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("template-check"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh '(prismpm template check) && (prismpm lock check)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit(".devcontainer/"), List::cons(Str::lit(".github/"), List::cons(Str::lit("AGENTS.md"), List::cons(Str::lit("CONFORMANCE.md"), List::cons(Str::lit("README.md"), List::cons(Str::lit("VERIFICATION.md"), List::cons(Str::lit("template.lock"), List::cons(Str::lit("template-contract.json"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("model/"), List::cons(Str::lit("Justfile"), List::<Str>::nil())))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("self-model-check"), Adt2::C0, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh scripts/self-model/check.sh"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("model/"), List::cons(Str::lit("app/"), List::cons(Str::lit("brand/"), List::cons(Str::lit("scripts/self-model/"), List::cons(Str::lit("scripts/experience/"), List::cons(Str::lit("scripts/mir/ledger.py"), List::cons(Str::lit("crates/uorust/src/"), List::cons(Str::lit("crates/uorust/core/frontier.toml"), List::cons(Str::lit("crates/uorust/std/frontier.toml"), List::cons(Str::lit("crates/uorust/crates/"), List::cons(Str::lit("crates/certified/manifest.toml"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("site/app.json"), List::cons(Str::lit(".github/workflows/"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("README.md"), List::<Str>::nil())))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("lint"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo clippy --workspace --all-targets --all-features --locked -- -D warnings'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("clippy.toml"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("test"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo test --workspace --locked'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::cons(Str::lit("model/"), List::cons(Str::lit("features/"), List::cons(Str::lit("app/"), List::cons(Str::lit("Justfile"), List::cons(Str::lit("CONFORMANCE.md"), List::cons(Str::lit("README.md"), List::cons(Str::lit("VERIFICATION.md"), List::cons(Str::lit("ANALYSIS.md"), List::cons(Str::lit("scripts/ci/"), List::cons(Str::lit("scripts/hooks/passes.py"), List::cons(Str::lit("scripts/release/"), List::cons(Str::lit("scripts/self-model/"), List::cons(Str::lit("bootstrap/acceptance-result.test.mjs"), List::cons(Str::lit("site/acceptance-result.mjs"), List::cons(Str::lit("site/guest-worker.mjs"), List::cons(Str::lit("site/spec.mjs"), List::cons(Str::lit("prism-factory/"), List::cons(Str::lit("brand/foundation/src/index.css"), List::cons(Str::lit("@base:frontier.toml"), List::cons(Str::lit("@base:crates/certified/manifest.toml"), List::<Str>::nil())))))))))))))))))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("deny"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh 'cargo deny --all-features check'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("deny.toml"), List::<Str>::nil())))))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("app"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh '(cd app && lexlean fmt --check && lexlean lock --check) && (bash -euo pipefail -c '\"'\"'case \"${PRISM_PROOF_EVENT:-}\" in pull_request:*|merge_group:*|push:refs/heads/main) if cargo run -q --locked -p xtask -- proof-reuse; then exit 0; fi ;; esac; rm -rf .prism-proof; prismpm --project app verify | tee .prism-proof.log; attestation=$(sed -n \"s/^verified attestation: \\([0-9a-f]\\{64\\}\\)$/\\1/p\" .prism-proof.log); rm -f .prism-proof.log; cargo run -q --locked -p xtask -- proof-keep \"$attestation\"'\"'\"')'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("app/"), List::cons(Str::lit("Justfile"), List::<Str>::nil()))))))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("uorust"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/sdk.sh '(cargo run -q --locked -p uorust -- check crates/uorust/tests/fixtures/thermostat --out crates/uorust/tests/fixtures/thermostat-prism --name thermostat --last-modified 2026-11-11T00:00:00Z) && (cd crates/uorust/tests/fixtures/thermostat-prism && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check) && (rm -rf target/uorust-self) && (cargo run -q --locked -p uorust -- mir . --out target/mir) && (cargo run -q --locked -p uorust -- project . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --mir target/mir --verdicts lift-verdicts.toml) && (cargo run -q --locked -p uorust -- check . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --register frontier.toml --mir target/mir --verdicts lift-verdicts.toml) && (cd target/uorust-self && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check) && (rm -rf target/uorust-advised) && (cargo run -q --locked -p uorust -- project crates/uorust/tests/fixtures/advised --out target/uorust-advised --name advised --last-modified 2026-11-11T00:00:00Z --advisories crates/uorust/tests/fixtures/advised/advisory-db) && (cd target/uorust-advised && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::<Str>::nil())))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("vv"), Adt2::C0, Str::lit("host"), Str::lit("echo \"vv: the acceptance gate passed\""), List::cons(Str::lit("Justfile"), List::<Str>::nil()), List::cons(Str::lit("fmt-check"), List::cons(Str::lit("model"), List::cons(Str::lit("template-check"), List::cons(Str::lit("self-model-check"), List::cons(Str::lit("lint"), List::cons(Str::lit("test"), List::cons(Str::lit("deny"), List::cons(Str::lit("app"), List::cons(Str::lit("uorust"), List::<Str>::nil())))))))))), List::cons(f15(Str::lit("prove-fixture"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/written.sh crates/uorust/tests/fixtures/mir-path/lift-verdicts.toml -- bash prism-factory/sdk.sh '(cargo run -q --locked -p uorust -- mir crates/uorust/tests/fixtures/mir-path --out target/mir-fixture) && (cargo run -q --locked -p uorust -- prove crates/uorust/tests/fixtures/mir-path --out target/mir-fixture-prove --scratch target/mir-fixture-scratch --name walk --last-modified 2026-11-11T00:00:00Z --mir target/mir-fixture --verdicts crates/uorust/tests/fixtures/mir-path/lift-verdicts.toml)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("!crates/uorust/core/"), List::cons(Str::lit("!crates/uorust/std/"), List::cons(Str::lit("!crates/uorust/crates/"), List::cons(Str::lit("!crates/uorust/ledger/"), List::cons(Str::lit("!crates/uorust/tests/fixtures/"), List::cons(Str::lit("crates/uorust/tests/fixtures/nist-sha256/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/dependabot.yml"), List::cons(Str::lit(".github/workflows/bootstrap.yml"), List::cons(Str::lit(".github/workflows/prismpm.yml"), List::cons(Str::lit(".github/workflows/template-update.yml"), List::cons(Str::lit("crates/uorust/tests/fixtures/mir-path/"), List::cons(Str::lit("prism-factory/written.sh"), List::<Str>::nil()))))))))))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("prove"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/written.sh lift-verdicts.toml -- bash prism-factory/sdk.sh '(cargo run -q --locked -p uorust -- mir . --out target/mir) && (cargo run -q --locked -p uorust -- prove . --out target/uorust-prove --scratch target/mir-prove --name uorust --last-modified 2026-11-11T00:00:00Z --mir target/mir --verdicts lift-verdicts.toml)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::cons(Str::lit("prism-factory/written.sh"), List::<Str>::nil()))))))))))))), List::<Str>::nil()), List::cons(f15(Str::lit("certified"), Adt2::C1, Str::lit("host"), Str::lit("bash prism-factory/written.sh crates/certified/ crates/uorust/src/certified.rs -- bash prism-factory/sdk.sh '(just certified-project) && (just certified-export \"\" ) && (just certified-sync)'"), List::cons(Str::lit("prismpm.lock"), List::cons(Str::lit("prism-factory/sdk.sh"), List::cons(Str::lit("Cargo.toml"), List::cons(Str::lit("Cargo.lock"), List::cons(Str::lit("rust-toolchain.toml"), List::cons(Str::lit(".cargo/"), List::cons(Str::lit("crates/"), List::cons(Str::lit("xtask/"), List::cons(Str::lit("frontier.toml"), List::cons(Str::lit("lift-verdicts.toml"), List::cons(Str::lit("app/lexlean.lock"), List::cons(Str::lit(".github/"), List::cons(Str::lit("Justfile"), List::cons(Str::lit("prism-factory/written.sh"), List::<Str>::nil())))))))))))))), List::cons(Str::lit("prove"), List::<Str>::nil())), List::<Adt1>::nil())))))))))))))))
}

pub fn f5(v0: Fn1, p21: Adt0, p22: List<Bytes>) -> R<Adt0> {
    let m20 = (p21, p22);
    {
        let mut m20 = m20;
        loop {
            let step = {
                let (v1, v2) = m20;
                let m18 = v2;
                match m18.uncons() {
                    None => {
                        Err::<(Adt0, List<Bytes>), R<Adt0>>(Ok(v1))
                    }
                    Some((v3, v4)) => {
                        Ok::<(Adt0, List<Bytes>), R<Adt0>>(({
                            let c19: Fn1 = v0.clone();
                            c19.apply(v1, v3)?
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m20 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f6(v0: Adt0, v1: Bytes) -> R<Adt0> {
    f16(v0, v1)
}

pub fn f7(v0: Bytes) -> R<List<Bytes>> {
    let v1: u64 = {
        let a23 = v0.clone();
        length_bytes(a23)
    };
    let v2: (u64, (u64, List<Bytes>)) = {
        let (h24, _) = f17(Fn2::F18(v0.clone()), v1, (0u64, (0u64, List::<Bytes>::nil())))?;
        h24
    };
    let v3: u64 = {
        let (h25, _) = {
            let (_, h26) = v2.clone();
            h26
        };
        h25
    };
    let v4: List<Bytes> = {
        let (_, h27) = {
            let (_, h28) = v2;
            h28
        };
        h27
    };
    let a29 = v3;
    let a30 = v1;
    if nat_lt(a29, a30) {
        Ok(f19(List::cons(f20(v0, v3, {
            let a31 = v1;
            let a32 = v3;
            nat_sub(a31, a32)
        }), v4)))
    } else {
        Ok(f19(v4))
    }
}

pub fn f8(v0: List<(Bytes, Bytes)>) -> List<(Bytes, Bytes)> {
    f21(Fn3::F22, List::<(Bytes, Bytes)>::nil(), v0)
}

pub fn f9(v0: List<(Str, Str)>) -> List<(Str, Str)> {
    f23(Fn4::F24, List::<(Str, Str)>::nil(), v0)
}

pub fn f10(v0: Str, v1: List<(Str, Str)>) -> Adt3 {
    f25(f26(Fn5::F27(v0), Str::lit("4"), v1))
}

pub fn f11(v0: Str, v1: List<(Str, bool)>) -> Option<bool> {
    f28(Fn6::F29(v0), None::<bool>, v1)
}

pub fn f12(v0: Adt3, v1: bool) -> Adt3 {
    if v1 {
        f30(v0)
    } else {
        f31(v0)
    }
}

pub fn f13(v0: List<Str>) -> Bytes {
    f32(f33(Fn7::F34, List::<Bytes>::nil(), v0))
}

pub fn f14(v0: Adt3) -> Str {
    let m33 = v0;
    match m33 {
        Adt3::C0 => {
            Str::lit("1")
        }
        Adt3::C1 => {
            Str::lit("2")
        }
        Adt3::C2 => {
            Str::lit("3")
        }
        Adt3::C3 => {
            Str::lit("4")
        }
        Adt3::C4 => {
            Str::lit("5")
        }
        Adt3::C5 => {
            Str::lit("6")
        }
        Adt3::C6 => {
            Str::lit("7")
        }
        Adt3::C7 => {
            Str::lit("8")
        }
        Adt3::C8 => {
            Str::lit("9")
        }
        Adt3::C9 => {
            Str::lit("10")
        }
        Adt3::C10 => {
            Str::lit("11")
        }
        Adt3::C11 => {
            Str::lit("12")
        }
        Adt3::C12 => {
            Str::lit("13")
        }
        Adt3::C13 => {
            Str::lit("14")
        }
        Adt3::C14 => {
            Str::lit("15")
        }
        Adt3::C15 => {
            Str::lit("16")
        }
    }
}

pub fn f15(v0: Str, v1: Adt2, v2: Str, v3: Str, v4: List<Str>, v5: List<Str>) -> Adt1 {
    Adt1::C0(v0, v1, v2, v3, v4, v5)
}

pub fn f16(v0: Adt0, v1: Bytes) -> R<Adt0> {
    let v2: (Bytes, Bytes) = f35(v1)?;
    let v3: Bytes = {
        let (h34, _) = v2;
        h34
    };
    let v4: (Bytes, Bytes) = f35({
        let (_, h35) = v2;
        h35
    })?;
    let a36 = v3.clone();
    let a37 = f36(Str::lit("tree"));
    if equal(a36, a37) {
        Ok(Adt0::C0(List::cons(({
            let (h38, _) = v4;
            h38
        }, {
            let (_, h39) = v4;
            h39
        }), match v0.clone() {
            Adt0::C0(h40, _, _, _) => {
                h40
            }
        }), match v0.clone() {
            Adt0::C0(_, h41, _, _) => {
                h41
            }
        }, match v0.clone() {
            Adt0::C0(_, _, h42, _) => {
                h42
            }
        }, match v0 {
            Adt0::C0(_, _, _, h43) => {
                h43
            }
        }))
    } else {
        let a44 = v3.clone();
        let a45 = f36(Str::lit("receipt"));
        if equal(a44, a45) {
            Ok(Adt0::C0(match v0.clone() {
                Adt0::C0(h46, _, _, _) => {
                    h46
                }
            }, List::cons((f37({
                let (h47, _) = v4;
                h47
            }), f38({
                let (_, h48) = v4;
                h48
            })), match v0.clone() {
                Adt0::C0(_, h49, _, _) => {
                    h49
                }
            }), match v0.clone() {
                Adt0::C0(_, _, h50, _) => {
                    h50
                }
            }, match v0 {
                Adt0::C0(_, _, _, h51) => {
                    h51
                }
            }))
        } else {
            let a52 = v3.clone();
            let a53 = f36(Str::lit("weight"));
            if equal(a52, a53) {
                Ok(Adt0::C0(match v0.clone() {
                    Adt0::C0(h54, _, _, _) => {
                        h54
                    }
                }, match v0.clone() {
                    Adt0::C0(_, h55, _, _) => {
                        h55
                    }
                }, List::cons((f37({
                    let (h56, _) = v4;
                    h56
                }), f37({
                    let (_, h57) = v4;
                    h57
                })), match v0.clone() {
                    Adt0::C0(_, _, h58, _) => {
                        h58
                    }
                }), match v0 {
                    Adt0::C0(_, _, _, h59) => {
                        h59
                    }
                }))
            } else {
                let a60 = v3;
                let a61 = f36(Str::lit("outcome"));
                if equal(a60, a61) {
                    Ok(Adt0::C0(match v0.clone() {
                        Adt0::C0(h62, _, _, _) => {
                            h62
                        }
                    }, match v0.clone() {
                        Adt0::C0(_, h63, _, _) => {
                            h63
                        }
                    }, match v0.clone() {
                        Adt0::C0(_, _, h64, _) => {
                            h64
                        }
                    }, List::cons((f37({
                        let (h65, _) = v4;
                        h65
                    }), f38({
                        let (_, h66) = v4;
                        h66
                    })), match v0 {
                        Adt0::C0(_, _, _, h67) => {
                            h67
                        }
                    })))
                } else {
                    Ok(v0)
                }
            }
        }
    }
}

pub fn f17(v0: Fn2, p72: u64, p73: (u64, (u64, List<Bytes>))) -> R<((u64, (u64, List<Bytes>)), bool)> {
    let m71 = (p72, p73);
    {
        let mut m71 = m71;
        loop {
            let step = {
                let (v1, v2) = m71;
                let m68 = v1;
                if m68 == 0 {
                    Err::<(u64, (u64, (u64, List<Bytes>))), R<((u64, (u64, List<Bytes>)), bool)>>(Ok((v2, false)))
                } else {
                    let v3 = m68 - 1;
                    let m70 = {
                        let c69: Fn2 = v0.clone();
                        c69.apply(v2.clone())?
                    };
                    match m70 {
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
                Ok(next) => { m71 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f18(v0: Bytes, v1: (u64, (u64, List<Bytes>))) -> R<Option<(u64, (u64, List<Bytes>))>> {
    f39(v0, v1)
}

pub fn f19(v0: List<Bytes>) -> List<Bytes> {
    f40(Fn8::F41, List::<Bytes>::nil(), v0)
}

pub fn f20(v0: Bytes, v1: u64, v2: u64) -> Bytes {
    let m77 = {
        let a74 = v0;
        let a75 = v1;
        let a76 = v2;
        slice_bytes(a74, a75, a76)
    };
    match m77 {
        None => {
            Bytes::lit(&[])
        }
        Some(v3) => {
            v3
        }
    }
}

pub fn f21(v0: Fn3, p81: List<(Bytes, Bytes)>, p82: List<(Bytes, Bytes)>) -> List<(Bytes, Bytes)> {
    let m80 = (p81, p82);
    {
        let mut m80 = m80;
        loop {
            let step = {
                let (v1, v2) = m80;
                let m78 = v2;
                match m78.uncons() {
                    None => {
                        Err::<(List<(Bytes, Bytes)>, List<(Bytes, Bytes)>), List<(Bytes, Bytes)>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<(Bytes, Bytes)>, List<(Bytes, Bytes)>), List<(Bytes, Bytes)>>(({
                            let c79: Fn3 = v0.clone();
                            c79.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m80 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f22(v0: List<(Bytes, Bytes)>, v1: (Bytes, Bytes)) -> List<(Bytes, Bytes)> {
    List::cons(v1, v0)
}

pub fn f23(v0: Fn4, p86: List<(Str, Str)>, p87: List<(Str, Str)>) -> List<(Str, Str)> {
    let m85 = (p86, p87);
    {
        let mut m85 = m85;
        loop {
            let step = {
                let (v1, v2) = m85;
                let m83 = v2;
                match m83.uncons() {
                    None => {
                        Err::<(List<(Str, Str)>, List<(Str, Str)>), List<(Str, Str)>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<(Str, Str)>, List<(Str, Str)>), List<(Str, Str)>>(({
                            let c84: Fn4 = v0.clone();
                            c84.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m85 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f24(v0: List<(Str, Str)>, v1: (Str, Str)) -> List<(Str, Str)> {
    List::cons(v1, v0)
}

pub fn f25(v0: Str) -> Adt3 {
    let a88 = v0.clone();
    let a89 = Str::lit("1");
    if equal(a88, a89) {
        Adt3::C0
    } else {
        let a90 = v0.clone();
        let a91 = Str::lit("2");
        if equal(a90, a91) {
            Adt3::C1
        } else {
            let a92 = v0.clone();
            let a93 = Str::lit("3");
            if equal(a92, a93) {
                Adt3::C2
            } else {
                let a94 = v0.clone();
                let a95 = Str::lit("4");
                if equal(a94, a95) {
                    Adt3::C3
                } else {
                    let a96 = v0.clone();
                    let a97 = Str::lit("5");
                    if equal(a96, a97) {
                        Adt3::C4
                    } else {
                        let a98 = v0.clone();
                        let a99 = Str::lit("6");
                        if equal(a98, a99) {
                            Adt3::C5
                        } else {
                            let a100 = v0.clone();
                            let a101 = Str::lit("7");
                            if equal(a100, a101) {
                                Adt3::C6
                            } else {
                                let a102 = v0.clone();
                                let a103 = Str::lit("8");
                                if equal(a102, a103) {
                                    Adt3::C7
                                } else {
                                    let a104 = v0.clone();
                                    let a105 = Str::lit("9");
                                    if equal(a104, a105) {
                                        Adt3::C8
                                    } else {
                                        let a106 = v0.clone();
                                        let a107 = Str::lit("10");
                                        if equal(a106, a107) {
                                            Adt3::C9
                                        } else {
                                            let a108 = v0.clone();
                                            let a109 = Str::lit("11");
                                            if equal(a108, a109) {
                                                Adt3::C10
                                            } else {
                                                let a110 = v0.clone();
                                                let a111 = Str::lit("12");
                                                if equal(a110, a111) {
                                                    Adt3::C11
                                                } else {
                                                    let a112 = v0.clone();
                                                    let a113 = Str::lit("13");
                                                    if equal(a112, a113) {
                                                        Adt3::C12
                                                    } else {
                                                        let a114 = v0.clone();
                                                        let a115 = Str::lit("14");
                                                        if equal(a114, a115) {
                                                            Adt3::C13
                                                        } else {
                                                            let a116 = v0.clone();
                                                            let a117 = Str::lit("15");
                                                            if equal(a116, a117) {
                                                                Adt3::C14
                                                            } else {
                                                                let a118 = v0;
                                                                let a119 = Str::lit("16");
                                                                if equal(a118, a119) {
                                                                    Adt3::C15
                                                                } else {
                                                                    Adt3::C3
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

pub fn f26(v0: Fn5, p123: Str, p124: List<(Str, Str)>) -> Str {
    let m122 = (p123, p124);
    {
        let mut m122 = m122;
        loop {
            let step = {
                let (v1, v2) = m122;
                let m120 = v2;
                match m120.uncons() {
                    None => {
                        Err::<(Str, List<(Str, Str)>), Str>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Str, List<(Str, Str)>), Str>(({
                            let c121: Fn5 = v0.clone();
                            c121.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m122 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f27(v0: Str, v1: Str, v2: (Str, Str)) -> Str {
    let a126 = {
        let (h125, _) = v2;
        h125
    };
    let a127 = v0;
    if equal(a126, a127) {
        let (_, h128) = v2;
        h128
    } else {
        v1
    }
}

pub fn f28(v0: Fn6, p132: Option<bool>, p133: List<(Str, bool)>) -> Option<bool> {
    let m131 = (p132, p133);
    {
        let mut m131 = m131;
        loop {
            let step = {
                let (v1, v2) = m131;
                let m129 = v2;
                match m129.uncons() {
                    None => {
                        Err::<(Option<bool>, List<(Str, bool)>), Option<bool>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Option<bool>, List<(Str, bool)>), Option<bool>>(({
                            let c130: Fn6 = v0.clone();
                            c130.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m131 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f29(v0: Str, v1: Option<bool>, v2: (Str, bool)) -> Option<bool> {
    let a135 = {
        let (h134, _) = v2;
        h134
    };
    let a136 = v0;
    if equal(a135, a136) {
        Some({
            let (_, h137) = v2;
            h137
        })
    } else {
        v1
    }
}

pub fn f30(v0: Adt3) -> Adt3 {
    let m138 = v0;
    match m138 {
        Adt3::C0 => {
            Adt3::C0
        }
        Adt3::C1 => {
            Adt3::C0
        }
        Adt3::C2 => {
            Adt3::C1
        }
        Adt3::C3 => {
            Adt3::C2
        }
        Adt3::C4 => {
            Adt3::C3
        }
        Adt3::C5 => {
            Adt3::C4
        }
        Adt3::C6 => {
            Adt3::C5
        }
        Adt3::C7 => {
            Adt3::C6
        }
        Adt3::C8 => {
            Adt3::C7
        }
        Adt3::C9 => {
            Adt3::C8
        }
        Adt3::C10 => {
            Adt3::C9
        }
        Adt3::C11 => {
            Adt3::C10
        }
        Adt3::C12 => {
            Adt3::C11
        }
        Adt3::C13 => {
            Adt3::C12
        }
        Adt3::C14 => {
            Adt3::C13
        }
        Adt3::C15 => {
            Adt3::C14
        }
    }
}

pub fn f31(v0: Adt3) -> Adt3 {
    let m139 = v0;
    match m139 {
        Adt3::C0 => {
            Adt3::C2
        }
        Adt3::C1 => {
            Adt3::C3
        }
        Adt3::C2 => {
            Adt3::C4
        }
        Adt3::C3 => {
            Adt3::C5
        }
        Adt3::C4 => {
            Adt3::C6
        }
        Adt3::C5 => {
            Adt3::C7
        }
        Adt3::C6 => {
            Adt3::C8
        }
        Adt3::C7 => {
            Adt3::C9
        }
        Adt3::C8 => {
            Adt3::C10
        }
        Adt3::C9 => {
            Adt3::C11
        }
        Adt3::C10 => {
            Adt3::C12
        }
        Adt3::C11 => {
            Adt3::C13
        }
        Adt3::C12 => {
            Adt3::C14
        }
        Adt3::C13 => {
            Adt3::C15
        }
        Adt3::C14 => {
            Adt3::C15
        }
        Adt3::C15 => {
            Adt3::C15
        }
    }
}

pub fn f32(v0: List<Bytes>) -> Bytes {
    let a140 = f42(v0, Bytes::lit(&[0x20]));
    let a141 = Bytes::lit(&[0x0a]);
    append_bytes(a140, a141)
}

pub fn f33(v0: Fn7, p145: List<Bytes>, p146: List<Str>) -> List<Bytes> {
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
                            let c143: Fn7 = v0.clone();
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

pub fn f34(v0: List<Bytes>, v1: Str) -> List<Bytes> {
    let a147 = v0;
    let a148 = List::cons(f36(v1), List::<Bytes>::nil());
    append_list(a147, a148)
}

pub fn f35(v0: Bytes) -> R<(Bytes, Bytes)> {
    let m149 = f43(v0.clone(), Bytes::lit(&[0x20]))?;
    match m149 {
        None => {
            Ok((v0, Bytes::lit(&[])))
        }
        Some(v1) => {
            Ok((f20(v0.clone(), 0u64, v1), f20(v0.clone(), {
                let a150 = v1;
                let a151 = 1u64;
                nat_add(a150, a151)?
            }, {
                let a155 = {
                    let a152 = v0;
                    length_bytes(a152)
                };
                let a156 = {
                    let a153 = v1;
                    let a154 = 1u64;
                    nat_add(a153, a154)?
                };
                nat_sub(a155, a156)
            })))
        }
    }
}

pub fn f36(v0: Str) -> Bytes {
    let a157 = v0;
    utf8_encode(a157)
}

pub fn f37(v0: Bytes) -> Str {
    let m159 = {
        let a158 = v0;
        utf8_decode(a158)
    };
    match m159 {
        None => {
            Str::lit("")
        }
        Some(v1) => {
            v1
        }
    }
}

pub fn f38(v0: Bytes) -> bool {
    let a160 = v0;
    let a161 = f36(Str::lit("pass"));
    equal(a160, a161)
}

pub fn f39(v0: Bytes, v1: (u64, (u64, List<Bytes>))) -> R<Option<(u64, (u64, List<Bytes>))>> {
    let v2: u64 = {
        let (h162, _) = v1.clone();
        h162
    };
    let v3: u64 = {
        let (h163, _) = {
            let (_, h164) = v1.clone();
            h164
        };
        h163
    };
    let v4: List<Bytes> = {
        let (_, h165) = {
            let (_, h166) = v1;
            h166
        };
        h165
    };
    let m167 = f44(v0.clone(), v2);
    match m167 {
        None => {
            Ok(None::<(u64, (u64, List<Bytes>))>)
        }
        Some(v5) => {
            let a168 = v5;
            let a169 = 10u8;
            if equal(a168, a169) {
                Ok(Some(({
                    let a170 = v2;
                    let a171 = 1u64;
                    nat_add(a170, a171)?
                }, ({
                    let a172 = v2;
                    let a173 = 1u64;
                    nat_add(a172, a173)?
                }, List::cons(f45(f20(v0, v3, {
                    let a174 = v2;
                    let a175 = v3;
                    nat_sub(a174, a175)
                })), v4)))))
            } else {
                Ok(Some(({
                    let a176 = v2;
                    let a177 = 1u64;
                    nat_add(a176, a177)?
                }, (v3, v4))))
            }
        }
    }
}

pub fn f40(v0: Fn8, p181: List<Bytes>, p182: List<Bytes>) -> List<Bytes> {
    let m180 = (p181, p182);
    {
        let mut m180 = m180;
        loop {
            let step = {
                let (v1, v2) = m180;
                let m178 = v2;
                match m178.uncons() {
                    None => {
                        Err::<(List<Bytes>, List<Bytes>), List<Bytes>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(List<Bytes>, List<Bytes>), List<Bytes>>(({
                            let c179: Fn8 = v0.clone();
                            c179.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m180 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f41(v0: List<Bytes>, v1: Bytes) -> List<Bytes> {
    List::cons(v1, v0)
}

pub fn f42(v0: List<Bytes>, v1: Bytes) -> Bytes {
    let m183 = f46(Fn9::F47(v1), None::<Bytes>, v0);
    match m183 {
        None => {
            Bytes::lit(&[])
        }
        Some(v2) => {
            v2
        }
    }
}

pub fn f43(v0: Bytes, v1: Bytes) -> R<Option<u64>> {
    let v2: u64 = {
        let a184 = v0.clone();
        length_bytes(a184)
    };
    let v3: u64 = {
        let a185 = v1.clone();
        length_bytes(a185)
    };
    let v4: u64 = {
        let (h186, _) = f48(Fn10::F49(v1.clone(), v3, v0.clone(), v2), {
            let a187 = v2;
            let a188 = 1u64;
            nat_add(a187, a188)?
        }, 0u64)?;
        h186
    };
    if f50(v0, v1, v4) {
        Ok(Some(v4))
    } else {
        Ok(None::<u64>)
    }
}

pub fn f44(v0: Bytes, v1: u64) -> Option<u8> {
    let a189 = v0;
    let a190 = v1;
    index_bytes(a189, a190)
}

pub fn f45(v0: Bytes) -> Bytes {
    if f51(v0.clone(), Bytes::lit(&[0x0d])) {
        f20(v0.clone(), 0u64, {
            let a192 = {
                let a191 = v0;
                length_bytes(a191)
            };
            let a193 = 1u64;
            nat_sub(a192, a193)
        })
    } else {
        v0
    }
}

pub fn f46(v0: Fn9, p197: Option<Bytes>, p198: List<Bytes>) -> Option<Bytes> {
    let m196 = (p197, p198);
    {
        let mut m196 = m196;
        loop {
            let step = {
                let (v1, v2) = m196;
                let m194 = v2;
                match m194.uncons() {
                    None => {
                        Err::<(Option<Bytes>, List<Bytes>), Option<Bytes>>(v1)
                    }
                    Some((v3, v4)) => {
                        Ok::<(Option<Bytes>, List<Bytes>), Option<Bytes>>(({
                            let c195: Fn9 = v0.clone();
                            c195.apply(v1, v3)
                        }, v4))
                    }
                }
            };
            match step {
                Ok(next) => { m196 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f47(v0: Bytes, v1: Option<Bytes>, v2: Bytes) -> Option<Bytes> {
    let m199 = v1;
    match m199 {
        None => {
            Some(v2)
        }
        Some(v3) => {
            Some({
                let a202 = {
                    let a200 = v3;
                    let a201 = v0;
                    append_bytes(a200, a201)
                };
                let a203 = v2;
                append_bytes(a202, a203)
            })
        }
    }
}

pub fn f48(v0: Fn10, p208: u64, p209: u64) -> R<(u64, bool)> {
    let m207 = (p208, p209);
    {
        let mut m207 = m207;
        loop {
            let step = {
                let (v1, v2) = m207;
                let m204 = v1;
                if m204 == 0 {
                    Err::<(u64, u64), R<(u64, bool)>>(Ok((v2, false)))
                } else {
                    let v3 = m204 - 1;
                    let m206 = {
                        let c205: Fn10 = v0.clone();
                        c205.apply(v2)?
                    };
                    match m206 {
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
                Ok(next) => { m207 = next; }
                Err(done) => break done,
            }
        }
    }
}

pub fn f49(v0: Bytes, v1: u64, v2: Bytes, v3: u64, v4: u64) -> R<Option<u64>> {
    if f50(v2, v0, v4) {
        Ok(None::<u64>)
    } else {
        let a212 = {
            let a210 = v4;
            let a211 = v1;
            nat_add(a210, a211)?
        };
        let a213 = v3;
        if nat_lt(a212, a213) {
            Ok(Some({
                let a214 = v4;
                let a215 = 1u64;
                nat_add(a214, a215)?
            }))
        } else {
            Ok(None::<u64>)
        }
    }
}

pub fn f50(v0: Bytes, v1: Bytes, v2: u64) -> bool {
    let m220 = {
        let a217 = v0;
        let a218 = v2;
        let a219 = {
            let a216 = v1.clone();
            length_bytes(a216)
        };
        slice_bytes(a217, a218, a219)
    };
    match m220 {
        None => {
            false
        }
        Some(v3) => {
            let a221 = v3;
            let a222 = v1;
            equal(a221, a222)
        }
    }
}

pub fn f51(v0: Bytes, v1: Bytes) -> bool {
    let v2: u64 = {
        let a223 = v0.clone();
        length_bytes(a223)
    };
    let v3: u64 = {
        let a224 = v1.clone();
        length_bytes(a224)
    };
    let a225 = v3;
    let a226 = v2;
    if nat_le(a225, a226) {
        let m232 = {
            let a229 = v0;
            let a230 = {
                let a227 = v2;
                let a228 = v3;
                nat_sub(a227, a228)
            };
            let a231 = v3;
            slice_bytes(a229, a230, a231)
        };
        match m232 {
            None => {
                false
            }
            Some(v4) => {
                let a233 = v4;
                let a234 = v1;
                equal(a233, a234)
            }
        }
    } else {
        false
    }
}

pub fn run(p0: Bytes) -> R<Bytes> {
    f0(p0)
}
