use std::convert::TryFrom;

use sessiond_dhall::error::Error;
use sessiond_dhall::semantics::*;
use sessiond_dhall::syntax::*;
use sessiond_dhall::*;

/// Test that showcases someone using the `dhall` crate directly for a simple operation. If
/// possible try not to break this too much. See
/// <https://github.com/Nadrieril/dhall-rust/issues/208>.
#[test]
fn manual_function_application() {
    /// Apply a `Natural -> Natural` function to an argument.
    fn apply_natnat_fn(f: &Nir<'_>, n: u64) -> u64 {
        // Convert the number to the internal representation. `Natural` is
        // arbitrary-precision, so this widens rather than being a plain `u64`.
        let n_nir = Nir::from_kind(NirKind::Num(NumKind::Natural(n.into())));
        // Apply `f` to `n`.
        let m_nir = f.app(n_nir);
        // Convert from the internal representation. The result may not fit back
        // into a `u64`, so narrowing it is fallible.
        match m_nir.kind() {
            NirKind::Num(NumKind::Natural(m)) => {
                u64::try_from(m).expect("result does not fit in a u64")
            }
            _ => panic!("`f` was not `Natural -> Natural`"),
        }
    }

    /// Auxiliary function to make `?` work.
    fn run(cx: Ctxt<'_>) -> Result<(), Error> {
        // Parse the type we want into the internal representation.
        let f_ty = "Natural -> Natural";
        let f_ty = Parsed::parse_str(f_ty)?
            .skip_resolve(cx)?
            .typecheck(cx)?
            .normalize(cx);

        // Parse the function `f` itself, and also check its type.
        let f = "\\(x: Natural) -> x + 3";
        let f = Parsed::parse_str(f)?
            .skip_resolve(cx)?
            .typecheck_with(cx, &f_ty.to_hir())?
            .normalize(cx);

        // Do whatever we want with `f`.
        for i in 0..5 {
            let n = apply_natnat_fn(f.as_nir(), i);
            assert_eq!(n, i + 3);
        }
        Ok(())
    }

    // The crate uses essentially a global context, created here.
    Ctxt::with_new(run).unwrap();
}
