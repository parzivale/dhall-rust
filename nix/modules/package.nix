# The workspace, built from the locked closure.
{ pkgs }:

let
  lib = pkgs.lib;

  src = lib.cleanSourceWith {
    src = ../..;
    filter =
      path: type:
      let
        base = baseNameOf (toString path);
      in
      !(builtins.elem base [
        "target"
        "dhall-lang"
        ".git"
        "result"
      ]);
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "sessiond-dhall-rust";
  # Keep in step with the crate versions; nothing checks this automatically.
  version = "2.0.0";
  inherit src;

  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.openssl ];

  # Part of the spec suite fetches remote imports, and the nix sandbox has no
  # network. The tests are the job of checks.nix, which stands up a proxy for
  # them; this target builds only.
  doCheck = false;

  meta = with lib; {
    description = "Implementation of the Dhall configuration language";
    homepage = "https://github.com/parzivale/dhall-rust";
    license = licenses.bsd2;
  };
}
