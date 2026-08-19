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
keeps the original BSD-2-Clause licence and copyright.

Relative to upstream it passes the whole dhall-lang test suite bar two cases
(see [Standard-compliance](#standard-compliance)), where upstream left roughly
a tenth of it unimplemented.

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
sessiond-serde-dhall = "1.0.0"
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

The nix devshell pins the Rust toolchain used to build and test this project.
There is no separately verified minimum supported version; the `1.76.0` that
used to be documented here was checked by a CI matrix that no longer exists.

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
for other known gaps.

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

To build and run the whole test suite:

```bash
$ nix flake check
```

This runs in the nix sandbox, with no network. The suite resolves remote
imports, so those hosts are served locally by `nix/spoof-imports.py` -- see that
file for what it serves and why. `nix flake check` is the authoritative signal:
CI runs exactly this.

For an interactive shell with the toolchain and `rust-analyzer` on `PATH`:

```bash
$ nix develop
```

Inside it, `cargo build` and `cargo test` work as usual. Note that a couple of
the import tests reach the live internet from there and fail, because the hosts
they name have moved on from the pinned revision; they pass under `nix flake
check`, which serves those requests locally.

You can run tests individually by name:

```bash
$ nix develop --command cargo test --test spec -- import_success::unit_SimpleRemote
```

There is also a helper for regenerating expected outputs, which brings its own
`dhall` and `fd`:

```bash
$ nix run .#update-tests -- missing
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
