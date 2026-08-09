# Pinned tool environments

The pinned real-tool lane provisions and runs one exact fixture contract with a
single command. For example:

```sh
just tool-case jq multi-file-fragments
just tool-case betterleaks multi-file
just tool-case biome multi-file
```

Run all eleven behavior-rich representative contracts across eight environments
with:

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
the checkout and user directory), isolated XDG, mise, Cargo (including its
target directory), Go module and build caches, npm, pip, and Bundler state, and
constructs `PATH` from controlled roots plus explicitly declared macOS host
shims.
The fixture harness passes an explicit generated `--config`, so neither legacy
nor canonical user dotfiles participate.

Before a case runs, the driver:

1. verifies the host is an allowed OS and architecture, mise is the exact
   required version, and the declared compiler, Xcode, and SDK minimum probes
   pass;
2. installs only the selected mise-managed tools in `--locked` mode and verifies
   every directly managed Rust or Ruby archive, and every Betterleaks source,
   patch, module-lock, and build-artifact digest, against committed values;
3. bootstraps Cargo, npm, Python-wheel, pure-Ruby Bundler, and Betterleaks Go
   module graphs from their committed locks as applicable; Cargo runs from `/`
   with an explicit manifest and controlled target root so ancestor `.cargo`
   files and host build caches cannot participate, while the patched Betterleaks
   build verifies modules and compiles with the network denied;
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

<!-- markdownlint-disable MD013 -->

| Environment | Runtime/tool | Integrity source | Representative |
| --- | --- | --- | --- |
| Data formats | jq 1.8.2 | SHA-256 + SLSA | `jq/multi-file-fragments` |
| Node | Node 24.18.0; Astro 7.2.0; @astrojs/check 0.9.10; TypeScript 6.0.3; Biome 2.5.7; sort-package-json 3.6.1 | mise SHA-256; npm SHA-512 integrity graph | `astro/multi-file-project`, `biome/multi-file`, `sort-package-json/unformatted` |
| Python | Python 3.14.5; embedded pip 26.1.1; Black 26.5.1 | mise SHA-256; platform-specific wheel SHA-256 closure | `black/unformatted` |
| Go | Go/gofmt 1.26.5 | mise SHA-256 | `go-fmt/unformatted` |
| Rust | Rust 1.90.0; rustfmt 1.8.0 | dated official standalone archives with independent SHA-256 digests | `rustfmt/unformatted` |
| Ruby | jdx/ruby 3.4.10-2; embedded Bundler 2.6.9 and precompiled bundled Racc 1.8.1; Asciidoctor 2.0.26; RuboCop 1.30.1 | relocatable archive SHA-256; system-only dylink closure; Bundler package checksums | `asciidoctor/multi-file`, `rubocop/autocorrect-strings` |
| Security | Go 1.26.5; Betterleaks 1.7.3+velvet-glove.1 | mise SHA-256; source, patch, module closure, and built-artifact SHA-256 | `betterleaks/multi-file` |
| native macOS | SwiftLint 0.65.0 | mise SHA-256 | `swiftlint/manual-issue` |

<!-- markdownlint-enable MD013 -->

### jq validation contract

The data-formats environment pins the official
[`jq-macos-arm64`](https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-macos-arm64)
1.8.2 release asset at SHA-256
`2d75340ba57a4b4b4c8708a21c2dc8e958a48aaa8bba13b27f77f6e4c0eca07e`.
Its SLSA v1
[`jq-attestation.json`](https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-attestation.json)
bundle is independently pinned at SHA-256
`01e9619236573939473c0f2eb2c5c38dc0f066fbdc89a5357a6f3f2954e00eed`;
the provenance resolves tag `jq-1.8.2` to commit
`34f7186b86743a083a589741b6cea95293524108` and the upstream
`.github/workflows/ci.yml` build. The exact version probe is `jq-1.8.2`.

Velvet Glove runs `jq empty` once per selected file. Plain `empty` exits zero
after parsing a valid JSON value stream and emits no output; malformed input
exits five and is classified as a source issue. Exit statuses one through four
are tool, usage, or configuration failures. In particular, `jq -e empty` is
incorrect because `empty` deliberately produces no result, so `-e` makes valid
input exit four.

Per-file invocation is part of both the immediate phase and its
compatibility-translated deferred workflow. jq otherwise feeds the bytes from
multiple path arguments through one parser without inserting a separator,
which can join two individually malformed fragments or reject two individually
valid files without trailing whitespace. Running once per file also gives the
runner exact issue attribution. The remaining limitation is deliberate and
recorded: within each file, `jq empty` accepts a possibly empty stream and more
than one whitespace-separated top-level JSON value. It does not enforce exactly
one nonempty JSON document.

The representative selector exercises the cross-file regression. Full jq
coverage additionally runs `clean`, `invalid`, and `operational-failure`; each
case covers both Claude and Codex immediate hooks and the
compatibility-translated deferred lifecycle.

### Betterleaks validation contract

The security environment starts from the upstream Betterleaks
[`v1.7.3` source archive](https://github.com/betterleaks/betterleaks/archive/refs/tags/v1.7.3.tar.gz),
whose tag resolves to commit
[`82b306a9d338121a6fd087002a94e5c7ab685355`](https://github.com/betterleaks/betterleaks/commit/82b306a9d338121a6fd087002a94e5c7ab685355).
The source archive is pinned at SHA-256
`7359ae820c62c276d31cef3d1431eb8beb6db07d5c44830bad03dbe9c0cf3850`.
Betterleaks is MIT-licensed. GitHub marks the tag commit as verified, but the
upstream Sigstore bundle authenticates the official release binaries rather
than GitHub's generated source archive or this downstream binary. This lane's
trust chain is therefore the tag-archive checksum, tag-to-commit identity,
checked patch and module checksums, locked compiler, and reproducible output
checksum. Redistributing the downstream binary would additionally require its
own attestation and complete third-party notices.
Velvet Glove applies the committed `closure.patch`, pinned at SHA-256
`2d57aa396d9c7f0337cf13c05fa06f661099035cb5f753a12e79ca2f46a38147`,
to update `github.com/klauspost/compress` to 1.18.7 and
`golang.org/x/text` to 0.39.0. The resulting `go.mod` and `go.sum` are pinned at
SHA-256
`a669cc877c8dac1c9f3927b57e246902b81bc37665147e4a2d301104f534819e`
and
`359a55b2abc25a4fa290093fed6bc6d7d3d2923906e4c77cf4d786581a61a38d`,
respectively.

The patched source is reproducibly compiled with the official Go 1.26.5
`go1.26.5.darwin-arm64.tar.gz` archive, locked by mise at SHA-256
`efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a`.
The build uses the local toolchain, read-only module mode, `CGO_ENABLED=0`,
`GOOS=darwin`, `GOARCH=arm64`, `-trimpath`, no VCS metadata, and an empty build
ID. Module download is the only network-enabled build step; module verification
and compilation run with active network denial and the proxy disabled. The
resulting binary is pinned at
SHA-256
`046177cad9aa9f924fe57adca4a1a8c54d0ad74ceed593147b127f5a486f8144`
and has the exact probe output
`betterleaks version 1.7.3+velvet-glove.1`.

The evaluated outer phase is:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> betterleaks {extra-args} __VELVET_GLOVE_BETTERLEAKS_FILES__ {files}
```

<!-- markdownlint-enable MD013 -->

Isolated Python mode prevents project-local modules, user site packages, and
ambient Python path configuration from replacing the adapter's standard-library
imports. The pinned lane supplies Python 3.14.5 as the `python` executable;
other installations must provide an interpreter with `-I` support. The marker
then gives the adapter an unambiguous boundary between
configured arguments and selected paths. It preflights that every selected
path is a readable regular file and not a symlink, then launches exactly one
nested batch:

<!-- markdownlint-disable MD013 -->

```text
betterleaks dir {extra-args} --redact=100 --verbose=true --no-color=true --no-banner=true --exit-code=10 --log-level=fatal --legacy-print=true {files}
```

<!-- markdownlint-enable MD013 -->

For handled hangup, interrupt, and termination signals, the adapter forwards
the signal to the native child. If that child does not exit within one second,
the adapter sends `KILL` and attempts a bounded reap before returning failure.

Complete redaction prevents source secrets from entering diagnostics. Fatal
logging suppresses elapsed-time summaries, while verbose legacy output retains
stable file, rule, and line evidence. The adapter accepts configured arguments
only in non-controlled long `--name=value` form. It rejects positional and short
arguments, the `--` separator, help/version exits, and options that could change
redaction, verbosity, color, banners, finding status, log level, legacy output,
baselines, reports, diagnostics, or validation. Fixed controls are appended
after allowed arguments as a second guard.

Before launch, the adapter removes `BETTERLEAKS_CONFIG`,
`BETTERLEAKS_CONFIG_TOML`, `GITLEAKS_CONFIG`, and `GITLEAKS_CONFIG_TOML` from
the child environment. Policy-sensitive cases pass
`--config=.betterleaks.toml` explicitly. This matters because a scan over
regular-file targets does not implicitly discover a project Betterleaks or
Gitleaks config; without an explicit option, Betterleaks uses its embedded
default policy.

Status zero means clean and the locked status 10 means findings. Statuses one,
two, and 126 cover Betterleaks configuration/process failures, adapter or spawn
failures, and unknown CLI options; every other status also fails closed as an
operational problem. The four cases cover a clean file, a stable fully redacted
finding, one mixed clean/finding two-file batch, and a missing explicit-config
failure whose native status one must remain operational rather than a finding.
The adapter narrowly canonicalizes only Betterleaks' leading console clock on
fatal diagnostics before Velvet Glove stores them. Immediate execution and the
compatibility-translated deferred lifecycle each run twice and prove that source
and configuration inputs are unchanged; only the expected hook diagnostic
artifact may be added.

The batch result is conservatively attributed to every selected candidate,
even when legacy diagnostics name the file containing a finding. The regular,
readable, nonsymlink check is a launch-time preflight rather than a filesystem
capability: another process could replace a path before Betterleaks opens it.
The adapter also controls config discovery and argument shape, but does not
validate the contents or confine the filesystem reach of an explicitly supplied
config; parse, read, and semantic config failures are left to Betterleaks and
classified as operational. These TOCTOU and config boundaries are recorded
limitations, not sandbox guarantees.

A `govulncheck` 1.6.0 source-mode symbol scan of this Go 1.26.5 build, using the
database modified 2026-07-27, reports no reachable package or symbol findings.
It retains the module-level
[`GO-2026-5932`](https://pkg.go.dev/vuln/GO-2026-5932) advisory because the
transitive graph contains `golang.org/x/crypto` 0.53.0, but the source
dependency graph does not import the affected `golang.org/x/crypto/openpgp`
packages. Binary mode reports only the same advisory through coarse
module-derived package and symbol matches; those matches do not establish
call-graph reachability. This is a recorded scanner limitation, not a claim that
the pinned artifact is exempt from future vulnerability-database changes.

### Astro validation contract

The Node environment pins the official Node 24.18.0
[`node-v24.18.0-darwin-arm64.tar.gz`](https://nodejs.org/dist/v24.18.0/node-v24.18.0-darwin-arm64.tar.gz)
archive at SHA-256
`e1a97e14c99c803e96c7339403282ea05a499c32f8d83defe9ef5ec66f979ed1`.
The official versioned npm publications are the upstream provenance references
for [Astro 7.2.0](https://www.npmjs.com/package/astro/v/7.2.0),
[`@astrojs/check` 0.9.10](https://www.npmjs.com/package/@astrojs/check/v/0.9.10),
and [TypeScript 6.0.3](https://www.npmjs.com/package/typescript/v/6.0.3).
Astro's official
[`astro@7.2.0` release](https://github.com/withastro/astro/releases/tag/astro%407.2.0)
resolves to commit `60e94329f94438c1fc9b513bd9669bf07c89b680`, and
`@astrojs/check`'s official
[`0.9.10` release](https://github.com/withastro/astro/releases/tag/%40astrojs/check%400.9.10)
resolves to `112d3ea14cf997218239fd8a436707e56a3815fb`. Both npm
artifacts were published by GitHub Actions OIDC workflows with SLSA provenance
binding their tarballs to those commits. TypeScript's official
[`v6.0.3` release](https://github.com/microsoft/TypeScript/releases/tag/v6.0.3)
resolves to `050880ce59e30b356b686bd3144efe24f875ebc8`; its npm
tarball has a registry signature but no SLSA attestation. The committed SHA-512
integrities are therefore the uniform package-integrity guard for this graph.
Astro and `@astrojs/check` are MIT-licensed; TypeScript is Apache-2.0-licensed.
Their direct entries in the committed npm lock carry these exact SHA-512
integrities, respectively:

- `sha512-lLTYzx3fOvCmtwD3JVBLQcbORbIOW1/j0R+3IvJx/XKwMGrk7mFnF0BYSOeRiNw1qHUR5mdA6+hRnyvyDfqrWQ==`
- `sha512-zgx/UQMozdjOa3bOxjgeCFdtpE3c9rRX6xHwa+2QXvy8z8Akifu2AtubHyv/zzC2znO8dl8fFWL4K+Ba9kS8HQ==`
- `sha512-y2TvuxSZPDyQakkFRPZHKFm+KKVqIisdg9/CZwm9ftvKXLP8NRWj38/ODjNbr43SsoXqNuAisEf1GdCxqWcdBw==`

The exact Astro product probe expects three leading spaces before
`astro  v7.2.0`.

The evaluated outer phase is:

```text
node --input-type=commonjs -e <adapter> -- astro check {extra-args}
```

It launches this nested command, with controlled options appended after any
allowed configured arguments:

<!-- markdownlint-disable MD013 -->

```text
astro check {extra-args} --silent --noSync --no-watch --root . --minimumSeverity=error --minimumFailingSeverity=error
```

<!-- markdownlint-enable MD013 -->

The case environment sets `NODE_PATH` to the same pinned `node_modules` graph
that supplies the Astro executable, and retained traces require exactly that
one controlled root plus the Astro, checker, and TypeScript manifests. The
adapter sets `ASTRO_TELEMETRY_DISABLED=1` and `CI=1`, removes `DEBUG`, and sets
`CLICOLOR=0`, `FORCE_COLOR=0`, and `NO_COLOR=1`. It also strips ANSI control
sequences before forwarding or classifying diagnostics.

Astro uses child status one for both checker findings and operational errors.
The adapter accepts child status zero only with a positive-file `Result` footer
reporting zero errors, and child status one only with that footer reporting one
or more errors. It maps every other outcome to outer status two, including a
raw status-one configuration failure with no completion footer. Outer statuses
zero, one, and two therefore mean clean, source issues, and operational failure.
The adapter buffers the child output so it can validate the terminal footer and
sets an explicit 16 MiB ceiling; output beyond that bounded limit fails closed
as an operational error rather than being truncated and misclassified.

The four cases are a clean component; a stable `ts(2322)` type error; a strong
three-file workspace proof whose two selected files are clean while an
unselected component fails; and `--tsconfig does-not-exist.json`, which must
lack the footer and become operational. The workspace case must name the
unselected failing file and report `Result (3 files)`. Both the immediate and
compatibility-translated deferred surfaces repeat execution twice and prove no
source mutation.

Both severity thresholds are `error`, so warnings and hints intentionally do
not fail this contract. Astro validation is whole-workspace, and findings are
therefore attributed conservatively to the candidates that triggered the run,
not to an inferred culprit. `--noSync` prevents Astro's normal generated-file
writes, but side-effectful project configuration, checker, or plugin code is
outside that no-mutation guarantee.

### Biome validation contract

The Node environment pins
[`@biomejs/biome` 2.5.7](https://www.npmjs.com/package/@biomejs/biome/v/2.5.7)
and its `@biomejs/cli-darwin-arm64` 2.5.7 native package. The official
[`@biomejs/biome@2.5.7` release](https://github.com/biomejs/biome/releases/tag/%40biomejs%2Fbiome%402.5.7)
resolves to verified commit
`191d051335821e804e7ffe484240ca326af86f7c`. npm registry signatures and SLSA
provenance bind both tarballs to that commit and the upstream
`.github/workflows/release.yml` run. Their committed SHA-512 integrities are:

- wrapper: `sha512-zr8K/DcY5tYsQOQwqMJ0AWElo6QgmgNI7idXgXLhevVszlt8RGVpesEJPqx3ThazLaOwjJ5Y8fz3BtH5fGZNsw==`
- macOS arm64 CLI: `sha512-vxo/Ls3/PYdQWyLhYYcgMOCzQypAjcY+iihS8M0wW03l16TCLW4zqZzGo75gm1VdCMj38hTVZ31KBWrZ4G9dJw==`

The extracted native binary independently matches the official release asset
at SHA-256
`f71fe80909d2f70f1e051320f5ba9dfd553bc5ef3bacef5cdee1b00ee96a285c`.
Biome is dual-licensed under MIT or Apache-2.0. The exact version probe is
`Version: 2.5.7`.

Both phases use the pinned Python 3.14.5 interpreter in isolated mode, while
the npm executable uses pinned Node 24.18.0:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> biome fix {extra-args} __VELVET_GLOVE_BIOME_FILES__ {files}
python -I -c <adapter> biome verify {extra-args} __VELVET_GLOVE_BIOME_FILES__ {files}
```

The adapter launches exactly one batch for each phase:

```text
biome check --write {extra-args} --colors=off --reporter=json --max-diagnostics=none --error-on-warnings --no-errors-on-unmatched -- {files}
biome check {extra-args} --colors=off --reporter=json --max-diagnostics=none --error-on-warnings --no-errors-on-unmatched -- {files}
```

<!-- markdownlint-enable MD013 -->

Isolated mode prevents project-local or ambient Python imports from replacing
the adapter's standard-library modules. Before launch, it verifies every
selected path is a readable regular nonsymlink file. It rejects configured
arguments that can change file or VCS selection, rule scope, mutation,
reporting, diagnostic completeness, server/watch behavior, or early exits.
Allowed configuration remains restricted to non-controlled long
`--name=value` arguments. A literal `--` separates the locked controls from
selected paths, including filenames that begin with a dash. The child receives
deterministic color, CI, and single-thread Rayon values,
while Biome binary overrides, Node injection paths, Biome log/config inputs,
Biome thread overrides, Rust logging/backtraces, and `DEBUG` are removed.

Biome uses status one for both source diagnostics and operational failures.
The adapter therefore accepts its exact-pinned JSON report only when all
counters are nonnegative integers, every selected file was processed, no file
or diagnostic was skipped, and diagnostic severities agree with the summary.
Status zero maps to clean only for a complete report with no diagnostics.
Status one maps to source issues only when every failing category is `parse`,
`format`, `lint/*`, `assist/*`, or `suppressions/*`; configuration errors,
unknown categories, malformed or incomplete reports, spawn/signals, and every
other status map to operational status two. Volatile duration fields are
removed before evidence is emitted. Output is drained concurrently with a
combined 16 MiB bound, and handled HUP/INT/TERM signals are forwarded to the
child process group before bounded termination and reap attempts.

The five cases cover clean input, one safe formatting repair, a persistent
parse issue, an invalid `biome.json` whose native status one must become
operational, and a three-file batch. The batch selects one dirty and one clean
file while leaving a dirty sentinel unselected; only the dirty selected file
may change. Immediate execution proves `fix` then authoritative `verify`, and
its second run is clean and mutation-free. The compatibility-deferred path is
seeded independently from pristine input, proves initial check, remedy, final
check, complete changed-file evidence, and then a clean verify-only repeat.
Batch outcome attribution remains conservative even though the full workspace
diff records the exact file Biome changed.

The JSON reporter is explicitly experimental upstream, so the accepted schema
is deliberately locked to Biome 2.5.7 and future patch updates require renewed
tests. The regular-file check is a launch-time preflight, not protection from a
concurrent path replacement. Project configuration can still choose rules and
safe-fix behavior within the adapter's control boundary; unsafe fixes are not
enabled.

The official binary is not RustSec-clean: its dependency inventory includes
`quick-xml` 0.38.4 findings RUSTSEC-2026-0194 and RUSTSEC-2026-0195, plus the
RUSTSEC-2026-0097 `rand` warning. The affected `quick-xml` path is introduced by
the JUnit reporter dependency; this contract locks the JSON reporter and
rejects reporter overrides, so that parser is not reachable through the
validated command. This is a narrow reachability argument, not a claim that
the upstream artifact is vulnerability-free or exempt from future advisories.

### Asciidoctor validation contract

The Ruby environment pins the dependency-free, pure-Ruby
[Asciidoctor 2.0.26 gem](https://rubygems.org/downloads/asciidoctor-2.0.26.gem)
at SHA-256
`16e3accf1fc206bbd6335848649d7fd65f31d2daa60d85af13d47a8ee4b071c1`.
The official [v2.0.26 release](https://github.com/asciidoctor/asciidoctor/releases/tag/v2.0.26)
resolves to commit `0b99b39c9df884d4aec13bba45f03cdbab505769`.
The upstream tag, commit, and gem are unsigned, so the committed RubyGems
package checksum is the integrity guard. The exact product probe prefix is
`Asciidoctor 2.0.26 [`; its second line describes the pinned Ruby runtime and
is intentionally not treated as version identity.

The evaluated outer phase is:

```text
ruby -ropen3 -e <adapter> -- asciidoctor {extra-args} {files}
```

It traces these nested commands, with the enforced options appended last so
configured extra arguments cannot override them:

```text
asciidoctor {extra-args} {files} --safe-mode=safe --failure-level=FATAL --out-file=/dev/null
asciidoctor {extra-args} {files} --safe-mode=safe --failure-level=WARNING --out-file=/dev/null
```

The adapter rejects `--`, help/version early exits, verbose forms that can
devolve into a version-only success, and quiet-mode diagnostic suppression as
operational failures so configured arguments cannot turn the validator into a
successful no-op or erase its evidence.

Asciidoctor's documented CLI status space uses zero for success and one for
syntax, usage, configuration, document-processing, and unexpected failures.
The adapter first runs the same batch silently at a FATAL threshold. A failed
preflight is emitted once and remapped to status two (operational failure); a
successful preflight is followed by the WARNING-threshold pass, where status
one represents document diagnostics. Retained traces prove both nested
invocations and the evaluated outer command. The four contract cases cover a
clean document, a stable missing-include diagnostic, a mixed multi-document
batch with conservative attribution, and an invalid backend that must be
operational.
Both immediate execution and the compatibility-translated deferred lifecycle
run twice without changing source files.

Safe mode permits local includes while rejecting lexical ancestor includes.
It is not a full filesystem sandbox: upstream documents that symlink targets
can bypass the safe-mode jail. The controlled temporary home, denied network,
explicit configuration, and process timeout therefore remain part of the
contract. Documents that intentionally include `../shared-partial.adoc` are a
known limitation of this conservative default.
The preflight buffers its child output and processes clean or source-issue
documents twice; custom Asciidoctor extensions with side effects would likewise
run twice and are outside this built-in contract. A fatal document diagnostic
is indistinguishable from a fatal CLI/configuration failure in the upstream
status model and is therefore conservatively classified as operational.

The mise-managed archive URLs and checksums are in
[`mise.lock`](../crates/hookkit-pkl-config/validation/provisioning/mise.lock).
The independently verified Rust and Ruby URLs and SHA-256 digests are in the
recipe registry. Node, Python, and Ruby package closures live beside it under
`node/`, `python/`, and `ruby/`; the Betterleaks dependency closure and patch
live under `betterleaks/`. Runtime components, auxiliary programs, bootstrap
commands, platform, architecture, minimum OS, and case-network policy are
schema-checked there as well. The current macOS 26 floor is dictated by the
official native Pkl 0.31.1 asset shared by the lane; the Rust, Ruby, and built
Betterleaks artifacts themselves support earlier macOS releases. The Apple
compiler and SDK are host-supplied prerequisites because Apple does not
distribute them as portable redistributable archives; their paths and
minimum-version probes are declared and recorded alongside the checksum-pinned
closure.

Successful runs write `target/pinned-tool-environments/artifacts/` by default.
`pinned-environment.json` records the selected recipe IDs, lock digest, host
OS/architecture and constraint, observed versions, resolved executable paths,
direct-archive, source-build input/output, and selected dependency-lock digests,
sandbox backend, active network-denial result, and outcome. The fixture harness
writes its stable surface-level report to `artifacts/fixtures/report.json`; the
environment record binds that report by SHA-256, while timestamped report copies
preserve run history. Successful fully instrumented contract runs also retain
their exact invocation traces, workspace snapshots and diffs, diagnostic
artifacts, and repeated deferred summaries below the tool's
`artifacts/fixtures/` directory.
Override the state and artifact roots with
`VELVET_GLOVE_PINNED_TOOL_STATE_DIR` and
`VELVET_GLOVE_PINNED_TOOL_ARTIFACT_DIR`.

These eleven smoke contracts establish the reproducible environment substrate;
they do not by themselves promote a tool's full pinned-real-tool coverage tier.
The generated coverage report retains gaps until every required target, surface,
and semantic case has evidence; jq, Betterleaks, Astro, Asciidoctor, and Biome
are covered only after each complete case matrix passes. Linux, Intel, and
full-catalog scheduling remain separate follow-up work.

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
