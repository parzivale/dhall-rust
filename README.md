<img src="https://github.com/dhall-lang/dhall-lang/blob/master/img/dhall-logo.svg" width="600" alt="Dhall Logo">

[![crates.io][cratesio-badge]][cratesio-url]
[![documentation][docs-badge]][docs-url]
[![CI status][ci-badge]][ci-url]
[![coverage status][codecov-badge]][codecov-url]
[![dependency status][depsrs-badge]][depsrs-url]

[cratesio-badge]: https://img.shields.io/crates/v/sessiond-serde-dhall.svg?style=flat-square
[docs-badge]: https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square
[ci-badge]: https://img.shields.io/github/actions/workflow/status/parzivale/dhall-rust/tests.yml?branch=master&style=flat-square
[codecov-badge]: https://img.shields.io/codecov/c/github/parzivale/dhall-rust?style=flat-square
[depsrs-badge]: https://deps.rs/repo/github/parzivale/dhall-rust/status.svg

[cratesio-url]: https://crates.io/crates/sessiond-serde-dhall
[docs-url]: https://docs.rs/sessiond-serde-dhall
[ci-url]: https://github.com/parzivale/dhall-rust/actions
[codecov-url]: https://codecov.io/gh/parzivale/dhall-rust
[depsrs-url]: https://deps.rs/repo/github/parzivale/dhall-rust

Dhall is a programmable configuration language optimized for
maintainability.

You can think of Dhall as: JSON + functions + types + imports

Note that while Dhall is programmable, Dhall is not Turing-complete.  Many
of Dhall's features take advantage of this restriction to provide stronger
safety guarantees and more powerful tooling.

You can find more details about the language by visiting the official website:

* [https://dhall-lang.org](http://dhall-lang.org/)

# STATUS

This is a maintained fork of [Nadrieril/dhall-rust][upstream], whose author
stopped maintaining it and invited someone else to take it on. It is published
under the `sessiond-` prefix so as not to claim the original crate names, and
keeps the original BSD-2-Clause licence and copyright. Versioning restarts at
`1.0.0` and is independent of upstream's `0.13.x`.

| Crate | Use it for |
| --- | --- |
| [`sessiond-serde-dhall`](https://crates.io/crates/sessiond-serde-dhall) | The public API. This is the one you want |
| [`sessiond-dhall`](https://crates.io/crates/sessiond-dhall) | Internal. The language implementation; no semver guarantees |
| [`sessiond-dhall-proc-macros`](https://crates.io/crates/sessiond-dhall-proc-macros) | Internal. Derive macros used by the above |

It passes the whole dhall-lang test suite bar two cases (see
[Standard-compliance](#standard-compliance)), where upstream left roughly a
tenth of it unimplemented. See [Differences from
upstream](#differences-from-upstream) for what changed, and
[CHANGELOG.md](CHANGELOG.md) for the full list — **read it before upgrading
from `serde_dhall`**, since some of it is breaking.

[upstream]: https://github.com/Nadrieril/dhall-rust

# `sessiond-dhall-rust`

This is the Rust implementation of the Dhall configuration language.
It is meant to be used to integrate Dhall in your application.

If you only want to convert Dhall to/from JSON or YAML, you should use the
official tooling instead; instructions can be found
[here](https://docs.dhall-lang.org/tutorials/Getting-started_Generate-JSON-or-YAML.html).

## Usage

The supported way of integrating Dhall in your application is via the
`sessiond-serde-dhall` crate, which handles both reading Dhall into Rust values and
writing Rust values back out as Dhall.

Add this to your `Cargo.toml`:

```toml
[dependencies]
sessiond-serde-dhall = "2.1.0"
```

Reading Dhall files is easy and leverages the wonderful [`serde`](https://crates.io/crates/serde) library.

```rust
use std::collections::BTreeMap;

// Some Dhall data
let data = "{ x = 1, y = 1 + 1 } : { x: Natural, y: Natural }";

// Deserialize it to a Rust type.
let deserialized_map: BTreeMap<String, u64> = sessiond_serde_dhall::from_str(data).parse().unwrap();

let mut expected_map = BTreeMap::new();
expected_map.insert("x".to_string(), 1);
expected_map.insert("y".to_string(), 2);

assert_eq!(deserialized_map, expected_map);
```

Serialization goes the other way, via `serialize()`. See the [crate
docs][docs-url] for the full API; a few things worth knowing about are below.

### Functions

A Dhall function deserializes into a `Function`, which you can call from Rust.
Functions serialize back out to Dhall as well.

```rust
use sessiond_serde_dhall::Function;

let f: Function =
    sessiond_serde_dhall::from_str("\\(x : Natural) -> x + 1").parse().unwrap();

assert_eq!(f.apply::<_, u64>(&41u64).unwrap(), 42);
```

### Maps

A `Prelude.Map.Type K V` is a `List { mapKey : K, mapValue : V }` in Dhall, and
deserializes as one. It does *not* collapse into a `HashMap`: a Dhall map is
ordered and may repeat a key, so folding it into a map would discard both.
Convert explicitly if you want one:

```rust
use std::collections::HashMap;
use serde::Deserialize;

#[derive(Deserialize)]
struct Entry {
    mapKey: String,
    mapValue: u64,
}

let entries: Vec<Entry> =
    sessiond_serde_dhall::from_str("toMap { x = 1, y = 2 }").parse().unwrap();
let map: HashMap<String, u64> =
    entries.into_iter().map(|e| (e.mapKey, e.mapValue)).collect();

assert_eq!(map["x"], 1);
```

### Imports

Imports resolve by default, including remote ones over HTTPS. If you are
parsing configuration you did not write, you probably want to say so:

```rust
let data = "12 + https://example.com/other_file.dhall : Natural";

// Refuse anything that would reach the network, but keep resolving local files.
assert!(
    sessiond_serde_dhall::from_str(data)
        .remote_imports(false)
        .parse::<u64>()
        .is_err()
);
```

`imports(false)` is the stricter version, refusing local imports too. The
`remote_imports` check is on where the request would actually go, so a relative
import inside an already-remote file is refused as well.

Remote imports follow the standard's rules: a `using [ ... ]` clause is sent
with the request and forwarded only to relative imports on the same host, and a
remote file importing another origin is allowed only when that origin's
response grants it access (CORS).

Remote imports are behind the default `reqwest` feature. Turn it off with
`default-features = false` if you do not want the HTTP client compiled in at
all — that also drops the dependency for `wasm32`, which cannot use it.

### Toolchain

The crates are edition 2024, so they need Rust 1.85 or later. That floor comes
from the edition rather than from a CI matrix, and nothing verifies a lower
bound beyond it: the nix devshell pins one toolchain, and that is what is
tested.

## Standard-compliance

This implementation is tested against the [Dhall
standard](https://github.com/dhall-lang/dhall-lang) at version `v20.2.0`, which
is pinned by the flake. It passes the whole standard test suite bar two cases:

* `import/success/unit/asLocation/RemoteCanonicalize4`, where the standard
  disagrees with [RFC 3986 §5.2](https://tools.ietf.org/html/rfc3986#section-5.2)
  and this implementation follows the RFC.
* `type-inference/success/prelude`, because an import served from the on-disk
  cache comes back alpha-normalized, so a type is inferred with `_` binders
  rather than the original names. The standard addresses cache entries by the
  hash of their contents and that hash is over the alpha-normalized form, so
  storing anything else would break interoperability with other implementations
  sharing the cache. The values are alpha-equivalent.

See
[here](https://github.com/Nadrieril/dhall-rust/issues?q=is%3Aopen+is%3Aissue+label%3Astandard-compliance)
for other known gaps. That is upstream's issue tracker; those issues predate
the fork and some of them are now fixed here.

## Differences from upstream

Summarised; [CHANGELOG.md](CHANGELOG.md) has the detail and marks what is
breaking.

**Correctness.** `Natural` and `Integer` are now arbitrary-precision, so
`18446744073709551615 + 1` gives the right answer instead of `0`; bignum
literals and CBOR bignum tags work. `Text` concatenation of two non-literals is
now normalized, which means **semantic hashes have changed** for expressions
containing one — the old ones disagreed with every other implementation.
`Sort` and expressions with free variables normalize, alpha-normalization
renames free variables correctly, and a `merge` whose handler's return type
depends on its argument reports an error rather than panicking.

**Imports.** CORS checking for transitive remote imports, custom import headers
via `using [ ... ]`, `Deserializer::remote_imports()`, and `?` now recovers only
from an import that could not be *retrieved* — a cyclic import, a failed
integrity check, or a file that was fetched but does not typecheck propagates,
as the standard requires.

**API.** Dhall functions are a representable type (`Function`, and a `Function`
variant on `SimpleValue`/`SimpleType`). A `Prelude.Map.Type` no longer collapses
into a `HashMap`. Import errors have real messages instead of `Debug` output,
and `annotate-snippets` 0.12 gives more accurate line numbers.

**Build.** Everything is a nix flake, `dhall-lang` is a flake input rather than
a git submodule, and the whole suite runs hermetically in the sandbox. Windows
support was dropped. All dependencies are current, including nine major
versions.

## Contributing

This section will cover how we can get started on contributing this project.

### Setting up the repository

To get a copy of this repository we can run:

```bash
$ git clone https://github.com/parzivale/dhall-rust.git
```

But we also might note that it's better practice to fork the repository to your own workspace.
There you can make changes and submit pull requests against this repository.

There is nothing else to set up. `dhall-lang`, which supplies most of the test
suite, used to be a git submodule and is now pinned by the flake; the test
harness finds it through `$DHALL_LANG_DIR`, which the devshell sets.

### Building and Testing

Everything goes through [nix](https://nixos.org/) with flakes enabled. The
toolchain, `openssl`, and the pinned `dhall-lang` all come from the flake, so
there is no `rustup` step.

| Command | What it does |
| --- | --- |
| `nix flake check` | Build, `rustfmt --check`, and the whole suite in release and debug. The authoritative signal: CI runs exactly this |
| `nix build` | Just the workspace |
| `nix build .#coverage` | `lcov.info`, an HTML report and a summary, under `result/` |
| `nix develop` | Interactive shell with the toolchain and `rust-analyzer` |
| `nix run .#update-tests -- missing` | Regenerate missing expected-output files; brings its own `dhall` and `fd` |

`x86_64-linux` and `aarch64-linux` are supported.

`nix flake check` runs in the nix sandbox, with no network. The suite resolves
remote imports, so those hosts are served locally by `nix/spoof-imports.py` --
see that file for what it serves and why.

Three checks genuinely need network, so they cannot be flake checks and run as
workflow steps through `nix develop` instead. The tools still come from the
flake, so they are pinned like everything else:

| Command | What it does |
| --- | --- |
| `nix develop --command cargo semver-checks --package sessiond-serde-dhall` | Compares the public API against the last release on crates.io |
| `nix develop --command cargo audit` | Checks dependencies against the RustSec advisory database |
| `nix develop --command wasm-pack test --node serde_dhall` | Runs the wasm test target |

Inside `nix develop`, `cargo build` and `cargo test` work as usual. Note that a
couple of the import tests reach the live internet from there and fail, because
the hosts they name have moved on from the pinned revision; they pass under `nix
flake check`, which serves those requests locally.

You can run tests individually by name:

```bash
$ nix develop --command cargo test --test spec -- import_success::unit_SimpleRemote
```

Now we can have fun and happy contributing!

### Test suite

The test suite uses tests from the pinned `dhall-lang` as well as from the
local `dhall/tests` directory.
The various tests are run according to the instructions present in
[`dhall-lang/tests/README.md`](https://github.com/dhall-lang/dhall-lang/blob/master/tests/README.md).

If one of the specification tests fails but you prefer the new output, or an
output file (a `fooB.dhall` file) does not exist yet, run the test(s) with
`--bless` to write the result file from this implementation's own output. This
happens often with ui tests (see below), since we may want to change the
phrasing of errors for example. Note that the `--bless` argument is only
accepted by the `spec` tests and will not be recognized if you also run other
tests.

```bash
$ nix develop --command cargo test --test spec -- -q --bless
```

A missing output file is an error rather than something generated silently: a
test with no expectation would otherwise pass against one it had just written
for itself, and the file would go uncommitted. Don't forget to commit what
`--bless` produces.

In addition to the usual dhall tests, we additionally run "ui tests", that
ensure that the output of the various errors stays good.
The output of the ui tests is stored in the local `dhall/tests` directory, even
for the tests coming from dhall-lang. They are stored in a `.txt` file with the
same name as the corresponding test.

### Commit messages

I try to keep commit messages somewhat in the style of [Conventional
Commits](https://www.conventionalcommits.org/en/v1.0.0). That means the commit
message should start with `feat:`, `test:`, `spec:`, `doc:`, `fix:`, `style:`,
`refactor:`, `chore:`, `perf:` or similar prefixes.

A breaking change should be indicated with `!` before the `:`.


## [Changelog](CHANGELOG.md)

## License

Licensed under the terms of the 2-Clause BSD License ([LICENSE](LICENSE) or
https://opensource.org/licenses/BSD-2-Clause)
