//! Platform- and toolchain-stable hashing for golden tests (docs/decisions.md D-007).
//!
//! `core::hash::Hash` + `DefaultHasher` are not guaranteed stable across Rust releases, and
//! `usize` hashes differently on 32/64-bit targets. Instead we feed the value's **serde data
//! model** into FNV-1a-64 through a tiny canonical `Serializer`: every primitive is written in
//! little-endian with a type tag, lengths are `u64`, enum variants are written by index.
//! The hash therefore changes only if the serialized shape or content of the value changes.

use core::fmt;
use serde::ser::{self, Serialize};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64-bit hasher whose state is a plain `u64`, so a running hash can be stored in
/// `GameState` and resumed after a save/load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StableHasher(pub u64);

impl Default for StableHasher {
    fn default() -> Self {
        StableHasher(FNV_OFFSET)
    }
}

impl StableHasher {
    pub fn resume(state: u64) -> Self {
        StableHasher(state)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn tag(&mut self, t: u8) {
        self.write(&[t]);
    }

    fn len(&mut self, n: usize) {
        self.write(&(n as u64).to_le_bytes());
    }

    pub fn finish(self) -> u64 {
        self.0
    }

    /// Feed `value`'s canonical serde form into the hasher.
    pub fn update<T: Serialize + ?Sized>(&mut self, value: &T) {
        value
            .serialize(&mut *self)
            .expect("stable hashing is infallible");
    }
}

/// Hash any serializable value from the FNV offset basis.
pub fn stable_hash<T: Serialize + ?Sized>(value: &T) -> u64 {
    let mut h = StableHasher::default();
    h.update(value);
    h.finish()
}

#[derive(Debug)]
pub struct HashError;

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("stable hash error")
    }
}

impl core::error::Error for HashError {}

impl ser::Error for HashError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        HashError
    }
}

type R = Result<(), HashError>;

macro_rules! prim {
    ($fn:ident, $t:ty, $tag:expr) => {
        fn $fn(self, v: $t) -> R {
            self.tag($tag);
            self.write(&v.to_le_bytes());
            Ok(())
        }
    };
}

impl ser::Serializer for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> R {
        self.tag(1);
        self.write(&[u8::from(v)]);
        Ok(())
    }
    prim!(serialize_i8, i8, 2);
    prim!(serialize_i16, i16, 3);
    prim!(serialize_i32, i32, 4);
    prim!(serialize_i64, i64, 5);
    prim!(serialize_i128, i128, 6);
    prim!(serialize_u8, u8, 7);
    prim!(serialize_u16, u16, 8);
    prim!(serialize_u32, u32, 9);
    prim!(serialize_u64, u64, 10);
    prim!(serialize_u128, u128, 11);
    fn serialize_f32(self, v: f32) -> R {
        self.tag(12);
        self.write(&v.to_bits().to_le_bytes());
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> R {
        self.tag(13);
        self.write(&v.to_bits().to_le_bytes());
        Ok(())
    }
    fn serialize_char(self, v: char) -> R {
        self.tag(14);
        self.write(&u32::from(v).to_le_bytes());
        Ok(())
    }
    fn serialize_str(self, v: &str) -> R {
        self.tag(15);
        self.len(v.len());
        self.write(v.as_bytes());
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> R {
        self.tag(16);
        self.len(v.len());
        self.write(v);
        Ok(())
    }
    fn serialize_none(self) -> R {
        self.tag(17);
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> R {
        self.tag(18);
        value.serialize(self)
    }
    fn serialize_unit(self) -> R {
        self.tag(19);
        Ok(())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> R {
        self.tag(20);
        Ok(())
    }
    fn serialize_unit_variant(self, _name: &'static str, idx: u32, _variant: &'static str) -> R {
        self.tag(21);
        self.write(&idx.to_le_bytes());
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _name: &'static str, value: &T) -> R {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        idx: u32,
        _variant: &'static str,
        value: &T,
    ) -> R {
        self.tag(22);
        self.write(&idx.to_le_bytes());
        value.serialize(self)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self, HashError> {
        match len {
            Some(n) => {
                self.tag(23);
                self.len(n);
            }
            None => self.tag(24),
        }
        Ok(self)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self, HashError> {
        self.tag(25);
        Ok(self)
    }
    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self, HashError> {
        self.tag(26);
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, HashError> {
        self.tag(27);
        self.write(&idx.to_le_bytes());
        Ok(self)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self, HashError> {
        match len {
            Some(n) => {
                self.tag(28);
                self.len(n);
            }
            None => self.tag(29),
        }
        Ok(self)
    }
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self, HashError> {
        self.tag(30);
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, HashError> {
        self.tag(31);
        self.write(&idx.to_le_bytes());
        Ok(self)
    }
}

impl ser::SerializeSeq for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.tag(0xFF);
        Ok(())
    }
}

impl ser::SerializeTuple for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeMap for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, k: &T) -> R {
        k.serialize(&mut **self)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.tag(0xFE);
        Ok(())
    }
}

impl ser::SerializeStruct for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, _key: &'static str, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn skip_field(&mut self, _key: &'static str) -> R {
        self.tag(0xFD);
        Ok(())
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut StableHasher {
    type Ok = ();
    type Error = HashError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, _key: &'static str, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn skip_field(&mut self, _key: &'static str) -> R {
        self.tag(0xFD);
        Ok(())
    }
    fn end(self) -> R {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::vec;

    #[test]
    fn fnv_known_vector() {
        // FNV-1a 64 of "a" is 0xaf63dc4c8601ec8c (reference vector).
        let mut h = StableHasher::default();
        h.write(b"a");
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn distinguishes_structure() {
        assert_ne!(stable_hash(&(1u32, 2u32)), stable_hash(&(2u32, 1u32)));
        assert_ne!(stable_hash(&vec![1u8, 2]), stable_hash(&vec![1u8, 2, 0]));
        assert_ne!(stable_hash(&Some(0u8)), stable_hash(&None::<u8>));
        let mut m = BTreeMap::new();
        m.insert(1u32, 2u32);
        assert_eq!(stable_hash(&m), stable_hash(&m.clone()));
    }

    #[test]
    fn usize_hashes_as_u64() {
        assert_eq!(stable_hash(&7usize), stable_hash(&7u64));
    }
}
