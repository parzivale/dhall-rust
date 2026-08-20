# Changelog

#### [Unreleased]

- BREAKING CHANGE: the crates are edition 2024 and so need Rust 1.85 or later
- `clippy::pedantic` is configured for the workspace. Nothing runs clippy in CI
  yet, so it is a signal rather than a gate
- The flake exposes `lib.toDhall`, which converts a Nix value to Dhall source
  the way `builtins.toJSON` converts one to JSON, and `lib.lambda` for the case
  where some of the values are not known until the Dhall function is applied.
  Neither is part of the Rust crates
- `nix fmt` formats the Nix, and `nix flake check` now checks that it is
  formatted alongside the Rust

#### [1.0.0] - 2026-08-19

##### Read this before upgrading

- BREAKING CHANGE: **the crates have been renamed.** This is a fork of
  [`Nadrieril/dhall-rust`](https://github.com/Nadrieril/dhall-rust), published
  under a prefix rather than under the original crate names: `dhall` is now
  `sessiond-dhall`, `serde_dhall` is now `sessiond-serde-dhall`, and
  `dhall_proc_macros` is now `sessiond-dhall-proc-macros`. The Rust paths follow
  (`sessiond_serde_dhall::from_str`, and so on); nothing else about the API
  changed as a result of the rename. Versioning restarts at `1.0.0` and is
  independent of upstream's `0.13.x`
- BREAKING CHANGE: **semantic hashes have changed for some expressions.** `l ++ r`
  was not normalized when neither side was a text literal, so it stayed a `++`
  node instead of becoming `"${l}${r}"`. Any `sha256:` you recorded for an
  expression containing a `Text` concatenation of two non-literals no longer
  matches, and the import will be rejected. The hashes this produces now agree
  with the other implementations; the ones it produced before did not
- BREAKING CHANGE: `Natural` and `Integer` are arbitrary-precision, so
  `sessiond_dhall::syntax::{Natural, Integer}` are `num_bigint::{BigUint, BigInt}` rather
  than `u64` and `i64`. `sessiond-serde-dhall` widens to `u128`/`i128` where needed and
  refuses a value that fits neither, instead of truncating
- BREAKING CHANGE: a `Prelude.Map.Type` no longer deserializes into a `HashMap`.
  It is a `List { mapKey : k, mapValue : v }` in Dhall and now deserializes as
  one. Collapsing it dropped repeated keys, discarded the ordering, made the
  result indistinguishable from a plain record, and panicked on a non-`Text`
  key. The `SimpleType` docs show the one-line conversion
- BREAKING CHANGE: `?` no longer recovers from every failure. Per the standard
  it recovers only when an import could not be *retrieved*; a cyclic import, a
  failed integrity check, and an import that was fetched but does not parse or
  typecheck now propagate
- BREAKING CHANGE: `sessiond_dhall::error::AnnotationType` is this crate's own enum
  rather than a re-export from `annotate-snippets`, so bumping that dependency
  is no longer a breaking change here
- Error messages have changed. `annotate-snippets` 0.12 reports more accurate
  line numbers and uses rustc's multi-span layout, and import failures have real
  messages in place of `Debug` output

##### Added

- Deserialize Dhall functions into the new `sessiond_serde_dhall::Function` type, and call them from Rust
  with `Function::apply()`. Functions serialize back to Dhall as well
- BREAKING CHANGE: Add a `Function` variant to `SimpleValue` and a `Function` variant to
  `SimpleType`, so that `T -> U` is now a representable type
- `Deserializer::remote_imports()` refuses imports that would reach the network
  while local ones keep resolving, which `imports(false)` could not express. The
  check is on where the request would go, so a relative import inside an
  already-remote file is refused too
- CORS checking for transitive remote imports, per `standard/imports.md`.
  Remote-to-remote imports were rejected outright; they are now allowed when the
  response grants the parent origin access
- Custom import headers. `using [ ... ]` clauses are sent with the request, and
  forwarded to relative imports on the same host but not to absolute ones.
  Headers are typechecked in an empty context, so one referring to a surrounding
  binding is an unbound variable rather than a way to leak program state
- `Parsed::resolve_without_remote_imports` and `Resolved::normalize_untyped`

##### Fixed

- `Natural` arithmetic silently produced wrong answers. `18446744073709551615 + 1`
  evaluated to `0` in release builds and panicked in debug
- Arbitrary-precision support means bignum literals and CBOR bignum tags (RFC
  8949 tags 2 and 3) now work, and `Integer/toDouble` handles values beyond
  `i64`
- A `merge` whose handler's return type depends on its argument panicked with
  "Trying to use a fresh variable outside of equality checking" instead of
  reporting the error
- `Sort` and expressions with free variables can be normalized. The standard
  defines normalization over untyped terms; this typechecked first
- Alpha-normalization renames free variables' indices correctly:
  `\(x : Bool) -> \(x : Bool) -> x@2` printed `x@2` rather than `x`

##### Changed

- The build and test setup is a nix flake. `nix flake check` builds, formats and
  runs the whole suite in the sandbox with no network; `nix build .#coverage`
  measures coverage. `dhall-lang` is a flake input rather than a git submodule
- All dependencies updated, including nine major versions
- The test harness fails on a missing expected-output file rather than writing
  one from its own output and passing

#### [0.13.0] - 2025-09-10

- Support enum struct variants in `SimpleType`
- BREAKING CHANGE: Change minimum supported version to 1.76.0 because of the `wasm_bindgen` dependency

#### [0.12.1] - 2023-02-01

#### [0.12.0] - 2022-08-15

- BREAKING CHANGE: Change minimum supported version to 1.60.0 because of `minicbor` dependency
- Use `minicbor` instead of the deprecated `serde_cbor` (https://github.com/Nadrieril/dhall-rust/pull/236)
- BREAKING CHANGE: Change minimum supported version to 1.58.0 because of `libtest-mimic` dependency (https://github.com/Nadrieril/dhall-rust/pull/234)
- Implement ToDhall for SimpleType (https://github.com/Nadrieril/dhall-rust/pull/233)

#### [0.11.2] - 2022-06-24

- Implement StaticType for u16 (https://github.com/Nadrieril/dhall-rust/pull/230)

#### [0.11.1] - 2022-05-19

- Improve error message on duplicate non-mergeable fields (https://github.com/Nadrieril/dhall-rust/pull/229)

#### [0.11.0] - 2022-01-01

- Fix reading file with a path relative to HOME (https://github.com/Nadrieril/dhall-rust/pull/224)
- Change `?` to only fall back on absent imports
- Add support for custom builtin types (https://github.com/Nadrieril/dhall-rust/pull/220)
- Add support for Unix shebangs
- `StaticType` derive supports records in Union Types (https://github.com/Nadrieril/dhall-rust/pull/219)
- BREAKING CHANGE: Change minimum supported version to 1.46.0 because of reqwest dependency.

#### [0.10.1] - 2021-04-03

#### [0.10.0] - 2021-02-04

- BREAKING CHANGE: Change minimum supported version to 1.44.0.
- BREAKING CHANGE: Support dhall v20.0.0
- `if` can return a type (https://github.com/Nadrieril/dhall-rust/pull/202)

#### [0.9.0] - 2020-11-20

- BREAKING CHANGE: Support Dhall v19.0.0
- Support reading a CBOR-encoded binary file (https://github.com/Nadrieril/dhall-rust/issues/199)

#### [0.8.0] - 2020-10-28

- Implement serialization (https://github.com/Nadrieril/dhall-rust/issues/164)
- BREAKING CHANGE: use u64/i64 instead of usize/isize in `NumKind`

#### [0.7.5] - 2020-10-28

- Make `SimpleValue` deserializable within other types (https://github.com/Nadrieril/dhall-rust/issues/184)

#### [0.7.4] - 2020-10-25

- Add new `Text/replace` builtin (https://github.com/Nadrieril/dhall-rust/pull/181)

#### [0.7.3] - 2020-10-24

- Add a `SimpleValue` type to the public interface (https://github.com/Nadrieril/dhall-rust/pull/183)

#### [0.7.2] - 2020-10-24

- Fix `reqwest` feature (https://github.com/Nadrieril/dhall-rust/pull/182)

#### [0.7.1] - 2020-10-16

- Add a `Display` impl for `SimpleType` (https://github.com/Nadrieril/dhall-rust/issues/179)
- Make reqwest an optional dependency (https://github.com/Nadrieril/dhall-rust/issues/178)

#### [0.7.0] - 2020-09-15

- BREAKING CHANGE: Support Dhall v18.0.0

#### [0.6.0] - 2020-08-05

- Allow trailing delimiters in records, lists, etc.
- BREAKING CHANGE: Support Dhall v17.0.0

    See https://github.com/dhall-lang/dhall-lang/releases/tag/v16.0.0 and
    https://github.com/dhall-lang/dhall-lang/releases/tag/v17.0.0 for details.

- Fix running tests on Windows. Developing on this lib should now be possible on Windows.

#### [0.5.3] - 2020-05-30

- Support import caching
- Support building on Windows
- Support building to wasm (but imports don't work)

#### [0.5.2] - 2020-04-12

- Fix #162
- Update to supporting Dhall v15.0.0
- Deserialize `Prelude.Map` and `toMap` to a map instead of a list.

#### [0.5.1] - 2020-04-09

- Small fixes

#### [0.5.0] - 2020-04-05

- Add `serde_dhall::from_file` to read a Dhall file directly.
- BREAKING CHANGE: reworked most of the `serde_dhall` API

    You need to replace uses of `from_str(s)` with `from_str(s).parse()`. The
    various type annotation methods have been removed; use instead the methods on
    the `Deserializer` struct.

#### 0.4.0

- `dhall` now uses the stable Rust toolchain !
- Implement record puns
- Add support for `with` keyword
- Implement remote imports with conservative sanity checking
- Implement `missing` and `env:VAR` imports
- Implement `as Text` and `as Location` imports
- Implement projection by expression
- Implement some normalization simplifications

#### 0.3.0

- Update to supporting dhall v14.0.0
- Add support for dotted field syntax
- Disallow Natural literals with leading zeros
- Add support for duplicate record fields
- Update to supporting dhall v13.0.0

#### 0.2.1

- Improve documentation and deserialize many more types

#### 0.2.0

- Update to supporting dhall v12.0.0

#### 0.1.0

- Initial release

<!-- next-url -->
[1.0.0]: https://github.com/parzivale/dhall-rust/releases/tag/v1.0.0
[0.13.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.12.1...serde_dhall-v0.13.0
[0.12.1]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.12.0...serde_dhall-v0.12.1
[0.12.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.11.2...serde_dhall-v0.12.0
[0.11.2]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.11.1...serde_dhall-v0.11.2
[0.11.1]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.11.0...serde_dhall-v0.11.1
[0.11.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.10.1...serde_dhall-v0.11.0
[0.10.1]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.10.0...serde_dhall-v0.10.1
[0.10.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.9.0...serde_dhall-v0.10.0
[0.9.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.8.0...serde_dhall-v0.9.0
[0.8.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.7.5...serde_dhall-v0.8.0
[0.7.5]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.7.4...serde_dhall-v0.7.5
[0.7.4]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.7.3...serde_dhall-v0.7.4
[0.7.3]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.7.2...serde_dhall-v0.7.3
[0.7.2]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.7.1...serde_dhall-v0.7.2
[0.7.1]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.7.0...serde_dhall-v0.7.1
[0.7.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.6.0...serde_dhall-v0.7.0
[0.6.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.5.3...serde_dhall-v0.6.0
[0.5.3]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.5.2...serde_dhall-v0.5.3
[0.5.2]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.5.1...serde_dhall-v0.5.2
[0.5.1]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.5.0...serde_dhall-v0.5.1
[0.5.0]: https://github.com/Nadrieril/dhall-rust/compare/serde_dhall-v0.4.0...serde_dhall-v0.5.0
