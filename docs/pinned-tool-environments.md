# Pinned tool environments

The pinned real-tool lane provisions and runs one exact fixture contract with a
single command:

```sh
just tool-case black unformatted
```

Run the behavior-rich representative contract for every environment with:

```sh
just tool-representatives
```

Both commands currently require macOS 26 or newer on Apple silicon, an Apple
developer toolchain with Xcode/SDK major 26 or newer and Apple Clang major 17 or
newer, and exactly mise 2026.5.15. Unknown tools, tools without a pinned recipe,
and undeclared fixture cases fail before execution. The available selectors and
their exact environment links are machine-readable in
[`recipes.json`](../crates/hookkit-pkl-config/validation/provisioning/recipes.json)
and are checked against the builtin validation manifest and fixture inventory.

## Reproducibility contract

Provisioning and case execution are deliberately separate phases. Provisioning
may use the network to fetch only locked artifacts and dependency graphs. The
case phase starts from `env -i`, uses a neutral system-temporary `HOME` (outside
the checkout and user directory), isolated XDG, mise,
Cargo (including its target directory), npm, pip, and Bundler state, and
constructs `PATH` from controlled roots plus explicitly declared macOS host
shims.
The fixture harness passes an explicit generated `--config`, so neither legacy
nor canonical user dotfiles participate.

Before a case runs, the driver:

1. verifies the host is an allowed OS and architecture, mise is the exact
   required version, and the declared compiler, Xcode, and SDK minimum probes
   pass;
2. installs only the selected mise-managed tools in `--locked` mode and verifies
   every directly managed Rust or Ruby archive against its committed SHA-256;
3. bootstraps Cargo, npm, Python-wheel, and pure-Ruby Bundler graphs from their
   committed locks as applicable; Cargo runs from `/` with an explicit manifest
   and controlled target root so ancestor `.cargo` files and host build caches
   cannot participate;
4. checks every declared executable resolves inside a controlled tool/state
   root or to an exact declared macOS host shim;
5. runs every shared, runtime, and tool version probe and rejects a mismatch;
6. enters mise's macOS network sandbox and requires an outbound AF_INET connect
   attempt to fail with a permission error; and
7. runs the selected fixture surfaces with Cargo offline. Selected cases can
   never turn a missing program into a skip.

The runner fails closed if the platform, version, executable path, active
network-denial probe, or fixture contract differs from the declaration.

## Locked representatives

| Environment | Runtime/tool | Integrity source | Representative |
| --- | --- | --- | --- |
| Node | Node 24.18.0; sort-package-json 3.6.1 | mise SHA-256; npm SHA-512 integrity graph | `sort-package-json/unformatted` |
| Python | Python 3.14.5; embedded pip 26.1.1; Black 26.5.1 | mise SHA-256; platform-specific wheel SHA-256 closure | `black/unformatted` |
| Go | Go/gofmt 1.26.3 | mise SHA-256 | `go-fmt/unformatted` |
| Rust | Rust 1.90.0; rustfmt 1.8.0 | dated official standalone archives with independent SHA-256 digests | `rustfmt/unformatted` |
| Ruby | jdx/ruby 3.4.10-2; embedded Bundler 2.6.9 and precompiled bundled Racc 1.8.1; RuboCop 1.30.1 | relocatable archive SHA-256; system-only dylink closure; Bundler package checksums | `rubocop/autocorrect-strings` |
| native macOS | SwiftLint 0.65.0 | mise SHA-256 | `swiftlint/manual-issue` |

The mise-managed archive URLs and checksums are in
[`mise.lock`](../crates/hookkit-pkl-config/validation/provisioning/mise.lock).
The independently verified Rust and Ruby URLs and SHA-256 digests are in the
recipe registry. Node, Python, and Ruby package closures live beside it under
`node/`, `python/`, and `ruby/`. Runtime components, auxiliary programs,
bootstrap commands, platform, architecture, minimum OS, and case-network policy
are schema-checked there as well. The current macOS 26 floor is dictated by the
official native Pkl 0.31.1 asset shared by the lane; the Rust and Ruby archives
themselves support earlier macOS releases. The Apple compiler and SDK are
host-supplied prerequisites because Apple does not distribute them as portable
redistributable archives; their paths and minimum-version probes are declared
and recorded alongside the checksum-pinned closure.

Successful runs write `target/pinned-tool-environments/artifacts/` by default.
`pinned-environment.json` records the selected recipe IDs, lock digest, host
OS/architecture and constraint, observed versions, resolved executable paths,
direct-archive and selected dependency-lock digests, sandbox backend, active
network-denial result, and outcome. The fixture harness writes its own
surface-level report below `artifacts/fixtures/`. Override the state and artifact
roots with `VELVET_GLOVE_PINNED_TOOL_STATE_DIR` and
`VELVET_GLOVE_PINNED_TOOL_ARTIFACT_DIR`.

These six smoke contracts establish the reproducible environment substrate;
they do not promote a tool's full pinned-real-tool coverage tier. The generated
coverage report intentionally retains gaps until every required target, surface,
and semantic case has evidence. Linux, Intel, and full-catalog scheduling remain
separate follow-up work.

## Updating a pin

Pin updates are reviewable data changes, not floating installs. Update the exact
version in `mise.toml` or the applicable package declaration, regenerate its lock
with mise 2026.5.15 and the target platform, review every changed URL/checksum,
update the exact probe and manifest provenance, then run:

```sh
cargo test -p hookkit-pkl-config --test provisioning_recipes
just tool-representatives
just check
```

Do not hand-edit a generated package checksum to make a failed fetch pass.
