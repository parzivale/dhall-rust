# Line coverage for the workspace. `nix build .#coverage`.
#
# Produces $out/lcov.info for tooling and $out/html for reading, plus a summary
# on stdout during the build.
#
# Replaces the old .github/workflows/coverage.yml, which used nightly's
# `-Zprofile`. That was removed from rustc, which is why the workflow had been
# sitting at `if: false` with "it's broken, don't know how to fix".
{ pkgs, package, dhall-lang }:

let
  proxy = import ./spoof-proxy.nix { inherit pkgs; };
  llvm = pkgs.llvmPackages.bintools-unwrapped;
in package.overrideAttrs (old: {
  pname = "sessiond-dhall-rust-coverage";

  nativeBuildInputs = (old.nativeBuildInputs or [ ])
    ++ proxy.nativeBuildInputs ++ [ pkgs.cargo-llvm-cov ];

  DHALL_LANG_DIR = dhall-lang;

  # cargo-llvm-cov needs the llvm tools that match the compiler. Pointed at
  # directly rather than put on PATH, because llvmPackages.bintools also
  # carries a linker, which would shadow the one the build uses.
  LLVM_COV = "${llvm}/bin/llvm-cov";
  LLVM_PROFDATA = "${llvm}/bin/llvm-profdata";

  # Replaces the build entirely: cargo-llvm-cov drives its own instrumented
  # build and test run, so the usual buildPhase would just be wasted work.
  buildPhase = ''
    runHook preBuild
    ${proxy.setup}

    mkdir -p $out

    # `--no-fail-fast` runs the whole suite rather than stopping at the first
    # failure, so the report covers everything. A failure still fails the
    # build: cargo's exit status carries through.
    cargo llvm-cov --workspace --no-fail-fast \
      --lcov --output-path $out/lcov.info
    # `--output-dir` gets its own `html` subdirectory, so point it at $out.
    cargo llvm-cov report --html --output-dir $out
    cargo llvm-cov report --summary-only | tee $out/summary.txt

    ${proxy.teardown}
    runHook postBuild
  '';

  # The report is the output; there is nothing to install or test on top.
  doCheck = false;
  installPhase = "true";
  dontFixup = true;
})
