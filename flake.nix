{
  description = "Implementation of the Dhall configuration language in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

    # The dhall-lang standard, which supplies the bulk of the spec test suite.
    # Replaces the git submodule that used to live at ./dhall-lang. Reaches the
    # test harness as DHALL_LANG_DIR; see stage_test_root in dhall/tests/spec.rs.
    dhall-lang = {
      url = "github:dhall-lang/dhall-lang/204a9d9dd167d2c9038539148a09825ded62f1b8";
      flake = false;
    };
  };

  # Everything of substance lives in nix/modules; this wires it together.
  outputs = { self, nixpkgs, dhall-lang }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];

      forAllSystems = f:
        nixpkgs.lib.genAttrs systems
          (system: f nixpkgs.legacyPackages.${system});

      packageFor = pkgs: import ./nix/modules/package.nix { inherit pkgs; };
    in {
      packages = forAllSystems (pkgs: rec {
        default = dhall-rust;
        dhall-rust = packageFor pkgs;
      });

      checks = forAllSystems (pkgs:
        import ./nix/modules/checks.nix {
          inherit pkgs dhall-lang;
          package = packageFor pkgs;
        });

      apps = forAllSystems (pkgs: import ./nix/modules/apps.nix { inherit pkgs; });

      devShells = forAllSystems (pkgs: {
        default = import ./nix/modules/devshell.nix { inherit pkgs dhall-lang; };
      });
    };
}
