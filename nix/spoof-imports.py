"""Serves dhall-lang's remote import fixtures locally.

The dhall standard test suite exercises remote imports against live hosts.
Running the suite inside the nix sandbox means no outbound network, so this
answers those requests locally instead, keeping `nix flake check` hermetic.

Used two ways:

  mitmdump --set connection_strategy=lazy -s spoof-imports.py
      Run as a mitmproxy addon, serving the hosts below.

Guarding against drift: the mappings below are hand-written against a dhall-lang
pin that moves. Bumping the pin could introduce a fixture pointing at a URL
nothing here maps, and the only symptom would be a test failing on the *body* of
our 404 page, which points nowhere near the cause.

So every request that is not served, and not in EXPECTED_404, is appended to
$SPOOF_MISS_LOG, and the check fails if that file ends up non-empty. This works
from the requests the run actually made rather than by scanning fixtures for
URLs: a scan cannot tell which URLs get fetched without reimplementing the
harness's ignore rules, and reports `as Location` targets, parser fixtures and
expected-output files as false positives.

Three sources, mirroring what the real hosts serve:

  raw.githubusercontent.com/dhall-lang/dhall-lang/<ref>/<path>
      From the pinned checkout at $DHALL_LANG_DIR/<path>. The <ref> is ignored:
      the fixtures name several revisions, and the pin is the one we have.

  raw.githubusercontent.com/Nadrieril/dhall-rust/<ref>/<path>
      dhall-lang's asLocation/RemoteChain fixtures chain through upstream
      dhall-rust at a revision whose files no longer exist on master. The four
      they need are one-liners, inlined as DHALL_RUST_BODIES.

  test.dhall-lang.org/cors/<name>.dhall
      Exists only on that server, defined inline in dhall-lang's
      nixops/logical.nix. Bodies reproduced in CORS_BODIES.

MAPPING NOTES

Anything else on those hosts gets a 404 whose body is exactly what the real
servers return. That body matters: dhall parses the error page as source, so
import/failure/unit/404 asserts on the resulting `unbound variable ` Not``.

Three of the DHALL_RUST_BODIES are byte-identical to dhall-lang's own copies and
could be served from that pin instead. They are not, deliberately: those URLs
name a frozen revision, whereas the dhall-lang pin moves. Serving them from a
moving pin would let an upstream edit silently change what a frozen URL returns.

There is deliberately no CORS emulation. dhall-rust rejects every
remote-to-remote import outright (`SanityCheck` in resolve.rs, where the CORS
check is still a TODO) and issues a plain GET that never sees the headers, so
response bodies are the only thing affecting outcomes. Implementing that TODO
would mean revisiting this file.
"""

import os
import pathlib
import sys

GITHUB_HOST = "raw.githubusercontent.com"
TEST_HOST = "test.dhall-lang.org"

DHALL_LANG_PREFIX = "/dhall-lang/dhall-lang/"
DHALL_RUST_PREFIX = "/Nadrieril/dhall-rust/"

# What GitHub raw and the nginx default return for a missing path. dhall feeds
# the body to its parser, so the exact text is load-bearing.
NOT_FOUND_BODY = "404: Not Found"

# URLs the suite expects to 404. Not treated as misses, since here a 404 is the
# correct answer rather than a missing mapping.
EXPECTED_404 = {
    (TEST_HOST, "/nonexistent-file.dhall"),
    # Hash-protected imports resolved from the seeded cache; the URL is only
    # reached if the cache misses, and 404 is what upstream serves.
    (TEST_HOST, "/random-string"),
}

# Files from upstream dhall-rust at f7d8c64a, keyed by path below the revision.
# Note EnvA uses HOME where dhall-lang's otherwise-identical copy uses
# DHALL_TEST_VAR; RemoteChainEnvB asserts on `.Environment "HOME"`, so this one
# is not interchangeable with the dhall-lang version.
_AS_LOCATION = "dhall/tests/import/success/unit/asLocation/"

DHALL_RUST_BODIES = {
    _AS_LOCATION + "Canonicalize3A.dhall": "./../bar/import.dhall as Location",
    _AS_LOCATION
    + "Canonicalize5A.dhall": "./foo/../../bar/import.dhall as Location",
    _AS_LOCATION + "MissingA.dhall": "missing as Location",
    _AS_LOCATION + "EnvA.dhall": "env:HOME as Location",
}

_GITHUB_CORS = (
    "https://raw.githubusercontent.com/dhall-lang/dhall-lang/"
    "5ff7ecd2411894dd9ce307dc23020987361d2d43/tests/import/data/cors/"
)

# From the `cors-endpoint` calls in dhall-lang's nixops/logical.nix. nginx
# serves them with `echo`, which appends a newline.
CORS_BODIES = {
    "AllowedAll.dhall": "42",
    "OnlyGithub.dhall": "42",
    "OnlySelf.dhall": "42",
    "OnlyOther.dhall": "42",
    "Empty.dhall": "42",
    "NoCORS.dhall": "42",
    "Null.dhall": "42",
    "SelfImportAbsolute.dhall": "https://test.dhall-lang.org/cors/NoCORS.dhall",
    "SelfImportRelative.dhall": "./NoCORS.dhall",
    "TwoHopsFail.dhall": _GITHUB_CORS + "OnlySelf.dhall",
    "TwoHopsSuccess.dhall": _GITHUB_CORS + "OnlyGithub.dhall",
}


def _dhall_lang_dir() -> pathlib.Path:
    directory = os.environ.get("DHALL_LANG_DIR")
    if not directory:
        raise RuntimeError("DHALL_LANG_DIR is not set")
    return pathlib.Path(directory)


def _strip_ref(path: str, prefix: str) -> str:
    """`/<owner>/<repo>/<ref>/rest` -> `rest`."""
    _ref, _, rest = path[len(prefix):].partition("/")
    return rest


def _read_from_pin(rel_path: str) -> "str | None":
    root = _dhall_lang_dir()
    target = root / rel_path
    try:
        # Resolve before comparing, so a `..` in a fixture cannot escape.
        target = target.resolve(strict=True)
        target.relative_to(root.resolve())
        return target.read_text()
    except (OSError, ValueError):
        return None


def lookup(host: str, path: str) -> "tuple[int, str]":
    """Resolve a request to (status, body)."""
    if host == GITHUB_HOST:
        if path.startswith(DHALL_LANG_PREFIX):
            body = _read_from_pin(_strip_ref(path, DHALL_LANG_PREFIX))
            return (200, body) if body is not None else (404, NOT_FOUND_BODY)
        if path.startswith(DHALL_RUST_PREFIX):
            body = DHALL_RUST_BODIES.get(_strip_ref(path, DHALL_RUST_PREFIX))
            return (
                (200, body + "\n") if body is not None else (404, NOT_FOUND_BODY)
            )
        return 404, NOT_FOUND_BODY

    if host == TEST_HOST:
        name = path[len("/cors/"):] if path.startswith("/cors/") else None
        if name in CORS_BODIES:
            return 200, CORS_BODIES[name] + "\n"
        return 404, NOT_FOUND_BODY

    # Reaching anything else would need the network this stands in for.
    return 502, f"spoof-imports: unexpected host {host}{path}\n"


def _canonicalize(path: str) -> str:
    """Collapse `..` segments, so `/foo/../x` matches a mapping for `/x`."""
    return os.path.normpath(path)


def _record_miss(host: str, path: str, status: int) -> None:
    """Note a request we could not serve, for the build to fail on."""
    line = f"{status} https://{host}{path}\n"
    print(f"spoof-imports: unmapped {line.strip()}", file=sys.stderr)

    miss_log = os.environ.get("SPOOF_MISS_LOG")
    if miss_log:
        with open(miss_log, "a", encoding="utf-8") as handle:
            handle.write(line)


def request(flow) -> None:
    """mitmproxy addon hook."""
    from mitmproxy import http

    host = flow.request.pretty_host
    path = flow.request.path
    status, body = lookup(host, path)

    if status != 200 and (host, _canonicalize(path)) not in EXPECTED_404:
        _record_miss(host, path, status)

    flow.response = http.Response.make(
        status, body, {"Content-Type": "text/plain; charset=utf-8"}
    )


