# Converts a Nix value to Dhall source text: the Dhall analogue of
# `builtins.toJSON`.
#
#   toDhall { port = 8080; hosts = [ "a" "b" ]; }
#   => { hosts = [ "a", "b" ], port = 8080 }
#
# This converts *values*, not Nix code. Nix is untyped, lazy and
# Turing-complete and Dhall is none of those, so there is no translation of
# arbitrary Nix expressions; what there is, is a translation of what a Nix
# expression evaluates to.
#
# Dhall is typed, so a few Nix values have no unambiguous Dhall counterpart:
# `null` and `[]` both need a type the Nix value does not carry. Rather than
# guess, those are errors naming the path to the offending value, and `raw` is
# the escape hatch for writing the Dhall out by hand.
#
# Depends on nothing but builtins, so it can be imported on its own:
#
#   let inherit (import ./nix/lib/to-dhall.nix) toDhall lambda raw; in ...
#
#
# # Lambdas
#
# A bare Nix lambda cannot be converted. Nix offers no way to look inside one
# -- there is no access to the body, and no annotation to say what the
# parameter's Dhall type would be. The only thing you can do with a Nix
# function is apply it.
#
# So that is what `lambda` does. You give it a parameter name and Dhall type,
# it builds an argument out of *references* into that parameter, applies your
# function to it, and renders the result. Wherever your template put the
# argument, the output has `input.whatever`:
#
#   lambda "input" { host = "Text"; port = "Natural"; } (input: {
#     server = { inherit (input) host port; };
#     debug = false;
#   })
#
#   \(input : { host : Text, port : Natural }) ->
#     { debug = False, server = { host = input.host, port = input.port } }
#
# The argument's leaves are opaque markers, not strings or numbers, so the
# template may only *place* them. Anything that inspects one -- arithmetic,
# comparison, Nix string interpolation -- fails at evaluation, because the
# value it wants is not known until Dhall applies the function. Build such
# expressions with `raw` instead:
#
#   raw "\"http://${"$"}{input.host}\""
#
# A Dhall lambda binds exactly one variable, so a function of several inputs
# is either one record parameter, as above, or a chain. `lambda` nests, which
# is the chain:
#
#   lambda "host" "Text" (host:
#     lambda "port" "Natural" (port: { inherit host port; }))
#
#   \(host : Text) -> \(port : Natural) -> { host = host, port = port }
#
# It composes with `toDhall` either way: the result is an ordinary value that
# can equally sit in a record field or a list element.
let
  inherit (builtins)
    all
    any
    attrNames
    concatStringsSep
    elem
    elemAt
    genList
    length
    map
    mapAttrs
    match
    replaceStrings
    split
    stringLength
    toJSON
    typeOf
    ;

  # Reserved words must be quoted even though they look like ordinary labels.
  # From dhall-lang/standard/dhall.abnf, `keyword`.
  keywords = [
    "if"
    "then"
    "else"
    "let"
    "in"
    "using"
    "missing"
    "assert"
    "as"
    "Infinity"
    "NaN"
    "merge"
    "Some"
    "toMap"
    "forall"
    "with"
  ];

  # Where we are in the value, for error messages: [ ".hosts" "[0]" ].
  showPath = path: if path == [ ] then "at the root" else "at " + concatStringsSep "" path;

  err = path: msg: throw "toDhall: ${msg}, ${showPath path}";

  hasNewline = s: length (split "\n" s) > 1;

  # Short values read better on one line, which is also what `dhall format`
  # does. The threshold is arbitrary.
  fitsInline = parts: !(any hasNewline parts) && stringLength (concatStringsSep ", " parts) <= 60;

  # `simple-label` in the grammar. Anything else has to be backquoted.
  isSimpleLabel = name: match "[A-Za-z_][A-Za-z0-9_/-]*" name != null && !(elem name keywords);

  renderLabel =
    path: name:
    if isSimpleLabel name then
      name
    else if match ".*`.*" name != null then
      # A quoted label runs to the next backquote, so there is no way to put
      # one inside it.
      err path "record field ${toJSON name} contains a backquote, which Dhall cannot quote"
    else
      "`" + name + "`";

  # Dhall's text escapes are a subset of JSON's. The order matters: backslash
  # is replaced first, and `replaceStrings` makes a single pass, so nothing
  # introduced by a replacement is rescanned.
  renderText =
    path: s:
    let
      escaped =
        replaceStrings
          [ "\\" ''"'' "\${" "\n" "\r" "\t" ]
          [
            "\\\\"
            ''\"''
            "\\\${"
            "\\n"
            "\\r"
            "\\t"
          ]
          s;
    in
    if match ".*[[:cntrl:]].*" escaped != null then
      # Nix string literals cannot spell the remaining control characters, so
      # they cannot be escaped here either. Refuse rather than emit a file
      # Dhall will reject with a worse message.
      err path "string contains a control character other than a tab, newline or carriage return"
    else
      ''"'' + escaped + ''"'';

  # A Dhall Double literal needs a fraction or an exponent; `toJSON` drops the
  # fraction from whole floats.
  renderDouble =
    f:
    let
      s = toJSON f;
    in
    if match ".*[.eE].*" s != null then s else s + ".0";

  # A parameter type, written either as Dhall source or as a nested attrset
  # standing for a record type. Note `{}` here is the empty record *type*,
  # which is what it should be in this position.
  renderType =
    path: ind: t:
    let
      parts = map (n: renderLabel path n + " : " + renderType (path ++ [ ".${n}" ]) (ind + "  ") t.${n}) (
        attrNames t
      );
    in
    if typeOf t == "string" then
      t
    else if typeOf t != "set" then
      err path "a parameter type must be Dhall source or an attrset, not a ${typeOf t}"
    else if t == { } then
      "{}"
    else if fitsInline parts then
      "{ " + concatStringsSep ", " parts + " }"
    else
      "{ " + concatStringsSep ("\n" + ind + ", ") parts + "\n" + ind + "}";

  # The argument handed to a `lambda` body: the parameter itself, and a marker
  # for every field reachable in its type, so that `input.db.host` in the
  # template comes out as `input.db.host` in the Dhall.
  argFor =
    path: ref: t:
    if typeOf t == "set" then
      mapAttrs (n: sub: argFor path "${ref}.${renderLabel path n}" sub) t
      // {
        __dhall = ref;
      }
    else
      { __dhall = ref; };

  renderLambda =
    path: ind: spec:
    let
      inherit (spec) param type body;
      head = "\\(" + renderLabel path param + " : " + renderType path (ind + "  ") type + ") ->";
      rendered = render (path ++ [ "(body)" ]) (ind + "    ") (body (argFor path param type));
      inner =
        if hasNewline rendered then head + "\n" + ind + "    " + rendered else head + " " + rendered;
    in
    if typeOf body != "lambda" then
      err path "a lambda's body must be a function of the parameter"
    # A lambda body runs as far as the grammar lets it, so anywhere but the
    # top of a document it has to be bracketed off from what follows.
    else if inBarePosition path then
      inner
    else if hasNewline inner then
      "( " + inner + "\n" + ind + ")"
    else
      "(" + inner + ")";

  # Carries Dhall source rather than a Nix value, so its Nix type says nothing
  # about the Dhall type it will render as.
  isRaw = x: typeOf x == "set" && (x ? __dhall || x ? __dhallLambda);

  # `x`, or `x.y.z`: a primitive expression, which never needs bracketing.
  isReference = text: match "[A-Za-z_][A-Za-z0-9_/-]*(\\.[A-Za-z_][A-Za-z0-9_/-]*)*" text != null;

  # A lambda body runs to the end of the enclosing expression, so at the top
  # of a document -- or in the body of a lambda that is itself there -- there
  # is nothing an expression could run into, and no brackets are needed.
  inBarePosition = path: all (p: p == "(body)") path;

  renderList =
    path: ind: xs:
    let
      parts = genList (i: render (path ++ [ "[${toString i}]" ]) (ind + "  ") (elemAt xs i)) (length xs);
    in
    if xs == [ ] then
      err path ''an empty list needs a type; write it as raw "[] : List Natural"''
    else if !(any isRaw xs) && !(all (x: typeOf x == typeOf (elemAt xs 0)) xs) then
      # Dhall would reject this too, but pointing at the Nix value is more use
      # than pointing at a line of generated source. Skipped once an escape
      # hatch is in play, since a Nix type is then no guide to the Dhall one.
      err path "list elements have different types, which Dhall does not allow"
    else if fitsInline parts then
      "[ " + concatStringsSep ", " parts + " ]"
    else
      "[ " + concatStringsSep ("\n" + ind + ", ") parts + "\n" + ind + "]";

  renderField =
    path: ind: attrs: name:
    let
      label = renderLabel path name;
      value = render (path ++ [ ".${name}" ]) (ind + "    ") attrs.${name};
    in
    if hasNewline value then label + " =\n" + ind + "    " + value else label + " = " + value;

  renderAttrs =
    path: ind: attrs:
    let
      parts = map (renderField path ind attrs) (attrNames attrs);
    in
    if attrs ? __dhallLambda then
      renderLambda path ind attrs.__dhallLambda
    # Raw Dhall, emitted as written. Bracketed off from its surroundings
    # everywhere but the top of a document, where there is nothing to
    # bracket it from.
    else if attrs ? __dhall then
      if typeOf attrs.__dhall != "string" then
        err path "raw Dhall must be a string"
      # A variable or a field access binds tighter than anything it could
      # land next to, so it needs no brackets. Most raw values are one,
      # since that is what a `lambda` argument is made of.
      else if inBarePosition path || isReference attrs.__dhall then
        attrs.__dhall
      else
        "(" + attrs.__dhall + ")"
    # Derivations, flake inputs and anything else store-pathish. Recursing
    # into one would walk a very large attrset and hit a function almost
    # immediately.
    else if attrs ? outPath then
      renderText path (toString attrs.outPath)
    # `{}` is the empty record *type*; the empty record literal is `{=}`.
    else if attrs == { } then
      "{=}"
    else if fitsInline parts then
      "{ " + concatStringsSep ", " parts + " }"
    else
      "{ " + concatStringsSep ("\n" + ind + ", ") parts + "\n" + ind + "}";

  render =
    path: ind: v:
    let
      t = typeOf v;
    in
    if t == "bool" then
      (if v then "True" else "False")
    # Non-negative is a Natural, negative is an Integer -- Dhall writes the
    # latter with its sign, which `toString` already does.
    else if t == "int" then
      toString v
    else if t == "float" then
      renderDouble v
    else if t == "string" then
      renderText path v
    # Interpolating the path is what copies it to the store, if it is not
    # there already; that is Nix's doing, not ours.
    else if t == "path" then
      renderText path (toString v)
    else if t == "list" then
      renderList path ind v
    else if t == "set" then
      renderAttrs path ind v
    else if t == "null" then
      err path ''null needs a type in Dhall; write it as raw "None Natural"''
    else if t == "lambda" then
      err path ''a bare lambda cannot be converted, because Nix cannot see inside one; wrap it with `lambda "name" type` to say what the parameter is called and what Dhall type it has''
    else
      err path "cannot convert a ${t}";

in
{
  # Nix value -> Dhall source.
  toDhall = value: render [ ] "" value;

  # Dhall source, spliced in as written. For the values Nix cannot express on
  # its own: `raw "None Natural"`, `raw "[] : List Text"`, a union, an import.
  raw = text: { __dhall = text; };

  # A Dhall function whose parameter supplies whatever is not known until the
  # function is applied. Curried so that the nesting that builds a
  # multi-argument function stays readable:
  #
  #   lambda "port" "Natural" (port: { inherit port; })
  #
  # `type` is the parameter's Dhall type, written as source or as a nested
  # attrset standing for a record type; `body` receives references into the
  # parameter.
  lambda = param: type: body: { __dhallLambda = { inherit param type body; }; };
}
