use std::collections::{BTreeMap, HashMap};
use std::result::Result as StdResult;

use sessiond_dhall::builtins::Builtin;
use sessiond_dhall::operations::OpKind;
use sessiond_dhall::semantics::{Hir, HirKind, Nir, NirKind};
pub use sessiond_dhall::syntax::NumKind;
use sessiond_dhall::syntax::{Expr, ExprKind, Span};
use sessiond_dhall::Ctxt;

use crate::{Error, ErrorKind, FromDhall, Function, Result, ToDhall};

#[derive(Debug, Clone)]
enum ValueKind {
    /// Invariant: the value must be printable with the given type.
    Val(SimpleValue, Option<SimpleType>),
    Ty(SimpleType),
}

#[doc(hidden)]
/// An arbitrary Dhall value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    kind: ValueKind,
}

/// A value of the kind that can be decoded by `sessiond_serde_dhall`, e.g. `{ x = True, y = [1, 2, 3] }`.
/// This can be obtained with [`from_str()`] or [`from_file()`].
/// It can also be deserialized into Rust types with [`from_simple_value()`].
///
/// # Examples
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use std::collections::BTreeMap;
/// use serde::Deserialize;
/// use sessiond_serde_dhall::{from_simple_value, NumKind, SimpleValue};
///
/// #[derive(Debug, PartialEq, Eq, Deserialize)]
/// struct Foo {
///     x: bool,
///     y: Vec<u64>,
/// }
///
/// let value: SimpleValue =
///     sessiond_serde_dhall::from_str("{ x = True, y = [1, 2, 3] }").parse()?;
///
/// assert_eq!(
///     value,
///     SimpleValue::Record({
///         let mut r = BTreeMap::new();
///         r.insert(
///             "x".to_string(),
///             SimpleValue::Num(NumKind::Bool(true))
///         );
///         r.insert(
///             "y".to_string(),
///             SimpleValue::List(vec![
///                 SimpleValue::Num(NumKind::Natural(1u32.into())),
///                 SimpleValue::Num(NumKind::Natural(2u32.into())),
///                 SimpleValue::Num(NumKind::Natural(3u32.into())),
///             ])
///         );
///         r
///     })
/// );
///
/// let foo: Foo = from_simple_value(value)?;
///
/// assert_eq!(
///     foo,
///     Foo {
///         x: true,
///         y: vec![1, 2, 3]
///     }
/// );
/// # Ok(())
/// # }
/// ```
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use std::collections::BTreeMap;
/// use sessiond_serde_dhall::{NumKind, SimpleValue};
///
/// let value: SimpleValue =
///     sessiond_serde_dhall::from_str("{ x = 1, y = 2 }").parse()?;
///
/// let mut map = BTreeMap::new();
/// map.insert("x".to_string(), SimpleValue::Num(NumKind::Natural(1u32.into())));
/// map.insert("y".to_string(), SimpleValue::Num(NumKind::Natural(2u32.into())));
/// assert_eq!(value, SimpleValue::Record(map));
/// # Ok(())
/// # }
/// ```
///
/// [`from_str()`]: crate::from_str()
/// [`from_file()`]: crate::from_file()
/// [`from_simple_value()`]: crate::from_simple_value()
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleValue {
    /// Numbers and booleans - `True`, `1`, `+2`, `3.24`
    Num(NumKind),
    /// A string of text - `"Hello world!"`
    Text(String),
    /// An optional value - `Some e`, `None`
    Optional(Option<Box<SimpleValue>>),
    /// A list of values - `[a, b, c, d, e]`
    List(Vec<SimpleValue>),
    /// A record value - `{ k1 = v1, k2 = v2 }`
    Record(BTreeMap<String, SimpleValue>),
    /// A union value (both the name of the variant and the variant's value) - `Left e`
    Union(String, Option<Box<SimpleValue>>),
    /// A function, left unevaluated until it is applied - `λ(x : Natural) → x + 1`
    Function(Function),
}

/// The type of a value that can be decoded by `sessiond_serde_dhall`, e.g. `{ x: Bool, y: List Natural }`.
///
/// A `SimpleType` is used when deserializing values to ensure they are of the expected type.
/// Rather than letting `serde` handle potential type mismatches, this uses the type-checking
/// capabilities of Dhall to catch errors early and cleanly indicate in the user's code where the
/// mismatch happened.
///
/// You would typically not manipulate `SimpleType`s by hand but rather let Rust infer it for your
/// datatype by deriving the [`StaticType`] trait, and using
/// [`Deserializer::static_type_annotation`]. If you need to supply a `SimpleType` manually, you
/// can either deserialize it like any other Dhall value, or construct it manually.
///
/// [`Deserializer::static_type_annotation`]: crate::Deserializer::static_type_annotation()
/// [`StaticType`]: crate::StaticType
///
/// # Type correspondence
///
/// The following Dhall types correspond to the following Rust types:
///
/// Dhall  | Rust
/// -------|------
/// `Bool`  | `bool`
/// `Natural`  | `u64`, `u32`, ...
/// `Integer`  | `i64`, `i32`, ...
/// `Double`  | `f64`, `f32`, ...
/// `Text`  | `String`
/// `List T`  | `Vec<T>`
/// `Optional T`  | `Option<T>`
/// `{ x: T, y: U }`  | structs
/// `{ _1: T, _2: U }`  | `(T, U)`, structs
/// `{ x: T, y: T }`  | `HashMap<String, T>`, structs
/// `< x: T \| y: U >`  | enums
/// `Prelude.Map.Type K V`  | `Vec<Entry>`, where `Entry` has `mapKey` and `mapValue`
/// `T -> U`  | [`Function`]
/// `Prelude.JSON.Type`  | unsupported
///
/// A `Prelude.Map.Type K V` is a `List { mapKey : K, mapValue : V }` in Dhall,
/// and deserializes as one. It does *not* collapse into a `HashMap`: a Dhall
/// map is ordered and may repeat a key, so folding it into a map would discard
/// both, and would make it indistinguishable from a plain record. Convert
/// explicitly if you want a `HashMap`:
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use std::collections::HashMap;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Entry {
///     mapKey: String,
///     mapValue: u64,
/// }
///
/// let entries: Vec<Entry> =
///     sessiond_serde_dhall::from_str("toMap { x = 1, y = 2 }").parse()?;
/// let map: HashMap<String, u64> =
///     entries.into_iter().map(|e| (e.mapKey, e.mapValue)).collect();
///
/// assert_eq!(map["x"], 1);
/// # Ok(())
/// # }
/// ```
///
/// # Examples
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use sessiond_serde_dhall::{SimpleType, StaticType};
///
/// #[derive(StaticType)]
/// struct Foo {
///     x: bool,
///     y: Vec<u64>,
/// }
///
/// let ty: SimpleType =
///     sessiond_serde_dhall::from_str("{ x: Bool, y: List Natural }").parse()?;
///
/// assert_eq!(Foo::static_type(), ty);
/// # Ok(())
/// # }
/// ```
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use std::collections::HashMap;
/// use sessiond_serde_dhall::SimpleType;
///
/// let ty: SimpleType =
///     sessiond_serde_dhall::from_str("{ x: Natural, y: Natural }").parse()?;
///
/// let mut map = HashMap::new();
/// map.insert("x".to_string(), SimpleType::Natural);
/// map.insert("y".to_string(), SimpleType::Natural);
/// assert_eq!(ty, SimpleType::Record(map));
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleType {
    /// Corresponds to the Dhall type `Bool`
    Bool,
    /// Corresponds to the Dhall type `Natural`
    Natural,
    /// Corresponds to the Dhall type `Integer`
    Integer,
    /// Corresponds to the Dhall type `Double`
    Double,
    /// Corresponds to the Dhall type `Text`
    Text,
    /// Corresponds to the Dhall type `Optional T`
    Optional(Box<SimpleType>),
    /// Corresponds to the Dhall type `List T`
    List(Box<SimpleType>),
    /// Corresponds to the Dhall type `{ x : T, y : U }`
    Record(HashMap<String, SimpleType>),
    /// Corresponds to the Dhall type `< x : T | y : U >`
    Union(HashMap<String, Option<SimpleType>>),
    /// Corresponds to the Dhall type `T -> U`
    Function(Box<SimpleType>, Box<SimpleType>),
}

impl Value {
    pub(crate) fn from_nir_and_ty<'cx>(
        cx: Ctxt<'cx>,
        x: &Nir<'cx>,
        ty: &Nir<'cx>,
    ) -> Result<Self> {
        Ok(if let Ok(val) = SimpleValue::from_nir(cx, x) {
            // The type is simple whenever the value is, except for the function types that
            // `SimpleType` cannot represent, e.g. `∀(a : Type) → a`.
            let ty = SimpleType::from_nir_opt(ty);
            Value {
                kind: ValueKind::Val(val, ty),
            }
        } else if let Ok(ty) = SimpleType::from_nir(x) {
            Value {
                kind: ValueKind::Ty(ty),
            }
        } else {
            let expr = x.to_hir_noenv().to_expr(cx, Default::default());
            return Err(Error(ErrorKind::Deserialize(format!(
                "this is neither a simple type nor a simple value: {}",
                expr
            ))));
        })
    }

    /// Converts a Value into a SimpleValue.
    pub(crate) fn to_simple_value(&self) -> Option<SimpleValue> {
        match &self.kind {
            ValueKind::Val(val, _) => Some(val.clone()),
            _ => None,
        }
    }

    /// Converts a Value into a SimpleType.
    pub(crate) fn to_simple_type(&self) -> Option<SimpleType> {
        match &self.kind {
            ValueKind::Ty(ty) => Some(ty.clone()),
            _ => None,
        }
    }

    /// Converts a value back to the corresponding AST expression.
    pub(crate) fn to_expr(&self) -> Result<Expr> {
        match &self.kind {
            ValueKind::Val(val, ty) => val.to_expr(ty.as_ref()),
            ValueKind::Ty(ty) => Ok(ty.to_expr()),
        }
    }
}

#[derive(Debug)]
struct NotSimpleValue;

impl SimpleValue {
    fn from_nir<'cx>(
        cx: Ctxt<'cx>,
        nir: &Nir<'cx>,
    ) -> StdResult<Self, NotSimpleValue> {
        Ok(match nir.kind() {
            NirKind::Num(lit) => SimpleValue::Num(lit.clone()),
            NirKind::TextLit(x) => SimpleValue::Text(
                x.as_text()
                    .expect("Normal form should ensure the text is a string"),
            ),
            NirKind::EmptyOptionalLit(_) => SimpleValue::Optional(None),
            NirKind::NEOptionalLit(x) => {
                SimpleValue::Optional(Some(Box::new(Self::from_nir(cx, x)?)))
            }
            // Note a `Prelude.Map.Type k v` gets no special treatment: it is a
            // `List { mapKey : k, mapValue : v }` in Dhall and stays one here.
            // Collapsing it into a record used to be convenient -- it let a map
            // deserialize straight into a `HashMap` -- but it silently dropped
            // duplicate keys, discarded the ordering, made the result
            // indistinguishable from a plain record, and panicked outright on a
            // non-`Text` key.
            NirKind::EmptyListLit(_) => SimpleValue::List(vec![]),
            NirKind::NEListLit(xs) => SimpleValue::List(
                xs.iter()
                    .map(|x| Self::from_nir(cx, x))
                    .collect::<StdResult<_, _>>()?,
            ),
            NirKind::RecordLit(kvs) => SimpleValue::Record(
                kvs.iter()
                    .map(|(k, v)| Ok((k.to_string(), Self::from_nir(cx, v)?)))
                    .collect::<StdResult<_, _>>()?,
            ),
            NirKind::UnionLit(field, x, _) => SimpleValue::Union(
                field.into(),
                Some(Box::new(Self::from_nir(cx, x)?)),
            ),
            NirKind::UnionConstructor(field, ty)
                if ty.get(field).map(|f| f.is_some()) == Some(false) =>
            {
                SimpleValue::Union(field.into(), None)
            }
            // A lambda, or a builtin that is still waiting for arguments (e.g. `Natural/subtract
            // 1`). Both are functions we can defer and apply later.
            NirKind::LamClosure { .. } | NirKind::AppliedBuiltin(_) => {
                SimpleValue::Function(
                    Function::from_nir(cx, nir).map_err(|_| NotSimpleValue)?,
                )
            }
            _ => return Err(NotSimpleValue),
        })
    }

    // Converts this to `Hir`, using the optional type annotation. Without the type, things like
    // empty lists and unions will fail to convert.
    fn to_hir<'cx>(
        &self,
        cx: Ctxt<'cx>,
        ty: Option<&SimpleType>,
    ) -> Result<Hir<'cx>> {
        use SimpleType as T;
        use SimpleValue as V;
        let hir = |k| Hir::new(HirKind::Expr(k), Span::Artificial);
        let type_error = || {
            Error(ErrorKind::Serialize(format!(
                "expected a value of type {}, found {:?}",
                ty.unwrap().to_expr(),
                self
            )))
        };
        let type_missing = || {
            Error(ErrorKind::Serialize(format!(
                "cannot serialize value without a type annotation: {:?}",
                self
            )))
        };
        let kind = match (self, ty) {
            // A function is already a complete expression; the annotation adds nothing, so we only
            // check that it is a function type at all.
            (V::Function(f), None)
            | (V::Function(f), Some(T::Function(..))) => return f.to_hir(cx),

            (V::Num(num @ NumKind::Bool(_)), Some(T::Bool))
            | (V::Num(num @ NumKind::Natural(_)), Some(T::Natural))
            | (V::Num(num @ NumKind::Integer(_)), Some(T::Integer))
            | (V::Num(num @ NumKind::Double(_)), Some(T::Double))
            | (V::Num(num), None) => ExprKind::Num(num.clone()),
            (V::Text(v), Some(T::Text)) | (V::Text(v), None) => {
                ExprKind::TextLit(v.clone().into())
            }

            (V::Optional(None), None) => return Err(type_missing()),
            (V::Optional(None), Some(T::Optional(t))) => {
                ExprKind::Op(OpKind::App(
                    hir(ExprKind::Builtin(Builtin::OptionalNone)),
                    t.to_hir(),
                ))
            }
            (V::Optional(Some(v)), None) => {
                ExprKind::SomeLit(v.to_hir(cx, None)?)
            }
            (V::Optional(Some(v)), Some(T::Optional(t))) => {
                ExprKind::SomeLit(v.to_hir(cx, Some(t))?)
            }

            (V::List(v), None) if v.is_empty() => return Err(type_missing()),
            (V::List(v), Some(T::List(t))) if v.is_empty() => {
                ExprKind::EmptyListLit(hir(ExprKind::Op(OpKind::App(
                    hir(ExprKind::Builtin(Builtin::List)),
                    t.to_hir(),
                ))))
            }
            (V::List(v), None) => ExprKind::NEListLit(
                v.iter()
                    .map(|v| v.to_hir(cx, None))
                    .collect::<Result<_>>()?,
            ),
            (V::List(v), Some(T::List(t))) => ExprKind::NEListLit(
                v.iter()
                    .map(|v| v.to_hir(cx, Some(t)))
                    .collect::<Result<_>>()?,
            ),

            (V::Record(v), None) => ExprKind::RecordLit(
                v.iter()
                    .map(|(k, v)| Ok((k.clone().into(), v.to_hir(cx, None)?)))
                    .collect::<Result<_>>()?,
            ),
            (V::Record(v), Some(T::Record(t))) => ExprKind::RecordLit(
                v.iter()
                    .map(|(k, v)| match t.get(k) {
                        Some(t) => {
                            Ok((k.clone().into(), v.to_hir(cx, Some(t))?))
                        }
                        None => Err(type_error()),
                    })
                    .collect::<Result<_>>()?,
            ),

            (V::Union(..), None) => return Err(type_missing()),
            (V::Union(variant, Some(v)), Some(T::Union(t))) => {
                match t.get(variant) {
                    Some(Some(variant_t)) => ExprKind::Op(OpKind::App(
                        hir(ExprKind::Op(OpKind::Field(
                            ty.unwrap().to_hir(),
                            variant.clone().into(),
                        ))),
                        v.to_hir(cx, Some(variant_t))?,
                    )),
                    _ => return Err(type_error()),
                }
            }
            (V::Union(variant, None), Some(T::Union(t))) => {
                match t.get(variant) {
                    Some(None) => ExprKind::Op(OpKind::Field(
                        ty.unwrap().to_hir(),
                        variant.clone().into(),
                    )),
                    _ => return Err(type_error()),
                }
            }

            (_, Some(_)) => return Err(type_error()),
        };
        Ok(hir(kind))
    }
    pub(crate) fn into_value(self, ty: Option<&SimpleType>) -> Result<Value> {
        // Check that the value is printable with the given type.
        Ctxt::with_new(|cx| self.to_hir(cx, ty).map(|_| ()))?;
        Ok(Value {
            kind: ValueKind::Val(self, ty.cloned()),
        })
    }

    /// Converts back to the corresponding AST expression.
    pub(crate) fn to_expr(&self, ty: Option<&SimpleType>) -> Result<Expr> {
        Ctxt::with_new(|cx| {
            Ok(self.to_hir(cx, ty)?.to_expr(cx, Default::default()))
        })
    }
}

#[derive(Debug)]
struct NotSimpleType;

impl SimpleType {
    pub(crate) fn from_nir_opt(nir: &Nir) -> Option<Self> {
        Self::from_nir(nir).ok()
    }

    fn from_nir(nir: &Nir) -> StdResult<Self, NotSimpleType> {
        Ok(match nir.kind() {
            NirKind::BuiltinType(b) => match b {
                Builtin::Bool => SimpleType::Bool,
                Builtin::Natural => SimpleType::Natural,
                Builtin::Integer => SimpleType::Integer,
                Builtin::Double => SimpleType::Double,
                Builtin::Text => SimpleType::Text,
                _ => unreachable!(),
            },
            NirKind::OptionalType(t) => {
                SimpleType::Optional(Box::new(Self::from_nir(t)?))
            }
            NirKind::ListType(t) => {
                SimpleType::List(Box::new(Self::from_nir(t)?))
            }
            NirKind::RecordType(kts) => SimpleType::Record(
                kts.iter()
                    .map(|(k, v)| Ok((k.into(), Self::from_nir(v)?)))
                    .collect::<StdResult<_, _>>()?,
            ),
            NirKind::UnionType(kts) => SimpleType::Union(
                kts.iter()
                    .map(|(k, v)| {
                        Ok((
                            k.into(),
                            v.as_ref().map(Self::from_nir).transpose()?,
                        ))
                    })
                    .collect::<StdResult<_, _>>()?,
            ),
            NirKind::PiClosure { annot, closure, .. } => {
                // `remove_binder` hands back the body with the bound variable left free. If the
                // type is dependent, e.g. `∀(a : Type) → List a`, that variable shows up as a
                // `Var` and the recursive call rejects it, which is what we want.
                let input = Self::from_nir(annot)?;
                let output = Self::from_nir(
                    &closure.remove_binder().ok_or(NotSimpleType)?,
                )?;
                SimpleType::Function(Box::new(input), Box::new(output))
            }
            _ => return Err(NotSimpleType),
        })
    }

    pub(crate) fn to_hir<'cx>(&self) -> Hir<'cx> {
        let hir = |k| Hir::new(HirKind::Expr(k), Span::Artificial);
        hir(match self {
            SimpleType::Bool => ExprKind::Builtin(Builtin::Bool),
            SimpleType::Natural => ExprKind::Builtin(Builtin::Natural),
            SimpleType::Integer => ExprKind::Builtin(Builtin::Integer),
            SimpleType::Double => ExprKind::Builtin(Builtin::Double),
            SimpleType::Text => ExprKind::Builtin(Builtin::Text),
            SimpleType::Optional(t) => ExprKind::Op(OpKind::App(
                hir(ExprKind::Builtin(Builtin::Optional)),
                t.to_hir(),
            )),
            SimpleType::List(t) => ExprKind::Op(OpKind::App(
                hir(ExprKind::Builtin(Builtin::List)),
                t.to_hir(),
            )),
            SimpleType::Record(kts) => ExprKind::RecordType(
                kts.iter()
                    .map(|(k, t)| (k.as_str().into(), t.to_hir()))
                    .collect(),
            ),
            SimpleType::Union(kts) => ExprKind::UnionType(
                kts.iter()
                    .map(|(k, t)| {
                        (k.as_str().into(), t.as_ref().map(|t| t.to_hir()))
                    })
                    .collect(),
            ),
            // `A -> B` is sugar for `∀(_ : A) → B`.
            SimpleType::Function(input, output) => {
                ExprKind::Pi("_".into(), input.to_hir(), output.to_hir())
            }
        })
    }

    /// Converts back to the corresponding AST expression.
    pub(crate) fn to_expr(&self) -> Expr {
        Ctxt::with_new(|cx| self.to_hir().to_expr(cx, Default::default()))
    }
}

impl crate::deserialize::Sealed for Value {}
impl crate::deserialize::Sealed for SimpleType {}
impl crate::serialize::Sealed for Value {}
impl crate::serialize::Sealed for SimpleType {}

impl FromDhall for Value {
    fn from_dhall(v: &Value) -> Result<Self> {
        Ok(v.clone())
    }
}
impl FromDhall for SimpleType {
    fn from_dhall(v: &Value) -> Result<Self> {
        v.to_simple_type().ok_or_else(|| {
            Error(ErrorKind::Deserialize(format!(
                "this cannot be deserialized into a simple type: {}",
                v
            )))
        })
    }
}
impl ToDhall for Value {
    fn to_dhall(&self, _ty: Option<&SimpleType>) -> Result<Value> {
        Ok(self.clone())
    }
}
impl ToDhall for SimpleType {
    fn to_dhall(&self, _ty: Option<&SimpleType>) -> Result<Value> {
        Ok(Value {
            kind: ValueKind::Ty(self.clone()),
        })
    }
}

impl Eq for ValueKind {}
impl PartialEq for ValueKind {
    fn eq(&self, other: &Self) -> bool {
        use ValueKind::*;
        match (self, other) {
            (Val(a, _), Val(b, _)) => a == b,
            (Ty(a), Ty(b)) => a == b,
            _ => false,
        }
    }
}
impl std::fmt::Display for Value {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter,
    ) -> StdResult<(), std::fmt::Error> {
        match self.to_expr() {
            Ok(expr) => expr.fmt(f),
            // A `Value` always carries a type it is printable with, so this is unreachable in
            // practice; falling back beats panicking in a `Display` impl.
            Err(_) => write!(f, "{:?}", self.kind),
        }
    }
}

impl std::fmt::Display for SimpleType {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter,
    ) -> StdResult<(), std::fmt::Error> {
        self.to_expr().fmt(f)
    }
}

#[test]
fn test_display_simpletype() {
    use SimpleType::*;
    let ty = List(Box::new(Optional(Box::new(Natural))));
    assert_eq!(ty.to_string(), "List (Optional Natural)".to_string())
}

#[test]
fn test_display_value() {
    use SimpleType::*;
    let ty = List(Box::new(Optional(Box::new(Natural))));
    let val = SimpleValue::List(vec![]);
    let val = Value {
        kind: ValueKind::Val(val, Some(ty)),
    };
    assert_eq!(val.to_string(), "[] : List (Optional Natural)".to_string())
}
