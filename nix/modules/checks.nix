# The test suite, run sandboxed.
#
# Part of the spec suite resolves remote imports, which the sandbox has no
# network for, so those hosts are served locally -- see ./spoof-proxy.nix.
{
  pkgs,
  package,
  dhall-lang,
}:

let
  proxy = import ./spoof-proxy.nix { inherit pkgs; };
in
rec {
  # Builds the workspace from the locked closure alone; no tests.
  build = package;

  # Both languages, so that `nix flake check` covers the formatting of the
  # build itself and not only of the code it builds. `nix fmt` is the other
  # half of this; see `formatter` in flake.nix.
  fmt =
    pkgs.runCommand "sessiond-dhall-rust-fmt"
      {
        nativeBuildInputs = [
          pkgs.cargo
          pkgs.rustfmt
          pkgs.nixfmt
        ];
      }
      ''
        cd ${package.src}
        cargo fmt --all -- --check
        find . -name '*.nix' -print0 | xargs -0 nixfmt --check
        touch $out
      '';

  # The lint set lives in the workspace `[lints.clippy]` table, which on its own
  # only asks; this is what tells. `--all-targets` so the tests are linted too,
  # and `-D warnings` because a warning nothing fails on is a warning that
  # accumulates.
  clippy = package.overrideAttrs (old: {
    pname = "sessiond-dhall-rust-clippy";

    nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.clippy ];

    buildPhase = ''
      runHook preBuild
      cargo clippy --workspace --all-targets --offline -- -D warnings
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      touch $out
      runHook postInstall
    '';
  });

  # nix/lib/to-dhall.nix, checked by typechecking what it emits. The fixture
  # evaluating at all is half the test: it asserts that the values Dhall
  # cannot represent are rejected rather than mistranslated.
  #
  # Deliberately the Haskell dhall rather than this implementation: it makes
  # the check say the output is standard Dhall, not merely Dhall we agree
  # with.
  to-dhall =
    pkgs.runCommand "sessiond-dhall-rust-to-dhall"
      {
        nativeBuildInputs = [ pkgs.dhall ];
        source = import ../lib/to-dhall-test.nix;
        passAsFile = [ "source" ];
      }
      ''
        cp "$sourcePath" fixture.dhall
        dhall type --file fixture.dhall
        touch $out
      '';

  tests = package.overrideAttrs (old: {
    pname = "sessiond-dhall-rust-tests";
    doCheck = true;

    nativeCheckInputs = (old.nativeCheckInputs or [ ]) ++ proxy.nativeBuildInputs;

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
    pname = "sessiond-dhall-rust-tests-debug";
    cargoBuildType = "debug";
    cargoCheckType = "debug";
  });
}
