use std::fmt;
use std::result::Result as StdResult;

use sessiond_dhall::operations::OpKind;
use sessiond_dhall::semantics::{Hir, Nir, NirKind};
use sessiond_dhall::syntax::{Expr, ExprKind, Span, binary};
use sessiond_dhall::{Ctxt, Parsed};

use crate::{Error, ErrorKind, FromDhall, Result, SimpleType, ToDhall, Value};

/// Serde has no notion of a function, so a [`Function`] is smuggled through the serde data model
/// as a newtype struct with this name, carrying the binary encoding of the function as its
/// payload. Both ends of the conversion recognize the name and handle it specially.
pub(crate) const FUNCTION_TOKEN: &str =
    "$sessiond_serde_dhall::private::Function";

fn dhall_err(e: impl Into<sessiond_dhall::error::Error>) -> Error {
    Error(ErrorKind::Dhall(e.into()))
}

/// A Dhall function, kept unevaluated so that it can be called from Rust with [`apply()`].
///
/// Dhall functions are values like any other, so a configuration file can expose one:
///
/// ```dhall
/// { port = 8080
/// , greeting = λ(name : Text) → "Hello, ${name}!"
/// }
/// ```
///
/// Deserializing that field into a `Function` gives you a deferred chunk of Dhall that you can
/// run whenever you like, as many times as you like:
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use serde::Deserialize;
/// use sessiond_serde_dhall::Function;
///
/// #[derive(Deserialize)]
/// struct Config {
///     port: u64,
///     greeting: Function,
/// }
///
/// let config: Config = sessiond_serde_dhall::from_str(
///     r#"{ port = 8080, greeting = \(name : Text) -> "Hello, ${name}!" }"#,
/// )
/// .parse()?;
///
/// assert_eq!(config.port, 8080);
/// assert_eq!(
///     config.greeting.apply::<_, String>("world")?,
///     "Hello, world!".to_string()
/// );
/// # Ok(())
/// # }
/// ```
///
/// # Evaluation
///
/// A `Function` stores the normal form of the function, with all of its imports already resolved
/// and inlined. It is therefore self-contained: it does not borrow from the file it came from, and
/// calling it never touches the filesystem or the network.
///
/// Each call to [`apply()`] type-checks and normalizes the application from scratch, so calling a
/// function in a hot loop is not free. Dhall functions are pure, so the result only ever depends
/// on the argument.
///
/// # Round-tripping
///
/// A `Function` serializes back to the Dhall it came from, so a configuration that contains
/// functions can be read, modified and written out again. Because what is stored is the normal
/// form, the function that comes back out is equivalent to, but not necessarily character-for-
/// character the same as, the one that went in.
///
/// ```rust
/// # fn main() -> sessiond_serde_dhall::Result<()> {
/// use sessiond_serde_dhall::Function;
///
/// let f: Function = sessiond_serde_dhall::from_str("\\(x : Natural) -> x + 1").parse()?;
/// assert_eq!(
///     sessiond_serde_dhall::serialize(&f).to_string()?,
///     "λ(x : Natural) → x + 1".to_string()
/// );
/// # Ok(())
/// # }
/// ```
///
/// [`apply()`]: Function::apply()
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The normal form of the function, as a closed expression free of imports.
    expr: Expr,
    /// The argument type, if it is a type `sessiond_serde_dhall` can represent.
    input_ty: Option<SimpleType>,
    /// The return type, if it is a type `sessiond_serde_dhall` can represent.
    output_ty: Option<SimpleType>,
}

impl Function {
    /// Builds a `Function` from a normalized value. Fails if the value isn't a function.
    pub(crate) fn from_nir<'cx>(cx: Ctxt<'cx>, nir: &Nir<'cx>) -> Result<Self> {
        let hir = nir.to_hir_noenv();
        let expr = hir.to_expr(cx, Default::default());
        let ty = hir.typecheck_noenv(cx).map_err(dhall_err)?.ty().clone();
        match ty.as_nir().kind() {
            NirKind::PiClosure { annot, closure, .. } => Ok(Function {
                expr,
                input_ty: SimpleType::from_nir_opt(annot),
                // A dependent function type like `∀(a : Type) → List a` has no
                // `SimpleType` equivalent, and `remove_binder` returns `None`
                // for one, so this is `None` too.
                output_ty: closure
                    .remove_binder()
                    .as_ref()
                    .and_then(|nir| SimpleType::from_nir_opt(nir)),
            }),
            _ => Err(Error(ErrorKind::Deserialize(format!(
                "this is not a function: {}",
                expr
            )))),
        }
    }

    /// Type-checks and normalizes an expression into a `Function`.
    fn from_expr(expr: Expr) -> Result<Self> {
        Ctxt::with_new(|cx| {
            let resolved = Parsed::from_expr_without_imports(expr)
                .skip_resolve(cx)
                .map_err(dhall_err)?;
            let typed = resolved.typecheck(cx).map_err(dhall_err)?;
            let normalized = typed.normalize(cx);
            Function::from_nir(cx, normalized.as_nir())
        })
    }

    /// Converts back to `Hir`. The expression is closed and import-free, so resolution is a
    /// formality; we go through `typecheck` only because that is the public way to get at the
    /// `Hir` of a resolved expression.
    pub(crate) fn to_hir<'cx>(&self, cx: Ctxt<'cx>) -> Result<Hir<'cx>> {
        Ok(Parsed::from_expr_without_imports(self.expr.clone())
            .skip_resolve(cx)
            .map_err(dhall_err)?
            .typecheck(cx)
            .map_err(dhall_err)?
            .as_hir()
            .clone())
    }

    pub(crate) fn to_binary(&self) -> Result<Vec<u8>> {
        binary::encode(&self.expr).map_err(dhall_err)
    }

    pub(crate) fn from_binary(data: &[u8]) -> Result<Self> {
        Self::from_expr(binary::decode(data).map_err(dhall_err)?)
    }

    /// Calls the function and deserializes its result.
    ///
    /// The argument is converted to Dhall the same way [`serialize()`] would do it, then the
    /// application is type-checked and normalized. A mismatch between the argument and what the
    /// function expects is reported as a Dhall type error.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn main() -> sessiond_serde_dhall::Result<()> {
    /// use sessiond_serde_dhall::Function;
    ///
    /// let f: Function =
    ///     sessiond_serde_dhall::from_str("\\(x : Natural) -> x + 1").parse()?;
    ///
    /// assert_eq!(f.apply::<_, u64>(1u64)?, 2);
    /// assert_eq!(f.apply::<_, u64>(41u64)?, 42);
    ///
    /// // Passing an argument of the wrong type is an error. Note that the Rust type of the
    /// // argument decides the Dhall type it is converted to: `1u64` is a `Natural`, whereas the
    /// // default integer type `1i32` would be an `Integer`.
    /// assert!(f.apply::<_, u64>("not a number").is_err());
    /// assert!(f.apply::<_, u64>(1i32).is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// A function of several arguments is curried, as it is in Dhall: applying one argument gives
    /// back another `Function`.
    ///
    /// ```rust
    /// # fn main() -> sessiond_serde_dhall::Result<()> {
    /// use sessiond_serde_dhall::Function;
    ///
    /// let f: Function =
    ///     sessiond_serde_dhall::from_str("\\(x : Natural) -> \\(y : Natural) -> x * y")
    ///         .parse()?;
    ///
    /// let times_six: Function = f.apply(6u64)?;
    /// assert_eq!(times_six.apply::<_, u64>(7u64)?, 42);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The argument may itself be a `Function`, so higher-order functions work too.
    ///
    /// ```rust
    /// # fn main() -> sessiond_serde_dhall::Result<()> {
    /// use sessiond_serde_dhall::Function;
    ///
    /// let twice: Function = sessiond_serde_dhall::from_str(
    ///     "\\(f : Natural -> Natural) -> \\(x : Natural) -> f (f x)",
    /// )
    /// .parse()?;
    /// let increment: Function =
    ///     sessiond_serde_dhall::from_str("\\(x : Natural) -> x + 1").parse()?;
    ///
    /// let increment_twice: Function = twice.apply(increment)?;
    /// assert_eq!(increment_twice.apply::<_, u64>(40u64)?, 42);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`serialize()`]: crate::serialize()
    pub fn apply<A, T>(&self, arg: A) -> Result<T>
    where
        A: ToDhall,
        T: FromDhall,
    {
        let arg = arg.to_dhall(self.input_ty.as_ref())?;
        T::from_dhall(&self.apply_value(&arg)?)
    }

    fn apply_value(&self, arg: &Value) -> Result<Value> {
        let app = Expr::new(
            ExprKind::Op(OpKind::App(self.expr.clone(), arg.to_expr()?)),
            Span::Artificial,
        );
        Ctxt::with_new(|cx| {
            let resolved = Parsed::from_expr_without_imports(app)
                .skip_resolve(cx)
                .map_err(dhall_err)?;
            let typed = resolved.typecheck(cx).map_err(dhall_err)?;
            let normalized = typed.normalize(cx);
            Value::from_nir_and_ty(cx, normalized.as_nir(), typed.ty().as_nir())
        })
    }

    /// The type of the function's argument, if `sessiond_serde_dhall` can represent it.
    ///
    /// This is `None` for functions whose argument type isn't a [`SimpleType`], e.g. the
    /// polymorphic `λ(a : Type) → λ(x : a) → x`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn main() -> sessiond_serde_dhall::Result<()> {
    /// use sessiond_serde_dhall::{Function, SimpleType};
    ///
    /// let f: Function =
    ///     sessiond_serde_dhall::from_str("\\(x : Natural) -> x + 1").parse()?;
    ///
    /// assert_eq!(f.input_type(), Some(SimpleType::Natural));
    /// # Ok(())
    /// # }
    /// ```
    pub fn input_type(&self) -> Option<SimpleType> {
        self.input_ty.clone()
    }

    /// The type of the function's result, if `sessiond_serde_dhall` can represent it.
    ///
    /// This is `None` for functions whose result type isn't a [`SimpleType`], and in particular
    /// for dependent function types like `∀(a : Type) → List a`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn main() -> sessiond_serde_dhall::Result<()> {
    /// use sessiond_serde_dhall::{Function, SimpleType};
    ///
    /// let f: Function =
    ///     sessiond_serde_dhall::from_str("\\(x : Natural) -> [x]").parse()?;
    ///
    /// assert_eq!(
    ///     f.output_type(),
    ///     Some(SimpleType::List(Box::new(SimpleType::Natural)))
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn output_type(&self) -> Option<SimpleType> {
        self.output_ty.clone()
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.expr.fmt(f)
    }
}

struct FunctionVisitor;

impl<'de> serde::de::Visitor<'de> for FunctionVisitor {
    type Value = Function;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a Dhall function")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> StdResult<Function, E>
    where
        E: serde::de::Error,
    {
        Function::from_binary(v).map_err(E::custom)
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> StdResult<Function, E>
    where
        E: serde::de::Error,
    {
        self.visit_bytes(&v)
    }

    fn visit_newtype_struct<D>(self, d: D) -> StdResult<Function, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        d.deserialize_bytes(self)
    }
}

impl<'de> serde::Deserialize<'de> for Function {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_newtype_struct(FUNCTION_TOKEN, FunctionVisitor)
    }
}

/// The payload of the newtype struct that carries a function through serde.
struct FunctionBytes<'a>(&'a [u8]);

impl serde::Serialize for FunctionBytes<'_> {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self.0)
    }
}

impl serde::Serialize for Function {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let bytes = self.to_binary().map_err(serde::ser::Error::custom)?;
        serializer
            .serialize_newtype_struct(FUNCTION_TOKEN, &FunctionBytes(&bytes))
    }
}
