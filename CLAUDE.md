# ciq Project Instructions

> **ciq** (CSV Interactive Query): type DuckDB SQL, watch an aligned grid update live, against an in-memory columnar table parsed once at startup. `jiq` for CSV — its inspiration, not its law.

## Read first — the canonical decisions live in `dev/`

Before changing anything, read **`dev/PLAN.md` §0** (the single source of truth for the five settled architecture decisions D1–D5) and the supporting docs:

- **`dev/PLAN.md`** — full spec. §0 overrides the body where they ever disagree.
- **`dev/DECISIONS.md`** — ADR log: what was open, what was decided, why, what it supersedes.
- **`dev/ASSUMPTIONS.md`** — unverified assumptions + how/when each is validated (A1 is closed).
- **`dev/TASKS.md`** — the dependency-ordered build spine. Resume by finding the next `TODO` whose deps are all `DONE`. Update task status as you go.

**Convention — *cite, don't re-declare*.** A decided fact lives in exactly one place (§0 or its `DECISIONS.md` entry); every other mention links to it rather than restating it. The plan's original "every section declares itself canonical" habit is what produced its contradictions — do not reintroduce it.

**Principle — jiq is inspiration, not law.** ciq's domain (tabular CSV, in-process DuckDB, SQL) differs fundamentally from jiq's (JSON, external `jq`, jq paths). Justify every reuse on ciq's own merits. jiq file/line citations in the docs are *illustrative* — grep the live jiq source to confirm (several are already stale; see `ASSUMPTIONS.md` A4).

## Build

- **Running the compiled binary needs nothing.** DuckDB is embedded via the `duckdb` crate's `bundled` feature, which statically compiles DuckDB's C++ *into* the `ciq` binary. The shipped binary is one self-contained file — no external `jq`-style binary (unlike jiq), no DuckDB install, no shared libraries. Downloaded-release / Homebrew users install nothing.
- **Building from source needs a C++ compiler — at build time only.** `bundled` uses the `cc` crate (not cmake) to compile DuckDB's amalgamation with the system `g++`/`clang++`. Standard Rust dev environments and CI images already have one (anyone who's built a Rust crate with a C dependency has it); nothing was installed to build ciq here. This is a build-time dependency, never a runtime one — so it affects us, CI, and `cargo install ciq` users, but not people running a released binary.
- Consequence: the **first build (and first build on a fresh CI runner) is slow (~90s–2min)** because it compiles DuckDB. Incremental rebuilds are fast. Don't mistake a slow first build for a hang; budget CI timeouts accordingly.
- The `duckdb` crate version is **pinned exactly** (`=1.10503.1`) in `Cargo.toml` — a minor bump can change SQL/sniffer/interrupt behavior. Upgrades are deliberate and must be re-gated by the suite (especially the A1 interrupt regression guard and any type-sniffer goldens).

## Testing

- Run the full suite: **`cargo test --all-features -- --test-threads=1`**.
- **Never** a bare `cargo test`, and **never** `--lib` — `--lib` skips the `tests/` integration + headless-session layer where engine semantics and harness sessions live; an agent that runs `--lib` gets a false green.
- `--test-threads=1` is load-bearing: the worker/channel and `TestBackend` tests touch process-global state and would race otherwise.
- Long-running builds/tests: run in background mode.

## Manual-test CSVs (gitignored — regenerate on demand)

The files the user drives the real TUI against live at the repo root and are **gitignored** (the `/*.csv` entry), so a fresh clone won't have them. When the user wants to manually test and they're missing, regenerate them with the deterministic generator (no deps, no randomness — same bytes every run):

- `python3 dev/gen_showcase.py` — writes both: **`showcase-xl.csv`** (1,000,000 rows × 100 cols, ~800 MB, ~2 min) and **`showcase-m.csv`** (100,000 rows × 20 cols, ~22 MB, seconds).
- `python3 dev/gen_showcase.py m` (or `xl`) — just that one tier. Prefer `m` unless the test is specifically about scale.
- Launch: `./target/release/ciq showcase-m.csv`.

Both carry the same 14 edge-case columns (NULL cadences, RFC-4180 quoting, CJK/emoji, quoted identifiers like `"Total ($)"` / `"order"`) plus typed filler columns. **Never commit them.** The tracked 5,000×14 test fixture is separate — `python3 dev/gen_showcase.py --fixture` regenerates `tests/fixtures/showcase.csv` byte-identically; don't confuse the two.

## Determinism rules (MUST — a flaky test gives a false fix signal)

- **No ambient time/rand in library logic.** No `Instant::now()`, `SystemTime::now()`, `rand::*` outside the named seam wrappers (the debouncer's `system_time_ms`, any explicit-seed sampler). Time enters logic as a `u64` parameter (jiq's time-as-parameter debouncer; there is **no `Clock` trait**). Enforced by `clippy.toml` `disallowed-methods`, which rides the clippy gate.
- **Stable ordering** for anything user-visible (column order, distinct values, suggestions); `SELECT DISTINCT` carries an explicit `ORDER BY`.
- **Fixed fixtures, no network, no `$HOME`.** Engine tests read tempfile CSVs; config tests read in-memory strings; clipboard/OSC are faked.
- Execution-time fields are **redacted from snapshots** so timing never flips a golden.

## Pre-Commit Requirements

Execute in order; all must pass before staging:

1. Strip implementation-detail comments; keep only comments that explain non-obvious *why*.
2. New logic ships **with its tests** (pure-core modules: ~100% line+branch — they can't be coverage-padded).
3. `cargo fmt --all --check` (zero diffs).
4. `cargo clippy --all-targets --all-features -- -D warnings` (zero — includes the determinism `disallowed-methods` gate).
5. `cargo build --release` (zero warnings).
6. `cargo build` (debug; zero warnings).
7. `cargo test --all-features -- --test-threads=1` (full suite, never `--lib`).
8. **TUI validation** — for any change touching the real-terminal shell (the §4.7 surface: glyph/color rendering & polarity, raw-mode keyboard/mouse, bracketed-paste framing, clipboard/OSC52, resize reflow, perceived feel): hand the user explicit test steps, **STOP, and wait** for them to drive it. Everything else is headless and needs no human.

After all green:
- **Sync with remote first:** `git fetch origin`; if you have local commits on `main`, `git rebase origin/main` (resolve conflicts locally, not in the PR); else `git pull --ff-only origin main`.
- Stage specific files by name (never `git add -A` / `git add .`).
- Commit with a **single-line** lowercase Conventional Commit message (no body, no issue refs).
- **Do not push, tag, or release on your own.** Pushing/tagging is deferred until the user explicitly asks to release — then invoke the **`ciq-release`** skill (see Versioning & Releasing).

## CI gates (4 jobs — see `dev/PLAN.md` §0/D5)

`test` (`cargo test … --test-threads=1`) · `coverage` (`cargo tarpaulin`) · `lint` (`fmt --check` + `clippy -D warnings`, the latter carrying `disallowed-methods`). **No** separate build job, **no** binary gate, **no** "7th gate."

**Coverage is tiered (D5):**
- **HARD floor (blocks build):** branch coverage of the pure-core module allowlist (SQL-context, ranking, grid-layout math, schema inference, scroll/search). Pure functions can't be padded, so a hard floor here is free of the gaming failure mode.
- **WARN only:** project-wide **95%** target — annotates below 95%, never fails the build.
- **HARD:** the shell-marker containment check — a `// ciq:shell-exempt` marker on any file *not* in the §4.7 list fails CI (the human-surface set cannot silently grow).

## Versioning & Releasing

**Pre-1.0 versioning policy.** ciq stays on `0.x` until it's feature-complete and stable. Only two bump kinds apply for now:

- **Minor** `0.X.0` — new features and breaking changes alike (pre-1.0, minor is the breaking lever).
- **Patch** `0.minor.Y` — bug fixes, refactors, polish, docs.

**No `1.0.0` / major bumps** until the user explicitly decides ciq is 1.0-ready.

**Releasing — invoke the `ciq-release` skill.** When the user asks to release / ship / publish / tag, invoke the **`ciq-release`** skill (`.claude/skills/ciq-release/SKILL.md`). Pass `patch` / `minor` if specified; otherwise it infers. Never reinvent the flow inline.

The flow is **cargo-dist, shell installer only** (curl-based, config in `dist-workspace.toml`, workflow in `.github/workflows/release.yml`): bump `version` in `Cargo.toml`, update `CHANGELOG.md`, commit `release vX.Y.Z`, push `main`, then push a `vX.Y.Z` tag — the **tag** (not the branch push) triggers the `Release` workflow, which builds every target and attaches the tarballs + `ciq-installer.sh` to a GitHub Release. Deliberately **no crates.io publish and no Homebrew tap** (unlike jiq). Because DuckDB is bundled per-target, the less-common legs (musl, Windows) can fail where macOS/Linux-gnu pass; cargo-dist blocks publishing until all legs are green, so a broken target means dropping it from `dist-workspace.toml` (re-run `dist generate`) and cutting a new patch tag rather than deleting the pushed tag.

## Documentation Site

`docs/` is **reserved** for the published GitHub Pages site (Jekyll + just-the-docs), a **Phase 5** deliverable — do not put working notes there (those go in `dev/`). README is one-liner intent only, pointing to `dev/PLAN.md`.

When `docs/` exists, a user-visible feature/shortcut/config change updates it in the same change set (feature → `docs/features/<page>.md` + `docs/quick-reference.md`; shortcut → both; config → `docs/configuration.md`).

## Rust Module Structure

- Rust 2024 edition.
- Use `{module_name}.rs`, never `mod.rs`. A directory module is declared from a sibling `{dir}.rs` (e.g. `src/engine.rs` declares `pub mod duckdb_engine;` etc.). When a type would force a same-name inner module (`schema::schema`), hoist it into the parent file to avoid clippy `module_inception`.
- Tests go in separate `{module_name}_tests.rs` files, wired via `#[cfg(test)] #[path = "dir/{name}_tests.rs"] mod {name}_tests;`. **Never** co-locate tests with implementation.
- Split large test files into a `{module_name}_tests/` directory with focused modules.
- **`lib.rs` re-exports every module** so tests construct internals directly — this testing seam is load-bearing, not incidental.

## Code Quality Principles

### File Organization
- **Max 1000 lines per file** (tests included) — refactor into focused modules.
- **Single responsibility** per file; related functionality grouped, unrelated code split out.

### DRY
- Extract repeated logic into functions/modules; traits for shared behavior; utility modules for common ops.

### Functions & Methods
- Focused (one thing well), self-explanatory, easy to reason about without tracing many files; clear names (what, not how); early returns over deep nesting.

### The engine boundary (load-bearing for both north stars)
- Everything outside `src/engine/` talks to **`trait QueryEngine`** (§0/D1), never to DuckDB directly. The worker owns the engine; the dispatcher holds a `Send + Sync` `InterruptHandle` clone and calls `.interrupt()` from its own thread (§0/D4). `Connection` is `Send` but `!Sync` — never share it across threads.
- A SQL error or a cancellation is a **normal `QueryOutcome` arm**, not an exceptional `Result`. `EngineError` is reserved for `load()`.
- Keep the pure core engine-free: it depends on plain data (`Schema`, `Table`, a seeded value cache passed *as data*), so it tests against `FakeEngine` with zero DuckDB/terminal linkage.

## Theme & Styling

All colors and styles are centralized in `src/theme.rs` (added when the render layer lands). When adding/modifying UI:

- **DO** add new colors to the appropriate module in `theme.rs`; use `theme::module::CONSTANT` in render files.
- **DON'T** hardcode `Color::*` or import `ratatui::style::Color` in render files.
- **No emoji** anywhere in output (ASCII type badges, etc.).

```rust
// Good
use crate::theme;
let style = Style::default().fg(theme::grid::HEADER);

// Bad
use ratatui::style::Color;
let style = Style::default().fg(Color::Cyan);
```
