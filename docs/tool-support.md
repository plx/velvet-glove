# Tool support status

Velvet Glove ships 150 builtin tool specs: declarative descriptions of how to
invoke a formatter or linter the user already has installed. "Validated"
here means the spec was probed against the real tool on macOS in September
2026, and its fixture cases — clean, issue, operational-failure, and (where
relevant) multi-file — pass on the deferred Stop surface and the immediate
Claude/Codex surfaces through the real-tool lane (see
`crates/velvet-glove/tests/tool-fixtures/README.md`). Nothing here is
pinned: users run whatever tool version is on their `PATH` or in their
project, and specs are written against each tool's stable documented
interface rather than one exact build. `velvet-glove tools`,
`velvet-glove init`, and `velvet-glove doctor` report which of these apply
to a given project and what's actually installed there. A weekly real-tool
CI lane re-runs the fixtures against fresh tool installs on `ubuntu-latest`
and `macos-latest` to catch upstream drift; see the fixtures README for how
it's wired and which tools it skips.

Status icons used below: ✅ validated, 🔧 fixed during validation (a real
bug was found and fixed), 🆕 new spec written and validated, ⚠️ deferred,
❔ unverified (not provisionable on CI), ⛔ disabled (not applicable to the
post-tool-use hook model).

## Summary

| Status | Count |
| --- | --- |
| ✅ Validated | 76 |
| 🔧 Fixed during validation | 51 |
| 🆕 New during validation | 16 |
| ⚠️ Deferred | 1 |
| ❔ Not provisionable | 2 |
| ⛔ Disabled, not applicable | 4 |
| **Total builtins** | **150** |

146 of the 150 are enabled; the 4 disabled ones are the commit-message and
branch-guard tools in [Secrets & generic hygiene](#secrets--generic-hygiene).

## Python

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `bandit` | security lint | 🆕 new & validated | 1.9.4 | if `.bandit` or `[tool.bandit]` | Missing or syntax-error file silently passes (exit 0). |
| `black` | format | ✅ validated | 26.5.1 | if `[tool.black]` in pyproject.toml | Malformed pyproject.toml exits 1, same code as "would reformat". |
| `flake8` | lint | ✅ validated | 7.4.1 | if `.flake8` or `[flake8]` section | Missing file reported as a style issue (E902), not operational. |
| `isort` | sort imports, autofix | ✅ validated | 9.0.1 | if `.isort.cfg` or `[tool.isort]` | |
| `mypy` | type check | ✅ validated | 2.3.1 | if `mypy.ini` or `[tool.mypy]` | Writes `.mypy_cache/`; needs the project's own interpreter for imports. |
| `pylint` | lint | ✅ validated | 4.0.9 | if `.pylintrc` or `[tool.pylint]` | |
| `pyright` | type check | 🆕 new & validated | 1.1.414 | if `pyrightconfig.json` or `[tool.pyright]` | First run of the npm wrapper downloads Node and the package. |
| `python-check-ast` (`pythonCheckAst`) | syntax check | 🔧 fixed & validated | Python 3.12.14 | opt-in only (linters already report syntax errors) | Stops at the first bad file in a batch. |
| `python-debug-statements` (`pythonDebugStatements`) | flag debug calls | ✅ validated | system grep | opt-in only (ruff's T100 is syntax-aware) | False positives on `breakpoint()` inside strings or docstrings. |
| `ruff` | lint + autofix | ✅ validated | 0.16.9 | default for python-lint | `--unfixable F401` still reports the import (documented upstream). |
| `ruff-format` (`ruffFormat`) | format | ✅ validated | 0.16.9 | opt-in only (ruff already formats) | Shares the ruff binary. |
| `ty` | type check | ✅ validated | 0.0.84 | if `ty.toml` or `[tool.ty]` | Third-party imports resolve only against the project's own environment. |

## Rust & TOML

Also covers a few adjacent dev-config tools (Just, mise, Pkl) that shared
this validation group.

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `cargo-check` (`cargoCheck`) | check only | ✅ validated | cargo 1.97.1 | opt-in only (cargo-clippy covers cargo check) | |
| `cargo-clippy` (`cargoClippy`) | lint + autofix | 🔧 fixed & validated | clippy 0.1.97 | if `**/Cargo.toml` | Workspace-scoped; a broken clippy.toml now blocks as a manual issue. |
| `cargo-deny` (`cargoDeny`) | dependency/license audit | 🆕 new & validated | cargo-deny 0.20.2 | if `**/deny.toml` | Fetches the RustSec advisory database over the network. |
| `cargo-fmt` (`cargoFmt`) | format | ✅ validated | rustfmt 1.9.0 | if `**/Cargo.toml` | |
| `just-format` (`justFormat`) | format Justfiles | 🔧 fixed & validated | just 1.45.0 | opt-in only (just's formatter is unstable) | |
| `mise` | format mise config | ✅ validated | mise 2026.9.1 | opt-in only (formats every mise config in the project) | Whole-project scope, not just the edited file. |
| `pkl` | evaluate a Pkl module | 🔧 fixed & validated | Pkl 0.31.1 | opt-in only (evaluates each module) | Evaluation can read files or fetch remote packages. |
| `pkl-format` (`pklFormat`) | format | ✅ validated | Pkl 0.31.1 | if `PklProject` | |
| `rustfmt` | format | 🔧 fixed & validated | rustfmt 1.9.0 | opt-in only (cargo-fmt knows the crate edition) | CLI `--edition` can override rustfmt.toml's edition and mis-style code. |
| `taplo` | TOML lint | 🔧 fixed & validated | taplo 0.10.0 | if `.taplo.toml`/`taplo.toml` | |
| `taplo-format` (`taploFormat`) | format | ✅ validated | taplo 0.10.0 | if `.taplo.toml`/`taplo.toml` | |
| `tombi` | TOML lint | ✅ validated | tombi 1.5.5 | if `tombi.toml`/`.tombi.toml` | |
| `tombi-format` (`tombiFormat`) | format | ✅ validated | tombi 1.5.5 | if `tombi.toml`/`.tombi.toml` | |

## Go

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `errcheck` (`errCheck`) | lint (unchecked errors) | ✅ validated | 1.20.0 | opt-in only (golangci-lint or go vet is the usual choice) | Workspace-scoped `./...` scan. |
| `go-fmt` (`goFmt`) | format | ✅ validated | go1.27.1 | default for go-format | |
| `go-vet` (`goVet`) | lint | ✅ validated | go1.27.1 | default for go-lint | No operational-failure case possible; the tool can't distinguish one. |
| `gofumpt` (`goFumpt`) | format (stricter) | ✅ validated | v0.12.0 | opt-in only (stricter than gofmt) | |
| `goimports` (`goImports`) | format + fix imports | ✅ validated | x/tools 0.50.0 | opt-in only (removes unused imports mid-edit) | Disruptive: removes unused imports as a side effect. |
| `golangci-lint` (`golangciLint`) | lint + autofix | ✅ validated | 2.14.0 | if `.golangci.yml`/`.yaml`/`.toml`/`.json` | Workspace-scoped `./...` scan. |
| `golangci-lint-fmt` (`golangciLintFmt`) | format (fmt subcommand) | 🆕 new & validated | 2.14.0 (fmt) | opt-in only (needs golangci-lint v2+) | Falls back to gofmt when no formatters are configured. |
| `golines` (`goLines`) | format (rewrap long lines) | ✅ validated | v0.13.0 | opt-in only (rewraps long lines) | Upstream project is archived, but the last release still works. |
| `gomod-tidy` (`gomodTidy`) | tidy go.mod/go.sum | ✅ validated | go1.27.1 | opt-in only (rewrites go.mod/go.sum, may use the network) | Rewrites go.mod/go.sum and may use the network. |
| `gosec` (`goSec`) | security lint | 🔧 fixed & validated | dev build | opt-in only (noisy security findings) | A build failure shares the same exit code as a finding. |
| `govulncheck` (`goVulnCheck`) | vulnerability scan | ✅ validated | v1.8.0 | opt-in only (queries the vulnerability database over the network) | Needs network access to vuln.go.dev. |
| `revive` | lint | ✅ validated | v1.17.0 | if `revive.toml` | No operational-failure case possible. |
| `staticcheck` | lint | ✅ validated | 2026.2.1 | if `staticcheck.conf` | No operational-failure case possible. |

## JavaScript & TypeScript

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `astro` | type check `.astro` files | ⚠️ deferred | 7.3.5 / @astrojs/check 0.9.10 | if `astro.config.*` | Not fixture-tested: needs a full Astro project plus ~270 packages. |
| `biome` | lint/format (opinionated) | 🔧 fixed & validated | 2.5.14 | if `biome.json` or `"@biomejs/biome"` in package.json | Config errors share an exit code with lint findings. |
| `deno` | format | 🔧 fixed & validated | deno 2.9.7 | if `deno.json`/`deno.jsonc` | Formatting issues, syntax errors and a broken deno.json all share exit 1. |
| `deno-check` (`denoCheck`) | type check | 🔧 fixed & validated | deno 2.9.7 | if `deno.json`/`deno.jsonc` | Runs per file for attribution; diagnostics name files as `file://` URLs. |
| `dprint` | format | ✅ validated | 0.57.4 | if `dprint.json` | Fetches formatting plugins by URL on first use. |
| `eslint` | lint + autofix | 🔧 fixed & validated | 10.11.0 | if `eslint.config.*`/`.eslintrc*` or `"eslint"` in package.json | Resolves project-local `node_modules/.bin/eslint` before any global install. |
| `knip` | find unused exports/deps, autofix | 🔧 fixed & validated | 6.38.0 | if knip config or `"knip"` in package.json | `--fix` deletes exports and dependencies. |
| `knip-strict` (`knipStrict`) | stricter dead-code check | 🔧 fixed & validated | 6.38.0 (--strict) | opt-in only (stricter knip) | Same disruptive `--fix` as knip. |
| `oxfmt` | format | ✅ validated | 0.70.0 | if `.oxfmtrc.json` or `"oxfmt"` in package.json | |
| `oxlint` | lint | ✅ validated | 1.84.0 | if `.oxlintrc.json`/`oxlint.config.ts` or `"oxlint"` in package.json | `--fix-suggestions` may change behavior, so it's excluded from quiet autofix. |
| `prettier` | format | 🔧 fixed & validated | 3.9.9 | if `.prettierrc*` or `"prettier"` in package.json | Invalid `.prettierrc` option value fails the format phase (exit 1). |
| `sort-package-json` (`sortPackageJson`) | sort package.json keys | ✅ validated | 4.0.0 | if `"sort-package-json"` in package.json | |
| `standard-js` (`standardJs`) | lint (opinionated, no config) | ✅ validated | 17.1.2 | if `"standard"` in package.json | |
| `stylelint` | lint CSS | ✅ validated | 17.15.0 | if `.stylelintrc*` or `"stylelint"` in package.json | Missing/unresolvable config also exits 78 (operational). |
| `tsc` | type check | ✅ validated | 7.0.2 | if `tsconfig.json` | Malformed/missing tsconfig reports as a diagnostic, not operational. |
| `tsserver` | type check (via tsc-files) | ✅ validated | tsc-files 1.1.4 + ts 7.0.2 | if `"tsc-files"` in package.json | Silently reports clean under pnpm/isolated node_modules; needs a hoisted npm install. |
| `vp-check` (`vpCheck`) | check (vite-plus) | 🔧 fixed & validated | vite-plus 0.3.3 | if `"vite-plus"` in package.json | |
| `vp-fmt` (`vpFmt`) | format (vite-plus) | 🔧 fixed & validated | vite-plus 0.3.3 | opt-in only (vp-check already formats) | |
| `vp-lint` (`vpLint`) | lint (vite-plus) | 🔧 fixed & validated | vite-plus 0.3.3 | opt-in only (vp-check already lints) | |
| `xo` | lint (opinionated) | ✅ validated | 5.0.1 | if `"xo"` in package.json | |

## Shell, YAML & JSON

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `jq` | query/check JSON | ✅ validated | jq-1.8.2 | opt-in only (rejects JSON with comments) | Rejects JSON-with-comments files (tsconfig.json, .vscode). |
| `ryl` | YAML lint | 🔧 fixed & validated | 0.22.0 | if `ryl.toml`/`.ryl.toml` | Falls back to a legacy `.yamllint` file, which `init` detection can't see. |
| `shellcheck` | lint | ✅ validated | 0.11.0 | default for shell-lint | Glob-only selection (`*.sh`/`*.bash`); extensionless scripts aren't linted. |
| `shellharden` | format (harden quoting) | 🆕 new & validated | 4.3.2 | opt-in only (rewriting quoting can change script behavior) | Can change behavior for scripts that rely on word-splitting. |
| `shfmt` | format | ✅ validated | v3.14.1 | opt-in only (imposes a shell layout) | |
| `yamlfmt` | format | ✅ validated | 0.21.0 | if `.yamlfmt*`/`yamlfmt.y*ml` | |
| `yamllint` | lint | ✅ validated | 1.38.0 | if `.yamllint*` | |
| `yq` | rewrite YAML | 🔧 fixed & validated | v4.53.6 | opt-in only (rewrites YAML layout) | |

## Markdown & prose

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `asciidoctor` | render/check AsciiDoc | ✅ validated | 2.0.26 | opt-in only (no zero-config checker or project indicator) | No distinct operational exit code; failures surface as manual issues. |
| `codespell` | fix spelling | 🆕 new & validated | 2.4.3 | if `.codespellrc` or `[tool.codespell]` | Its fix can rename a misspelled identifier in the file. |
| `contextlint` | lint (docs context rules) | 🔧 fixed & validated | 1.1.1 | if `contextlint.config.json` | |
| `harper` | prose/grammar lint | ✅ validated | 2.11.0 | opt-in only (zero-config, no project indicator) | A missing input path is linted as literal text, not reported as a failure. |
| `lychee` | check links | ✅ validated | 0.24.2 | if `lychee.toml`/`.lycheeignore` | Checks links over the network. |
| `markdownlint` (`markdownLint`) | lint Markdown | ✅ validated | markdownlint-cli 0.49.1 | if `.markdownlint.json`/`.yaml`/`rc` | |
| `markdownlint-cli2` (`markdownlintCli2`) | lint Markdown | 🆕 new & validated | 0.23.3 | if `.markdownlint-cli2.*` | Shares the markdown-lint role with markdownlint/rumdl; no default. |
| `mdschema` | validate Markdown front matter | 🔧 fixed & validated | 0.15.4 | if `.mdschema.yml` | Name collides with macOS's own `/usr/bin/mdschema` and an npm package; PATH order matters. |
| `rumdl` | lint Markdown | ✅ validated | 0.2.77 | if `.rumdl.toml` or `[tool.rumdl]` | Searches parent directories for `.rumdl.toml`, which may live outside the workspace. |
| `typos` | fix typos | ✅ validated | 1.50.3 | if `typos.toml`/`_typos.toml`/`.typos.toml` | Its fix can rename identifiers in the file. |
| `vale` | prose lint | ✅ validated | 3.23.0 | if `.vale.ini`/`_vale.ini` | Report-only; has no autofix. |

## GitHub Actions & containers

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `actionlint` | lint GH Actions workflows | 🔧 fixed & validated | 1.7.12 | if `.github/workflows/*.y*ml` | |
| `dclint` | lint Docker Compose files | ✅ validated | 3.1.0 | if `.dclintrc*` | No distinct operational-failure exit code. |
| `ghalint-action` (`ghalintAction`) | lint composite actions | 🔧 fixed & validated | 1.5.6 | if `ghalint.y*ml`/`.ghalint.y*ml` | Bad YAML and other operational errors share the same exit code as findings. |
| `ghalint-workflow` (`ghalintWorkflow`) | lint GH Actions workflows | 🔧 fixed & validated | 1.5.6 | if `ghalint.y*ml`/`.ghalint.y*ml` | Always runs from the project root; a nested workflow file isn't itself linted. |
| `hadolint` | lint Dockerfiles | 🔧 fixed & validated | 2.15.1 | default for docker-lint | Missing `--config` shares the same exit code as a finding. |
| `pinact` | check action pins | ✅ validated | 5.0.0 | if `.pinact.y*ml`/`.github/pinact.y*ml` | Verifies pins over the network. |
| `pinact-update` (`pinactUpdate`) | bump action versions | ✅ validated | 5.0.0 | opt-in only (bumps action versions) | Rewrites action version pins. |
| `zizmor` | security audit GH Actions | 🔧 fixed & validated | 1.30.1 | if `zizmor.y*ml`/`.github/zizmor.y*ml` | `--fix` only applies audits zizmor marks safe; most findings need a manual fix. |

## Infrastructure, Protobuf & Bazel

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `buf-format` (`bufFormat`) | format Protobuf | ✅ validated | buf 1.73.0 | if `buf.yaml`/`buf.work.yaml` | Some syntax errors leave the file unchanged and unreported (buf-lint catches them). |
| `buf-lint` (`bufLint`) | lint Protobuf | ✅ validated | buf 1.73.0 | if `buf.yaml`/`buf.work.yaml` | Proto parse errors share the same exit code as lint violations. |
| `buildifier-format` (`buildifierFormat`) | format Bazel files | ✅ validated | buildifier 10.1.0 | if `MODULE.bazel`/`WORKSPACE*` | |
| `buildifier-lint` (`buildifierLint`) | lint Bazel files | ✅ validated | buildifier 10.1.0 | if `MODULE.bazel`/`WORKSPACE*` | |
| `hclfmt` | format HCL (via terragrunt) | 🔧 fixed & validated | terragrunt v1.1.6 | if `terragrunt.hcl` | Reformats every `.hcl` file under the project root, not just the edited one. |
| `terraform` | format | ✅ validated | v1.16.4 | default for terraform-format | |
| `tflint` (`tfLint`) | lint + autofix Terraform | 🔧 fixed & validated | 0.64.0 | if `.tflint.hcl` | Lints/fixes every module directory under the project root, not just the edited one. |
| `tofu` | format (OpenTofu) | ✅ validated | v1.12.6 | opt-in only (terraform is the role default) | Same fmt contract as terraform. |
| `vacuum` | lint OpenAPI specs | 🔧 fixed & validated | 0.30.6 | opt-in only (OpenAPI files aren't recognized by name) | Runs one process per file. |

## Ruby

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `brakeman` | security lint (Rails) | 🔧 fixed & validated | 8.0.6 | if `config/brakeman.yml`/`.ignore` | |
| `bundle-audit` (`bundleAudit`) | dependency vuln audit | 🔧 fixed & validated | 0.9.3 | if `.bundler-audit.yml` | Clones ruby-advisory-db over the network on first run. |
| `erb` (executable `erb_lint`) | lint ERB templates | ✅ validated | erb_lint 0.9.0 | if `.erb-lint.yml`/`.erb_lint.yml` | Invalid config shares the same exit code as issues found. |
| `fasterer` | perf lint | 🔧 fixed & validated | 0.11.0 | if `.fasterer.yml` | Bad `.fasterer.yml` shares the same exit code as offenses found. |
| `reek` | code-smell lint | ✅ validated | 6.5.0 | if `.reek.yml` | A Ruby syntax error in a scanned file looks clean (exit 0). |
| `rubocop` | lint + autofix | ✅ validated | 1.91.0 | if `.rubocop.yml` or "rubocop" in Gemfile | |
| `sorbet` (executable `srb`) | type check | 🔧 fixed & validated | 0.6.13508 | if `sorbet/config` | Only runs for files under a `sorbet/config` ancestor. |
| `standard-rb` (`standardRb`) | lint (opinionated, no config) | ✅ validated | standard 1.56.0 | if `.standard.yml` or "standard" in Gemfile | Invalid `.standard.yml` shares the same exit code as issues found. |

## Swift, C/C++ & JVM

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `clang-format` (`clangFormat`) | format C/C++ | 🔧 fixed & validated | 23.1.2 | if `.clang-format`/`_clang-format` | No operational-failure case possible; shares an exit code with format issues. |
| `cmake-format` (`cmakeFormat`) | format CMake files | 🔧 fixed & validated | 0.6.13 | if `.cmake-format.yaml`/`.json`/`.py` | Needs the `[YAML]` install extra, or a YAML config crashes it. |
| `cpplint` (`cppLint`) | lint C/C++ | ✅ validated | 2.0.2 | if `CPPLINT.cfg` | Doesn't attribute issues per file; a batch marks every cited file manual. |
| `google-java-format` (`googleJavaFormat`) | format Java | 🔧 fixed & validated | 1.36.1 | opt-in only (imposes Google style) | Imposes Google's style with no config option. |
| `ktfmt` | format Kotlin (opinionated) | 🆕 new & validated | 0.59 | opt-in only (imposes Meta style, no config file) | Imposes Meta's style with no config file to detect. |
| `ktlint` | lint Kotlin | ✅ validated | 1.8.0 | if `ktlint` in `.editorconfig` | |
| `swift-format` (`appleSwiftFormat`) | format Swift | 🆕 new & validated | Swift 6.4 toolchain | default for swift-format (unless `.swiftformat` present) | macOS-only in CI (needs Xcode/Swift 6); applies Apple's 2-space style with no config. |
| `swiftformat` | format Swift | 🆕 new & validated | 0.63.0 | if `.swiftformat` (preferred over swift-format then) | Not verified on ubuntu-latest CI. |
| `swiftlint` | lint Swift | 🔧 fixed & validated | 0.65.1 | if `.swiftlint.yml`/`.yaml` | macOS-only in practice; no official Linux release. |

## Other languages (Lua, PHP, SQL, XML, Elixir, Zig, Dart, Gleam, CUE, Haskell)

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `cue-fmt` (`cueFmt`) | format CUE | 🆕 new & validated | v0.17.1 | default for cue-format | |
| `dart-format` (`dartFormat`) | format Dart | 🆕 new & validated | Dart SDK 3.13.4 | default for dart-format | A batch with one unparseable file still formats the rest, but exits 65. |
| `gleam-format` (`gleamFormat`) | format Gleam | 🆕 new & validated | 1.18.1 | default for gleam-format | Runs per-file: one unparseable file in a batch formats nothing. |
| `hlint` | lint Haskell | 🆕 new & validated | 3.10 | if `.hlint.yaml` | Runs under Rosetta 2 on macOS (no native arm64 binary). |
| `luacheck` | lint Lua | ✅ validated | 1.2.0 | if `.luacheckrc` | Linux-only in CI; upstream publishes no macOS release asset. |
| `mix-compile` (`mixCompile`) | compile Elixir project | ✅ validated | Elixir 1.20.4 | opt-in only (compiles the whole project) | Whole-project scope, not just the edited file. |
| `mix-fmt` (`mixFmt`) | format Elixir | ✅ validated | Elixir 1.20.4 | if `.formatter.exs`/`mix.exs` | |
| `mix-test` (`mixTest`) | run Elixir test suite | ✅ validated | Elixir 1.20.4 | opt-in only (runs the test suite) | Blames the test file for a failure, not the lib code that broke it. |
| `ormolu` | format Haskell | 🆕 new & validated | 0.8.1.1 | opt-in only (no config file, not the only Haskell formatter) | |
| `php-cs` (`phpCs`, executable `phpcs`) | lint + autofix PHP | 🔧 fixed & validated | PHP_CodeSniffer 4.0.4 | if `phpcs.xml*`/`.phpcs.xml*` | Not in mise's registry; needs a separate PHP runtime plus phpcs.phar/phpcbf.phar on PATH. |
| `selene` | lint Lua | ✅ validated | 0.31.0 | if `selene.toml` | No operational-failure case possible; shares an exit code with findings. |
| `sql-fluff` (`sqlFluff`) | lint SQL | ✅ validated | 4.3.0 | if `.sqlfluff` or `[tool.sqlfluff]` | Assumes no user-level `~/.sqlfluff` overrides the dialect. |
| `stylua` | format Lua | ✅ validated | 2.5.2 | if `stylua.toml`/`.stylua.toml` | A Lua parse error is classified as operational, not a source issue. |
| `xmllint` | lint/format XML | ✅ validated | libxml2 20913 | default for xml-lint | Needs `apt-get install libxml2-utils` on Linux; ships with macOS. |
| `zig-fmt` (`zigFmt`) | format Zig | 🆕 new & validated | 0.16.0 | default for zig-format | Runs per-file: a batch still formats the good files but exits 1 for all. |

`ocamlformat` is **not in the catalog** — see
[Not supported or deferred](#not-supported-or-deferred).

## Nix

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `alejandra` | format Nix | ✅ validated | Alejandra 4.0.0 | if `alejandra.toml` | Linux-only in CI; upstream ships only Linux musl binaries. |
| `deadnix` | find/remove dead Nix code | ✅ validated | deadnix 1.3.1 | opt-in only (its fix deletes unused bindings) | A parse error exits 0 under `--fail` (a tool limitation). |
| `nixfmt` (`nixFmt`) | format Nix | ✅ validated | 1.4.0 | default for nix-format | |
| `nixpkgs-fmt` (`nixpkgsFormat`) | format Nix (alternate) | ✅ validated | 1.3.0 | opt-in only | Error-tolerant parser: garbage input exits 0 and is left untouched. |
| `nil` | Nix LSP-style checks | ❔ unverified (not provisionable) | not tested | opt-in only (draft spec for an LSP-first tool) | Distributed only as a Nix flake; no CI-installable binary. |
| `nixf-diagnose` (`nixfDiagnose`) | diagnose Nix syntax | ❔ unverified (not provisionable) | not tested | opt-in only (draft spec) | Nix-only distribution; no CI-installable binary. |

## Secrets & generic hygiene

| Tool | What it does | Status | Tested with | Picked by `init` | Notes |
| --- | --- | --- | --- | --- | --- |
| `betterleaks` | secret scan | 🔧 fixed & validated | 1.8.1 | if `.betterleaks.toml` | |
| `check-merge-conflict` (`checkMergeConflict`) | flag merge-conflict markers | 🔧 fixed & validated | system grep | opt-in only | Misses a conflict made only of `=======` (git never writes one). |
| `detect-private-key` (`detectPrivateKey`) | flag private key material | 🔧 fixed & validated | system grep | opt-in only | |
| `editorconfig-checker` (`editorconfigChecker`) | check `.editorconfig` compliance | ✅ validated | 4.0.1 | if `.editorconfig-checker.json`/`.ecrc` | mise/aqua installs only an `ec` binary, not the `editorconfig-checker` this spec expects. |
| `gitleaks` | secret scan | 🔧 fixed & validated | 8.30.1 | if `.gitleaks.toml` | One process per edited file. |
| `byte-order-marker` (`byteOrderMarker`) | strip BOM, autofix | 🔧 fixed & validated | hk 1.58.1 | opt-in only | Not yet in the weekly CI lane (hk provisioning gap). |
| `check-added-large-files` (`checkAddedLargeFiles`) | flag large edited files | 🔧 fixed & validated | hk 1.58.1 | opt-in only | Checks current size of files edited this turn, not git's staged set. |
| `check-case-conflict` (`checkCaseConflict`) | flag case-colliding filenames | 🔧 fixed & validated | hk 1.58.1 | opt-in only | Only compares files edited together in the same turn. |
| `check-conventional-commit` (`checkConventionalCommit`) | check commit message format | ⛔ disabled | n/a | n/a | Commit-msg hook; doesn't fit the post-tool-use model. |
| `check-executables-have-shebangs` (`checkExecutablesHaveShebangs`) | flag missing shebangs | 🔧 fixed & validated | hk 1.58.1 | opt-in only | A missing path shares the same exit code as a real issue. |
| `check-symlinks` (`checkSymlinks`) | flag broken symlinks | 🔧 fixed & validated | hk 1.58.1 | opt-in only | Not yet in the weekly CI lane (hk provisioning gap). |
| `cocogitto-commit-msg` (`cocogittoCommitMsg`, executable `cog`) | check commit message format | ⛔ disabled | n/a | n/a | Commit-msg hook (`cog verify --file`); doesn't fit the post-tool-use model. |
| `fix-smart-quotes` (`fixSmartQuotes`) | flag smart quotes | 🔧 fixed & validated | hk 1.58.1 | opt-in only | No autofix: hk's fix mode can destroy data (reported upstream). |
| `mixed-line-ending` (`mixedLineEnding`) | normalize line endings, autofix | 🔧 fixed & validated | hk 1.58.1 | opt-in only | Only flags files that mix CRLF and LF. |
| `newlines` | fix end-of-file newlines, autofix | 🔧 fixed & validated | hk 1.58.1 | opt-in only | |
| `no-commit-to-branch` (`noCommitToBranch`, executable `git`) | block commits to a branch | ⛔ disabled | n/a | n/a | Pre-commit branch guard; not linked to edited files. |
| `trailing-whitespace` (`trailingWhitespace`) | strip trailing whitespace, autofix | 🔧 fixed & validated | hk 1.58.1 | opt-in only | Strips Markdown hard line breaks too (hk has no Markdown exemption). |
| `harper-commit-message` (`harperCommitMessage`, executable `harper-cli`) | lint commit message prose | ⛔ disabled | n/a | n/a | Commit-msg lifecycle tool; agent file edits never reach COMMIT_EDITMSG. |

## Not supported or deferred

- **`nil`** — not provisionable: distributed only as a Nix flake, no
  crates.io/npm/release binary to install on CI.
- **`nixf-diagnose`** — not provisionable: Nix-only distribution, same
  reason as `nil`.
- **`ocamlformat`** — not in the catalog at all. Upstream ships only source
  tarballs and there's no mise plugin, so installing it means building an
  opam switch and the OCaml toolchain from source. It can follow the
  ormolu/gleam-format archetype once a prebuilt binary exists.
- **`astro`** (`astro check`) — deferred, not fixture-tested: needs
  `@astrojs/check` and `typescript` installed in a project-local
  `node_modules` (~270 packages) plus a real Astro project layout, which
  can't be provisioned as a small fixture. The rendered command was
  confirmed manually against a scratch Astro project.
- **`check-conventional-commit`**, **`cocogitto-commit-msg`**,
  **`no-commit-to-branch`**, **`harper-commit-message`** — disabled. All
  four are commit-message or branch-guard lifecycle hooks; none of them has
  a meaningful relationship to a post-tool-use file-edit event, so they're
  kept in the catalog but switched off rather than force-fit into the hook
  model.

Platform-limited on the weekly real-tool CI lane (still fully supported
where the platform provides them):

- **`swiftlint`** and **`swift-format`** — macOS-only in CI. `swiftlint` has
  no official Linux release; `swift-format` needs the Apple/Xcode or
  swift.org Swift 6 toolchain, which isn't available via mise on
  `ubuntu-latest`.
- **`alejandra`** and **`luacheck`** — Linux-only in CI. `alejandra` ships
  only Linux musl release binaries; `luacheck` has no macOS release asset
  upstream.
- **`xmllint`** — ships with macOS, but needs
  `apt-get install libxml2-utils` on Linux, which is outside mise's scope.

## Adding or fixing a tool

Drop a new spec in `crates/hookkit-pkl-config/src/builtins/tools/`, add
fixture cases under
`crates/velvet-glove/tests/tool-fixtures/<tool-id>/`, and match one of the
five archetypes in `docs/validation-architecture.md` (batch formatter,
workspace linter with autofix, plain checker, per-file checker,
stdout-diff formatter). The fixture case vocabulary, directory layout, and
lane semantics are in
`crates/velvet-glove/tests/tool-fixtures/README.md`. Budgets in
`docs/validation-architecture.md` cap spec size and fixture scope, and are
enforced by the guardrail tests — don't edit those to make a change fit;
open an issue instead.
