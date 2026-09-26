// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A lossless journal line for an envelope that carries a non-finite float (GAP-126,
//! decision D-77).
//!
//! # Why this exists
//!
//! JSON has no spelling for NaN or an infinity, and `serde_json` writes both as `null`,
//! which reads back as an error for an `f64` and as `None` for an `Option<f64>`. A journal
//! written that way held lines it could not read back: mid-session, one such line made the
//! whole session unreadable; as the last line, the torn-tail tolerance dropped the envelope
//! with only a log line to say so. Real producers can put a non-finite value in an envelope
//! (a diverged filter's covariance, a ratio over a zero, a derived range), so refusing the
//! envelope would lose the very record an investigation of that fault needs. The journal
//! carries it instead, bit for bit.
//!
//! # The encoding
//!
//! **Only an envelope that carries a non-finite value is written differently.** Every
//! other envelope is written exactly as before, byte for byte, so existing journals, the
//! gated round-trip row and every finite line stay what they were. An envelope that does
//! carry one is written as [`MARKER`] followed by JSON in which:
//!
//! - a non-finite `f64` is the string `"\u0000f64:"` and its sixteen hex digits of bits,
//!   and a non-finite `f32` is `"\u0000f32:"` and eight; the bits, not a name, so NaN's
//!   sign and payload survive with it;
//! - a genuine string (a value, a map key or a `char`) that begins with U+0000 gains one
//!   more U+0000, so no string can be taken for a float;
//! - everything else is what `serde_json` writes.
//!
//! The escape applies inside a marked line only. A line without the marker is plain JSON
//! and is read exactly as it always was, so no line written before this existed can be
//! misread by it.
//!
//! # Why an adapter and not a field attribute
//!
//! Several hundred float fields reach the journal across fifteen event families, and some
//! sit inside internally tagged enums, which serde buffers before the field's own type
//! sees them. An attribute on each field would be one forgotten field away from the old
//! defect. The adapters below wrap `serde_json`'s serializer and deserializer, so every
//! float in every type is covered by construction, including inside buffered content: the
//! deserializer turns a float token into a float **before** serde buffers it.

use serde::de::{self, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, Visitor};
use serde::ser::{self, Serialize, Serializer};
use std::cell::Cell;
use std::fmt;

/// The first character of a line whose JSON uses the escape above. A plaintext envelope
/// line starts with `{` and a sealed one with `#` (`sealing.rs`), so the three never meet.
pub const MARKER: char = '~';

/// The prefix of every escaped string: a float token, or a genuine string that began
/// with it.
const ESCAPE: char = '\0';
const F64_TOKEN: &str = "\0f64:";
const F32_TOKEN: &str = "\0f32:";

/// What the serializer does when it meets a non-finite float.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Write plain JSON; stop at the first non-finite float and say so.
    Detect,
    /// Write the escaped form.
    Escape,
}

/// Shared by every level of one serialization.
struct Context {
    mode: Mode,
    found: Cell<bool>,
}

/// Encode an envelope (or any value) as one journal line body.
///
/// Returns the plain JSON when every float is finite, which is byte-identical to
/// `serde_json::to_string`, and [`MARKER`] followed by the escaped JSON when one is not.
///
/// # Errors
///
/// Whatever `serde_json` refuses for a reason other than a non-finite float.
pub fn to_line<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    let detect = Context {
        mode: Mode::Detect,
        found: Cell::new(false),
    };
    match write(value, &detect) {
        Ok(line) => Ok(line),
        Err(_) if detect.found.get() => {
            let escape = Context {
                mode: Mode::Escape,
                found: Cell::new(false),
            };
            let body = write(value, &escape)?;
            let mut line = String::with_capacity(body.len() + 1);
            line.push(MARKER);
            line.push_str(&body);
            Ok(line)
        }
        Err(err) => Err(err),
    }
}

fn write<T: Serialize + ?Sized>(value: &T, ctx: &Context) -> Result<String, serde_json::Error> {
    let mut buf = Vec::with_capacity(256);
    let mut ser = serde_json::Serializer::new(&mut buf);
    value.serialize(Esc {
        inner: &mut ser,
        ctx,
    })?;
    // `serde_json` writes UTF-8 only; a failure here would be its defect, reported as an
    // encoding error rather than trusted.
    String::from_utf8(buf).map_err(|e| ser::Error::custom(e.to_string()))
}

/// Decode one journal line body, marked or not.
///
/// # Errors
///
/// When the JSON does not describe a `T`, or a marked line holds a malformed float token.
pub fn from_line<T: de::DeserializeOwned>(line: &str) -> Result<T, serde_json::Error> {
    let Some(body) = line.strip_prefix(MARKER) else {
        return serde_json::from_str(line);
    };
    let mut de = serde_json::Deserializer::from_str(body);
    let value = T::deserialize(Unesc(&mut de))?;
    de.end()?;
    Ok(value)
}

// ---------------------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------------------

struct Esc<'c, S> {
    inner: S,
    ctx: &'c Context,
}

/// A value to be serialized through [`Esc`].
struct Wrap<'a, 'c, T: ?Sized> {
    value: &'a T,
    ctx: &'c Context,
}

impl<T: Serialize + ?Sized> Serialize for Wrap<'_, '_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(Esc {
            inner: serializer,
            ctx: self.ctx,
        })
    }
}

impl<'c, S> Esc<'c, S> {
    fn wrap<'a, T: ?Sized>(&self, value: &'a T) -> Wrap<'a, 'c, T> {
        Wrap {
            value,
            ctx: self.ctx,
        }
    }
}

impl<S: Serializer> Esc<'_, S> {
    /// A non-finite float: stop (detect) or write its token (escape).
    fn non_finite(self, token: &str) -> Result<S::Ok, S::Error> {
        match self.ctx.mode {
            Mode::Detect => {
                self.ctx.found.set(true);
                Err(ser::Error::custom(
                    "a non-finite float needs the escaped line",
                ))
            }
            Mode::Escape => self.inner.serialize_str(token),
        }
    }
}

/// A genuine string, escaped when it could be taken for a float token.
fn escaped(v: &str, mode: Mode) -> Option<String> {
    (mode == Mode::Escape && v.starts_with(ESCAPE)).then(|| format!("{ESCAPE}{v}"))
}

impl<'c, S: Serializer> Serializer for Esc<'c, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Esc<'c, S::SerializeSeq>;
    type SerializeTuple = Esc<'c, S::SerializeTuple>;
    type SerializeTupleStruct = Esc<'c, S::SerializeTupleStruct>;
    type SerializeTupleVariant = Esc<'c, S::SerializeTupleVariant>;
    type SerializeMap = Esc<'c, S::SerializeMap>;
    type SerializeStruct = Esc<'c, S::SerializeStruct>;
    type SerializeStructVariant = Esc<'c, S::SerializeStructVariant>;

    fn serialize_f64(self, v: f64) -> Result<S::Ok, S::Error> {
        if v.is_finite() {
            self.inner.serialize_f64(v)
        } else {
            self.non_finite(&format!("{F64_TOKEN}{:016x}", v.to_bits()))
        }
    }

    fn serialize_f32(self, v: f32) -> Result<S::Ok, S::Error> {
        if v.is_finite() {
            self.inner.serialize_f32(v)
        } else {
            self.non_finite(&format!("{F32_TOKEN}{:08x}", v.to_bits()))
        }
    }

    fn serialize_str(self, v: &str) -> Result<S::Ok, S::Error> {
        match escaped(v, self.ctx.mode) {
            Some(e) => self.inner.serialize_str(&e),
            None => self.inner.serialize_str(v),
        }
    }

    fn serialize_char(self, v: char) -> Result<S::Ok, S::Error> {
        if self.ctx.mode == Mode::Escape && v == ESCAPE {
            self.inner.serialize_str("\0\0")
        } else {
            self.inner.serialize_char(v)
        }
    }

    fn serialize_bool(self, v: bool) -> Result<S::Ok, S::Error> {
        self.inner.serialize_bool(v)
    }
    fn serialize_i8(self, v: i8) -> Result<S::Ok, S::Error> {
        self.inner.serialize_i8(v)
    }
    fn serialize_i16(self, v: i16) -> Result<S::Ok, S::Error> {
        self.inner.serialize_i16(v)
    }
    fn serialize_i32(self, v: i32) -> Result<S::Ok, S::Error> {
        self.inner.serialize_i32(v)
    }
    fn serialize_i64(self, v: i64) -> Result<S::Ok, S::Error> {
        self.inner.serialize_i64(v)
    }
    fn serialize_i128(self, v: i128) -> Result<S::Ok, S::Error> {
        self.inner.serialize_i128(v)
    }
    fn serialize_u8(self, v: u8) -> Result<S::Ok, S::Error> {
        self.inner.serialize_u8(v)
    }
    fn serialize_u16(self, v: u16) -> Result<S::Ok, S::Error> {
        self.inner.serialize_u16(v)
    }
    fn serialize_u32(self, v: u32) -> Result<S::Ok, S::Error> {
        self.inner.serialize_u32(v)
    }
    fn serialize_u64(self, v: u64) -> Result<S::Ok, S::Error> {
        self.inner.serialize_u64(v)
    }
    fn serialize_u128(self, v: u128) -> Result<S::Ok, S::Error> {
        self.inner.serialize_u128(v)
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<S::Ok, S::Error> {
        self.inner.serialize_bytes(v)
    }
    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        self.inner.serialize_none()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<S::Ok, S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_some(&wrapped)
    }
    fn serialize_unit(self) -> Result<S::Ok, S::Error> {
        self.inner.serialize_unit()
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<S::Ok, S::Error> {
        self.inner.serialize_unit_struct(name)
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<S::Ok, S::Error> {
        self.inner.serialize_unit_variant(name, index, variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_newtype_struct(name, &wrapped)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        let wrapped = self.wrap(value);
        self.inner
            .serialize_newtype_variant(name, index, variant, &wrapped)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        let ctx = self.ctx;
        Ok(Esc {
            inner: self.inner.serialize_seq(len)?,
            ctx,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, S::Error> {
        let ctx = self.ctx;
        Ok(Esc {
            inner: self.inner.serialize_tuple(len)?,
            ctx,
        })
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, S::Error> {
        let ctx = self.ctx;
        Ok(Esc {
            inner: self.inner.serialize_tuple_struct(name, len)?,
            ctx,
        })
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, S::Error> {
        let ctx = self.ctx;
        Ok(Esc {
            inner: self
                .inner
                .serialize_tuple_variant(name, index, variant, len)?,
            ctx,
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        let ctx = self.ctx;
        Ok(Esc {
            inner: self.inner.serialize_map(len)?,
            ctx,
        })
    }
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, S::Error> {
        let ctx = self.ctx;
        Ok(Esc {
            inner: self.inner.serialize_struct(name, len)?,
            ctx,
        })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        let ctx = self.ctx;
        Ok(Esc {
            inner: self
                .inner
                .serialize_struct_variant(name, index, variant, len)?,
            ctx,
        })
    }
    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

impl<S: ser::SerializeSeq> ser::SerializeSeq for Esc<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_element(&wrapped)
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.inner.end()
    }
}

impl<S: ser::SerializeTuple> ser::SerializeTuple for Esc<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_element(&wrapped)
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.inner.end()
    }
}

impl<S: ser::SerializeTupleStruct> ser::SerializeTupleStruct for Esc<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(&wrapped)
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.inner.end()
    }
}

impl<S: ser::SerializeTupleVariant> ser::SerializeTupleVariant for Esc<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(&wrapped)
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.inner.end()
    }
}

impl<S: ser::SerializeMap> ser::SerializeMap for Esc<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), S::Error> {
        let wrapped = self.wrap(key);
        self.inner.serialize_key(&wrapped)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_value(&wrapped)
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.inner.end()
    }
}

impl<S: ser::SerializeStruct> ser::SerializeStruct for Esc<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(key, &wrapped)
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), S::Error> {
        self.inner.skip_field(key)
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.inner.end()
    }
}

impl<S: ser::SerializeStructVariant> ser::SerializeStructVariant for Esc<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), S::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(key, &wrapped)
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), S::Error> {
        self.inner.skip_field(key)
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.inner.end()
    }
}

// ---------------------------------------------------------------------------------------
// Deserialization
// ---------------------------------------------------------------------------------------

/// A deserializer that reads the escaped form: every visitor it hands a value to is
/// wrapped, so a float token becomes a float and an escaped string loses its escape at
/// whatever depth it sits.
struct Unesc<D>(D);

/// What a string in a marked line stands for.
enum Token<'s> {
    /// A genuine string, unescaped.
    Text(&'s str),
    F64(f64),
    F32(f32),
}

fn token<E: de::Error>(v: &str) -> Result<Token<'_>, E> {
    if !v.starts_with(ESCAPE) {
        return Ok(Token::Text(v));
    }
    if let Some(rest) = v.strip_prefix(ESCAPE).filter(|r| r.starts_with(ESCAPE)) {
        return Ok(Token::Text(rest));
    }
    if let Some(hex) = v.strip_prefix(F64_TOKEN) {
        if hex.len() == 16 {
            if let Ok(bits) = u64::from_str_radix(hex, 16) {
                return Ok(Token::F64(f64::from_bits(bits)));
            }
        }
    }
    if let Some(hex) = v.strip_prefix(F32_TOKEN) {
        if hex.len() == 8 {
            if let Ok(bits) = u32::from_str_radix(hex, 16) {
                return Ok(Token::F32(f32::from_bits(bits)));
            }
        }
    }
    Err(E::custom(format!(
        "a journal line holds a malformed escaped token {v:?}"
    )))
}

struct UnescVisitor<V>(V);

struct UnescSeed<S>(S);

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for UnescSeed<S> {
    type Value = S::Value;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S::Value, D::Error> {
        self.0.deserialize(Unesc(deserializer))
    }
}

macro_rules! forward_with_visitor {
    ($($method:ident),* $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
                self.0.$method(UnescVisitor(visitor))
            }
        )*
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Unesc<D> {
    type Error = D::Error;

    forward_with_visitor!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_i128,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_u128,
        deserialize_char,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_seq,
        deserialize_map,
        deserialize_identifier,
        deserialize_ignored_any,
    );

    /// A float is a number or a float token, so the inner deserializer is asked for
    /// whichever it holds rather than for a number it may not have.
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_any(UnescVisitor(visitor))
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_any(UnescVisitor(visitor))
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_unit_struct(name, UnescVisitor(visitor))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_newtype_struct(name, UnescVisitor(visitor))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_tuple(len, UnescVisitor(visitor))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_tuple_struct(name, len, UnescVisitor(visitor))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_struct(name, fields, UnescVisitor(visitor))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0
            .deserialize_enum(name, variants, UnescVisitor(visitor))
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for UnescVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(f)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<V::Value, E> {
        match token(v)? {
            Token::Text(text) => self.0.visit_str(text),
            Token::F64(x) => self.0.visit_f64(x),
            Token::F32(x) => self.0.visit_f32(x),
        }
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<V::Value, E> {
        match token(v)? {
            Token::Text(text) => self.0.visit_borrowed_str(text),
            Token::F64(x) => self.0.visit_f64(x),
            Token::F32(x) => self.0.visit_f32(x),
        }
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<V::Value, E> {
        match token(&v)? {
            Token::Text(text) if text.len() == v.len() => self.0.visit_string(v),
            Token::Text(text) => self.0.visit_string(text.to_owned()),
            Token::F64(x) => self.0.visit_f64(x),
            Token::F32(x) => self.0.visit_f32(x),
        }
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<V::Value, E> {
        self.0.visit_bool(v)
    }
    fn visit_i8<E: de::Error>(self, v: i8) -> Result<V::Value, E> {
        self.0.visit_i8(v)
    }
    fn visit_i16<E: de::Error>(self, v: i16) -> Result<V::Value, E> {
        self.0.visit_i16(v)
    }
    fn visit_i32<E: de::Error>(self, v: i32) -> Result<V::Value, E> {
        self.0.visit_i32(v)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<V::Value, E> {
        self.0.visit_i64(v)
    }
    fn visit_i128<E: de::Error>(self, v: i128) -> Result<V::Value, E> {
        self.0.visit_i128(v)
    }
    fn visit_u8<E: de::Error>(self, v: u8) -> Result<V::Value, E> {
        self.0.visit_u8(v)
    }
    fn visit_u16<E: de::Error>(self, v: u16) -> Result<V::Value, E> {
        self.0.visit_u16(v)
    }
    fn visit_u32<E: de::Error>(self, v: u32) -> Result<V::Value, E> {
        self.0.visit_u32(v)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<V::Value, E> {
        self.0.visit_u64(v)
    }
    fn visit_u128<E: de::Error>(self, v: u128) -> Result<V::Value, E> {
        self.0.visit_u128(v)
    }
    fn visit_f32<E: de::Error>(self, v: f32) -> Result<V::Value, E> {
        self.0.visit_f32(v)
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<V::Value, E> {
        self.0.visit_f64(v)
    }
    fn visit_char<E: de::Error>(self, v: char) -> Result<V::Value, E> {
        self.0.visit_char(v)
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<V::Value, E> {
        self.0.visit_bytes(v)
    }
    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<V::Value, E> {
        self.0.visit_borrowed_bytes(v)
    }
    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<V::Value, E> {
        self.0.visit_byte_buf(v)
    }
    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_none()
    }
    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        self.0.visit_some(Unesc(deserializer))
    }
    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_unit()
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<V::Value, D::Error> {
        self.0.visit_newtype_struct(Unesc(deserializer))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<V::Value, A::Error> {
        self.0.visit_seq(Unesc(seq))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.0.visit_map(Unesc(map))
    }
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.0.visit_enum(Unesc(data))
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Unesc<A> {
    type Error = A::Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, A::Error> {
        self.0.next_element_seed(UnescSeed(seed))
    }
    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Unesc<A> {
    type Error = A::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.0.next_key_seed(UnescSeed(seed))
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, A::Error> {
        self.0.next_value_seed(UnescSeed(seed))
    }
    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Unesc<A> {
    type Error = A::Error;
    type Variant = Unesc<A::Variant>;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), A::Error> {
        let (value, variant) = self.0.variant_seed(UnescSeed(seed))?;
        Ok((value, Unesc(variant)))
    }
}

impl<'de, A: de::VariantAccess<'de>> de::VariantAccess<'de> for Unesc<A> {
    type Error = A::Error;
    fn unit_variant(self) -> Result<(), A::Error> {
        self.0.unit_variant()
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, A::Error> {
        self.0.newtype_variant_seed(UnescSeed(seed))
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.0.tuple_variant(len, UnescVisitor(visitor))
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.0.struct_variant(fields, UnescVisitor(visitor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Plain {
        x: f64,
        y: Option<f64>,
        s: String,
        c: char,
        m: BTreeMap<String, f32>,
    }

    /// Internally tagged, which serde buffers before the float's own type sees it.
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "kebab-case")]
    enum Tagged {
        Range { metres: f64, tag: String },
        Empty,
    }

    fn bits(values: &[f64]) -> Vec<u64> {
        values.iter().map(|v| v.to_bits()).collect()
    }

    #[test]
    fn a_finite_value_is_written_exactly_as_serde_json_writes_it() {
        let value = Plain {
            x: 0.1 + 0.2,
            y: None,
            s: "\0not a token".into(),
            c: '\0',
            m: BTreeMap::from([("\0k".to_string(), 1.5_f32)]),
        };
        let line = to_line(&value).expect("encodes");
        assert_eq!(line, serde_json::to_string(&value).expect("serde_json"));
        assert_eq!(from_line::<Plain>(&line).expect("decodes"), value);
    }

    #[test]
    fn non_finite_values_come_back_bit_for_bit_and_strings_are_not_mistaken_for_them() {
        let negative_quiet_nan_with_payload = f64::from_bits(0xfff8_0000_0000_beef);
        let value = Plain {
            x: negative_quiet_nan_with_payload,
            y: Some(f64::NEG_INFINITY),
            s: "\0f64:7ff0000000000000".into(),
            c: '\0',
            m: BTreeMap::from([
                ("\0f32:7fc00000".to_string(), f32::INFINITY),
                ("plain".to_string(), f32::from_bits(0x7fc0_0001)),
            ]),
        };
        let line = to_line(&value).expect("encodes");
        assert!(line.starts_with(MARKER), "{line}");
        let back: Plain = from_line(&line).expect("decodes");
        assert_eq!(bits(&[back.x]), bits(&[value.x]));
        assert_eq!(back.y.map(f64::to_bits), value.y.map(f64::to_bits));
        assert_eq!(
            back.s, value.s,
            "a string that looks like a token stays a string"
        );
        assert_eq!(back.c, '\0');
        let keys: Vec<_> = back.m.keys().cloned().collect();
        assert_eq!(
            keys,
            vec!["\0f32:7fc00000".to_string(), "plain".to_string()]
        );
        let got: Vec<u32> = back.m.values().map(|v| v.to_bits()).collect();
        let want: Vec<u32> = value.m.values().map(|v| v.to_bits()).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn a_non_finite_value_inside_an_internally_tagged_enum_comes_back() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let value = vec![
                Tagged::Range {
                    metres: v,
                    tag: "\0".into(),
                },
                Tagged::Empty,
            ];
            let line = to_line(&value).expect("encodes");
            let back: Vec<Tagged> = from_line(&line).expect("decodes");
            match &back[0] {
                Tagged::Range { metres, tag } => {
                    assert_eq!(metres.to_bits(), v.to_bits());
                    assert_eq!(tag, "\0");
                }
                Tagged::Empty => panic!("the variant changed"),
            }
            assert_eq!(back[1], Tagged::Empty);
        }
    }

    #[test]
    fn a_malformed_token_is_an_error_not_a_guess() {
        let err = from_line::<Plain>(
            "~{\"x\":\"\\u0000f64:zz\",\"y\":null,\"s\":\"\",\"c\":\"a\",\"m\":{}}",
        )
        .expect_err("refused");
        assert!(err.to_string().contains("malformed"), "{err}");
    }
}
