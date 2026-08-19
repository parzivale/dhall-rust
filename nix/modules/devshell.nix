# Development environment. `nix develop`, or automatically via .envrc.
#
# Still load-bearing for CI: the wasm and security-audit jobs run through
# `nix develop`, because both need network and so cannot be flake checks.
# The dhall binary and fd used to live here for update-tests.sh; that is now
# `nix run .#update-tests`, which carries its own.
{ pkgs, dhall-lang }:

pkgs.mkShell {
  nativeBuildInputs = [ pkgs.pkg-config ];

  buildInputs = with pkgs; [
    cargo
    rustc
    clippy
    rustfmt
    rust-analyzer

    openssl

    # serde_dhall has a wasm test target. wasm32-unknown-unknown links with
    # lld, and `wasm-pack test --node` needs a node to run the result in.
    wasm-pack
    lld
    nodejs

    # `cargo audit` for the security workflow; also handy locally.
    cargo-audit

    # `cargo llvm-cov` for the coverage workflow.
    cargo-llvm-cov
  ];

  # cargo-llvm-cov needs the matching llvm tools. Pointed at directly rather
  # than put on PATH: llvmPackages.bintools also carries a linker, which would
  # shadow the one the normal build uses.
  LLVM_COV = "${pkgs.llvmPackages.bintools-unwrapped}/bin/llvm-cov";
  LLVM_PROFDATA =
    "${pkgs.llvmPackages.bintools-unwrapped}/bin/llvm-profdata";

  # The spec suite reads the standard tests from here. Pointing it at the pinned
  # input is what lets ./dhall-lang stay out of the working tree entirely -- see
  # stage_test_root in dhall/tests/spec.rs.
  DHALL_LANG_DIR = dhall-lang;
}
