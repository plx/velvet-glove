# Pinned tool environments

The pinned real-tool lane provisions and runs one exact fixture contract with a
single command. For example:

```sh
just tool-case jq multi-file-fragments
just tool-case buf-format multi-file
just tool-case vacuum multi-file
just tool-case betterleaks multi-file
just tool-case biome multi-file
just tool-case prettier multi-file
just tool-case contextlint multi-file-project
just tool-case dclint autofix-multi-file
just tool-case eslint multi-file
just tool-case ghalint-workflow multi-workflow
just tool-case go-fmt multi-file
just tool-case go-vet test-findings
just tool-case errcheck multi-file
just tool-case goimports multi-file
just tool-case cargo-clippy workspace-autofix
just tool-case cargo-fmt workspace-multi
```

Run all twenty-three behavior-rich representative contracts across eighteen environments
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
   every directly managed Rust or Ruby archive, every Betterleaks or ghalint
   source, patch, module-lock, and build-artifact digest, and the errcheck and
   goimports proxy, module-input, reproducible-artifact, and Go build-identity chains against
   committed values;
3. bootstraps Cargo, npm, Python-wheel, pure-Ruby Bundler, Betterleaks,
   ghalint, errcheck, and goimports Go module graphs from their committed locks as
   applicable; Cargo runs from `/`
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
| Buf data formats | Buf 1.72.0; Python 3.14.5; Apple diff | mise SHA-256; signed upstream checksum manifest; exact host-program probe | `buf-format/multi-file` |
| Vacuum data formats | Vacuum 0.30.0; Python 3.14.5 | official release archive SHA-256; exact member/binary/license/README closure; independently audited upstream checksums and Sigstore bundle | `vacuum/multi-file` |
| Node | Node 24.18.0; Astro 7.2.0; @astrojs/check 0.9.10; TypeScript 6.0.3; Biome 2.5.7; sort-package-json 3.6.1 | mise SHA-256; npm SHA-512 integrity graph | `astro/multi-file-project`, `biome/multi-file`, `sort-package-json/unformatted` |
| Prettier | Node 24.19.0; npm 11.17.0; Prettier 3.9.6; Python 3.14.5 | official Node archive SHA-256; one-package npm SHA-512 integrity graph | `prettier/multi-file` |
| Contextlint | Node 24.19.0; npm 11.17.0; @contextlint/cli and core 1.1.1; Python 3.14.5 | official Node archive SHA-256; exact npm SHA-512 integrity closure | `contextlint/multi-file-project` |
| dclint | Node 24.19.0; npm 11.17.0; dclint 3.1.0; Python 3.14.5 | official Node archive SHA-256; one-package npm SHA-512 integrity graph | `dclint/autofix-multi-file` |
| ESLint | Node 24.19.0; npm 11.17.0; ESLint 10.8.1; Python 3.14.5 | official Node archive SHA-256; exact npm SHA-512 integrity closure | `eslint/multi-file` |
| GitHub Actions | Go 1.26.5; ghalint 1.5.6+velvet-glove.1; Python 3.14.5 | mise SHA-256; source, closure patch, module graph, and reproducible built-artifact SHA-256 | `ghalint-workflow/multi-workflow` |
| Python | Python 3.14.5; embedded pip 26.1.1; Black 26.5.1 | mise SHA-256; platform-specific wheel SHA-256 closure | `black/unformatted` |
| Go | Go/gofmt/go vet 1.26.5; Python 3.14.5 | official Go archive SHA-256; exact mise archive lock | `go-fmt/multi-file`, `go-vet/test-findings` |
| Errcheck Go | Go 1.26.5; errcheck 1.20.0; Python 3.14.5 | official Go archive SHA-256; Go proxy zip SHA-256; exact module sums; reproducible binary SHA-256 and embedded build metadata | `errcheck/multi-file` |
| Goimports Go | Go 1.26.5; goimports/x/tools 0.48.0; Python 3.14.5 | official Go archive SHA-256; four-module proxy closure; exact module sums; reproducible binary SHA-256 and embedded build metadata | `goimports/multi-file` |
| Rust | Rust 1.90.0; rustfmt 1.8.0 | dated official standalone archives with independent SHA-256 digests | `rustfmt/unformatted` |
| Cargo Clippy/Fmt | Rust/Cargo 1.97.1; Clippy 0.1.97; cargo-fmt/rustfmt 1.9.0; Python 3.14.5 | dated official Rust archive SHA-256; independently checked signed channel manifest | `cargo-clippy/workspace-autofix`, `cargo-fmt/workspace-multi` |
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

### buf format validation contract

The separate Buf data-formats environment pins the official
[`buf-Darwin-arm64.tar.gz`](https://github.com/bufbuild/buf/releases/download/v1.72.0/buf-Darwin-arm64.tar.gz)
1.72.0 release archive at SHA-256
`be040ae0ca381103dfda68a36738695c4db3e48de8e91412acdc3d991f39b91e`.
Its `bin/buf` member is byte-identical to the direct Darwin arm64 release
binary and has SHA-256
`5176f23a6118b9978de1340c3e3301a4ed0d48e16a669510be44b4c355170d57`.
The v1.72.0 tag resolves to GitHub-verified commit
[`7d6f05675219fa077f776e9f05b7c7d1a9882e0c`](https://github.com/bufbuild/buf/commit/7d6f05675219fa077f776e9f05b7c7d1a9882e0c).
The upstream
[`sha256.txt`](https://github.com/bufbuild/buf/releases/download/v1.72.0/sha256.txt)
and
[`sha256.txt.minisig`](https://github.com/bufbuild/buf/releases/download/v1.72.0/sha256.txt.minisig)
are independently pinned at SHA-256
`c6ddd4f90a2829ea04efbfbbbc44f8f5d4a0f2dda3bec5ec3fbb652c2d394c06`
and
`468ac733bfeef624cfa2fe45d85d0c6f0d4e3fa1238bc4c9ec7cb7b425ac48fd`.
Both minisign signatures verify with Buf's
[documented public key](https://buf.build/docs/cli/installation/#github),
`RWQ/i9xseZwBVE7pEniCNjlNOeeyp4BQgdZDLQcAohxEAH5Uj5DEKjv6` and trusted
key ID `54019C796CDC8B3F` at timestamp 2026-07-17T20:09:43Z. GitHub provides no
artifact attestation for this release, so the signed checksum manifest and
exact mise lock are the artifact trust boundary. Buf is Apache-2.0-licensed,
with the commit's license file pinned at SHA-256
`995b27237c0d8ef8c970d36da9e81f1472790ae18f3d7d5a966781b53d78f242`.
The source archive used for the independent source-mode security scan has
SHA-256
`52ee072d93e17adec529ca13dd701c0939b3a210a1c2803379007c7a830f502d`.
The exact product probe is `buf --version` → `1.72.0`.

The evaluated phases use the shared pinned Python 3.14.5 interpreter in
isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> buf <write|verify> {extra-args} __VELVET_GLOVE_BUF_WORKSPACE__ {workspace}
```

<!-- markdownlint-enable MD013 -->

Every configured extra argument is rejected because Buf format options can
change selection, symlink behavior, mutation, output, completion status, or
process behavior. The adapter resolves the managed `buf` executable before
replacing child `PATH` with `/usr/bin:/bin`. Each phase first runs:

```text
ABS_BUF config ls-modules --log-format=text --format=json
```

The adapter accepts one complete UTF-8 JSON object on every module line
and checks the union of module paths, includes, and excludes. The root `buf.yaml`
must be a unique regular file, begin with one canonical `version: v1` or
`version: v2` header, and contain one YAML document. Every physical `.proto`
file in the non-symlink workspace tree must be covered by at least one module
and no applicable exclusion. A valid configuration that intentionally leaves
such a Proto outside its modules, includes, or under an exclusion therefore
fails operationally; this conservative restriction prevents a selected file
from producing a successful no-op. Proto files beneath `.git`, `node_modules`,
or `target` are also rejected because the runner deliberately does not
snapshot those directories.

After a successful preflight, the exact nested phase command is one of:

<!-- markdownlint-disable MD013 -->

```text
ABS_BUF format --disable-symlinks --error-format=text --log-format=text --write WORKSPACE
ABS_BUF format --disable-symlinks --error-format=text --log-format=text --diff --exit-code WORKSPACE
```

<!-- markdownlint-enable MD013 -->

Buf invokes the host `diff` program for the second command. The environment
therefore declares `/usr/bin/diff` as an auxiliary program and probes its exact
product string, `Apple diff (based on FreeBSD diff)`. `PATH` is fixed to the
two system executable directories, `DIFF_OPTIONS`, `DEBUG`, and every ambient
`BUF_*` variable are removed, and `BUF_CACHE_DIR` is placed beneath the
harness-controlled absolute `TMPDIR`. The pinned lane records Python 3.14.5,
the managed Buf path, and the Apple diff host prerequisite in every run.

Native status zero is accepted only with empty stdout and stderr. Verify
status 100 is a source formatting issue only when stderr is empty and stdout
contains one or more complete, sorted unified-diff blocks with consistent
paths and hunk counts. The adapter removes only the generated old/new header
mtime fields, replacing each with `<mtime>`; malformed or incomplete status
100 output is operational failure status two. Native configuration, usage,
I/O, spawn, signal, and output-over-16-MiB failures likewise map to two. This
matters because native Buf uses status one for configuration failures and can
misreport arbitrary `diff` subprocess output as status 100.

The four cases cover clean input, one unformatted source, a multi-file
workspace, and a configuration-induced no-op. The operational case has one
clean included Proto plus one dirty excluded Proto: native format alone would
return clean without touching the dirty file, while the module preflight runs
once and the adapter rejects the uncovered path before launching format. The
multi-file case selects one dirty and one clean Proto while an unselected dirty
Proto remains in module scope. One workspace invocation repairs both dirty
files, the workspace write snapshot records their exact bytes, conservative
candidate attribution remains separate from exact changed-file evidence, and
the authoritative verify and repeat are clean.

Immediate execution orders write before verify. Deferred compatibility first
checks the pristine workspace, records status 100 and the canonical diff,
conditionally applies the remedy, and runs a final clean check. A second
deferred attempt on repaired bytes is verify-only. Both surfaces prove the
complete workspace diff and a mutation-free idempotent rerun.

This is a formatting contract, not Proto syntax validation: Buf can accept
some malformed or invalid-UTF-8 inputs as clean or format them destructively.
`--disable-symlinks` and link-count preflight reject the demonstrated symlink
and hard-link escapes, but filesystem and config replacement races remain.
Buf writes are not transactional; a late error can leave earlier files in a
multi-file workspace already formatted. The controlled operational fixture
fails before mutation, while the general partial-write and TOCTOU boundaries
remain explicit limitations. The predictable cache subdirectory is trusted
only because the pinned lane supplies a private controlled `TMPDIR`.

The Darwin arm64 binary was built with Go 1.26.5, `CGO_ENABLED=0`, and only
system dynamic libraries. A `govulncheck` 1.6.0 binary scan reports only
[`GO-2026-5932`](https://pkg.go.dev/vuln/GO-2026-5932) through coarse OpenPGP
module/symbol metadata; an exact v1.72.0 source scan finds it only at module
level and not imported or called. This is a recorded scanner limitation, not
an audit-clean or future-security claim. The release binary has only an ad-hoc
linker signature rather than a Developer ID signature or notarization.

### Vacuum validation contract

The dedicated Vacuum data-formats environment pins the official
[`vacuum_0.30.0_darwin_arm64.tar.gz`](https://github.com/daveshanley/vacuum/releases/download/v0.30.0/vacuum_0.30.0_darwin_arm64.tar.gz)
archive at SHA-256
`bebcc32f58db734bcf329ef6f0754d2b1051d55961ee92aac1d2b1192fad78e8`.
The annotated `v0.30.0` tag object
`5502edc731a0f54a549620ea64e67eb9ef533739` peels to source and release
commit `328ff253522138616096eeabf1dc1c8895dac215`. The archive contains exactly
`LICENSE`, `README.md`, and `vacuum`; their reviewed license, README, and
binary SHA-256 values are
`a4c0921c8f302fdb282c41bcb85e09375561f9c9b38e77c258d89d17492555cf`,
`b57124010840e63ce1263938b623b8e663599e265958d5ae2731ae7aca605522`,
and
`b8fc23e87917742f2b81bb55addc8d1b968759c7ad5e7372ad23748197c53afa`.
The embedded license is MIT. The exact product probe is
`vacuum version` → `0.30.0`.

The upstream
[`checksums.txt`](https://github.com/daveshanley/vacuum/releases/download/v0.30.0/checksums.txt)
and
[`checksums.txt.sigstore.json`](https://github.com/daveshanley/vacuum/releases/download/v0.30.0/checksums.txt.sigstore.json)
were independently audited at SHA-256
`2dac5adb73fe190e2657108f2ab408fafbc0fe5323b33825b03a6537de0207c8`
and
`08dc6453c5f396db405f04f3c0709424fb0a549200e7fbb3768d268c0c2a07bc`.
The bundle subject digest is the checksums-file digest. Its certificate names
repository `daveshanley/vacuum`, workflow `Publish`, source commit
`328ff253522138616096eeabf1dc1c8895dac215`, and SAN
`https://github.com/daveshanley/vacuum/.github/workflows/publish.yaml@refs/heads/main`,
with issuance at 2026-07-23T12:18:43Z by `sigstore-intermediate`. These bundle
details are a committed, independently reviewed provenance record; the runner
does not fetch or cryptographically verify the Sigstore bundle at run time.
Its enforced artifact boundary is the official archive SHA-256 plus the exact
extracted closure above.

The binary is a thin arm64 Mach-O with minimum macOS 12.0 and only system
dynamic-library dependencies. The runner checks those properties and binds
the embedded hardened-runtime flag `0x10000(runtime)` and TeamIdentifier
`HFX5KEHACT` after verifying the binary bytes. Those embedded fields are
metadata checks, not a claim that an Apple code signature or the unsigned
upstream tag establishes artifact trust. The provenance file participates in
the content-addressed installation identity, so the outer provisioner and
denied-network runner resolve the same exact cached binary.

The evaluated command uses shared pinned Python 3.14.5 in isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> vacuum {extra-args} __VELVET_GLOVE_VACUUM_FILES__ {files}
```

<!-- markdownlint-enable MD013 -->

Every configured extra argument is rejected. After validating the complete
selection, the adapter runs one batch with these fixed native arguments:

<!-- markdownlint-disable MD013 -->

```text
vacuum lint --config=vacuum.conf.yaml --base=. --no-update-check --remote=false --no-style --no-banner --details --errors --silent --all-results --no-clip --fail-severity=error --fix=false --timeout=5 --lookup-timeout=500 --turbo=false --hard-mode=false --skip-check=false --ext-refs=false --resolve-all-refs=false --nested-refs-doc-context=false --allow-private-networks=false --allow-http=false --fetch-timeout=5 INPUT...
```

<!-- markdownlint-enable MD013 -->

The explicit empty config and private base prevent user or project Vacuum
configuration from participating. Update checks, remote lookup, HTTP and
private-network access, formatting, result clipping, and fast or skip modes
are fixed off; all error-severity findings are requested. A clean result must
be status zero with no output. Native status one is a source issue only when
stdout contains Vacuum's stable rule and category fields and stderr is empty.
Native status two is operational failure. All other statuses, incomplete
issue evidence, unexpected clean output, signal, spawn, scope, mutation,
cleanup, or output-limit behavior fail operationally.

`--remote=false` does not disable Vacuum's local `$ref` resolver. The exact
0.30.0/libopenapi 0.38.7 source dispatches file lookup only for the literal
`$ref` key; exact-binary probes also found no local or external read for
`$dynamicRef`, `$recursiveRef`, or external `operationRef`. The adapter still
fails closed before launch on raw or mixed-escaped `$ref`, `$dynamicRef`, and
`$recursiveRef`, and on every `\x24`, `\u0024`, or `\U00000024` spelling
regardless of context. This deliberately rejects some harmless strings and
comments, but prevents an encoded key from reopening local resolution without
depending on a YAML parser. External `operationRef` remains syntactically
usable; remote access is disabled both by the fixed arguments and the active
lane sandbox.

Each selected path must be a normalized absolute UTF-8 path to a unique
regular `.json`, `.yaml`, or `.yml` file with link count one. Symlinks,
hard-link aliases, duplicate identities, control characters, more than 256
files, a file over 16 MiB, or a batch over 64 MiB are rejected. The adapter
reads through no-follow descriptors and snapshots content, device, inode,
mode, size, mtime, and ctime. Because even a validated original path could be
replaced between preflight and child launch, Vacuum never receives an original
path: every byte sequence is copied to a unique owned 0600 file under an owned
0700 private tree, and diagnostics are mapped back to stable source-relative
names. Originals are reopened and compared after the child exits.

The child receives private `HOME`, XDG configuration/cache, `TMPDIR`, current
directory, config, and base paths, with `PATH=/usr/bin:/bin`, fixed C/UTC/color
settings, and Vacuum, proxy, dynamic-loader, and Go runtime poison removed.
Combined output is capped at 16 MiB. HUP, INT, and TERM are forwarded to an
owned process group; both normal and exceptional exits perform bounded
descendant sweeps, and cleanup blocks and drains late signals before emitting
one normalized diagnostic. Private directory names and ANSI escapes are
removed from forwarded output.

The four-case matrix covers a silent clean document, a stable rule violation,
a clean-plus-violating multi-file batch, and malformed input that must remain
operational. Claude and Codex immediate hooks and the compatibility-translated
deferred lifecycle run the same evaluated command without mutating sources.
The focused lifecycle matrix additionally exercises no-op status spoofing,
source replacement, hostile umasks, private-path failures, output exhaustion,
signals during spawn and stop, ignoring descendants, and normal-exit orphans.

This contract intentionally validates self-contained copied documents, not
OpenAPI projects that depend on local reference files. Its reference-byte
guard is conservative and can over-reject escaped dollar text. A descendant
that deliberately escapes into a new session or process group cannot be
contained by the adapter, and the preflight/postcheck cannot eliminate all
external filesystem races; the copied-input design prevents those races from
changing what Vacuum reads.

### gofmt validation contract

The Go environment pins the official
[`go1.26.5.darwin-arm64.tar.gz`](https://dl.google.com/go/go1.26.5.darwin-arm64.tar.gz)
archive at SHA-256
`efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a`
through the committed mise lock. The upstream
[`go1.26.5` tag](https://go.googlesource.com/go/+/refs/tags/go1.26.5)
resolves to commit
[`c19862e5f8415b4f24b189d065ed739517c548ba`](https://go.googlesource.com/go/+/c19862e5f8415b4f24b189d065ed739517c548ba).
The exact probe starts with the literal prefix `go version go1.26.5` followed
by a space and the platform; `gofmt` ships in that same toolchain archive. The
evaluated outer command also uses the shared pinned Python 3.14.5 interpreter
in isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> gofmt <write|verify> {extra-args} __VELVET_GLOVE_GOFMT_FILES__ {files}
```

<!-- markdownlint-enable MD013 -->

The adapter rejects every extra argument because native gofmt flags can change
selection, output, or mutation. Each selected path must be normalized,
absolute, UTF-8, canonical, and a unique regular file with link count one;
duplicate paths, symlinks, and hard links fail before the child starts. The
managed gofmt executable is resolved first, then the child receives a fixed
locale, timezone, color, scheduling, telemetry, and toolchain environment with
`PATH=/usr/bin:/bin`. Every inherited `GO*`, `DYLD_*`, `LD_*`, and `DEBUG`
override is removed.

The native commands are deliberately narrow:

```text
ABS_GOFMT -l FILE...
ABS_GOFMT -w FILE...
```

As implemented by the pinned
[`gofmt` source](https://go.googlesource.com/go/+/refs/tags/go1.26.5/src/cmd/gofmt/gofmt.go),
`-l` writes dirty filenames to stdout but still exits zero; parse and I/O
failures exit two, and a batch can emit earlier dirty filenames before a later
parse failure. The adapter therefore lets status two dominate all stdout.
Status-zero checks accept only complete, unique, argv-ordered selected-path
lines and empty stderr. The runner classifies a nonempty valid listing as a
source formatting issue and empty output as clean.

Every write first runs that same read-only `-l` preflight and confirms no file
changed. A status-two preflight stops without invoking `-w`, preventing a dirty
valid file earlier in the batch from being partially repaired before a later
invalid file fails. A successful `-w` must be silent, exit zero, and preserve
every selected device/inode identity; the explicit deferred workflow then runs
the authoritative `-l` final check. Output and selected bytes are bounded at
64 MiB, and cancellation is forwarded to a dedicated child process group with
bounded TERM/KILL cleanup.

The four-case matrix covers clean input, one-file formatting, a selected dirty
plus selected clean batch with an unselected dirty sentinel, and a dirty-valid
plus parse-invalid operational failure. Both immediate and explicit deferred
surfaces run for Claude and Codex. Immediate mutation proves the exact complete
workspace diff and a clean idempotent repeat; deferred mutation additionally
proves the explicit final check and a verify-only clean retry. The operational
case proves the preflight invoked only `-l` and preserved every byte despite
mixed stdout and stderr. The remaining boundary is filesystem
concurrency: canonical-path, identity, and byte snapshots reject demonstrated
link aliases but cannot eliminate external path-replacement races between
checks. Native multi-file `-w` is not transactional, so a late write-time I/O
failure such as a permission change or storage failure can still leave earlier
files mutated even though deterministic parse/read failures are caught by the
preflight.

### go vet validation contract

The go vet recipe reuses the unchanged Go environment and archive documented
above: Go 1.26.5 at SHA-256
`efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a`
and tag commit `c19862e5f8415b4f24b189d065ed739517c548ba`. It adds no package,
lock, license, or bootstrap step. The denied-network runner resolves `go`
inside the managed mise root and requires the exact probe
`go version go1.26.5 darwin/arm64`. The evaluated immediate phase and its
compatibility-translated deferred check share one isolated Python command:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> go {extra-args} __VELVET_GLOVE_GO_VET_WORKSPACE__ {workspace-indicator}
```

<!-- markdownlint-enable MD013 -->

All extra arguments are rejected because analyzer flags such as
`-printf=false` can manufacture a false clean, and ambient Go variables that
can alter toolchain, analyzer, module, or build semantics are rejected before
any child starts. A canonical inherited `GOMODCACHE` is treated only as the
runner-controlled read cache. Every invocation receives a newly allocated
0700 private home, temporary directory, GOPATH, GOTMPDIR, XDG cache, and
GOCACHE. The child environment fixes `CGO_ENABLED=0`, `GOENV=off`,
`GOFLAGS=-mod=readonly`, `GOPROXY=off`, `GOSUMDB=off`, `GOVCS=*:off`,
`GOTOOLCHAIN=local`, and `GOWORK=off`; compiler, loader, debug, CI, and all
other inherited Go channels are removed. The active mise deny-network wrapper
remains authoritative.

The adapter snapshots the unique regular `go.mod`, optional `go.sum`, and all
physical Go files outside fixed skipped subtrees. Symlinks and hard links are
rejected. It then runs exactly these commands through the selected Go
executable, checking the complete snapshot after each child:

```text
go env -json GOARCH GOMODCACHE GOOS GOROOT GOVERSION
go mod verify
go list -mod=readonly -json ./...
go vet -json -mod=readonly ./...
```

The environment record must contain only the requested fields, name exact Go
1.26.5, retain the controlled module cache, and point to an executable GOROOT
toolchain. Module verification must emit its exact success line. The complete
concatenated `go list` stream must describe the selected main module without
incomplete packages or dependency errors, cover every physical Go file as an
enabled or explicitly ignored source, and derive the exact production,
internal-test, and external-test action set.

Go 1.26.5 `go vet -json` does not use its status to distinguish findings:
valid diagnostics still exit zero. Its stdout is a concatenated JSON-object
stream with one object per requested action, including anonymous `{}` objects
for clean actions. The adapter decodes that stream through EOF exactly once,
rejecting duplicate JSON fields, non-finite numbers, truncation, trailing
garbage, and a missing completion newline. The record count must equal the
trusted action count. Every nonempty object must name one expected unrepeated
package or test action; embedded analyzer errors fail operationally. Analyzer
names, diagnostic arrays, messages, canonical workspace paths, positive
line/byte-column positions, UTF-8 boundaries, related information, and
sorted nonoverlapping suggested-fix edits all use a closed validated schema.
Only validated findings map to adapter status 1; a validated all-clean stream
maps to 0. Every native nonzero, any stderr, malformed or incomplete scope,
schema or position error, mutation, signal, descendant leak, or combined
output beyond 16 MiB maps to 2 while preserving bounded native evidence.

The five-case matrix covers a silent clean module, a printf finding, a
multi-package module whose finding is outside the selected candidate file,
both internal- and external-test findings, and an unselected syntax failure.
Claude and Codex goldens cover both immediate and compatibility-deferred
surfaces, including exact status, output, attribution, no mutation, and
idempotence. The evaluated adversarial lifecycle covers analyzer-disable false
cleans through argv and environment, malformed/truncated/multiple JSON,
package/action-count drift, embedded errors, invalid positions and fixes,
mutation, symlink and hard-link aliases, the output cap, pre-allocation,
in-child, and post-removal signals, inherited- and closed-pipe descendants,
and primary-plus-cleanup error composition.

Anonymous `{}` records are deliberately conservative: their total count is
bound to the trusted package/test action count, but Go supplies no identity by
which to prove which clean action each object represents. Build-tag-ignored
files are trusted only when `go list` reports them ignored, and a directory
whose physical files are all ignored is rejected rather than silently
accepted. CGO is disabled, so cgo-only scopes are unsupported. Local replace
targets and already-cached external module sources can be read outside the
workspace snapshot; the adapter proves the main-module inventory, not an
immutable dependency closure. A concurrent replacement changed and restored
between snapshots can evade detection, and a descendant that deliberately
escapes its owned session or process group cannot be reaped. These are stated
boundaries rather than coverage claims.

### errcheck validation contract

The dedicated errcheck environment reuses the locked official Go 1.26.5
Darwin arm64 archive without changing the shared gofmt closure. It pins
`github.com/kisielk/errcheck` 1.20.0 at tag commit
[`4d54a96416c48063572cc1c24ae072fff58a63b4`](https://github.com/kisielk/errcheck/commit/4d54a96416c48063572cc1c24ae072fff58a63b4).
The official Go proxy
[`v1.20.0.zip`](https://proxy.golang.org/github.com/kisielk/errcheck/@v/v1.20.0.zip)
is pinned at SHA-256
`50dbdc1e07128552bda3dad27dfaad9dca100d16869bf58485fe05ed4a45f0b6`.
The committed `go.mod` and `go.sum` are pinned at SHA-256
`06abec38397f045f72e5496d0430dd3473ef2be2fe0187b4d29cd7ff7dd968ef`
and
`594d33a278d8c5313b8b7015f6d8e9590ed0e53ea393296fa9c03ea58a8fa145`.
They lock errcheck plus `golang.org/x/mod` 0.35.0, `golang.org/x/sync`
0.20.0, and `golang.org/x/tools` 0.44.0 with their exact Go sums.

Provisioning downloads that module graph while network access is allowed,
then verifies the root proxy zip and the committed module inputs. Under the
same denied-network sandbox used for cases, it runs `go mod verify`, enumerates
the exact package dependency closure, and executes this version-preserving
build from `/` with a fresh transactional build cache:

```text
go install -trimpath -ldflags "-s -w -buildid=" github.com/kisielk/errcheck@v1.20.0
```

The local file proxy is the only build source, `GOTOOLCHAIN=local`,
`CGO_ENABLED=0`, `GOOS=darwin`, and `GOARCH=arm64` are fixed, and the resulting
binary must have SHA-256
`4f369aeb1bd8454d6ebb6789fedd948ef216fe04c6be629d5016aca78908aa0c`.
Both the provisioner and denied-network runner require `go version -m` to name
Go 1.26.5, errcheck v1.20.0 with its module sum, exactly the three dependency
modules above, trimpath, the Darwin arm64 target, and disabled CGO. The
content-addressed installation identity includes the complete recipe, while
the evidence record cross-links the Go archive checksum, mise lock, proxy and
module hashes, binary hash, and embedded build metadata.

The evaluated command uses pinned Python 3.14.5 in isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> errcheck go {extra-args} __VELVET_GLOVE_ERRCHECK_WORKSPACE__ {workspace-indicator}
```

<!-- markdownlint-enable MD013 -->

Extra arguments are rejected because native errcheck flags can alter package
scope and diagnostic semantics. The workspace `go.mod`, optional `go.sum`,
and every physical Go source outside fixed excluded subtrees must be unique,
canonical regular files. Before invoking errcheck, the adapter creates private
home, temporary, build, and Go-path roots; removes ambient Go, compiler,
loader, CI, and debug configuration; fixes `GOPROXY=off`, `GOSUMDB=off`,
`GOVCS=*:off`, `GOTOOLCHAIN=local`, and `GOFLAGS=-mod=readonly`; and runs three
preflights through the managed Go launcher:

```text
go env -json GOARCH GOMODCACHE GOOS GOROOT GOVERSION
go mod verify
go list -mod=readonly -json ./...
```

The package inventory must account for every physical production, internal
test, and external test Go file or explicitly report it ignored. The native
checker then runs exactly `errcheck -abspath -mod=readonly ./...`. Clean status
zero must be silent. Status one is accepted only as complete UTF-8 diagnostics
whose absolute canonical paths, positive line and column, source text,
workspace membership, sort order, and unchanged file snapshots all validate.
Status two is operational failure; every other status, malformed output,
scope omission, mutation, or preflight inconsistency is normalized to two.
Combined child output is bounded, signals are forwarded to owned process
groups, and normal and exceptional exits perform bounded descendant cleanup.
The adapter retains the exact process-group ID through leader reaping, bounds
post-leader pipe draining, kills and confirms same-group descendant exit, and
composes child and private-root cleanup failures. Signal handlers cover private
allocation through removal; a blocked post-removal cutoff drains pending
HUP/INT/TERM before exit. Random private paths are normalized in every emitted
child or cleanup diagnostic.

The four-case matrix covers a silent clean workspace, one unchecked-error
diagnostic, a multi-package/multi-file workspace that proves complete module
scope beyond the selected files, and an operational module failure. Claude and Codex immediate hooks and the
compatibility-translated deferred lifecycle execute the same read-only command.
The evaluated adversarial lifecycle additionally covers false clean/no-op
preflights, malformed and unstable diagnostics, source and control mutation,
symlink and hard-link aliases, hostile environments, bounded output, signals,
inherited-pipe and closed-stdio normal-exit descendants, pre-allocation and
post-removal signal cutoffs, sanitized initialization failures, and composed
child/private cleanup failures. Filesystem replacement races and descendants that
deliberately escape their process group remain explicit operating-system
boundaries; the adapter fails closed on every demonstrated instance but cannot
make external concurrent actors transactional.

### goimports validation contract

The dedicated goimports environment reproducibly builds
`golang.org/x/tools/cmd/goimports` v0.48.0 from tag commit
[`05f9cb5d358503005bd6f82b17916d226ca7b210`](https://go.googlesource.com/tools/+/05f9cb5d358503005bd6f82b17916d226ca7b210)
and tree `ca40a4b11d95c9392ef4a87520efea157c8aefb5`. The official Go proxy
archive is pinned at SHA-256
`8529e7bd696890fd79d3e1c37c7d1a3e2e26fb4b392b5beebfa7134ad2f65755`.
The committed `go.mod` and `go.sum` are pinned at SHA-256
`9de464c8f30dde87a846b165fadd6620a150e54352265f8b22a7b63959510778`
and
`d43f495d37c149ddc7145f20b13b84812ba3aea895834e7595d6eacd62bc7a44`.
They intentionally contain only x/tools v0.48.0 and the binary's exact build
dependencies: x/mod v0.38.0, x/sync v0.22.0, and x/telemetry at
`49f421fb7959`. Provisioning downloads those four module versions explicitly;
it does not broaden the closure with `go mod download all`.

The source is built from the sealed local file proxy with the official locked
Go 1.26.5 Darwin arm64 toolchain:

```text
go install -trimpath -ldflags "-s -w -buildid=" golang.org/x/tools/cmd/goimports@v0.48.0
```

`GOTOOLCHAIN=local`, `CGO_ENABLED=0`, `GOOS=darwin`, `GOARCH=arm64`, and
`GOARM64=v8.0` are fixed. The resulting 5,814,322-byte binary has SHA-256
`2d7d2892651e4452091f0fe8e280c7b6e14f3b6964854516fd7372442d57fd27`.
Both provisioning and denied-network case execution require `go version -m`
to name Go 1.26.5, x/tools v0.48.0, exactly the three dependencies above,
trimpath, disabled CGO, and the Darwin arm64 v8.0 target. goimports has no
native version flag, so this exact build record and binary hash are the version
probe.

The evaluated immediate phase and explicit deferred workflow use pinned Python
3.14.5 in isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> goimports go verify|write {extra-args} __VELVET_GLOVE_GOIMPORTS_FILES__ {workspace-indicator} {files}
```

<!-- markdownlint-enable MD013 -->

The adapter rejects every extra argument and accepts one canonical root
`go.mod` with an empty dependency graph. It rejects workspaces, nested modules,
vendor trees, semantic module directives, linked directory topology, and
symlink or hard-link source/control aliases. Every physical `.go` file under
the module is bounded, retained, and copied to a read-only private shadow,
including tests, generated and build-tag-excluded files, dot/underscore and
testdata directories, and node_modules. Native `-w` is never invoked.

Every child runs from a complete replacement environment with denied module
resolution. The validated Go bytes are copied into an adapter-owned minimal
GOROOT whose retained `src` directory is empty, GOMODCACHE and GOPATH remain
empty retained semantic trees, telemetry is disabled by its private mode file,
and a retained mode-0400 regular file prevents the mutable goimports module
index from being created. The adapter validates exact Go version, environment,
build metadata, parsed module, and main-module scope; then an exact canary must
add one standard-library import and one physical main-module import.

For every selected file, native `-l`, stdin formatting with fixed `-srcdir`,
and a candidate fixed-point rerun must agree. All candidates are then installed
in a second private shadow and the whole selected batch must remain a fixed
point, closing sibling-file ordering effects. Candidate sources are limited to
goimports' binary-baked standard-library manifest, physical packages in the
inventoried main module, and syntactic imports in inventoried same-directory
siblings. That sibling rule can mirror an externally named path, but external
dependency fetching, resolution, validity, compilation, and type correctness
are not claimed.

Because resolution consumes the full inventoried module, the deferred check
scope is workspace-wide. A later workflow write anywhere under that module
invalidates and reruns an earlier goimports check, including a write to an
unselected same-directory sibling.

Verify emits only validated dirty paths in selected order. Write opens retained
descriptors only for proven-dirty files, revalidates each immediately before
truncation, and performs an authoritative post-commit shadow check. A partial
adapter-owned write is restored best-effort. A completed write is rolled back
only while its identity, link count, mode, exact candidate bytes, and captured
post-write metadata still match; a detected concurrent edit is preserved and
reported unsafe to overwrite. Clean files retain validated bytes, identity,
link count, mode, size, and mtime, while reads may update atime and clean-file
ctime is not promised.

The four-case matrix covers clean input, a missing standard-library import, a
three-selected-file batch proving physical main-module and same-directory
sibling import discovery, and a dirty-before-syntax-error operational batch.
Claude and Codex goldens cover immediate and explicit deferred check, remedy,
final-check, exact diff, and idempotence behavior. The evaluated adversarial
lifecycle covers ambient resolver poisoning, malformed Go and formatter
evidence, non-fixed candidates, semantic-tree injection, alias and topology
races, aggregate bounds, partial commits, guarded rollback, signals, timeouts,
and inherited or closed-pipe descendants. SIGKILL or power loss, failed
rollback, the narrow final-check-to-truncate and guarded-rollback intervals,
and descendants that deliberately escape their process group remain explicit
operating-system limitations.

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

### Prettier validation contract

The dedicated Prettier environment pins the official Node.js
[`node-v24.19.0-darwin-arm64.tar.gz`](https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz)
archive at SHA-256
`8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d`.
That archive supplies Node `v24.19.0` and npm `11.17.0`. npm installs exactly
one runtime dependency with scripts, audit, and funding calls disabled:
[`prettier` 3.9.6](https://www.npmjs.com/package/prettier/v/3.9.6), whose locked
registry integrity is
`sha512-OpN0zzVdiaiAhxpuuj5efpIS4sY9j7bY6uR5mnj5yPzGkdkjNKSJeUThPb60Jw29QuAZgA4o+/iB49kFiaBX6g==`.
The official 3.9.6 release/tag resolves to commit
[`8f0c95057cc91d5836409466cd9d9af3bb901e84`](https://github.com/prettier/prettier/commit/8f0c95057cc91d5836409466cd9d9af3bb901e84).
The tag is unsigned, so the committed npm integrity and Node archive checksum
are the executable trust boundary. The exact product probe is `3.9.6`.

Both phases run the evaluated adapter through pinned Python 3.14.5 in isolated
mode and bind the dedicated Node and Prettier paths from the same case-only
root:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> node prettier format {extra-args} __VELVET_GLOVE_PRETTIER_FILES__ {files}
python -I -c <adapter> node prettier verify {extra-args} __VELVET_GLOVE_PRETTIER_FILES__ {files}
```

The read-only native command is:

```text
node prettier.cjs --config=<private-json-or-/dev/null> --list-different --log-level=log {safe-extra-args} --no-editorconfig --ignore-path=/dev/null --with-node-modules --no-color -- {files}
```

After that complete batch preflight succeeds, format mode may run:

```text
node prettier.cjs --config=<private-json-or-/dev/null> --write --log-level=error {safe-extra-args} --no-editorconfig --ignore-path=/dev/null --with-node-modules --no-color -- {files}
```

<!-- markdownlint-enable MD013 -->

The adapter rejects links, aliases, nonregular selected files, early-exit and
scope-changing options, plugins, executable configuration, configuration
overrides, and implicit config, ignore, EditorConfig, cache, and pragma
discovery. An explicit `.prettierrc` or JSON file is opened without following
links, bounded to 1 MiB, decoded as UTF-8 JSON data, restricted to a reviewed
formatting-option allowlist, canonicalized, and copied mode 0600 to a private
directory outside the project. Node receives only fixed locale, timezone,
terminal, CI, color, and trace values; Node, Prettier, loader, and debug
injection variables are removed. Private paths in native output and adapter
errors are normalized before evidence is emitted.

Native status zero is accepted only with empty output. Status one is a source
issue only when read-only stdout is a newline-terminated, duplicate-free subset
of the exact selected absolute paths and stderr is empty. Every other outcome,
including malformed evidence, configuration diagnostics, status two, excess
output, signals, or a normally exiting child that leaves a same-process-group
descendant, becomes operational status two. HUP, INT, and TERM are forwarded
to an active child group; cleanup retains those handlers through private-config
removal, then blocks and drains them at a documented process-exit cutoff before
restoring handlers while leaving the signals blocked.

The four cases cover clean input, one unformatted source, an invalid numeric
option that upstream reports with status one but the adapter classifies as
operational, and a multi-file batch containing one dirty and one selected-clean
file plus an untouched unselected sentinel. Immediate execution proves the
read-only format preflight, exact mutation, authoritative verify, complete
workspace diff, and a clean idempotent repeat. The compatibility-deferred path
starts from an independent pristine copy and proves initial issue detection,
one conditional remedy, final verification, exact changed-file evidence, and
a clean fixed-state repeat. Both Claude and Codex surfaces execute every case;
the representative is `prettier/multi-file`.

The hostile evaluated-adapter probe additionally proves that a source config
replaced after validation cannot affect the private data copy, random private
paths are normalized on child and unwritable-temporary-root failures, private
state is removed after ordinary failures and signals during both active-child
and cleanup windows, a closed-stdio same-group orphan is killed and rejected,
and a mixed dirty-valid plus parse-invalid batch never reaches `--write`.
These controls do not eliminate concurrent selected-file replacement or
content changes after the launch-time check, a target change after the format
preflight, or partial writes from a late native write failure; unsafe rollback
is deliberately not attempted. A descendant that deliberately escapes into a
new session or process group is outside adapter containment. Disabling plugins
and executable configuration sharply narrows child behavior, but Prettier and
its parser still process untrusted project bytes and are not a code sandbox.

### Contextlint validation contract

The separate Contextlint environment reuses the official Node.js
[`node-v24.19.0-darwin-arm64.tar.gz`](https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz)
release archive, SHA-256
`8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d`,
but installs it into a distinct case-only root. The archive supplies Node
`v24.19.0` and npm `11.17.0`. `npm ci --ignore-scripts` installs the exact
106-package lock closure headed by
[`@contextlint/cli` 1.1.1](https://www.npmjs.com/package/@contextlint/cli/v/1.1.1)
and
[`@contextlint/core` 1.1.1](https://www.npmjs.com/package/@contextlint/core/v/1.1.1).
Their registry integrities are, respectively,
`sha512-QCyjqmdaoanH9L8AduX2jH7vRm2yryHpxroLai0PHHP2lijBTG96UEICCuSIHbkoQ4FXulrokQst5+eTf34v9g==`
and
`sha512-ui2ymL90ZlV260NZD8pgki6fwCUM1bX2wj1LbDy5H4u7w8JyTvxIBORxzhWlklDUmsXf1wVxIZXdbvuRYRsqfQ==`.
Every nonroot locked package has a registry URL and integrity, and the graph
contains no lifecycle scripts. The committed package and lock files have
SHA-256
`e8ed6fc071fc602be902f704287d2f6dcc2ca3ab6ff328c7c6805e2da4149e11`
and
`5befd86e5ac7979d3c062fa55d2a5486466458851e754134679e8f5f180d5fff`.

The official lightweight
[`v1.1.1` release tag](https://github.com/nozomi-koborinai/contextlint/releases/tag/v1.1.1)
directly targets commit
[`66cfbffa1175df349379f270e56673c73ff13c54`](https://github.com/nozomi-koborinai/contextlint/commit/66cfbffa1175df349379f270e56673c73ff13c54).
Because a lightweight tag has no tag-object signature, the committed npm
integrities and Node archive checksum are the executable trust boundary. The
driver verifies both package manifests, the CLI-to-core 1.1.1 dependency, the
CLI symlink's exact resolution to `@contextlint/cli/dist/index.js`, `npm ls
--all`, the Node/npm probes, and the system-only Mach-O dependency closure
before a case runs. The exact product probe reads the CLI package manifest with
the dedicated Node runtime and returns `1.1.1`.

The evaluated outer command runs through pinned Python 3.14.5 in isolated mode
and binds the dedicated Node and canonical CLI-JS paths rather than consulting
the shared Node graph:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> node <contextlint-cli> <workspace>/contextlint.config.json __VELVET_GLOVE_CONTEXTLINT_FILES__ {selected-files}
```

Every invocation then executes exactly two native children. The private
positive-completion witness is:

```text
node --disable-proto=throw --permission --allow-fs-read={package-graph} --allow-fs-read={private-root} <contextlint-cli> lint --config {private-root}/contextlint.config.json --cwd {private-root} --format json -- {private-root}/probe.md
```

It must exit one with empty stderr and the byte-stable SEC-001 diagnostic for a
missing synthetic completion section. Only then may the project command run:

```text
node --disable-proto=throw --permission --allow-fs-read={package-graph} --allow-fs-read={workspace} --allow-fs-read={private-root} <contextlint-cli> lint --config {private-root}/project.config.json --cwd {workspace} --format json -- {complete-markdown-inventory}
```

<!-- markdownlint-enable MD013 -->

The adapter accepts no extra CLI arguments. The source configuration must be
bounded UTF-8 JSON with no duplicate keys, a top-level object, and a nonempty
`rules` list whose every `rule` is in the adapter's exact 21-rule Contextlint
1.1.1 built-in allowlist. Per-rule `options` remain data interpreted by that
pinned built-in rule and are not independently schema-validated by Velvet
Glove; malformed or unsupported semantics must therefore fail through the
native completion and status checks. Other Contextlint configuration data may
be present, but `include` patterns never control execution scope. The validated
source config is copied to a mode-0600 private file and only that authoritative
copy is passed to the project child.

Scope is the complete physical, case-insensitive `.md` and `.markdown`
inventory below the indicator's real, non-symlink workspace. The runner uses
character-class globs for every suffix letter, so mixed-case names such as
`notes.Md`, `notes.mD`, and `guide.mArKdOwN` are selectable. Tool-local root
and nested exclusions prevent candidates below `.git`, `node_modules`, and
`.velvet-glove`; the adapter likewise skips only physical directories with
those names. Every symlink the inventory encounters is rejected, including an
excluded-root entry that is itself a symlink. Symlinks nested inside a real
skipped directory are unwalked and outside inventory. Hard-linked Markdown
files, duplicate inodes, nonregular Markdown entries, an empty inventory, more
than 4,096 Markdown files, any one file over 16 MiB, more than 64 MiB total
Markdown, and more than 100,000 traversed entries are also rejected. Runner
candidates must be unique members of that inventory, but the project child
receives the entire inventory, so config includes and candidate selection
cannot hide a project or cross-file finding. The config and all Markdown
identities, modes, sizes, mtimes, and SHA-256s are checked again after native
completion.

Native Contextlint interprets each explicit file path as a glob. To prevent a
literal physical file from becoming a silent zero-match pattern, the adapter
rejects any absolute indicator, traversed-directory, Markdown-inventory,
candidate, or temporary-root path component containing a character from the
exact pinned glob-magic set `\*?[]{}()` before launching a child. Ordinary
non-Markdown files with those characters remain outside inventory.

The private root is a unique mode-0700 directory under an absolute real
`TMPDIR` outside the workspace; its config, authoritative config copy, and
probe document are mode 0600 and fsynced. Node receives a minimal fixed
home/temp/cache, locale, timezone, terminal, CI, color, and worker environment;
ambient Node, Contextlint, npm, loader, debug, and configuration channels are
absent. Permission mode grants only the three declared read roots and no
writes, child processes, workers, or native addons. Raw private paths are
normalized in child output, adapter failures, and temporary-root creation
errors. Combined output is capped at 16 MiB.

Native status zero or one is accepted only with empty stderr and an exact JSON
array whose entries contain only `file`, `line`, `severity`, `message`, and
`ruleId`. Status one must correspond to at least one error; status zero must
have no error. Any nonempty validated report, including warning-only output,
maps to Velvet Glove source status one. Empty output maps to clean. Status two
and all other native statuses, malformed or incomplete JSON, contradictory
severity/status, diagnostics outside the physical inventory, permission
errors, mutation, spawn/read errors, excess output, or failed cleanup map to
operational status two.

HUP, INT, and TERM are blocked across guarded spawn, forwarded to an active
owned process group, retained throughout private-root cleanup, and drained at
the process-exit cutoff. Every exit path closes pipes and performs bounded
termination/reap confirmation. A normally exiting leader with a same-group
descendant is killed and rejected; inherited output pipes after leader exit
also fail within a fixed bound. Cleanup errors compose with the primary error
instead of replacing it.

The four cases are `clean`, warning-only `source-issue`,
`multi-file-project`, and `operational-failure`. The multi-file representative
selects two documents but proves the native child receives all three physical
documents, including an unselected file whose diagnostic and a project-level
missing-file diagnostic must remain visible. All four execute on Claude and
Codex immediate hooks and their compatibility-deferred workflows. Retained
evidence binds the exact adapter, two-child argv/status sequence, managed
Node/CLI graph, controlled environment, authoritative private config, complete
workspace snapshots, no mutation, and clean semantic repeats.

The adapter and native project command are read-only, but these controls do not
eliminate time-of-check/time-of-use races created by another process.
Contextlint receives file paths rather than retained descriptors, so launch and
final snapshots detect persistent Markdown changes but cannot detect a
transient replacement restored to the same observed identity and bytes.
Copying the config prevents replacement after the copy from changing child
authority, but a concurrent writer can still affect which coherent JSON
snapshot the initial bounded read captures. Concurrent workspace topology,
referenced-target, Markdown, config, or executable replacement therefore
cannot be eliminated.

Physical `.git`, `node_modules`, and `.velvet-glove` subtrees and ordinary
non-Markdown workspace objects are outside inventory. Node permission mode
grants the lexical workspace read root rather than symlink-safe physical
containment, so built-in rule links or options targeting unwalked content in a
real skipped subtree can follow a nested symlink for existence checks. A
descendant that deliberately creates a new session or process group and closes
inherited pipes is outside adapter containment. Node's permission model narrows
Node APIs but is not an OS sandbox, and the pinned CLI still parses untrusted
Markdown and rule data; the active macOS deny-network wrapper remains
authoritative for network isolation.

### dclint validation contract

The dedicated dclint environment reuses the official Node.js
[`node-v24.19.0-darwin-arm64.tar.gz`](https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz)
archive at SHA-256
`8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d`,
but installs it into a distinct case-only root. That archive supplies Node
`v24.19.0` and npm `11.17.0`. npm installs exactly one runtime dependency with
scripts, audit, and funding calls disabled:
[`dclint` 3.1.0](https://www.npmjs.com/package/dclint/v/3.1.0), whose registry
integrity is
`sha512-afTGdzRFUXK4yCpIiEW/LOR+9TOMEDhNldDp56VCWzn7JDmD451PcUi640GGlMHgbHKJ10rDBm4PtpcBbjqlXw==`.
The package identifies the upstream
[`v3.1.0` release](https://github.com/zavoloklom/docker-compose-linter/releases/tag/v3.1.0).
The committed package and lock digests, Node/npm identities, exact executable
symlink target, and native dependency closure are checked before every selected
case. The product probe must return exactly `3.1.0`.

The immediate phases and explicit deferred workflow run the same evaluated
adapter through pinned Python 3.14.5 in isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> dclint fix <project-root> {extra-args} __VELVET_GLOVE_DCLINT_FILES__ {files}
python -I -c <adapter> dclint verify <project-root> {extra-args} __VELVET_GLOVE_DCLINT_FILES__ {files}
```

Every native read-only child receives:

```text
dclint --formatter=json --color=false --max-warnings=0 --config=<private-json> {files}
```

Only the proven-fixable subset may receive the write form:

```text
dclint --formatter=json --color=false --max-warnings=0 --config=<same-private-json> --fix {proven-fixable-files}
```

<!-- markdownlint-enable MD013 -->

Extra arguments are either empty or exactly one normalized project-relative
`--config=<path.json>` outside fixed skipped directories. The adapter opens a
unique regular config without following links, bounds it to 1 MiB, rejects
duplicate keys and non-finite or deeply nested JSON, and accepts only the
reviewed data fields and built-in rule options. Executable-loading keys,
nonempty excludes, diagnostic suppression, unknown rules or options, and
absolute, parent, skipped-subtree, non-JSON, symlink, or hard-link config paths
fail before native execution. The coherent source snapshot is normalized into
a mode-0600, fsynced private copy; only that copy is passed to dclint, and both
source and private identities are rechecked around every child.

dclint 3.1.0's top-level-order fixer reconstructs the document from only its
configured order. Its native default omits the Compose-supported `models` map,
and an incomplete custom order can therefore delete omitted keys while still
converging cleanly. The adapter replaces every default or numeric-enabled form
with the complete order `x-properties`, `version`, `name`, `include`,
`services`, `models`, `networks`, `volumes`, `secrets`, `configs`.
`x-properties` is dclint's sentinel for all actual `x-*` extension keys. An
explicit user order and severity are retained only when the order is an exact
complete permutation; incomplete or duplicate orders fail before a child.
Service-key ordering appends otherwise unlisted keys, but dclint throws when a
key belongs to two effective groups, so duplicate membership after merging
custom groups with native defaults is likewise rejected before spawn.

The native `no-version-field` fixer has a separate data-loss defect: during any
write it removes the first line whose trimmed text starts with `version:`, even
when that line is nested extension data and a `disable-line` directive hid the
diagnostic. The adapter marks that rule nonfixable, injects
`no-version-field: 0` into every normalized config, and rejects every explicit
numeric or array-form enable before native execution. An unexpected native
report that nevertheless marks this rule fixable also contradicts the pinned
fixability table and fails operationally. This global disable is intentional;
the adapter does not attempt to parse or rewrite untrusted YAML itself.

The project root and selected files must be normalized absolute physical paths.
Selections are stable-sorted, unique regular files with one link, remain below
the project, contain at most 4,096 files and 64 MiB in aggregate, and cannot
alias by inode. Before native execution the adapter snapshots every retained
file and directory below the project except fixed `.git`, `.velvet-glove`,
`node_modules`, and `target` subtrees. The retained snapshot is bounded to
8,192 files, 8,192 directories, and 128 MiB. Encountered retained links,
nonregular files, unreadable objects, and unstable identities fail closed.

Native status zero or one is accepted only with empty stderr and an exact JSON
record for every requested file in order. Each record's path, fields, message
shape, known rule name, locations, counters, severity class, validation-rule
semantics, and fixed dclint 3.1.0 rule fixability must agree. An empty complete
report maps to clean; any messages map to source status one. Unsupported status,
stderr, malformed or incomplete JSON, non-finite values, counter/status
contradictions, unknown rules, ambiguous fixability, and excess output map to
operational status two.

Fix mode always runs that read-only batch first. Validation diagnostics and
reports without a proven fixable message stop without a write. Otherwise only
the files carrying validated fixable messages are passed to `--fix`. The write
must change bytes, preserve the identities and modes of changed files, and
leave every other retained byte, path, mode, and directory unchanged. A second
full read-only batch over all selected files is authoritative; it must be
converged, and its per-file result must exactly match the write child's result
for each fixed file. Operational failure after the baseline exists attempts to
restore retained file bytes, modes, and mtimes plus directory topology and
modes. Rollback failure composes with the primary failure instead of replacing
it.

`TMPDIR` must name an existing absolute directory outside the retained project.
The adapter canonicalizes accepted symlink and trailing-slash spellings before
the outside-project check and private `mkdtemp`, revalidates the resolved path,
and exports only that canonical root to native children. The private directory
is mode 0700 and removed on every exit path; raw random paths are normalized in
failures. Node, dynamic-loader, debug, dclint-config, and color injection
variables are scrubbed, while the child `PATH`, locale, timezone, terminal, CI,
color, and warning settings are fixed. Combined child output is capped at
32 MiB.

HUP, INT, and TERM are atomically blocked around spawn, forwarded to the owned
process group, and drained across child and private-config cleanup to a
deterministic process-exit cutoff. Native leaders are reaped with bounded TERM
and KILL escalation; a normally exiting leader that leaves a same-group
descendant is swept and rejected. Cleanup and cancellation failures compose
with the primary error.

The five cases are `clean`, persistent nonfixable `source-issue`, invalid-YAML
`validation-issue`, `autofix-multi-file`, and pre-spawn config
`operational-failure`. The representative selects one dirty and one clean file,
preserves an unselected sentinel, retains a Compose `models` map and two
extension maps, and proves a nested `x-meta.version` protected by a line-level
disable survives an unrelated service-order repair. All five execute through
Claude and Codex immediate hooks and the explicit deferred workflow from
independent pristine baselines. Retained evidence binds the exact adapter,
private config bytes, nested argv/status sequence, managed Node/CLI graph,
controlled environment, complete workspace diffs, authoritative recheck, and
clean or persistent-issue idempotence.

Physical fixed skipped subtrees are outside the retained snapshot, including
objects reached only below them. Concurrent project, config, selected-file, or
executable replacement cannot be eliminated between bounded checks and native
path access. Rollback restores the asserted byte/mode/mtime and topology
contract but not original file inode identity or directory mtimes, and rollback
itself can fail. A descendant that deliberately creates a new session or
process group is outside adapter containment. dclint and its YAML parser still
process untrusted project bytes and are not a code sandbox; the active macOS
deny-network wrapper remains authoritative for network isolation.

### ESLint validation contract

The dedicated ESLint environment reuses the official Node.js
[`node-v24.19.0-darwin-arm64.tar.gz`](https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz)
archive at SHA-256
`8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d`,
but installs it in a distinct case-only root. That archive supplies Node
`v24.19.0` and npm `11.17.0`. npm `ci` installs the complete dependency closure
from the committed lock with scripts, audit, and funding calls disabled. The
direct package is
[`eslint` 10.8.1](https://www.npmjs.com/package/eslint/v/10.8.1), published
2026-08-07 with registry integrity
`sha512-wqA7W2jbsC/BnV9Iv1UZpKVFkO1AdNoSmYW8NWG4HNOBbkAMvIqDZ27pI2f07dqn583NcIC44ckjAcOXDL1QbQ==`
and npm shasum `fb37d514c19b6dd5b2d6b70169fd26fddfa97967`. Its package
`gitHead` and the upstream
[`v10.8.1` release](https://github.com/eslint/eslint/releases/tag/v10.8.1)
both bind commit `c049dc3c4294da7afe3d920a1a5fdeba388f4983`.
Before every selected case the driver checks the committed package/lock
digests, exact root engines and dependency, direct tarball URL and integrity,
installed package version, npm bin target, Node Mach-O closure, and exact
`v10.8.1` product probe.

Both the immediate pipeline and compatibility-deferred workflow run the same
evaluated adapter through pinned Python 3.14.5 in isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> node <eslint-cli> fix {extra-args} __VELVET_GLOVE_ESLINT_FILES__ {files}
python -I -c <adapter> node <eslint-cli> verify {extra-args} __VELVET_GLOVE_ESLINT_FILES__ {files}
```

Every native read-only child receives this fixed shape, with a distinct cache
directory for each invocation:

```text
node eslint.js --format=json --no-color --no-config-lookup --config=<private-cjs> --no-ignore --no-warn-ignored --no-inline-config --max-warnings=0 --concurrency=off --cache --cache-strategy=content --cache-location=<private-cache> --suppressions-location=<private-json> --pass-on-unpruned-suppressions -- {files}
```

Fix dry-runs add `--fix-dry-run`; the narrowly admitted native write adds
`--fix` and receives only files whose exact replacement bytes were predicted.

<!-- markdownlint-enable MD013 -->

Extra arguments are unsupported. The adapter never discovers or executes
project ESLint configuration, plugins, parsers, processors, ignore files,
inline configuration, or suppressions. It accepts only normalized absolute,
unique regular `.js`, `.cjs`, and `.mjs` selections inside the project root.
Symlinks, hard links, duplicate inode aliases, non-UTF-8 paths or source,
files over 16 MiB, and batches over 64 MiB fail before native execution.
TypeScript and JSX are intentionally outside the core built-in contract.

The optional project-root `.velvet-glove-eslint.json` is bounded to 1 MiB and
must be one strict UTF-8 JSON object without duplicate keys or non-finite
values. It accepts only optional `$schema` and `rules` keys. `rules` can change
only the severity of `eqeqeq`, `no-debugger`, `no-undef`, `no-unused-vars`,
`no-var`, `prefer-const`, and `semi`, using `off`, `warn`, `error`, or numeric
zero through two. Every other key, rule, option, plugin, parser, or executable
channel rejects before a child. The normalized rule map becomes a mode-0600,
fsynced private CJS flat config with fixed module/CommonJS language modes; an
empty mode-0600 suppressions document and mode-0700 per-child cache directories
live in the same unique private root. Only those private paths are passed to
ESLint, and raw randomized paths are normalized from retained diagnostics and
evidence.

Node and the ESLint JavaScript CLI resolve to absolute files in the managed
root. Native children receive a minimal fixed path, locale, timezone,
terminal, CI, color, and worker environment. Project/user home, temporary and
XDG cache roots plus ambient Node, npm, ESLint, loader, debug, and configuration
variables are absent. Combined output across the adapter lifecycle is capped
at 16 MiB.

Native status zero or one is accepted only with empty stderr and one strict
JSON result for every selected file. Every result path, message severity,
fatal/error/warning count, fixable count, suppression list, and optional
predicted output must be internally consistent. Status one must correspond to
at least one warning or error; status zero must be diagnostic-free. The adapter
emits stable relative-path JSON diagnostics for issues. Status two and all
other native statuses, stderr, malformed or incomplete JSON, duplicate or
out-of-scope results, count/status contradictions, mutation during a read-only
check, excess output, spawn, signal, private-state, or cleanup failure map to
operational status two.

Fix mode first runs the complete native read-only batch, snapshots selected
file identities, modes, and bytes, then runs `--fix-dry-run` over the same
batch. Fatal diagnostics or a dry-run with no predicted changes stop without a
write. Otherwise only predicted files reach `--fix`; the write must preserve
their identities and modes and produce byte-for-byte the dry-run output while
leaving every other selection unchanged. A final read-only batch over all
selected files is authoritative and must match the dry-run diagnostics after
removing native source/output fields. Immediate and deferred retained runs use
independent pristine baselines, and the clean repeat proves idempotence.

HUP, INT, and TERM are guarded around spawn, forwarded to the owned process
group, retained through private cleanup, and drained at the exit cutoff. A
normally exiting leader that leaves same-group descendants is killed and
rejected; pipes, process waits, and cleanup are bounded, and composed failures
preserve the primary diagnostic.

The five cases are `clean`, persistent `source-issue`, exact `autofix`,
`multi-file`, and pre-spawn `config-failure`. The representative selects one
dirty and one clean file, leaves an unselected CommonJS sentinel untouched,
and proves exact batch attribution and mutation. All five execute through
Claude and Codex immediate hooks and compatibility-deferred workflows from
independent baselines. Retained evidence binds the evaluated adapter, private
config/suppressions/cache modes and bytes, every nested argv/status sequence,
managed Node/CLI graph, controlled environment, workspace diffs, final
verification, and semantic repeat.

These bounded checks cannot eliminate concurrent selected-file, source-config,
temporary-root, or executable replacement after validation. Native ESLint
writes by path, so a late failure can leave an earlier predicted write applied;
the adapter intentionally does not attempt an unsafe rollback. A descendant
that deliberately creates a new session or process group is outside adapter
containment. ESLint still parses untrusted JavaScript and is not a code
sandbox; the active macOS deny-network wrapper remains authoritative for
network isolation.

### ghalint workflow validation contract

The dedicated GitHub Actions environment builds ghalint from the upstream
[`v1.5.6` source archive](https://github.com/suzuki-shunsuke/ghalint/archive/refs/tags/v1.5.6.tar.gz),
pinned at SHA-256
`1188047b654a86390d49b776153c1a7b3eddde30ebcc0d024dfab9585785b02b`.
The annotated tag peels to commit
[`050e825989101021ece297e4d2f726f519ba89ee`](https://github.com/suzuki-shunsuke/ghalint/commit/050e825989101021ece297e4d2f726f519ba89ee).
Velvet Glove applies the committed closure patch at SHA-256
`5e3c2480665eefffa019adf5c57e27e1c1d05a74b9dccf2d5bc345017a17d6ed`,
updating only `golang.org/x/text` from 0.28.0 to 0.39.0. The patched `go.mod`
and `go.sum` are independently pinned at SHA-256
`ada0a9434578f54fd6a50fe8ed9ef26374afa631d5527660723062663d686f16`
and
`53a4a1b1a7dcd2a6da2dc1cc0cc32ca4bcb5b8ea86832749e18879b8be594dbb`.

Provisioning downloads that complete module graph, then enters the active
deny-network sandbox for `go mod verify` and the reproducible build. Locked Go
1.26.5 runs with `GOTOOLCHAIN=local`, `-mod=readonly`, `CGO_ENABLED=0`,
`GOOS=darwin`, `GOARCH=arm64`, `-trimpath`, `-buildvcs=false`, an empty build
ID, and source epoch `1777591460`. The resulting
`ghalint 1.5.6+velvet-glove.1` arm64 Mach-O is pinned at SHA-256
`03437b6c73d1332460d24f2c9fe22d3dea0fe68e4e52b0a8a534b3f2854274fa`.
Its embedded module path, Go 1.26.5 compiler, x/text 0.39.0 dependency,
trimpath, CGO setting, minimum macOS version, and system-only dynamic-library
closure are checked before use. The upstream release binary is deliberately
excluded because it embeds stale Go 1.26.2; the unchanged module `go 1.26.2`
language directive does not authorize that binary. The exact product probe is
`ghalint --version` → `ghalint version 1.5.6+velvet-glove.1`.

The immediate phase and explicit deferred workflow use the same evaluated
adapter through pinned Python 3.14.5 in isolated mode:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> ghalint <project-root> {extra-args} __VELVET_GLOVE_GHALINT_WORKFLOW_FILES__ {files}
```

The native validation child is:

```text
ghalint run [--config=<private-yaml>] <project-root>
```

<!-- markdownlint-enable MD013 -->

Extra arguments are either empty or exactly one normalized project-relative
`--config=<path.yml|path.yaml>` outside fixed skipped directories. The adapter
rejects ambiguous implicit configs; validates a unique regular, single-link,
at-most-1-MiB source; and copies its exact bytes into a mode-0600 private file.
The source and private identities are rechecked around both the exact version
probe and validation child. `TMPDIR` must resolve to an existing normalized
absolute directory outside the project. Dynamic-loader, Go, GitHub, ghalint,
debug, locale, and color control channels are replaced by a minimal child
environment with a fixed executable path.

ghalint 1.5.6 discovers only direct files below `.github/workflows`, scanning
`*.yml` before `*.yaml`; nested workflows and other suffixes are outside its
native scope. The adapter independently inventories exactly that physical set,
rejects ghalint's native zero-file success, and requires every marker-delimited
selected candidate to be a unique member of that inventory. The selected
subset triggers one authoritative native workspace scan; inventory is bounded
to 4,096 workflows. Before spawning ghalint it also snapshots the complete
retained project, excluding physical `.git`, `.velvet-glove`, `node_modules`,
and `target` subtrees, with bounds of 8,192 files, 8,192 directories, 128 MiB
total, and 16 MiB per file. Encountered symlinks, hard-linked files,
nonregular objects, aliased workflows, or unstable identities fail closed.

Native clean status zero is accepted only with empty stdout and stderr and an
unchanged project. Native status one is ambiguous: ghalint uses it for policy
findings, workflow YAML parse errors, invalid configuration, and other failures
while emitting timestamped human logs. The adapter therefore accepts it only
after every log line matches the closed v1.5.6 grammar. Policy records must name
one of the pinned policy IDs and its exact documentation reference; action
policies must carry `action`, job-secret findings must carry `env_name`, and
the two direct workflow-secret messages map only to `workflow_secrets`.
Workflow parse records may carry at most one pinned structured `permission` or
`secrets` field. Configuration records may carry only the corresponding code
reference, `path.Match` pattern reference, or policy name, and they always map
to operational status two. Unknown messages, fields, policies, references,
field combinations, malformed quoting, excess output, any other status, or a
project mutation also map operationally. Accepted findings are emitted as
stable JSON records, and workspace invocation conservatively attributes them
to every selected candidate.

The six cases are `clean`, ordinary `source-issue`, `policy-grammar`,
`malformed`, `config-failure`, and `multi-workflow`. The grammar case executes
real action-reference, GitHub App, and direct workflow-secret policies. The
malformed case exercises independent `permission` and `secrets` YAML parse
shapes; the invalid action pattern produces the native `pattern_reference`
configuration shape. The representative supplies both top-level suffixes,
selects one candidate, finds an issue in its unselected top-level sibling,
leaves a nested ignored workflow outside scope, and proves conservative
workspace attribution. All six run through Claude and Codex immediate hooks
and the explicit deferred lifecycle from independent
pristine baselines. Focused lifecycle probes additionally cover the alternate
configuration `policy_name` shape, every grammar contradiction, selection and
config aliases, source/config/executable replacement, retained-project
mutation, unexpected-exception normalization, per-child output reset, private
path redaction, unwritable temporary roots without a child, composed cleanup
failures, pre-cleanup and post-block signals, and bounded inherited-pipe and
closed-stdio same-group descendants with final SIGKILL-survival confirmation.

Physical fixed skipped subtrees and nested workflows remain outside the
retained/native scope. Concurrent project, workflow, config, or executable
replacement cannot be eliminated between bounded checks and native path
access. The read-only adapter does not perform rollback because every detected
mutation is operational failure. A descendant that deliberately creates a new
session or process group is outside containment. ghalint and its YAML parser
still process untrusted project bytes and are not a code sandbox; the active
macOS deny-network wrapper remains authoritative for network isolation.

### Cargo Clippy validation contract

The dedicated Cargo Clippy environment installs selected components from the
official dated
[`rust-1.97.1-aarch64-apple-darwin.tar.xz`](https://static.rust-lang.org/dist/2026-07-16/rust-1.97.1-aarch64-apple-darwin.tar.xz)
distribution archive at SHA-256
`c9748cc86107734a2a024069908a895de7caa2d37062fb641eef9f756938ace2`.
That one archive supplies the paired Rust 1.97.1, Cargo 1.97.1, and Clippy
0.1.97 closure. Its cache key hashes the canonical archive identity together
with the exact installed component set (`rustc`, the arm64 standard library,
Cargo, `clippy-preview`, and `rustfmt-preview`). That component-qualified root
is shared by Cargo Clippy and Cargo Fmt, while a pre-Cargo-Fmt archive-only
cache cannot satisfy or shadow it. The exact probes are:

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo 1.97.1 (c980f4866 2026-06-30)
clippy 0.1.97 (8bab26f4f6 2026-07-14)
```

The archive checksum was independently cross-checked against the official
[`channel-rust-1.97.1.toml`](https://static.rust-lang.org/dist/channel-rust-1.97.1.toml)
manifest at SHA-256
`03569b1886ceb5c05276b50c8431ab111de944cd6140fe1fa7d821dd8e0f29cf`.
Its detached signature has SHA-256
`14553bf89b963f1d1f0a92413b91510ed43f8d50c68fe665763747d815022017`
and validates against the Rust release-key fingerprint
`108F66205EAEB0AAA8DD5E1C85AB96E6FA1BE5FE`; the official HTTPS key file has
SHA-256
`e54b09a439647e006b4831eec9785cbaaf3e07ab371c3a6ee6a68e1bdb9fbc6b`.
The pinned driver enforces the archive digest; the signed-manifest chain is an
independent review-time cross-check, not a runtime PGP verification claim. The
standalone Clippy component archive was also checked at SHA-256
`5e44c0ac5ca9b6f14a3c9031a61f583348b902f908f46e95717aef1dbd2807db`;
its `cargo-clippy` and `clippy-driver` bytes match the copies bundled in the
full archive. Rust and Clippy sources are dual-licensed under MIT or
Apache-2.0, while the binary distribution also contains third-party notices.

Rust 1.97.1 is intentionally separate from the retained Rust 1.90/rustfmt
environment. It avoids changing earlier rustfmt evidence and incorporates the
Cargo fixes described by
[CVE-2026-33056](https://blog.rust-lang.org/2026/03/21/cve-2026-33056/),
[CVE-2026-5222 and CVE-2026-5223](https://blog.rust-lang.org/2026/05/25/cve-2026-5223/),
plus the LLVM correctness fix in the
[Rust 1.97.1 release](https://blog.rust-lang.org/2026/07/16/Rust-1.97.1/).

Both phases use pinned Python 3.14.5 in isolated mode. The rendered outer
commands are:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> cargo cargo-clippy fix {extra-args} __VELVET_GLOVE_CARGO_CLIPPY_WORKSPACE__ {workspace-indicator}
python -I -c <adapter> cargo cargo-clippy verify {extra-args} __VELVET_GLOVE_CARGO_CLIPPY_WORKSPACE__ {workspace-indicator}
```

Each completed clean or source-result phase launches this exact sequence of
three read-only children from a private invocation directory; an operational
failure stops at the child that proves it:

```text
cargo metadata --format-version=1 --no-deps --manifest-path {workspace-indicator} --frozen --quiet --color=never
cargo-clippy clippy --manifest-path {workspace-indicator} --workspace --all-targets --all-features --no-deps --frozen --quiet --jobs=1 --keep-going --color=never --message-format=json -- --cap-lints=allow
cargo-clippy clippy --manifest-path {workspace-indicator} --workspace --all-targets --all-features --no-deps --frozen --quiet --jobs=1 --keep-going --color=never --message-format=json -- -Dwarnings
```

<!-- markdownlint-enable MD013 -->

The adapter rejects every configured extra argument. It requires one canonical
`Cargo.toml`, a unique regular `Cargo.lock`, exactly one workspace package, no
custom build target, and a same-toolchain closure for Cargo, rustc, rustdoc,
`cargo-clippy`, and `clippy-driver`. Cargo runs from a private directory, so
workspace `.cargo/config*` files do not participate; inherited Cargo-home and
invocation-ancestor configuration are rejected. Compiler flags, wrappers,
Clippy configuration overrides, loader injection, compiler caches, and debug
inputs are cleared or replaced with exact values. The paired Cargo receives a
private target directory, offline/frozen mode, one job, no incremental build,
stable locale/color controls, and an explicit root Clippy configuration or an
empty private sentinel that prevents ancestor config discovery.

Cargo status 101 covers lint, compilation, configuration, and operational
failures. The adapter therefore requires version-one metadata, then performs a
coverage check whose lint levels are capped at `allow`. That check must finish
cleanly and emit exact selected-package artifacts; only dependency-information
rules targeting those artifacts may prove that every physical workspace Rust
source participated. This prevents an unlinted path dependency from satisfying
the source-coverage witness. The authoritative run then requires a terminal
`build-finished` JSON record, bounded version-locked Cargo summary lines, and
code-bearing primary diagnostics in a validated workspace `.rs` file. A
completed clean check maps to outer status zero; validated source diagnostics
map to one; configuration, dependency, incomplete output, source-coverage,
signal, launch, cleanup, and every other failure map to two.

Native `cargo clippy --fix` uses Cargo's local diagnostic server, which is
incompatible with the lane's active network denial. The remedy instead parses
the same read-only JSON, accepts only `MachineApplicable` byte replacements,
deduplicates them, rejects conflicting or overlapping spans, revalidates file
identity and content hashes, prepares all replacement files, then performs
atomic per-file renames with best-effort rollback. The ordinary final phase is
still the authoritative verification. Captured child output is drained
concurrently with a combined 16 MiB limit, and handled HUP/INT/TERM signals are
forwarded to the active process group before bounded termination and reap.

The four cases cover a clean package, a persistent non-machine-applicable
`clippy::ptr_arg` source issue, an invalid root `clippy.toml` that must map
native status 101 to operational failure, and a workspace repair. The repair
selects one dirty and one clean source while an unselected compiled module is
also dirty; exactly the selected dirty source and that unselected module must
change. A hostile workspace Cargo configuration attempts to cap lints and
force compiler/Clippy environment overrides, but the two validated repairs
remain visible. Immediate execution proves remedy then authoritative check and
a mutation-free second run. Deferred execution starts independently from
pristine bytes, proves initial issues, remedy, final clean verification, exact
workspace mutation attribution, and a verify-only fixed-state repeat.

This is deliberately narrower than general Cargo workspace support. Every
physical `.rs` file must compile in the single package under the joint
`--all-targets --all-features` configuration, so dormant, target-gated,
compile-test, and mutually exclusive-feature layouts can fail operationally.
Multi-package workspaces, workspace-local path dependencies, custom build
targets, and projects that depend on workspace Cargo configuration are
unsupported. Dependency and procedural macro code still executes inside the
controlled offline/network-denied lane;
the adapter is not a code sandbox. File, config, and executable preflights
cannot eliminate replacement races, and a filesystem failure can leave a
partially applied or incompletely rolled-back multi-file repair.

### Cargo Fmt validation contract

Cargo Fmt shares the dedicated Rust 1.97.1 closure above, including the same
official dated archive, archive SHA-256, signed-channel-manifest cross-check,
license material, and minimum host constraints. The `rustfmt-preview`
component adds the paired `cargo-fmt` and `rustfmt` executables. The complete
exact probe set used by this contract is:

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo 1.97.1 (c980f4866 2026-06-30)
rustfmt 1.9.0-stable (8bab26f4f6 2026-07-14)  # cargo-fmt --version
rustfmt 1.9.0-stable (8bab26f4f6 2026-07-14)  # rustfmt --version
```

The reviewed upstream entry point is
[`cargo-fmt/main.rs`](https://github.com/rust-lang/rust/blob/8bab26f4f68e0e26f0bb7960be334d5b520ea452/src/tools/rustfmt/src/cargo-fmt/main.rs)
at the Rust release commit; its Git blob object is
`9b4adc41a8b0fb62aaf09d733dc26448c3de7459`. That source establishes that
`cargo-fmt` performs its own Cargo metadata query before launching rustfmt, so
the trace contract binds those descendants as well as the adapter's direct
children.

Both phases use pinned Python 3.14.5 in isolated mode. The rendered outer
commands are:

<!-- markdownlint-disable MD013 -->

```text
python -I -c <adapter> cargo cargo-fmt rustfmt format {extra-args} __VELVET_GLOVE_CARGO_FMT_WORKSPACE__ {workspace-indicator}
python -I -c <adapter> cargo cargo-fmt rustfmt verify {extra-args} __VELVET_GLOVE_CARGO_FMT_WORKSPACE__ {workspace-indicator}
```

The workspace indicator is the root `Cargo.lock`; the adapter uses its sibling
`Cargo.toml`. A completed clean or source-result invocation records eight
managed child events in this order: root Cargo metadata, coverage-copy Cargo
metadata, coverage `cargo-fmt`, its internal Cargo metadata, coverage rustfmt,
real-workspace `cargo-fmt`, its internal Cargo metadata, and real-workspace
rustfmt. The adapter's direct commands are locked to:

```text
cargo metadata --format-version=1 --no-deps --manifest-path {root-or-coverage}/Cargo.toml --locked --offline --quiet
cargo-fmt fmt --all --manifest-path {coverage}/Cargo.toml --check -- --config-path {config-directory} --color never --files-with-diff
cargo-fmt fmt --all --manifest-path {root}/Cargo.toml [--check] -- --config-path {config-directory} --color never --files-with-diff
```

<!-- markdownlint-enable MD013 -->

Before the real formatting command, the adapter snapshots every validated file
plus the retained non-symlink directory topology and modes. Its private copy
reproduces that topology and those modes, then appends one parseable formatting
defect to every physical `.rs` file. The coverage check must exit one, write no
stderr, and report exactly that complete physical source set. Its private
metadata must describe the same packages and target roots as the real
workspace. This positive witness rejects an `autobins = false` or otherwise
dormant Rust source instead of accepting a clean no-op. The original workspace
must retain exact file snapshots and directory topology/modes throughout
metadata and coverage.

Every configured extra argument is rejected. Cargo, cargo-fmt, and rustfmt must
resolve through one launcher directory and their paired binaries must exist
beside the selected rustc. The adapter rejects ambient Cargo-home and
invocation-ancestor configuration, runs children from a private directory with
locked/offline metadata, a private target directory, one job, no incremental
build, stable locale and color, and clears Cargo, Rust, wrapper, compiler-cache,
debug, and dynamic-loader overrides. Exactly one root `rustfmt.toml` or
`.rustfmt.toml` is honored; when neither exists, an empty private configuration
prevents ancestor discovery.

Native check status one is formatting evidence only when stdout is a complete,
unique `--files-with-diff` list inside the validated physical source set and
stderr is empty. Every other incomplete, configuration, signal, launch,
cleanup, scope, or unexpected-status result maps to operational failure. A
format phase must exit zero and its reported files must equal the exact
workspace byte diff; added, removed, mode-changed (including a file whose bytes
also changed), touched-only, or non-Rust paths fail. Added or removed retained
directories and retained-directory mode changes fail as well. Once the
real-workspace child starts, every operational failure makes baseline file
content/mode/mtime and retained directory topology/modes eligible for
best-effort rollback with explicit failure reporting, including a signal or
output/read failure before the post-run scan. The ordinary final phase remains
authoritative, and the clean remedy repeat proves idempotence.

The five cases cover a clean package, one ordinary formatting issue, an invalid
root rustfmt configuration, an `autobins = false` dormant-source coverage
failure, and a two-member workspace. The workspace case selects one dirty and
one clean file in `alpha`; `cargo fmt --all` also repairs the unselected dirty
file in `beta`. Both exact changed paths are recorded, while findings remain
conservatively attributed to the selected candidates. Immediate and deferred
surfaces each run from pristine bytes and prove source/failure classification,
exact mutation, the authoritative postcheck, and clean fixed-state repetition.
The selected real-tool lane also executes the literal adapter in a focused
hostile lifecycle probe: HUP, INT, and TERM must reap an ignoring same-group
descendant; output beyond 16 MiB, source symlinks and hard links, combined
format-plus-mode and mtime-only changes, retained-directory add/remove/mode
changes, and an extra argument must fail closed. Unwritable private-root
initialization must normalize its diagnostic, and injected signals at both the
initialization and post-cleanup cutoffs must remain contained. Repeated TERM
across private-root cleanup must preserve exact file content/mode/mtime and
retained-directory topology/mode rollback; every path must remove its private
root; and a deterministic rollback failure must be reported. A normally exiting
formatter that leaves a closed-stdio same-process-group descendant is swept and
rejected before its delayed mutation can escape the final scan.

This contract deliberately supports only one self-contained locked workspace.
External path dependencies, non-member manifests, nested rustfmt
configuration, symlink directories or files, hard-linked files, missing locks,
and physical Rust files skipped beneath `.git`, `.velvet-glove`,
`node_modules`, or `target` are rejected or outside scope. Rustfmt establishes
formatting, not compilation. Cargo metadata and rustfmt still execute project
configuration inside the active network-denied lane; the adapter is not a code
sandbox. Every normally completed child is checked for remaining members of its
owned process group, but a child that deliberately escapes into a new session
or process group cannot be contained by the adapter. File, config, and
executable checks cannot eliminate concurrent path replacement. File and
directory inode identities and directory mtimes are not restored, and a
filesystem failure can defeat file content/mode/mtime or retained-directory
topology/mode rollback.

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
recipe registry. Shared Node, dedicated Prettier, dedicated Contextlint,
dedicated dclint, dedicated ESLint, Python, and Ruby package closures live
beside it under `node/`, `prettier/`, `contextlint/`, `dclint/`, `eslint/`,
`python/`, and `ruby/`; the Betterleaks and ghalint dependency closures and
patches live under `betterleaks/` and `ghalint-workflow/`. Runtime
components, auxiliary programs, bootstrap
commands, platform, architecture, minimum OS, and case-network policy are
schema-checked there as well. The current macOS 26 floor is dictated by the
official native Pkl 0.31.1 asset shared by the lane; the Rust, Ruby, and built
Betterleaks and ghalint artifacts themselves support earlier macOS releases. The Apple
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

These twenty smoke contracts establish the reproducible environment
substrate; they do not by themselves promote a tool's full pinned-real-tool
coverage tier.
The generated coverage report retains gaps until every required target, surface,
and semantic case has evidence; jq, Buf Format, Vacuum, Betterleaks, Astro,
Asciidoctor, Biome, Prettier, Contextlint, dclint, ESLint, ghalint Workflow,
gofmt, Cargo Clippy, and Cargo Fmt are covered only after each complete case
matrix passes. Linux,
Intel, and full-catalog scheduling remain separate follow-up work.

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
