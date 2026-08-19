use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use serde::de::value::{
    MapAccessDeserializer, MapDeserializer, SeqDeserializer,
};
use serde::de::{Deserialize as _, VariantAccess as _};

use num_traits::ToPrimitive;
use sessiond_dhall::syntax::NumKind;

use crate::function::FUNCTION_TOKEN;
use crate::value::SimpleValue;
use crate::{Error, ErrorKind, Function, Value};

pub trait Sealed {}

/// A data structure that can be deserialized from a Dhall expression.
///
/// This is automatically implemented for any type that [serde] can deserialize.
/// In fact, this trait cannot be implemented manually. To implement it for your type,
/// use serde's derive mechanism.
///
/// # Example
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use serde::Deserialize;
///
/// // Use serde's derive
/// #[derive(Deserialize)]
/// struct Point {
///     x: u64,
///     y: u64,
/// }
///
/// // Convert a Dhall string to a Point.
/// let point: Point = sessiond_serde_dhall::from_str("{ x = 1, y = 1 + 1 }").parse()?;
/// # Ok(())
/// # }
/// ```
///
/// [serde]: https://serde.rs
pub trait FromDhall: Sealed + Sized {
    #[doc(hidden)]
    fn from_dhall(v: &Value) -> crate::Result<Self>;
}

impl<T> Sealed for T where T: serde::de::DeserializeOwned {}

/// Deserialize a Rust value from a Dhall [`SimpleValue`].
///
/// # Example
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use std::collections::BTreeMap;
/// use serde::Deserialize;
///
/// // We use serde's derive feature
/// #[derive(Deserialize)]
/// struct Point {
///     x: u64,
///     y: u64,
/// }
///
/// // Some Dhall data
/// let mut data = BTreeMap::new();
/// data.insert(
///     "x".to_string(),
///     sessiond_serde_dhall::SimpleValue::Num(sessiond_serde_dhall::NumKind::Natural(1u32.into()))
/// );
/// data.insert(
///     "y".to_string(),
///     sessiond_serde_dhall::SimpleValue::Num(sessiond_serde_dhall::NumKind::Natural(2u32.into()))
/// );
/// let data = sessiond_serde_dhall::SimpleValue::Record(data);
///
/// // Parse the Dhall value as a Point.
/// let point: Point = sessiond_serde_dhall::from_simple_value(data)?;
///
/// assert_eq!(point.x, 1);
/// assert_eq!(point.y, 2);
/// # Ok(())
/// # }
/// ```
///
pub fn from_simple_value<T>(v: SimpleValue) -> crate::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    T::deserialize(Deserializer(Cow::Owned(v)))
}

impl<T> FromDhall for T
where
    T: serde::de::DeserializeOwned,
{
    fn from_dhall(v: &Value) -> crate::Result<Self> {
        let sval = v.to_simple_value().ok_or_else(|| {
            Error(ErrorKind::Deserialize(format!(
                "this cannot be deserialized into the serde data model: {}",
                v
            )))
        })?;
        from_simple_value(sval)
    }
}

struct Deserializer<'a>(Cow<'a, SimpleValue>);

impl<'de: 'a, 'a> serde::de::IntoDeserializer<'de, Error> for Deserializer<'a> {
    type Deserializer = Deserializer<'a>;
    fn into_deserializer(self) -> Self::Deserializer {
        self
    }
}

impl<'de: 'a, 'a> serde::Deserializer<'de> for Deserializer<'a> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> crate::Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        use NumKind::*;
        use SimpleValue::*;

        // A function has no counterpart in the serde data model, so we hand it to the visitor as a
        // newtype struct. `Function` and `SimpleValue` know how to pick it up from there; any
        // other type will refuse it.
        if matches!(self.0.as_ref(), Function(_)) {
            return visitor.visit_newtype_struct(self).map_err(|_| {
                Error(ErrorKind::Deserialize(
                    "cannot deserialize a Dhall function into this type; \
                     deserialize it into a `sessiond_serde_dhall::Function` instead"
                        .to_string(),
                ))
            });
        }

        let val = |x| Deserializer(Cow::Borrowed(x));
        match self.0.as_ref() {
            Num(Bool(x)) => visitor.visit_bool(*x),
            // Dhall's Natural and Integer are unbounded, but serde's data model
            // stops at 128 bits. Refuse rather than truncate: a silently wrong
            // number is worse than a failed parse.
            Num(Natural(x)) => match (x.to_u64(), x.to_u128()) {
                (Some(x), _) => visitor.visit_u64(x),
                (None, Some(x)) => visitor.visit_u128(x),
                (None, None) => Err(Error(ErrorKind::Deserialize(format!(
                    "Natural {} is too large for Rust's integer types",
                    x
                )))),
            },
            Num(Integer(x)) => match (x.to_i64(), x.to_i128()) {
                (Some(x), _) => visitor.visit_i64(x),
                (None, Some(x)) => visitor.visit_i128(x),
                (None, None) => Err(Error(ErrorKind::Deserialize(format!(
                    "Integer {} is out of range for Rust's integer types",
                    x
                )))),
            },
            Num(Double(x)) => visitor.visit_f64((*x).into()),
            Text(x) => visitor.visit_str(x),
            List(xs) => {
                visitor.visit_seq(SeqDeserializer::new(xs.iter().map(val)))
            }
            Optional(None) => visitor.visit_none(),
            Optional(Some(x)) => visitor.visit_some(val(x)),
            Record(m) => visitor.visit_map(MapDeserializer::new(
                m.iter().map(|(k, v)| (k.as_str(), val(v))),
            )),
            Union(field_name, Some(x)) => visitor.visit_enum(
                MapAccessDeserializer::new(MapDeserializer::new(
                    Some((field_name.as_str(), val(x))).into_iter(),
                )),
            ),
            Union(field_name, None) => visitor.visit_enum(
                MapAccessDeserializer::new(MapDeserializer::new(
                    Some((field_name.as_str(), ())).into_iter(),
                )),
            ),
            Function(_) => unreachable!("handled above"),
        }
    }

    fn deserialize_newtype_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> crate::Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        match self.0.as_ref() {
            SimpleValue::Function(f) if name == FUNCTION_TOKEN => {
                visitor.visit_byte_buf(f.to_binary()?)
            }
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_tuple<V>(
        self,
        _: usize,
        visitor: V,
    ) -> crate::Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let val = |x| Deserializer(Cow::Borrowed(x));
        match self.0.as_ref() {
            // Blindly takes keys in sorted order.
            SimpleValue::Record(m) => visitor
                .visit_seq(SeqDeserializer::new(m.iter().map(|(_, v)| val(v)))),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> crate::Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        match self.0.as_ref() {
            SimpleValue::Record(m) if m.is_empty() => visitor.visit_unit(),
            _ => self.deserialize_any(visitor),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit_struct seq
        tuple_struct map struct enum identifier ignored_any
    }
}

struct SimpleValueVisitor;

impl<'de> serde::de::Visitor<'de> for SimpleValueVisitor {
    type Value = SimpleValue;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any valid Dhall value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Num(NumKind::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Num(NumKind::Integer(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Num(NumKind::Natural(value.into())))
    }

    fn visit_i128<E>(self, value: i128) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Num(NumKind::Integer(value.into())))
    }

    fn visit_u128<E>(self, value: u128) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Num(NumKind::Natural(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Num(NumKind::Double(value.into())))
    }

    fn visit_str<E>(self, value: &str) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Text(String::from(value)))
    }

    fn visit_string<E>(self, value: String) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Text(value))
    }

    fn visit_none<E>(self) -> Result<SimpleValue, E> {
        Ok(SimpleValue::Optional(None))
    }

    fn visit_some<D>(self, val: D) -> Result<SimpleValue, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = val.deserialize_any(SimpleValueVisitor)?;
        Ok(SimpleValue::Optional(Some(Box::new(val))))
    }

    fn visit_newtype_struct<D>(self, d: D) -> Result<SimpleValue, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // The only newtype struct we ever get here is the one our own `Deserializer` uses to pass
        // functions along.
        Function::deserialize(d).map(SimpleValue::Function)
    }

    fn visit_enum<V>(self, visitor: V) -> Result<SimpleValue, V::Error>
    where
        V: serde::de::EnumAccess<'de>,
    {
        let (name, variant): (String, _) = visitor.variant()?;
        // Serde does not allow me to check what kind of variant it is. This will work for dhall
        // values, because there are only two possible kinds of cvariants, but doesn't work in
        // general. Given that the `serde_value` crate ignores enums, I assume this is not fixable
        // :(.
        let val = variant.newtype_variant().ok();
        Ok(SimpleValue::Union(name, val))
    }

    fn visit_seq<V>(self, mut visitor: V) -> Result<SimpleValue, V::Error>
    where
        V: serde::de::SeqAccess<'de>,
    {
        let mut vec = Vec::new();
        while let Some(elem) = visitor.next_element()? {
            vec.push(elem);
        }
        Ok(SimpleValue::List(vec))
    }

    fn visit_map<V>(self, mut visitor: V) -> Result<SimpleValue, V::Error>
    where
        V: serde::de::MapAccess<'de>,
    {
        let mut record = BTreeMap::default();
        while let Some((key, value)) = visitor.next_entry()? {
            record.insert(key, value);
        }
        Ok(SimpleValue::Record(record))
    }
}

impl<'de> serde::de::Deserialize<'de> for SimpleValue {
    fn deserialize<D>(deserializer: D) -> Result<SimpleValue, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SimpleValueVisitor)
    }
}
