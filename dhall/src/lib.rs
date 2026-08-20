#![doc(html_root_url = "https://docs.rs/sessiond-dhall/2.1.0")]
#![expect(
    clippy::implicit_hasher,
    clippy::module_inception,
    clippy::needless_lifetimes,
    clippy::new_ret_no_self,
    clippy::try_err,
    clippy::unnecessary_wraps,
    clippy::useless_format
)]
// The parser, printer, encoder, decoder and typechecker each exhaustively match
// on `ExprKind`, `OpKind`, `Builtin` and friends, which run to thirty variants.
// Importing those variants one by one costs more in noise than the glob does in
// ambiguity, and the modules that glob-import do little else.
//
// `allow` rather than `expect`, unlike the rest of the crate: both of these are
// early lints, and a crate-level expectation for one is not marked fulfilled by
// a firing in a submodule, so `expect` would suppress them and then warn that it
// had nothing to suppress.
#![allow(clippy::enum_glob_use, clippy::wildcard_imports)]
// This crate is the implementation of the Dhall language, not the API users are
// meant to program against — that is `serde_dhall`, where these lints stay on.
// Almost every function here returns `Result` and can fail for the same handful
// of reasons, so per-function `# Errors` sections would restate the crate-level
// error documentation several dozen times over.
#![expect(clippy::missing_errors_doc, clippy::missing_panics_doc)]

pub mod builtins;
pub mod ctxt;
pub mod error;
pub mod operations;
pub mod semantics;
pub mod syntax;
pub mod utils;

use std::path::Path;
use url::Url;

use crate::error::{Error, TypeError};
use crate::semantics::parse;
use crate::semantics::resolve;
use crate::semantics::resolve::ImportLocation;
use crate::semantics::{Hir, Nir, Tir, Type, typecheck, typecheck_with};
use crate::syntax::Expr;

pub use ctxt::*;

#[derive(Debug, Clone)]
pub struct Parsed(Expr, ImportLocation);

/// An expression where all imports have been resolved
///
/// Invariant: there must be no `Import` nodes or `ImportAlt` operations left.
#[derive(Debug, Clone)]
pub struct Resolved<'cx>(Hir<'cx>);

/// A typed expression
#[derive(Debug, Clone)]
pub struct Typed<'cx> {
    pub hir: Hir<'cx>,
    pub ty: Type<'cx>,
}

/// A normalized expression.
///
/// This is actually a lie, because the expression will only get normalized on demand.
#[derive(Debug, Clone)]
pub struct Normalized<'cx>(Nir<'cx>);

/// Controls conversion from `Nir` to `Expr`
#[derive(Copy, Clone, Default)]
pub struct ToExprOptions {
    /// Whether to convert all variables to `_`
    pub alpha: bool,
}

impl Parsed {
    /// Construct from an `Expr`. This `Expr` will have imports disabled.
    #[must_use]
    pub fn from_expr_without_imports(e: Expr) -> Self {
        Parsed(e, ImportLocation::dhall_code_without_imports())
    }

    pub fn parse_file(f: &Path) -> Result<Parsed, Error> {
        parse::parse_file(f)
    }
    pub fn parse_remote(url: Url) -> Result<Parsed, Error> {
        parse::parse_remote(url)
    }
    pub fn parse_str(s: &str) -> Result<Parsed, Error> {
        parse::parse_str(s)
    }
    pub fn parse_binary_file(f: &Path) -> Result<Parsed, Error> {
        parse::parse_binary_file(f)
    }
    pub fn parse_binary(data: &[u8]) -> Result<Parsed, Error> {
        parse::parse_binary(data)
    }

    pub fn resolve(self, cx: Ctxt<'_>) -> Result<Resolved<'_>, Error> {
        resolve::resolve(cx, self)
    }
    /// Resolve imports, but refuse to fetch any remote one.
    ///
    /// Local imports read files the caller could have read anyway; a remote
    /// import fetches and runs code from a third party. Use this for
    /// configuration you did not write.
    pub fn resolve_without_remote_imports(
        self,
        cx: Ctxt<'_>,
    ) -> Result<Resolved<'_>, Error> {
        resolve::resolve_without_remote_imports(cx, self)
    }
    pub fn skip_resolve(self, cx: Ctxt<'_>) -> Result<Resolved<'_>, Error> {
        resolve::skip_resolve(cx, self)
    }

    /// Converts a value back to the corresponding AST expression.
    #[must_use]
    pub fn to_expr(&self) -> Expr {
        self.0.clone()
    }

    #[must_use]
    pub fn add_let_binding(self, label: syntax::Label, value: Expr) -> Parsed {
        let Parsed(expr, import_location) = self;
        Parsed(expr.add_let_binding(label, value), import_location)
    }
}

impl<'cx> Resolved<'cx> {
    pub fn typecheck(&self, cx: Ctxt<'cx>) -> Result<Typed<'cx>, TypeError> {
        Ok(Typed::from_tir(&typecheck(cx, &self.0)?))
    }
    pub fn typecheck_with(
        self,
        cx: Ctxt<'cx>,
        ty: &Hir<'cx>,
    ) -> Result<Typed<'cx>, TypeError> {
        Ok(Typed::from_tir(&typecheck_with(cx, &self.0, ty)?))
    }
    /// Normalize without typechecking first.
    ///
    /// The standard defines normalization over untyped terms, so an expression
    /// that does not typecheck still has a normal form. Prefer
    /// [`typecheck`](Resolved::typecheck) followed by
    /// [`Typed::normalize`]: an ill-typed expression can make evaluation
    /// diverge or panic, and this skips the check that would have caught it.
    ///
    /// For a well-typed expression the two agree, since `Typed::normalize`
    /// does not consult the type either.
    #[must_use]
    pub fn normalize_untyped(&self, cx: Ctxt<'cx>) -> Normalized<'cx> {
        Normalized(self.0.eval_closed_expr(cx))
    }

    /// Converts a value back to the corresponding AST expression.
    #[must_use]
    pub fn to_expr(&self, cx: Ctxt<'cx>) -> Expr {
        self.0.to_expr_noopts(cx)
    }

    /// Alpha-normalize: convert back to an AST expression with every bound
    /// variable renamed to `_`.
    ///
    /// This is purely syntactic, so unlike [`Typed::normalize`] followed by
    /// [`Normalized::to_expr_alpha`] it neither typechecks nor evaluates, and
    /// works on expressions with free variables.
    #[must_use]
    pub fn to_expr_alpha(&self, cx: Ctxt<'cx>) -> Expr {
        self.0.to_expr_alpha(cx)
    }
}

impl<'cx> Typed<'cx> {
    fn from_tir(tir: &Tir<'cx, '_>) -> Self {
        Typed {
            hir: tir.as_hir().clone(),
            ty: tir.ty().clone(),
        }
    }
    /// Reduce an expression to its normal form, performing beta reduction
    #[must_use]
    pub fn normalize(&self, cx: Ctxt<'cx>) -> Normalized<'cx> {
        Normalized(self.hir.eval_closed_expr(cx))
    }

    /// Converts a value back to the corresponding AST expression.
    fn to_expr(&self, cx: Ctxt<'cx>) -> Expr {
        self.hir.to_expr(cx, ToExprOptions { alpha: false })
    }

    #[must_use]
    pub fn as_hir(&self) -> &Hir<'cx> {
        &self.hir
    }
    #[must_use]
    pub fn ty(&self) -> &Type<'cx> {
        &self.ty
    }
    pub fn get_type(&self) -> Result<Normalized<'cx>, TypeError> {
        Ok(Normalized(self.ty.clone().into_nir()))
    }
}

impl<'cx> Normalized<'cx> {
    /// Converts a value back to the corresponding AST expression.
    #[must_use]
    pub fn to_expr(&self, cx: Ctxt<'cx>) -> Expr {
        self.0.to_expr(cx, ToExprOptions::default())
    }
    /// Converts a value back to the corresponding Hir expression.
    #[must_use]
    pub fn to_hir(&self) -> Hir<'cx> {
        self.0.to_hir_noenv()
    }
    #[must_use]
    pub fn as_nir(&self) -> &Nir<'cx> {
        &self.0
    }
    /// Converts a value back to the corresponding AST expression, alpha-normalizing in the process.
    #[must_use]
    pub fn to_expr_alpha(&self, cx: Ctxt<'cx>) -> Expr {
        self.0.to_expr(cx, ToExprOptions { alpha: true })
    }
}

macro_rules! derive_traits_for_wrapper_struct {
    ($ty:ident) => {
        impl std::cmp::PartialEq for $ty {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl std::cmp::Eq for $ty {}

        impl std::fmt::Display for $ty {
            fn fmt(
                &self,
                f: &mut std::fmt::Formatter,
            ) -> Result<(), std::fmt::Error> {
                self.0.fmt(f)
            }
        }
    };
}

derive_traits_for_wrapper_struct!(Parsed);

impl From<Parsed> for Expr {
    fn from(other: Parsed) -> Self {
        other.to_expr()
    }
}

impl Eq for Normalized<'_> {}
impl PartialEq for Normalized<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
