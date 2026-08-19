# The test suite, run sandboxed.
#
# Part of the spec suite resolves remote imports, which the sandbox has no
# network for, so those hosts are served locally -- see ./spoof-proxy.nix.
{ pkgs, package, dhall-lang }:

let
  proxy = import ./spoof-proxy.nix { inherit pkgs; };
in rec {
  # Builds the workspace from the locked closure alone; no tests.
  build = package;

  # Mirrors what .github/workflows/style.yml enforces, so that `nix flake
  # check` covers everything CI does rather than most of it.
  fmt = pkgs.runCommand "dhall-rust-fmt" {
    nativeBuildInputs = [ pkgs.cargo pkgs.rustfmt ];
  } ''
    cd ${package.src}
    cargo fmt --all -- --check
    touch $out
  '';

  tests = package.overrideAttrs (old: {
    pname = "dhall-rust-tests";
    doCheck = true;

    nativeCheckInputs =
      (old.nativeCheckInputs or [ ]) ++ proxy.nativeBuildInputs;

    # The harness stages its own tree from this, so nothing is written into the
    # source directory.
    DHALL_LANG_DIR = dhall-lang;

    preCheck = proxy.setup;
    postCheck = proxy.teardown;
  });

  # Same suite in debug, where `debug_assertions` changes which tests the
  # harness selects. Release skips nothing debug covers except the
  # `is_too_slow` set, which only runs in release -- so the two together are
  # what covers everything.
  tests-debug = tests.overrideAttrs (_: {
    pname = "dhall-rust-tests-debug";
    cargoBuildType = "debug";
    cargoCheckType = "debug";
  });
}
