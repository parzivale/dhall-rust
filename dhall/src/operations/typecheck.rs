use std::borrow::Cow;
use std::cmp::max;
use std::collections::HashMap;

use crate::builtins::Builtin;
use crate::error::{ErrorBuilder, TypeError};
use crate::operations::{BinOp, OpKind};
use crate::semantics::{
    Binder, Closure, Hir, HirKind, Nir, NirKind, Tir, TyEnv, Type, merge_maps,
    mk_span_err, mkerr,
};
use crate::syntax::{Const, ExprKind, Span};

fn check_rectymerge(
    span: &Span,
    x: &Nir<'_>,
    y: &Nir<'_>,
) -> Result<(), TypeError> {
    let not_record_err = || match span {
        Span::DuplicateRecordFieldsSugar(_, r) => {
            mk_span_err((**r).clone(), "DuplicateFieldName")
        }
        _ => mk_span_err(span.clone(), "RecordTypeMergeRequiresRecordType"),
    };

    let NirKind::RecordType(kts_x) = x.kind() else {
        return not_record_err();
    };
    let NirKind::RecordType(kts_y) = y.kind() else {
        return not_record_err();
    };
    for (k, tx) in kts_x {
        if let Some(ty) = kts_y.get(k) {
            // TODO: store Type in RecordType ?
            check_rectymerge(span, tx, ty)?;
        }
    }
    Ok(())
}

fn typecheck_binop<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    op: BinOp,
    l: &Tir<'cx, '_>,
    r: &Tir<'cx, '_>,
) -> Result<Type<'cx>, TypeError> {
    use BinOp::*;
    use NirKind::{ListType, RecordType};

    let cx = env.cx();
    let span_err = |msg: &str| mk_span_err(span.clone(), msg);

    Ok(match op {
        RightBiasedRecordMerge => {
            let x_type = l.ty();
            let y_type = r.ty();

            // Extract the LHS record type
            let RecordType(kts_x) = x_type.kind() else {
                return span_err("MustCombineRecord");
            };
            // Extract the RHS record type
            let RecordType(kts_y) = y_type.kind() else {
                return span_err("MustCombineRecord");
            };

            // Union the two records, prefering
            // the values found in the RHS.
            let kts = merge_maps(kts_x, kts_y, |_, _, r_t| r_t.clone());

            let u = max(l.ty().ty(), r.ty().ty());
            Nir::from_kind(RecordType(kts)).to_type(u)
        }
        RecursiveRecordMerge => {
            check_rectymerge(span, &l.ty().to_nir(), &r.ty().to_nir())?;

            let hir = Hir::new(
                HirKind::Expr(ExprKind::Op(OpKind::BinOp(
                    RecursiveRecordTypeMerge,
                    l.ty().to_hir(env.as_varenv()),
                    r.ty().to_hir(env.as_varenv()),
                ))),
                span.clone(),
            );
            let x_u = l.ty().ty();
            let y_u = r.ty().ty();
            Type::new(hir.eval(env), max(x_u, y_u))
        }
        RecursiveRecordTypeMerge => {
            check_rectymerge(span, &l.eval(env), &r.eval(env))?;

            // A RecordType's type is always a const
            let xk = l.ty().as_const().unwrap();
            let yk = r.ty().as_const().unwrap();
            Type::from_const(max(xk, yk))
        }
        ListAppend => {
            if !matches!(l.ty().kind(), ListType(..)) {
                return span_err("BinOpTypeMismatch");
            }

            if l.ty() != r.ty() {
                return span_err("BinOpTypeMismatch");
            }

            l.ty().clone()
        }
        Equivalence => {
            if l.ty() != r.ty() {
                return span_err("EquivalenceTypeMismatch");
            }
            if l.ty().ty().as_const() != Some(Const::Type) {
                return span_err("EquivalenceArgumentsMustBeTerms");
            }

            Type::from_const(Const::Type)
        }
        op => {
            let t = Type::from_builtin(
                cx,
                match op {
                    BoolAnd | BoolOr | BoolEQ | BoolNE => Builtin::Bool,
                    NaturalPlus | NaturalTimes => Builtin::Natural,
                    TextAppend => Builtin::Text,
                    ListAppend
                    | RightBiasedRecordMerge
                    | RecursiveRecordMerge
                    | RecursiveRecordTypeMerge
                    | Equivalence => unreachable!(),
                    ImportAlt => unreachable!("ImportAlt leftover in tck"),
                },
            );

            if *l.ty() != t {
                return span_err("BinOpTypeMismatch");
            }

            if *r.ty() != t {
                return span_err("BinOpTypeMismatch");
            }

            t
        }
    })
}

/// The type a single `merge` handler returns, for the variant it handles.
///
/// `variant_type` is the payload the variant carries, or `None` for a variant
/// that carries nothing — in which case the handler is the return value rather
/// than a function producing it.
fn merge_handler_return_type<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    record: &Tir<'cx, '_>,
    scrut: &Tir<'cx, '_>,
    x: &crate::syntax::Label,
    handler_type: &Nir<'cx>,
    variant_type: Option<&Nir<'cx>>,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::PiClosure;

    // Union alternative without type
    let Some(variant_type) = variant_type else {
        return Type::new_infer_universe(env, handler_type.clone());
    };

    let PiClosure { closure, annot, .. } = handler_type.kind() else {
        return mkerr(
            ErrorBuilder::new(format!("merge handler is not a function"))
                .span_err(span.clone(), format!("in this merge expression"))
                .span_err(
                    record.span(),
                    format!(
                        "the handler for `{}` has type: `{}`",
                        x,
                        handler_type.to_expr_tyenv(env)
                    ),
                )
                .span_help(
                    scrut.span(),
                    format!(
                        "the corresponding variant has type: `{}`",
                        variant_type.to_expr_tyenv(env)
                    ),
                )
                .help(format!(
                    "a handler for this variant must be a function that takes \
                     an input of type: `{}`",
                    variant_type.to_expr_tyenv(env)
                ))
                .format(),
        );
    };

    if variant_type != annot {
        return mkerr(
            ErrorBuilder::new(format!("Wrong handler input type"))
                .span_err(span.clone(), format!("in this merge expression"))
                .span_err(
                    record.span(),
                    format!(
                        "the handler for `{}` expects a value of type: `{}`",
                        x,
                        annot.to_expr_tyenv(env)
                    ),
                )
                .span_err(
                    scrut.span(),
                    format!(
                        "but the corresponding variant has type: `{}`",
                        variant_type.to_expr_tyenv(env)
                    ),
                )
                .format(),
        );
    }

    // TODO: this actually doesn't check anything yet
    match closure.remove_binder() {
        Some(v) => Type::new_infer_universe(env, v.clone()),
        None => mk_span_err(span.clone(), "MergeReturnTypeIsDependent"),
    }
}

fn typecheck_merge<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    record: &Tir<'cx, '_>,
    scrut: &Tir<'cx, '_>,
    type_annot: Option<&Tir<'cx, '_>>,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::{OptionalType, RecordType, UnionType};

    let span_err = |msg: &str| mk_span_err(span.clone(), msg);

    let record_type = record.ty();
    let RecordType(handlers) = record_type.kind() else {
        return span_err("Merge1ArgMustBeRecord");
    };

    let scrut_type = scrut.ty();
    let variants = match scrut_type.kind() {
        UnionType(kts) => Cow::Borrowed(kts),
        OptionalType(ty) => {
            let mut kts = HashMap::new();
            kts.insert("None".into(), None);
            kts.insert("Some".into(), Some(ty.clone()));
            Cow::Owned(kts)
        }
        _ => return span_err("Merge2ArgMustBeUnionOrOptional"),
    };

    let mut inferred_type = None;
    for (x, handler_type) in handlers {
        let Some(variant_type) = variants.get(x) else {
            return span_err("MergeHandlerMissingVariant");
        };
        let handler_return_type = merge_handler_return_type(
            env,
            span,
            record,
            scrut,
            x,
            handler_type,
            variant_type.as_ref(),
        )?;
        match &inferred_type {
            None => inferred_type = Some(handler_return_type),
            Some(t) => {
                if t != &handler_return_type {
                    return span_err("MergeHandlerTypeMismatch");
                }
            }
        }
    }
    for x in variants.keys() {
        if !handlers.contains_key(x) {
            return span_err("MergeVariantMissingHandler");
        }
    }

    let type_annot = type_annot
        .as_ref()
        .map(|t| t.eval_to_type(env))
        .transpose()?;
    Ok(match (inferred_type, type_annot) {
        (Some(t1), Some(t2)) => {
            if t1 != t2 {
                return span_err("MergeAnnotMismatch");
            }
            t1
        }
        (Some(t), None) | (None, Some(t)) => t,
        (None, None) => return span_err("MergeEmptyNeedsAnnotation"),
    })
}

/// Typecheck `f arg`.
fn typecheck_app<'cx>(
    env: &TyEnv<'cx>,
    f: &Tir<'cx, '_>,
    arg: &Tir<'cx, '_>,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::PiClosure;

    // TODO: store Type in closure
    let PiClosure { annot, closure, .. } = f.ty().kind() else {
        return mkerr(
            ErrorBuilder::new(format!(
                "expected function, found `{}`",
                f.ty().to_expr_tyenv(env)
            ))
            .span_err(
                f.span(),
                format!("function application requires a function"),
            )
            .format(),
        );
    };

    if arg.ty().as_nir() != annot {
        return mkerr(
            ErrorBuilder::new(format!("wrong type of function argument"))
                .span_err(
                    f.span(),
                    format!(
                        "this expects an argument of type: {}",
                        annot.to_expr_tyenv(env),
                    ),
                )
                .span_err(
                    arg.span(),
                    format!(
                        "but this has type: {}",
                        arg.ty().to_expr_tyenv(env),
                    ),
                )
                .note(format!(
                    "expected type `{}`\n   found type `{}`",
                    annot.to_expr_tyenv(env),
                    arg.ty().to_expr_tyenv(env),
                ))
                .format(),
        );
    }

    let arg_nf = arg.eval(env);
    Type::new_infer_universe(env, closure.apply(arg_nf))
}

/// Typecheck `toMap record` where `record` has no fields, which is only well
/// typed if the annotation pins down the map's value type.
fn typecheck_tomap_empty<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    annot: Option<Tir<'cx, '_>>,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::{ListType, RecordType};

    let cx = env.cx();
    let span_err = |msg: &str| mk_span_err(span.clone(), msg);

    let Some(annot) = annot else {
        return span_err(
            "`toMap` applied to an empty record requires a type annotation",
        );
    };
    let annot_val = annot.eval_to_type(env)?;

    let err_msg = "The type of `toMap x` must be of the form \
                   `List { mapKey : Text, mapValue : T }`";
    let ListType(arg) = annot_val.kind() else {
        return span_err(err_msg);
    };
    let RecordType(kts) = arg.kind() else {
        return span_err(err_msg);
    };
    if kts.len() != 2 {
        return span_err(err_msg);
    }
    match kts.get("mapKey") {
        Some(t) if *t == Nir::from_builtin(cx, Builtin::Text) => {}
        _ => return span_err(err_msg),
    }
    if kts.get("mapValue").is_none() {
        return span_err(err_msg);
    }
    Ok(annot_val)
}

/// Typecheck `toMap record` where `record` has at least one field. Every field
/// must share a type, which becomes the map's value type.
fn typecheck_tomap_nonempty<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    kts: &HashMap<crate::syntax::Label, Nir<'cx>>,
    annot: Option<Tir<'cx, '_>>,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::RecordType;

    let cx = env.cx();
    let span_err = |msg: &str| mk_span_err(span.clone(), msg);

    let entry_type = kts.iter().next().unwrap().1.clone();
    for t in kts.values() {
        if *t != entry_type {
            return span_err(
                "Every field of the record must have the same type",
            );
        }
    }

    let mut kts = HashMap::new();
    kts.insert("mapKey".into(), Nir::from_builtin(cx, Builtin::Text));
    kts.insert("mapValue".into(), entry_type);
    let output_type: Type = Nir::from_builtin(cx, Builtin::List)
        .app(Nir::from_kind(RecordType(kts)))
        .to_type(Const::Type);
    if let Some(annot) = annot {
        let annot_val = annot.eval_to_type(env)?;
        if output_type != annot_val {
            return span_err("Annotation mismatch");
        }
    }
    Ok(output_type)
}

/// Typecheck `scrut.x`, which selects a record field or a union constructor.
fn typecheck_field<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    scrut: &Tir<'cx, '_>,
    x: &crate::syntax::Label,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::{PiClosure, RecordType, UnionType};

    let span_err = |msg: &str| mk_span_err(span.clone(), msg);

    match scrut.ty().kind() {
        RecordType(kts) => match kts.get(x) {
            Some(val) => Type::new_infer_universe(env, val.clone()),
            None => span_err("MissingRecordField"),
        },
        NirKind::Const(_) => {
            let scrut = scrut.eval_to_type(env)?;
            let UnionType(kts) = scrut.kind() else {
                return span_err("NotARecord");
            };
            match kts.get(x) {
                // Constructor has type T -> < x: T, ... >
                Some(Some(ty)) => Ok(Nir::from_kind(PiClosure {
                    binder: Binder::new(x.clone()),
                    annot: ty.clone(),
                    closure: Closure::new_constant(scrut.to_nir()),
                })
                .to_type(scrut.ty())),
                Some(None) => Ok(scrut),
                None => span_err("MissingUnionField"),
            }
        }
        _ => span_err("NotARecord"),
    }
}

/// Typecheck `record with a.b.c = expr`.
fn typecheck_with<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    record: Tir<'cx, '_>,
    labels: Vec<crate::syntax::Label>,
    expr: Tir<'cx, '_>,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::RecordType;

    let mut record_ty = record.into_ty().into_nir();
    let mut current = &mut record_ty;
    // We dig through the current record type with the provided labels.
    for label in labels {
        let RecordType(kts) = current.kind_mut() else {
            return mk_span_err(span.clone(), "WithMustBeRecord");
        };
        // Get existing entry or insert empty record type into it.
        current = kts
            .entry(label)
            .or_insert_with(|| Nir::from_kind(RecordType(HashMap::new())));
    }
    *current = expr.into_ty().into_nir();

    Type::new_infer_universe(env, record_ty)
}

pub fn typecheck_operation<'cx>(
    env: &TyEnv<'cx>,
    span: &Span,
    opkind: OpKind<Tir<'cx, '_>>,
) -> Result<Type<'cx>, TypeError> {
    use NirKind::RecordType;
    use OpKind::*;

    let cx = env.cx();
    let span_err = |msg: &str| mk_span_err(span.clone(), msg);

    Ok(match opkind {
        App(f, arg) => typecheck_app(env, &f, &arg)?,
        BinOp(o, l, r) => typecheck_binop(env, span, o, &l, &r)?,
        BoolIf(x, y, z) => {
            if *x.ty().kind() != NirKind::from_builtin(cx, Builtin::Bool) {
                return span_err("InvalidPredicate");
            }
            if y.ty().ty().as_const().is_none() {
                return span_err("IfBranchMustBeTermTypeOrKind");
            }
            if y.ty() != z.ty() {
                return span_err("IfBranchMismatch");
            }

            y.ty().clone()
        }
        Merge(record, scrut, type_annot) => {
            typecheck_merge(env, span, &record, &scrut, type_annot.as_ref())?
        }
        ToMap(record, annot) => {
            if record.ty().ty().as_const() != Some(Const::Type) {
                return span_err("`toMap` only accepts records of type `Type`");
            }
            let record_t = record.ty();
            let RecordType(kts) = record_t.kind() else {
                return span_err("The argument to `toMap` must be a record");
            };

            if kts.is_empty() {
                typecheck_tomap_empty(env, span, annot)?
            } else {
                typecheck_tomap_nonempty(env, span, kts, annot)?
            }
        }
        Field(scrut, x) => typecheck_field(env, span, &scrut, &x)?,
        Projection(record, labels) => {
            let record_type = record.ty();
            let RecordType(kts) = record_type.kind() else {
                return span_err("ProjectionMustBeRecord");
            };

            let mut new_kts = HashMap::new();
            for l in labels {
                match kts.get(&l) {
                    None => return span_err("ProjectionMissingEntry"),
                    Some(t) => {
                        new_kts.insert(l.clone(), t.clone());
                    }
                }
            }

            Type::new_infer_universe(env, Nir::from_kind(RecordType(new_kts)))?
        }
        ProjectionByExpr(record, selection) => {
            let record_type = record.ty();
            let RecordType(rec_kts) = record_type.kind() else {
                return span_err("ProjectionMustBeRecord");
            };

            let selection_val = selection.eval_to_type(env)?;
            let RecordType(sel_kts) = selection_val.kind() else {
                return span_err("ProjectionByExprTakesRecordType");
            };

            for (l, sel_ty) in sel_kts {
                match rec_kts.get(l) {
                    Some(rec_ty) => {
                        if rec_ty != sel_ty {
                            return span_err("ProjectionWrongType");
                        }
                    }
                    None => return span_err("ProjectionMissingEntry"),
                }
            }

            selection_val
        }
        With(record, labels, expr) => {
            typecheck_with(env, span, record, labels, expr)?
        }
        Completion(..) => {
            unreachable!("This case should have been handled in resolution")
        }
    })
}
