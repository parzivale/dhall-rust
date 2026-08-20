use itertools::Itertools;
use num_bigint::BigUint;
use num_traits::{One, Zero};
use std::collections::HashMap;
use std::iter::once;

use crate::operations::{BinOp, OpKind};
use crate::semantics::{
    Nir, NirKind, Ret, TextLit, merge_maps, ret_kind, ret_nir, ret_op, ret_ref,
};
use crate::syntax::{ExprKind, Label, NumKind};

/// The simplification rules for `&&`, `||`, `==` and `!=`.
// Operators that coincidentally share a result — `True && x` and `True == x`
// both reduce to `x` — stay separate, so that each operator's rules read as one
// block. Where the two sides of a *single* operator share one, they are merged.
#[expect(clippy::match_same_arms)]
fn normalize_bool_binop<'cx>(o: BinOp, x: Nir<'cx>, y: Nir<'cx>) -> Ret<'cx> {
    use BinOp::*;
    use NirKind::Num;
    use NumKind::Bool;

    match (o, x.kind(), y.kind()) {
        (BoolAnd, Num(Bool(true)), _) => ret_nir(y),
        (BoolAnd, _, Num(Bool(true))) => ret_nir(x),
        (BoolAnd, Num(Bool(false)), _) | (BoolAnd, _, Num(Bool(false))) => {
            ret_kind(Num(Bool(false)))
        }
        (BoolAnd, _, _) if x == y => ret_nir(x),
        (BoolOr, Num(Bool(true)), _) | (BoolOr, _, Num(Bool(true))) => {
            ret_kind(Num(Bool(true)))
        }
        (BoolOr, Num(Bool(false)), _) => ret_nir(y),
        (BoolOr, _, Num(Bool(false))) => ret_nir(x),
        (BoolOr, _, _) if x == y => ret_nir(x),
        (BoolEQ, Num(Bool(true)), _) => ret_nir(y),
        (BoolEQ, _, Num(Bool(true))) => ret_nir(x),
        (BoolEQ, Num(Bool(x)), Num(Bool(y))) => ret_kind(Num(Bool(x == y))),
        (BoolEQ, _, _) if x == y => ret_kind(Num(Bool(true))),
        (BoolNE, Num(Bool(false)), _) => ret_nir(y),
        (BoolNE, _, Num(Bool(false))) => ret_nir(x),
        (BoolNE, Num(Bool(x)), Num(Bool(y))) => ret_kind(Num(Bool(x != y))),
        (BoolNE, _, _) if x == y => ret_kind(Num(Bool(false))),
        _ => ret_op(OpKind::BinOp(o, x, y)),
    }
}

// A simplification table, one operator at a time. The boolean operators are in
// `normalize_bool_binop`; everything else is here.
fn normalize_binop<'cx>(o: BinOp, x: Nir<'cx>, y: Nir<'cx>) -> Ret<'cx> {
    use BinOp::*;
    use NirKind::{EmptyListLit, NEListLit, Num, RecordLit, RecordType};
    use NumKind::Natural;

    match (o, x.kind(), y.kind()) {
        (BoolAnd | BoolOr | BoolEQ | BoolNE, _, _) => {
            normalize_bool_binop(o, x, y)
        }

        (NaturalPlus, Num(Natural(n)), _) if n.is_zero() => ret_nir(y),
        (NaturalPlus, _, Num(Natural(n))) if n.is_zero() => ret_nir(x),
        (NaturalPlus, Num(Natural(x)), Num(Natural(y))) => {
            ret_kind(Num(Natural(x + y)))
        }
        (NaturalTimes, Num(Natural(n)), _) if n.is_zero() => {
            ret_kind(Num(Natural(BigUint::zero())))
        }
        (NaturalTimes, _, Num(Natural(n))) if n.is_zero() => {
            ret_kind(Num(Natural(BigUint::zero())))
        }
        (NaturalTimes, Num(Natural(n)), _) if n.is_one() => ret_nir(y),
        (NaturalTimes, _, Num(Natural(n))) if n.is_one() => ret_nir(x),
        (NaturalTimes, Num(Natural(x)), Num(Natural(y))) => {
            ret_kind(Num(Natural(x * y)))
        }

        (ListAppend, EmptyListLit(_), _) => ret_nir(y),
        (ListAppend, _, EmptyListLit(_)) => ret_nir(x),
        (ListAppend, NEListLit(xs), NEListLit(ys)) => {
            ret_kind(NEListLit(xs.iter().chain(ys.iter()).cloned().collect()))
        }

        (TextAppend, NirKind::TextLit(x), _) if x.is_empty() => ret_nir(y),
        (TextAppend, _, NirKind::TextLit(y)) if y.is_empty() => ret_nir(x),
        (TextAppend, NirKind::TextLit(x), NirKind::TextLit(y)) => {
            ret_kind(NirKind::TextLit(x.concat(y)))
        }
        (TextAppend, NirKind::TextLit(x), _) => ret_kind(NirKind::TextLit(
            x.concat(&TextLit::interpolate(y.clone())),
        )),
        (TextAppend, _, NirKind::TextLit(y)) => ret_kind(NirKind::TextLit(
            TextLit::interpolate(x.clone()).concat(y),
        )),
        // Neither side is a literal. The standard still normalises this to a
        // text literal of two interpolations, rather than leaving a `++` node:
        // `l ++ r` is `"${l}${r}"`. Without this the normal form differs from
        // every other implementation, and so does the semantic hash.
        (TextAppend, _, _) => ret_kind(NirKind::TextLit(
            TextLit::interpolate(x.clone())
                .concat(&TextLit::interpolate(y.clone())),
        )),

        (RightBiasedRecordMerge, _, RecordLit(kvs)) if kvs.is_empty() => {
            ret_nir(x)
        }
        (RightBiasedRecordMerge, RecordLit(kvs), _) if kvs.is_empty() => {
            ret_nir(y)
        }
        (RightBiasedRecordMerge, RecordLit(kvs1), RecordLit(kvs2)) => {
            let mut kvs = kvs2.clone();
            for (x, v) in kvs1 {
                // Insert only if key not already present
                kvs.entry(x.clone()).or_insert_with(|| v.clone());
            }
            ret_kind(RecordLit(kvs))
        }
        (RightBiasedRecordMerge, _, _) if x == y => ret_nir(y),

        (RecursiveRecordMerge, _, RecordLit(kvs)) if kvs.is_empty() => {
            ret_nir(x)
        }
        (RecursiveRecordMerge, RecordLit(kvs), _) if kvs.is_empty() => {
            ret_nir(y)
        }
        (RecursiveRecordMerge, RecordLit(kvs1), RecordLit(kvs2)) => {
            let kvs = merge_maps(kvs1, kvs2, |_, v1, v2| {
                Nir::from_partial_expr(ExprKind::Op(OpKind::BinOp(
                    RecursiveRecordMerge,
                    v1.clone(),
                    v2.clone(),
                )))
            });
            ret_kind(RecordLit(kvs))
        }

        (RecursiveRecordTypeMerge, RecordType(kts_x), RecordType(kts_y)) => {
            let kts = merge_maps(
                kts_x,
                kts_y,
                // If the Label exists for both records, then we hit the recursive case.
                |_, l: &Nir, r: &Nir| {
                    Nir::from_partial_expr(ExprKind::Op(OpKind::BinOp(
                        RecursiveRecordTypeMerge,
                        l.clone(),
                        r.clone(),
                    )))
                },
            );
            ret_kind(RecordType(kts))
        }

        (Equivalence, _, _) => ret_kind(NirKind::Equivalence(x, y)),

        _ => ret_op(OpKind::BinOp(o, x, y)),
    }
}

fn normalize_field<'cx>(v: &Nir<'cx>, field: &Label) -> Ret<'cx> {
    use self::BinOp::{RecursiveRecordMerge, RightBiasedRecordMerge};
    use NirKind::{Op, RecordLit, UnionConstructor, UnionType};
    use OpKind::{BinOp, Field, Projection};
    let nothing_to_do = || ret_op(Field(v.clone(), field.clone()));

    match v.kind() {
        RecordLit(kvs) => match kvs.get(field) {
            Some(r) => ret_ref(r),
            None => nothing_to_do(),
        },
        UnionType(kts) => {
            ret_kind(UnionConstructor(field.clone(), kts.clone()))
        }
        Op(Projection(x, _)) => normalize_field(x, field),
        Op(BinOp(RightBiasedRecordMerge, x, y)) => match (x.kind(), y.kind()) {
            (_, RecordLit(kvs)) => match kvs.get(field) {
                Some(r) => ret_ref(r),
                None => normalize_field(x, field),
            },
            (RecordLit(kvs), _) => match kvs.get(field) {
                Some(r) => ret_op(Field(
                    Nir::from_kind(Op(BinOp(
                        RightBiasedRecordMerge,
                        Nir::from_kind(RecordLit(
                            once((field.clone(), r.clone())).collect(),
                        )),
                        y.clone(),
                    ))),
                    field.clone(),
                )),
                None => normalize_field(y, field),
            },
            _ => nothing_to_do(),
        },
        Op(BinOp(RecursiveRecordMerge, x, y)) => match (x.kind(), y.kind()) {
            (RecordLit(kvs), _) => match kvs.get(field) {
                Some(r) => ret_op(Field(
                    Nir::from_kind(Op(BinOp(
                        RecursiveRecordMerge,
                        Nir::from_kind(RecordLit(
                            once((field.clone(), r.clone())).collect(),
                        )),
                        y.clone(),
                    ))),
                    field.clone(),
                )),
                None => normalize_field(y, field),
            },
            (_, RecordLit(kvs)) => match kvs.get(field) {
                Some(r) => ret_op(Field(
                    Nir::from_kind(Op(BinOp(
                        RecursiveRecordMerge,
                        x.clone(),
                        Nir::from_kind(RecordLit(
                            once((field.clone(), r.clone())).collect(),
                        )),
                    ))),
                    field.clone(),
                )),
                None => normalize_field(x, field),
            },
            _ => nothing_to_do(),
        },
        _ => nothing_to_do(),
    }
}

#[must_use]
pub fn normalize_operation(opkind: OpKind<Nir<'_>>) -> Ret<'_> {
    use self::BinOp::RightBiasedRecordMerge;
    use NirKind::{Num, Op, RecordLit, RecordType};
    use NumKind::Bool;
    use OpKind::*;

    match opkind {
        App(v, a) => ret_kind(v.app_to_kind(a)),
        BinOp(o, x, y) => normalize_binop(o, x, y),
        BoolIf(b, e1, e2) => {
            match b.kind() {
                Num(Bool(true)) => ret_nir(e1),
                Num(Bool(false)) => ret_nir(e2),
                _ => {
                    match (e1.kind(), e2.kind()) {
                        // Simplify `if b then True else False`
                        (Num(Bool(true)), Num(Bool(false))) => ret_nir(b),
                        _ if e1 == e2 => ret_nir(e1),
                        _ => ret_op(BoolIf(b, e1, e2)),
                    }
                }
            }
        }
        Merge(handlers, variant, ty) => normalize_merge(handlers, variant, ty),
        ToMap(v, annot) => normalize_tomap(v, annot),
        Field(v, field) => normalize_field(&v, &field),
        Projection(_, ls) if ls.is_empty() => {
            ret_kind(RecordLit(HashMap::new()))
        }
        Projection(v, ls) => match v.kind() {
            RecordLit(kvs) => ret_kind(RecordLit(
                ls.iter()
                    .filter_map(|l| kvs.get(l).map(|x| (l.clone(), x.clone())))
                    .collect(),
            )),
            Op(Projection(v2, _)) => {
                normalize_operation(Projection(v2.clone(), ls))
            }
            Op(BinOp(RightBiasedRecordMerge, l, r)) => match r.kind() {
                RecordLit(kvs) => {
                    let r_keys = kvs.keys().cloned().collect();
                    normalize_operation(BinOp(
                        RightBiasedRecordMerge,
                        Nir::from_partial_expr(ExprKind::Op(Projection(
                            l.clone(),
                            ls.difference(&r_keys).cloned().collect(),
                        ))),
                        Nir::from_partial_expr(ExprKind::Op(Projection(
                            r.clone(),
                            ls.intersection(&r_keys).cloned().collect(),
                        ))),
                    ))
                }
                _ => ret_op(Projection(v, ls)),
            },
            _ => ret_op(Projection(v, ls)),
        },
        ProjectionByExpr(v, t) => match t.kind() {
            RecordType(kts) => normalize_operation(Projection(
                v,
                kts.keys().cloned().collect(),
            )),
            _ => ret_op(ProjectionByExpr(v, t)),
        },
        With(record, labels, expr) => normalize_with(record, labels, expr),
        Completion(..) => {
            unreachable!("This case should have been handled in resolution")
        }
    }
}

/// `merge handlers variant`, which picks the handler matching the variant the
/// union or optional actually holds.
fn normalize_merge<'cx>(
    handlers: Nir<'cx>,
    variant: Nir<'cx>,
    ty: Option<Nir<'cx>>,
) -> Ret<'cx> {
    use NirKind::{
        EmptyOptionalLit, NEOptionalLit, UnionConstructor, UnionLit,
    };
    use OpKind::Merge;

    let NirKind::RecordLit(kvs) = handlers.kind() else {
        return ret_op(Merge(handlers, variant, ty));
    };
    // A handler that is not there yet leaves the `merge` unreduced rather than
    // failing; the typechecker is what rejects a genuinely missing handler.
    match variant.kind() {
        UnionConstructor(l, _) => match kvs.get(l) {
            Some(h) => ret_ref(h),
            None => ret_op(Merge(handlers, variant, ty)),
        },
        UnionLit(l, v, _) => match kvs.get(l) {
            Some(h) => ret_kind(h.app_to_kind(v.clone())),
            None => ret_op(Merge(handlers, variant, ty)),
        },
        EmptyOptionalLit(_) => match kvs.get("None") {
            Some(h) => ret_ref(h),
            None => ret_op(Merge(handlers, variant, ty)),
        },
        NEOptionalLit(v) => match kvs.get("Some") {
            Some(h) => ret_kind(h.app_to_kind(v.clone())),
            None => ret_op(Merge(handlers, variant, ty)),
        },
        _ => ret_op(Merge(handlers, variant, ty)),
    }
}

/// `toMap record`, which turns a record into the list of `mapKey`/`mapValue`
/// pairs the standard defines, in key order.
fn normalize_tomap<'cx>(v: Nir<'cx>, annot: Option<Nir<'cx>>) -> Ret<'cx> {
    use NirKind::{EmptyListLit, NEListLit, RecordLit};
    use OpKind::ToMap;

    match v.kind() {
        RecordLit(kvs) if kvs.is_empty() => {
            // An empty record carries no element type, so only the annotation
            // can say what the empty list is a list of.
            match annot.as_ref().map(Nir::kind) {
                Some(NirKind::ListType(t)) => ret_kind(EmptyListLit(t.clone())),
                _ => ret_op(ToMap(v, annot)),
            }
        }
        RecordLit(kvs) => ret_kind(NEListLit(
            kvs.iter()
                .sorted_by_key(|(k, _)| *k)
                .map(|(k, v)| {
                    let mut rec = HashMap::new();
                    rec.insert("mapKey".into(), Nir::from_text(k));
                    rec.insert("mapValue".into(), v.clone());
                    Nir::from_kind(NirKind::RecordLit(rec))
                })
                .collect(),
        )),
        _ => ret_op(ToMap(v, annot)),
    }
}

/// `record with a.b.c = expr`, which replaces one nested field.
fn normalize_with<'cx>(
    mut record: Nir<'cx>,
    labels: Vec<Label>,
    expr: Nir<'cx>,
) -> Ret<'cx> {
    use NirKind::{Op, RecordLit};

    let mut labels = labels.into_iter();
    let mut current = &mut record;
    // We dig through the current record with the provided labels.
    while let RecordLit(kvs) = current.kind_mut() {
        if let Some(label) = labels.next() {
            // Get existing entry or insert empty record into it.
            let nir = kvs
                .entry(label)
                .or_insert_with(|| Nir::from_kind(RecordLit(HashMap::new())));
            // Disgusting, but the normal assignment works with -Zpolonius, so this
            // is safe. See https://github.com/rust-lang/rust/issues/70255 .
            current = unsafe { &mut *std::ptr::from_mut(nir) };
        } else {
            break;
        }
    }

    // If there are still some fields to dig through, we need to create a `with` expression
    // with the remaining fields.
    let labels: Vec<_> = labels.collect();
    *current = if labels.is_empty() {
        expr
    } else {
        Nir::from_kind(Op(OpKind::With(current.clone(), labels, expr)))
    };

    ret_nir(record)
}
