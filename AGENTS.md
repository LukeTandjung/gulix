# Gulix Repository Guidelines

## Rust ownership

Cargo owns the Rust workspace, dependencies, builds, checks, and tests. Run Rust
commands from the repository root. Keep development tools in devenv and Rust
dependencies in Cargo.

The ast-grep rules apply only to Gulix-owned paths listed in `ast-grep-rules/`.
Upstream Helix files are intentionally excluded. Add a new Gulix-owned path to
the rule scope when introducing one.

## Rust module layout

`mod.rs` files are barrel export files only.

Allowed in `mod.rs`:
- module documentation;
- `mod ...;` and `pub mod ...;`;
- `pub use ...;`.

Disallowed in `mod.rs`:
- structs, enums, traits, and type aliases;
- functions and impl blocks;
- constants and statics;
- macro definitions.

Move implementation logic into named sibling files and re-export it from
`mod.rs`. Do not use scoped visibility such as `pub(crate)` in Gulix-owned code;
keep items private or make the API explicitly public.

Before finishing Rust module work, run:

```sh
ast-grep scan --report-style short
```

## Rust readability

Use vertical whitespace to make execution stages easy to scan. Separate proof
blocks from adjacent executable statements, mutations from post-mutation
proofs, and assertions from returned values. Separate validation and other
distinct stages with blank lines. Merge adjacent proof blocks when they reason
about the same program point.

## Testing

Do not write example-based tests. Use integration and property-based tests for
runtime behaviour, and Verus proofs for pure state and layout contracts. Tests
must add behavioural coverage beyond the implementation's verified contracts.

## Verus

Cargo owns `helix-term`'s pinned `vstd` dependency. Contracts live beside the
production functions they constrain, and verified tests live under the matching
`helix-term/tests/<layer>/` path. Do not create a duplicate abstract model or
claim that `todo!()` code is verified. devenv provides only the
`cargo-verus`/`verus` executables and their required rustup toolchain.

Verify all annotated core modules and their behavioral tests with:

```sh
devenv tasks run project:verify
```

Keep every `--verify-module` flag in that task's single `cargo verus` invocation;
separate invocations can reuse cargo-verus's cache and skip later modules.
Verus covers pure state and layout contracts. Keep real Typst, asynchronous
runtime, and Tern integration tests for behavior outside that pure boundary.
