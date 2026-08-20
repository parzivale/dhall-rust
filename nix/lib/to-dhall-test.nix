# Fixture for the `to-dhall` check. Evaluates to Dhall source exercising
# everything the converter can emit; the check then typechecks that source
# with an independent implementation, so this verifies we emit *standard*
# Dhall rather than merely something we would accept ourselves.
#
# The cases that must be rejected are asserted here instead, since a throw is
# not something a Dhall file can show.
let
  inherit (import ./to-dhall.nix) toDhall lambda raw;

  # Forces `v` and expects the conversion to have thrown.
  rejects =
    name: v:
    let
      result = builtins.tryEval (builtins.deepSeq v v);
    in
    if result.success then
      throw "to-dhall: expected ${name} to be rejected, got: ${toString v}"
    else
      true;

  rejected = [
    (rejects "null" (toDhall {
      a = null;
    }))
    (rejects "an empty list" (toDhall {
      a = [ ];
    }))
    (rejects "a bare lambda" (toDhall {
      a = x: x;
    }))
    (rejects "a heterogeneous list" (toDhall {
      a = [
        1
        "two"
      ];
    }))
    (rejects "a backquote in a field name" (toDhall {
      "a`b" = 1;
    }))
    (rejects "a non-string raw value" (toDhall {
      a = raw 1;
    }))
    (rejects "a lambda body that is not a function" (toDhall (lambda "x" "Natural" 1)))
    (rejects "a parameter type that is neither source nor an attrset" (toDhall (lambda "x" 1 (x: x))))
  ];

  value =
    lambda "input"
      {
        host = "Text";
        port = "Natural";
        db = {
          user = "Text";
        };
      }
      (input: {
        # References into the parameter, at every depth.
        server = { inherit (input) host port; };
        username = input.db.user;
        whole = input;

        # Scalars. A negative int is an Integer, a non-negative one a Natural.
        debug = false;
        retries = 3;
        negative = -3;
        ratio = 0.5;
        whole_double = 2.0;

        # Text, including everything that has to be escaped.
        escapes = ''
          say "hi"
          	tab \ and ''${""}'';

        # Labels needing backquotes: a keyword, and a name that is not a label.
        "let" = "quoted keyword";
        "weird name" = true;

        # Collections. `{=}` is the empty record literal.
        tags = [
          "a"
          "b"
        ];
        matrix = [
          {
            x = 1;
            y = 2;
          }
          {
            x = 3;
            y = 4;
          }
        ];
        empty = { };

        # Values Nix cannot express, via the escape hatch.
        absent = raw "None Natural";
        nothing = raw "[] : List Text";
        url = raw ''"http://''${input.host}"'';

        # Anything store-pathish becomes its path, rather than being walked.
        drv = {
          outPath = "/nix/store/0000000000000000000000000000000-x";
        };

        # A lambda nested in a value, and the chain that makes a function of two
        # arguments out of one-argument lambdas.
        double = lambda "n" "Natural" (n: {
          doubled = n;
        });
        pair = lambda "a" "Text" (
          a:
          lambda "b" "Natural" (b: {
            inherit a b;
          })
        );
      });

in
builtins.deepSeq rejected (toDhall value)
