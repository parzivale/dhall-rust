# Development environment. `nix develop`, or automatically via .envrc.
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

    # serde_dhall has a wasm test target.
    wasm-pack

    # `nix run .#update-tests` brings its own dhall and fd, but they are handy
    # to have directly. cbor2diag.rb is not in nixpkgs -- if you need to
    # regenerate parser .diag files, `gem install cbor-diag` first.
    dhall
    fd
    ruby
  ];

  # The spec suite reads the standard tests from here. Pointing it at the pinned
  # input is what lets ./dhall-lang stay out of the working tree entirely -- see
  # stage_test_root in dhall/tests/spec.rs.
  DHALL_LANG_DIR = dhall-lang;
}
