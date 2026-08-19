# Serves the suite's remote imports locally, for running the tests with no
# network. See ../spoof-imports.py for what it serves and why.
#
# Returns the pieces a derivation needs to stand the proxy up, so that the test
# checks and the coverage package share one definition rather than two that
# drift.
{ pkgs }:

let
  spoofAddon = ../spoof-imports.py;
  port = "8080";
in {
  # mitmproxy for the proxy; git because the grammar-sync test shells out to
  # `git diff --no-index`.
  nativeBuildInputs = [ pkgs.mitmproxy pkgs.git ];

  setup = ''
    export HOME="$TMPDIR/home"
    mkdir -p "$HOME"

    # Any request the addon cannot serve lands here, so a dhall-lang bump that
    # introduces an unmapped URL says so, rather than failing a test on the
    # contents of our 404 page. Exported before the proxy starts: it is read by
    # that child, which inherits the environment as it was at spawn.
    export SPOOF_MISS_LOG="$TMPDIR/spoof-misses"

    # Reported from a trap, not afterwards: an unmapped URL usually *fails* the
    # test that requested it, and a failing check phase means later phases never
    # run -- so the report would appear only when it was not needed.
    reportSpoofMisses() {
      [ -s "$SPOOF_MISS_LOG" ] || return 0
      echo "" >&2
      echo "The tests requested URLs that nix/spoof-imports.py does" >&2
      echo "not map. Add them there, or to its EXPECTED_404 if a 404" >&2
      echo "is the right answer:" >&2
      sort -u "$SPOOF_MISS_LOG" | sed 's/^/  /' >&2
      echo "" >&2
    }
    trap reportSpoofMisses EXIT

    confdir="$TMPDIR/mitmproxy"
    # connection_strategy=lazy stops mitmproxy dialling the real host before
    # running the addon -- there is no network to dial.
    mitmdump --quiet \
      --set confdir="$confdir" \
      --set connection_strategy=lazy \
      --listen-host 127.0.0.1 --listen-port ${port} \
      --scripts ${spoofAddon} \
      >"$TMPDIR/mitmdump.log" 2>&1 &
    mitmPid=$!

    # Both the port and the generated CA have to be up before any test runs, or
    # the first import races the proxy's startup.
    for _ in $(seq 1 200); do
      if [ -f "$confdir/mitmproxy-ca-cert.pem" ] \
         && (exec 3<>/dev/tcp/127.0.0.1/${port}) 2>/dev/null; then
        break
      fi
      sleep 0.1
    done

    if ! kill -0 "$mitmPid" 2>/dev/null; then
      echo "import-spoofing proxy failed to start:" >&2
      cat "$TMPDIR/mitmdump.log" >&2
      exit 1
    fi

    export HTTP_PROXY=http://127.0.0.1:${port}
    export HTTPS_PROXY=http://127.0.0.1:${port}
    # nixpkgs patches openssl to consult NIX_SSL_CERT_FILE ahead of
    # SSL_CERT_FILE, and the sandbox presets it to /no-cert-file.crt. Both have
    # to point at the proxy's CA or the handshake is rejected.
    export SSL_CERT_FILE="$confdir/mitmproxy-ca-cert.pem"
    export NIX_SSL_CERT_FILE="$confdir/mitmproxy-ca-cert.pem"
  '';

  teardown = ''
    kill "$mitmPid" 2>/dev/null || true

    # Tests can pass while still hitting an unmapped URL (an ignored test, or
    # one that tolerates the 404 body); fail on that too.
    if [ -s "$SPOOF_MISS_LOG" ]; then
      exit 1
    fi
  '';
}
