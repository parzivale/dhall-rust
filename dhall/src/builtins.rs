use std::collections::{BTreeMap, HashMap};

// Referred to by their crate paths: `Natural` and `Integer` are also NumKind
// variants, which are in scope here and would shadow the type names.
use num_bigint::{BigUint, Sign};
use num_traits::{ToPrimitive, Zero};

use crate::operations::{BinOp, OpKind};
use crate::semantics::{Hir, HirKind, Nir, NirKind, NzEnv, VarEnv, nze};
use crate::syntax::Const::Type;
use crate::syntax::{
    Const, Expr, ExprKind, InterpolatedText, InterpolatedTextContents, Label,
    NaiveDouble, NumKind, Span, UnspannedExpr, V,
};
use crate::{Ctxt, Parsed};

/// Built-ins
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Builtin {
    Bool,
    Natural,
    Integer,
    Double,
    Text,
    List,
    Optional,
    OptionalNone,
    NaturalBuild,
    NaturalFold,
    NaturalIsZero,
    NaturalEven,
    NaturalOdd,
    NaturalToInteger,
    NaturalShow,
    NaturalSubtract,
    IntegerToDouble,
    IntegerShow,
    IntegerNegate,
    IntegerClamp,
    DoubleShow,
    ListBuild,
    ListFold,
    ListLength,
    ListHead,
    ListLast,
    ListIndexed,
    ListReverse,
    TextShow,
    TextReplace,
}

impl Builtin {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        use Builtin::*;
        match s {
            "Bool" => Some(Bool),
            "Natural" => Some(Natural),
            "Integer" => Some(Integer),
            "Double" => Some(Double),
            "Text" => Some(Text),
            "List" => Some(List),
            "Optional" => Some(Optional),
            "None" => Some(OptionalNone),
            "Natural/build" => Some(NaturalBuild),
            "Natural/fold" => Some(NaturalFold),
            "Natural/isZero" => Some(NaturalIsZero),
            "Natural/even" => Some(NaturalEven),
            "Natural/odd" => Some(NaturalOdd),
            "Natural/toInteger" => Some(NaturalToInteger),
            "Natural/show" => Some(NaturalShow),
            "Natural/subtract" => Some(NaturalSubtract),
            "Integer/toDouble" => Some(IntegerToDouble),
            "Integer/show" => Some(IntegerShow),
            "Integer/negate" => Some(IntegerNegate),
            "Integer/clamp" => Some(IntegerClamp),
            "Double/show" => Some(DoubleShow),
            "List/build" => Some(ListBuild),
            "List/fold" => Some(ListFold),
            "List/length" => Some(ListLength),
            "List/head" => Some(ListHead),
            "List/last" => Some(ListLast),
            "List/indexed" => Some(ListIndexed),
            "List/reverse" => Some(ListReverse),
            "Text/show" => Some(TextShow),
            "Text/replace" => Some(TextReplace),
            _ => None,
        }
    }
}

/// A partially applied builtin.
/// Invariant: the evaluation of the given args must not be able to progress further
#[derive(Debug, Clone)]
pub struct BuiltinClosure<'cx> {
    env: NzEnv<'cx>,
    b: Builtin,
    /// Arguments applied to the closure so far.
    args: Vec<Nir<'cx>>,
}

impl<'cx> BuiltinClosure<'cx> {
    #[must_use]
    pub fn new(b: Builtin, env: NzEnv<'cx>) -> NirKind<'cx> {
        apply_builtin(b, Vec::new(), env)
    }
    #[must_use]
    pub fn apply(&self, a: Nir<'cx>) -> NirKind<'cx> {
        use std::iter::once;
        let args = self.args.iter().cloned().chain(once(a)).collect();
        apply_builtin(self.b, args, self.env.clone())
    }
    #[must_use]
    pub fn to_hirkind(&self, venv: VarEnv) -> HirKind<'cx> {
        HirKind::Expr(self.args.iter().fold(
            ExprKind::Builtin(self.b),
            |acc, v| {
                ExprKind::Op(OpKind::App(
                    Hir::new(HirKind::Expr(acc), Span::Artificial),
                    v.to_hir(venv),
                ))
            },
        ))
    }
}

#[must_use]
pub fn rc(x: UnspannedExpr) -> Expr {
    Expr::new(x, Span::Artificial)
}

// Ad-hoc macro to help construct the types of builtins
macro_rules! make_type {
    (Type) => { rc(ExprKind::Const(Const::Type)) };
    (Bool) => { rc(ExprKind::Builtin(Builtin::Bool)) };
    (Natural) => { rc(ExprKind::Builtin(Builtin::Natural)) };
    (Integer) => { rc(ExprKind::Builtin(Builtin::Integer)) };
    (Double) => { rc(ExprKind::Builtin(Builtin::Double)) };
    (Text) => { rc(ExprKind::Builtin(Builtin::Text)) };
    ($var:ident) => {
        rc(ExprKind::Var(V(stringify!($var).into(), 0)))
    };
    (Optional $ty:ident) => {
        rc(ExprKind::Op(OpKind::App(
            rc(ExprKind::Builtin(Builtin::Optional)),
            make_type!($ty)
        )))
    };
    (List $($rest:tt)*) => {
        rc(ExprKind::Op(OpKind::App(
            rc(ExprKind::Builtin(Builtin::List)),
            make_type!($($rest)*)
        )))
    };
    ({ $($label:ident : $ty:ident),* }) => {{
        let mut kts = BTreeMap::new();
        $(
            kts.insert(
                Label::from(stringify!($label)),
                make_type!($ty),
            );
        )*
        rc(ExprKind::RecordType(kts))
    }};
    ($ty:ident -> $($rest:tt)*) => {
        rc(ExprKind::Pi(
            "_".into(),
            make_type!($ty),
            make_type!($($rest)*)
        ))
    };
    (($($arg:tt)*) -> $($rest:tt)*) => {
        rc(ExprKind::Pi(
            "_".into(),
            make_type!($($arg)*),
            make_type!($($rest)*)
        ))
    };
    (forall ($var:ident : $($ty:tt)*) -> $($rest:tt)*) => {
        rc(ExprKind::Pi(
            stringify!($var).into(),
            make_type!($($ty)*),
            make_type!($($rest)*)
        ))
    };
}

#[must_use]
pub fn type_of_builtin(cx: Ctxt<'_>, b: Builtin) -> Hir<'_> {
    use Builtin::*;
    let expr = match b {
        Bool | Natural | Integer | Double | Text => make_type!(Type),
        List | Optional => make_type!(
            Type -> Type
        ),

        NaturalFold => make_type!(
            Natural ->
            forall (natural: Type) ->
            forall (succ: natural -> natural) ->
            forall (zero: natural) ->
            natural
        ),
        NaturalBuild => make_type!(
            (forall (natural: Type) ->
                forall (succ: natural -> natural) ->
                forall (zero: natural) ->
                natural) ->
            Natural
        ),
        NaturalIsZero | NaturalEven | NaturalOdd => make_type!(
            Natural -> Bool
        ),
        NaturalToInteger => make_type!(Natural -> Integer),
        NaturalShow => make_type!(Natural -> Text),
        NaturalSubtract => make_type!(Natural -> Natural -> Natural),

        IntegerToDouble => make_type!(Integer -> Double),
        IntegerShow => make_type!(Integer -> Text),
        IntegerNegate => make_type!(Integer -> Integer),
        IntegerClamp => make_type!(Integer -> Natural),

        DoubleShow => make_type!(Double -> Text),
        TextShow => make_type!(Text -> Text),
        TextReplace => make_type!(
            forall (needle: Text) ->
            forall (replacement: Text) ->
            forall (haystack: Text) ->
            Text
        ),
        ListBuild => make_type!(
            forall (a: Type) ->
            (forall (list: Type) ->
                forall (cons: a -> list -> list) ->
                forall (nil: list) ->
                list) ->
            List a
        ),
        ListFold => make_type!(
            forall (a: Type) ->
            (List a) ->
            forall (list: Type) ->
            forall (cons: a -> list -> list) ->
            forall (nil: list) ->
            list
        ),
        ListLength => make_type!(forall (a: Type) -> (List a) -> Natural),
        ListHead | ListLast => {
            make_type!(forall (a: Type) -> (List a) -> Optional a)
        }
        ListIndexed => make_type!(
            forall (a: Type) ->
            (List a) ->
            List { index: Natural, value: a }
        ),
        ListReverse => make_type!(
            forall (a: Type) -> (List a) -> List a
        ),

        OptionalNone => make_type!(
            forall (A: Type) -> Optional A
        ),
    };
    Parsed::from_expr_without_imports(expr)
        .resolve(cx)
        .unwrap()
        .0
}

// Ad-hoc macro to help construct closures
macro_rules! make_closure {
    (var($var:ident)) => {{
        rc(ExprKind::Var(V(
            Label::from(stringify!($var)).into(),
            0
        )))
    }};
    (λ($var:tt : $($ty:tt)*) -> $($body:tt)*) => {{
        let var = Label::from(stringify!($var));
        let ty = make_closure!($($ty)*);
        let body = make_closure!($($body)*);
        rc(ExprKind::Lam(var, ty, body))
    }};
    (Type) => {
        rc(ExprKind::Const(Type))
    };
    (Natural) => {
        rc(ExprKind::Builtin(Builtin::Natural))
    };
    (List $($ty:tt)*) => {{
        let ty = make_closure!($($ty)*);
        rc(ExprKind::Op(OpKind::App(
            rc(ExprKind::Builtin(Builtin::List)),
            ty
        )))
    }};
    (Some($($v:tt)*)) => {
        rc(ExprKind::SomeLit(
            make_closure!($($v)*)
        ))
    };
    (1 + $($v:tt)*) => {
        rc(ExprKind::Op(OpKind::BinOp(
            BinOp::NaturalPlus,
            make_closure!($($v)*),
            rc(ExprKind::Num(NumKind::Natural(BigUint::from(1u32))))
        )))
    };
    ([ $($head:tt)* ] # $($tail:tt)*) => {{
        let head = make_closure!($($head)*);
        let tail = make_closure!($($tail)*);
        rc(ExprKind::Op(OpKind::BinOp(
            BinOp::ListAppend,
            rc(ExprKind::NEListLit(vec![head])),
            tail,
        )))
    }};
}

/// What applying a builtin to its arguments produced.
enum Ret<'cx> {
    NirKind(NirKind<'cx>),
    Nir(Nir<'cx>),
    /// The application cannot be reduced any further yet.
    DoneAsIs,
}

/// Evaluates a hardcoded Dhall expression in `env`. The expressions this is
/// used with are known-good, hence the unwraps.
fn make_closure<'cx>(env: &NzEnv<'cx>, e: Expr) -> Nir<'cx> {
    Parsed::from_expr_without_imports(e)
        .resolve(env.cx())
        .unwrap()
        .typecheck(env.cx())
        .unwrap()
        .as_hir()
        .eval(env.clone())
}

/// The builtins that name a type, plus `None`.
fn apply_type_builtin<'cx>(b: Builtin, args: &[Nir<'cx>]) -> Ret<'cx> {
    use NirKind::*;

    match (b, args) {
        (
            Builtin::Bool
            | Builtin::Natural
            | Builtin::Integer
            | Builtin::Double
            | Builtin::Text,
            [],
        ) => Ret::NirKind(BuiltinType(b)),
        (Builtin::Optional, [t]) => Ret::NirKind(OptionalType(t.clone())),
        (Builtin::List, [t]) => Ret::NirKind(ListType(t.clone())),
        (Builtin::OptionalNone, [t]) => {
            Ret::NirKind(EmptyOptionalLit(t.clone()))
        }
        _ => Ret::DoneAsIs,
    }
}

/// The arithmetic and `show` builtins over `Natural`, `Integer` and `Double`.
fn apply_numeric_builtin<'cx>(b: Builtin, args: &[Nir<'cx>]) -> Ret<'cx> {
    use NirKind::*;
    use NumKind::{Bool, Double, Integer, Natural};

    match (b, args) {
        (Builtin::NaturalIsZero, [n]) => match n.kind() {
            Num(Natural(n)) => Ret::NirKind(Num(Bool(n.is_zero()))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::NaturalEven, [n]) => match n.kind() {
            Num(Natural(n)) => Ret::NirKind(Num(Bool(!n.bit(0)))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::NaturalOdd, [n]) => match n.kind() {
            Num(Natural(n)) => Ret::NirKind(Num(Bool(n.bit(0)))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::NaturalToInteger, [n]) => match n.kind() {
            Num(Natural(n)) => Ret::NirKind(Num(Integer(n.clone().into()))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::NaturalShow, [n]) => match n.kind() {
            Num(Natural(n)) => Ret::Nir(Nir::from_text(n)),
            _ => Ret::DoneAsIs,
        },
        (Builtin::NaturalSubtract, [a, b]) => match (a.kind(), b.kind()) {
            // Truncated subtraction: `a - b` is 0 when it would go negative.
            (Num(Natural(a)), Num(Natural(b))) => {
                Ret::NirKind(Num(Natural(if b > a {
                    b - a
                } else {
                    BigUint::zero()
                })))
            }
            (Num(Natural(a)), _) if a.is_zero() => Ret::Nir(b.clone()),
            (_, Num(Natural(b))) if b.is_zero() => {
                Ret::NirKind(Num(Natural(BigUint::zero())))
            }
            _ if a == b => Ret::NirKind(Num(Natural(BigUint::zero()))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::IntegerShow, [n]) => match n.kind() {
            Num(Integer(n)) => {
                let s = if n.sign() == Sign::Minus {
                    n.to_string()
                } else {
                    format!("+{n}")
                };
                Ret::Nir(Nir::from_text(s))
            }
            _ => Ret::DoneAsIs,
        },
        (Builtin::IntegerToDouble, [n]) => match n.kind() {
            // `to_f64` saturates to +/-infinity rather than failing, which is
            // what the standard asks for on values beyond Double's range.
            Num(Integer(n)) => Ret::NirKind(Num(Double(NaiveDouble::from(
                n.to_f64().unwrap_or(f64::INFINITY),
            )))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::IntegerNegate, [n]) => match n.kind() {
            Num(Integer(n)) => Ret::NirKind(Num(Integer(-n))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::IntegerClamp, [n]) => match n.kind() {
            // Clamps to 0 for negatives; no upper bound to clamp to now.
            Num(Integer(n)) => Ret::NirKind(Num(Natural(
                n.to_biguint().unwrap_or_else(BigUint::zero),
            ))),
            _ => Ret::DoneAsIs,
        },
        (Builtin::DoubleShow, [n]) => match n.kind() {
            Num(Double(n)) => Ret::Nir(Nir::from_text(n)),
            _ => Ret::DoneAsIs,
        },
        _ => Ret::DoneAsIs,
    }
}

/// `Text/show` and `Text/replace`.
fn apply_text_builtin<'cx>(b: Builtin, args: &[Nir<'cx>]) -> Ret<'cx> {
    use NirKind::*;

    match (b, args) {
        (Builtin::TextShow, [v]) => match v.kind() {
            TextLit(tlit) => {
                if let Some(s) = tlit.as_text() {
                    // Printing InterpolatedText takes care of all the escaping
                    let txt: InterpolatedText<Expr> =
                        std::iter::once(InterpolatedTextContents::Text(s))
                            .collect();
                    Ret::Nir(Nir::from_text(txt))
                } else {
                    Ret::DoneAsIs
                }
            }
            _ => Ret::DoneAsIs,
        },
        (Builtin::TextReplace, [needle, replacement, haystack]) => {
            // Helper to match a Nir as a text literal
            fn nir_to_string(n: &Nir) -> Option<String> {
                match n.kind() {
                    TextLit(n_lit) => n_lit.as_text(),
                    _ => None,
                }
            }

            // The needle needs to be fully evaluated as Text otherwise no
            // progress can be made
            match nir_to_string(needle) {
                // When the needle is empty the haystack is returned untouched
                Some(n) if n.is_empty() => Ret::Nir(haystack.clone()),
                Some(n) => {
                    // The haystack needs to be fully evaluated as Text otherwise no
                    // progress can be made
                    if let Some(h) = nir_to_string(haystack) {
                        // Fast case when replacement is fully evaluated
                        if let Some(r) = nir_to_string(replacement) {
                            Ret::Nir(Nir::from_text(h.replace(&n, &r)))
                        } else {
                            use itertools::Itertools;

                            let parts = h.split(&n).map(|s| {
                                InterpolatedTextContents::Text(s.to_string())
                            });
                            let replacement = InterpolatedTextContents::Expr(
                                replacement.clone(),
                            );

                            Ret::Nir(Nir::from_kind(NirKind::TextLit(
                                nze::nir::TextLit::new(Itertools::intersperse(
                                    parts,
                                    replacement,
                                )),
                            )))
                        }
                    } else {
                        Ret::DoneAsIs
                    }
                }
                _ => Ret::DoneAsIs,
            }
        }
        _ => Ret::DoneAsIs,
    }
}

/// The `List/*` builtins.
fn apply_list_builtin<'cx>(
    env: &NzEnv<'cx>,
    b: Builtin,
    args: &[Nir<'cx>],
) -> Ret<'cx> {
    use NirKind::*;
    use NumKind::Natural;

    let cx = env.cx();
    match (b, args) {
        (Builtin::ListLength, [_, l]) => match l.kind() {
            EmptyListLit(_) => Ret::NirKind(Num(Natural(BigUint::zero()))),
            NEListLit(xs) => {
                Ret::NirKind(Num(Natural(BigUint::from(xs.len()))))
            }
            _ => Ret::DoneAsIs,
        },
        (Builtin::ListHead, [_, l]) => match l.kind() {
            EmptyListLit(n) => Ret::NirKind(EmptyOptionalLit(n.clone())),
            NEListLit(xs) => {
                Ret::NirKind(NEOptionalLit(xs.iter().next().unwrap().clone()))
            }
            _ => Ret::DoneAsIs,
        },
        (Builtin::ListLast, [_, l]) => match l.kind() {
            EmptyListLit(n) => Ret::NirKind(EmptyOptionalLit(n.clone())),
            NEListLit(xs) => Ret::NirKind(NEOptionalLit(
                xs.iter().next_back().unwrap().clone(),
            )),
            _ => Ret::DoneAsIs,
        },
        (Builtin::ListReverse, [_, l]) => match l.kind() {
            EmptyListLit(n) => Ret::NirKind(EmptyListLit(n.clone())),
            NEListLit(xs) => {
                Ret::NirKind(NEListLit(xs.iter().rev().cloned().collect()))
            }
            _ => Ret::DoneAsIs,
        },
        (Builtin::ListIndexed, [t, l]) => {
            match l.kind() {
                EmptyListLit(_) | NEListLit(_) => {
                    // Construct the returned record type: { index: Natural, value: t }
                    let mut kts = HashMap::new();
                    kts.insert(
                        "index".into(),
                        Nir::from_builtin(cx, Builtin::Natural),
                    );
                    kts.insert("value".into(), t.clone());
                    let t = Nir::from_kind(RecordType(kts));

                    // Construct the new list, with added indices
                    let list = match l.kind() {
                        EmptyListLit(_) => EmptyListLit(t),
                        NEListLit(xs) => NEListLit(
                            xs.iter()
                                .enumerate()
                                .map(|(i, e)| {
                                    let mut kvs = HashMap::new();
                                    kvs.insert(
                                        "index".into(),
                                        Nir::from_kind(Num(Natural(
                                            BigUint::from(i),
                                        ))),
                                    );
                                    kvs.insert("value".into(), e.clone());
                                    Nir::from_kind(RecordLit(kvs))
                                })
                                .collect(),
                        ),
                        _ => unreachable!(),
                    };
                    Ret::NirKind(list)
                }
                _ => Ret::DoneAsIs,
            }
        }
        (Builtin::ListBuild, [t, f]) => {
            let list_t = Nir::from_builtin(cx, Builtin::List).app(t.clone());
            Ret::Nir(
                f.app(list_t)
                    .app(
                        make_closure(
                            env,
                            make_closure!(
                                λ(T : Type) ->
                                λ(a : var(T)) ->
                                λ(as : List var(T)) ->
                                [ var(a) ] # var(as)
                            ),
                        )
                        .app(t.clone()),
                    )
                    .app(EmptyListLit(t.clone()).into_nir()),
            )
        }
        (Builtin::ListFold, [_, l, _, cons, nil]) => match l.kind() {
            EmptyListLit(_) => Ret::Nir(nil.clone()),
            NEListLit(xs) => {
                let mut v = nil.clone();
                for x in xs.iter().cloned().rev() {
                    v = cons.app(x).app(v);
                }
                Ret::Nir(v)
            }
            _ => Ret::DoneAsIs,
        },
        _ => Ret::DoneAsIs,
    }
}

/// `Natural/build` and `Natural/fold`, which recurse through the normalizer
/// rather than reducing in one step.
fn apply_natural_recursion<'cx>(
    env: &NzEnv<'cx>,
    b: Builtin,
    args: &[Nir<'cx>],
) -> Ret<'cx> {
    use NirKind::*;
    use NumKind::Natural;

    let cx = env.cx();
    match (b, args) {
        (Builtin::NaturalBuild, [f]) => Ret::Nir(
            f.app(Nir::from_builtin(cx, Builtin::Natural))
                .app(make_closure(
                    env,
                    make_closure!(
                        λ(x : Natural) ->
                        1 + var(x)
                    ),
                ))
                .app(Num(Natural(BigUint::zero())).into_nir()),
        ),
        (Builtin::NaturalFold, [n, t, succ, zero]) => match n.kind() {
            Num(Natural(n)) if n.is_zero() => Ret::Nir(zero.clone()),
            Num(Natural(n)) => {
                let fold = Nir::from_builtin(cx, Builtin::NaturalFold)
                    .app(Num(Natural(n - 1u32)).into_nir())
                    .app(t.clone())
                    .app(succ.clone())
                    .app(zero.clone());
                Ret::Nir(succ.app(fold))
            }
            _ => Ret::DoneAsIs,
        },
        _ => Ret::DoneAsIs,
    }
}

fn apply_builtin<'cx>(
    b: Builtin,
    args: Vec<Nir<'cx>>,
    env: NzEnv<'cx>,
) -> NirKind<'cx> {
    use Builtin::*;

    // Dispatched by family so that each group of rules stays a readable size.
    // Every arm falls back to `DoneAsIs` when the arguments are not yet in a
    // shape it can reduce, so an unhandled combination is never a silent error.
    let ret = match b {
        Bool | Natural | Integer | Double | Text | List | Optional
        | OptionalNone => apply_type_builtin(b, &args),
        NaturalIsZero | NaturalEven | NaturalOdd | NaturalToInteger
        | NaturalShow | NaturalSubtract | IntegerToDouble | IntegerShow
        | IntegerNegate | IntegerClamp | DoubleShow => {
            apply_numeric_builtin(b, &args)
        }
        TextShow | TextReplace => apply_text_builtin(b, &args),
        ListBuild | ListFold | ListLength | ListHead | ListLast
        | ListIndexed | ListReverse => apply_list_builtin(&env, b, &args),
        NaturalBuild | NaturalFold => apply_natural_recursion(&env, b, &args),
    };

    match ret {
        Ret::NirKind(v) => v,
        Ret::Nir(v) => v.kind().clone(),
        Ret::DoneAsIs => {
            NirKind::AppliedBuiltin(BuiltinClosure { env, b, args })
        }
    }
}

impl std::cmp::PartialEq for BuiltinClosure<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.b == other.b && self.args == other.args
    }
}
impl std::cmp::Eq for BuiltinClosure<'_> {}

impl std::fmt::Display for Builtin {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use Builtin::*;
        f.write_str(match *self {
            Bool => "Bool",
            Natural => "Natural",
            Integer => "Integer",
            Double => "Double",
            Text => "Text",
            List => "List",
            Optional => "Optional",
            OptionalNone => "None",
            NaturalBuild => "Natural/build",
            NaturalFold => "Natural/fold",
            NaturalIsZero => "Natural/isZero",
            NaturalEven => "Natural/even",
            NaturalOdd => "Natural/odd",
            NaturalToInteger => "Natural/toInteger",
            NaturalShow => "Natural/show",
            NaturalSubtract => "Natural/subtract",
            IntegerToDouble => "Integer/toDouble",
            IntegerNegate => "Integer/negate",
            IntegerClamp => "Integer/clamp",
            IntegerShow => "Integer/show",
            DoubleShow => "Double/show",
            ListBuild => "List/build",
            ListFold => "List/fold",
            ListLength => "List/length",
            ListHead => "List/head",
            ListLast => "List/last",
            ListIndexed => "List/indexed",
            ListReverse => "List/reverse",
            TextShow => "Text/show",
            TextReplace => "Text/replace",
        })
    }
}
