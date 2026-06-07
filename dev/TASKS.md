# ciq — Build Task Breakdown

> The **resumable spine** for building ciq. Dependency-ordered, incremental, each task small with **machine-checkable exit criteria**. A future session resumes by scanning the Status column: do the next `TODO` whose deps are all `DONE`.
>
> **Read first:** [`PLAN.md`](PLAN.md) §0 (canonical decisions), [`DECISIONS.md`](DECISIONS.md) (D1–D5 + rationale), [`ASSUMPTIONS.md`](ASSUMPTIONS.md) (A1 gates the engine).
>
> **Status values:** `TODO` · `IN-PROGRESS` · `BLOCKED(reason)` · `DONE` · `DEFERRED`.
> **Rule:** never start a task whose deps aren't all `DONE`. Phase 1 gates everything. The A1 spike (P0.5) gates the real engine impl.

---

## Conventions every code task inherits (the standing checklist)

These are not repeated per-task; they apply to **every** task that writes Rust:
- Rust 2024; `{name}.rs` (never `mod.rs`); tests in **separate** `{name}_tests.rs` (or `{name}_tests/` when large); files **< 1000 lines** (split if over); all colors in `theme.rs`; everything re-exported via `lib.rs` so tests reach internals.
- New logic ships **with its tests** in the same task. Pure-core modules aim for ~100% line+branch (D5 hard floor); seam modules behavior-covered.
- Determinism: **no** `Instant::now()`/`SystemTime::now()`/`rand` in logic (only seam wrappers). Time enters as a `u64` parameter.
- Pre-commit (local): `cargo fmt --all --check` → `cargo clippy --all-targets --all-features -- -D warnings` → `cargo build --release` → `cargo test --all-features -- --test-threads=1`. **Never** bare `cargo test`, **never** `--lib`.
- jiq is **inspiration, not law** — grep the live jiq source when porting; re-judge fit for tabular/SQL; don't copy by inheritance.
- **When in doubt on a design fork, launch a dynamic workflow** to deep-dive before coding.

---

## Phase 0 — Engine spike  ·  Status: DONE

| ID | Task | Status | Exit criteria |
|---|---|---|---|
| P0.1 | Benchmark DuckDB vs DataFusion vs Polars | **DONE** | `ciq-spike/RESULTS.md` exists; DuckDB chosen; DataFusion = fallback. |

---

## Phase 0.5 — A1 reuse-after-interrupt micro-spike  ·  Status: **DONE (A1 PASS)**  ·  was: GATES P1.4

> Closed 2026-06-07. Spike at `../ciq-spike/interrupt-spike/` (`FINDINGS.md` + `RESULTS-A1.txt`), real `duckdb 1.10503.1`, 5M-row fixture. **Verdict: keep one long-lived connection per session** — reuse-after-interrupt confirmed. Unblocks P1.4. See [`ASSUMPTIONS.md`](ASSUMPTIONS.md) A1.

| ID | Task | Deps | Status | Result |
|---|---|---|---|---|
| P0.5.1 | Prove reuse-after-interrupt on a real `duckdb` connection | P0.1 | **DONE** | PASS — same connection re-queries 5,000,000 rows after interrupt, across 2 cycles. No rebuild needed. |
| P0.5.2 | Measure interrupt latency under a fanned-out aggregate | P0.1 | **DONE** | 0.78 ms / 0.69 ms — ~200× under the 150 ms debounce. |
| P0.5.3 | Confirm `SET threads=<bounded>` caps DuckDB thread fan-out | P0.1 | **DONE** | `threads=4`: interactive filter 17.7 ms ≪ 150 ms. Apply `SET threads=<bounded>` at load in `DuckdbEngine`. |
| P0.5.4 | Validate `try_clone()` fallback | P0.5.1 | **DONE** | PASS — cloned connection sees all 5M rows of `t`, no re-parse. Documented fallback; **not needed** for this crate version. |

---

## Phase 1 — Scaffold + headless harness + QueryEngine wrapper + parse-once  ·  Status: **DONE** ✅  ·  was: HARD GATE for all later phases

> **Phase 1 exit met (2026-06-07):** all P1.1–P1.9 DONE; `cargo test --all-features -- --test-threads=1` green (46 tests) with no TTY; fmt + clippy (incl. determinism gate) clean; release builds clean; tiered-coverage + shell-containment gates locally verified. Human-validation: none (no interactive surface yet). **Phase 2 (vertical slice) is unblocked.** Caveat: CI workflow is authored + locally proven but won't *execute* until a GitHub remote exists.

> The testability prerequisite. Nothing in Phase 2+ starts until P1 exits green. Stands up the crate, the engine trait + real impl, the `src/schema/` module, the headless harness, and the 4 CI gates + D5 tiered coverage.

| ID | Task | Deps | Status | Exit criteria |
|---|---|---|---|---|
| P1.1 | Cargo crate scaffold + `lib.rs`/`main.rs`/`error.rs` skeleton; pin `duckdb` crate (bundled) exactly | — | **DONE** | `cargo build` OK; `duckdb = "=1.10503.1"` pinned; `lib.rs` re-export seam in place; `ciq --version`/`--help` work; fmt+clippy clean. |
| P1.2 | `src/schema/` — `Schema`, `ColumnMeta`, `ColumnType` ([§0/D2](#)) | P1.1 | **DONE** | `crate::schema::{Schema, ColumnMeta, ColumnType}` exist; ColumnType = Int/Float/Bool/Date/Timestamp/Text/Other(String); 12 tests (alignment, lookup, empty, dup-header, verbatim names). No DuckDB dep, no `Connection` field. (Note: `Schema`/`ColumnMeta` live in `src/schema.rs` directly + `ColumnType` in `src/schema/types.rs` to avoid clippy `module_inception`.) |
| P1.3 | `trait QueryEngine` + `QueryOutcome` + `InterruptHandle` + columnar `Table`/`Column`/`Cell` ([§0/D1](#)) | P1.2 | **DONE** | Trait matches D1 (`query(sql)->QueryOutcome`, no cancel arg; `distinct->QueryOutcome`; `interrupt_handle()`). `QueryOutcome` exhaustive-match test; `Table` = `Vec<Column>` with `row()` view; `Cell::Null` distinct from `Text("")` (Q12); `InterruptHandle` Send+Sync+Clone over `Arc<dyn Interruptible>`. 20 tests. (Trait in `src/engine.rs`; types in `src/engine/types.rs`. `CsvOpts` placeholder added.) |
| P1.4 | `src/engine/duckdb_engine.rs` — `DuckdbEngine` real impl: parse-once via `read_csv_auto`; **one long-lived `Connection`** per session (A1 PASS — no rebuild); `query`; `distinct`; `interrupt_handle()` over `Arc<duckdb::InterruptHandle>`; `SET threads=4` at load | P1.3, **P0.5 ✅** | **DONE** | Loads fixture once; typed columnar rows; `created_at -> DATE` golden ✅; malformed SQL -> `Error{message,sql}` ✅; A1 regression guard ✅ (worker OWNS engine since `Connection` is Send+!Sync; dispatcher holds Send+Sync handle; interrupt -> `Cancelled`, same connection still counts 20000); empty field -> `Cell::Null` (Q12 default); `distinct()` works. Engine owns DuckDB-type→`ColumnType` mapping (D2). 27 tests, release clean. |
| P1.5 | `src/engine/fake_engine.rs` — deterministic in-memory `FakeEngine` (no DuckDB) | P1.3 | **DONE** | Implements `QueryEngine` over canned outcomes (default + exact-match overrides); counting hooks (`load_count`/`query_count`/`distinct_count`); models interrupt -> `Cancelled`; no-terminal-IO self-test. 7 tests (34 total). |
| P1.6 | `src/harness/` — `EngineHarness` (load fixture, fire SQL, assert `QueryOutcome`) | P1.4, P1.5 | **DONE** | `EngineHarness::from_csv`/`open`; no-TTY self-test (`TERM` unset); deterministic. `harness` mod is `#[cfg(test)]` (uses dev-dep `tempfile`); promote behind a `testutil` feature when `tests/` E2E needs it. |
| P1.7 | `AppHarness` minimal stub (renders `App` to `ratatui::TestBackend`; `now_ms` synthetic-clock seam) | P1.6 | **DONE** | Minimal `App` (Loading/Ready phase + status) renders a bordered placeholder; `AppHarness::screen()` returns serialized buffer; deterministic render; no-TTY self-test; `advance(ms)` seam for P2 debouncer. Added `ratatui = "0.29"`. 46 tests. |
| P1.8 | CI: 4 gates (`test --test-threads=1` / `tarpaulin` / `fmt` / `clippy -D warnings`) + `clippy.toml` `disallowed-methods` riding clippy ([§0/D5](#), S1) | P1.1 | **DONE** | `.github/workflows/ci.yml` (test/lint/shell-containment/coverage jobs); `clippy.toml` bans Instant/SystemTime::now + rand. **Verified locally**: planted `SystemTime::now()` in a lib module fails clippy; removed → clean. No build job, no binary gate, no "7th gate". (CI can't *run* until a GitHub remote exists, but every gate's logic is locally proven.) |
| P1.9 | CI: D5 tiered coverage — pure-core **line** floor (hard) + project-wide 95% **warn-only** + shell-marker containment (hard) | P1.8, P1.2, P1.3 | **DONE** | `dev/core-modules.txt` allowlist (schema + engine/types now; grows per phase); `dev/ci/core-floor.sh` (HARD, **verified** passes ≥floor / fails <floor exit 1), `coverage-warn.sh` (WARN-only, exit 0), `check-shell-exempt.sh` + `dev/shell-exempt.txt` (HARD, **verified** fires on planted marker). Gates on **line** rate not branch — tarpaulin branch support is weak (D5 risk #1); documented, switch to branch when trustworthy. |

**Phase 1 exit:** all P1 tasks `DONE`; `cargo test --all-features -- --test-threads=1` green with no TTY; the 4 CI gates + tiered coverage green. **Human-validation: none** (no interactive surface yet).

---

## Phase 1.5 — Debug logging infrastructure  ·  Status: TODO  ·  Deps: P1

> Stand up `--debug` file logging before Phase 2 so the worker/engine/cancel code is instrumented *as it's written* (cheaper than retrofitting). Mirrors jiq's `log` + `env_logger` + RAII `Timer` pattern, with ciq's twist: logs to a **`/tmp/ciq/` folder** (created on demand). Goes to a **file only**, never stdout/stderr (which would corrupt the TUI). The wall-clock calls (`Instant::now`/`SystemTime::now`) are confined to this logging module as the documented `clippy.toml` seam exception — logic code stays clock-free.

| ID | Task | Deps | Status | Exit criteria |
|---|---|---|---|---|
| P1.5.1 | `--debug` CLI flag + `CIQ_DEBUG=1` env (**explicit opt-in ONLY — NOT auto-on in debug builds**, deliberate divergence from jiq); `init_logger` writes to `/tmp/ciq/ciq-debug.log` (create `/tmp/ciq/` if missing); `log` + `env_logger` deps; timestamped lines, file-only | P1.1 | **DONE** | **Verified on release binary**: no `--debug` → `/tmp/ciq/` never created, zero effect; `--debug` → dir created + timestamped line in `ciq-debug.log`; nothing ever to stdout/stderr. `log::debug!` no-ops when logger off. |
| P1.5.2 | RAII `Timer` (logs `[TIMING] {label} took {ms}ms` on drop) — **reads the clock only when logging is active** (`Option<Instant>` gated on `log_enabled!`), so zero hot-path cost when off; wall-clock confined here behind `#[allow(clippy::disallowed_methods)]` as the documented seam | P1.5.1 | **DONE** | `Timer::new` reads clock only if debug enabled (`clock_was_read()` test asserts `None` when off); clippy green (only `Instant`/`SystemTime::now` in the crate live in `logging.rs`, allow-annotated). 51 tests. |

**Phase 1.5 exit:** ✅ `--debug` produces a timestamped `/tmp/ciq/ciq-debug.log`; binary without the flag is silent and overhead-free (verified on release build — no clock read, no dir, no logger); determinism gate green (wall-clock confined to the logging seam, `Timer` gated on `log_enabled!`). Ready for Phase 2 to instrument query/load/cancel timing.

---

## Phase 2 — Vertical slice: shell + worker + DuckDB + grid + run-on-debounce  ·  Status: TODO  ·  Deps: P1, P1.5

> The end-to-end loop a user feels: type SQL → see aligned grid update live, stale results discarded, in-flight cancellable. Renderer split pure-layout vs blit.

| ID | Task | Deps | Status | Exit criteria |
|---|---|---|---|---|
| P2.1 | `src/query/debouncer.rs` — time-as-`u64`-parameter debouncer (150ms) | P1.1 | **DONE** | `should_execute_at(now_ms)` / `schedule_execution_at(now_ms)`; tests pass explicit `u64` at exact boundaries; no Clock trait. |
| P2.2 | `src/query/worker/types.rs` — `QueryRequest { query, request_id }` (no cancel_token), `QueryResponse::{ProcessedSuccess, Error, Cancelled}`, `ProcessedResult` (adds `rows`+`schema`+`grid: GridLayout` per S6) | P1.3 | TODO | Channel enums + `ProcessedResult` field list match §0/S6 (not `column_widths`). Exhaustive-match tests. |
| P2.3 | `src/query/worker/thread.rs` — `spawn_worker`, blocking `recv()` loop, `catch_unwind`+panic hook; worker owns `QueryEngine` | P2.2, P1.5 | TODO | Worker runs SQL on injected `QueryEngine`; panic in engine -> `QueryResponse::Error`, never crashes harness. |
| P2.4 | `src/query/query_state.rs` — latest-`request_id` tracking + stale-discard; pure `is_stale` | P2.2 | **DONE** | `is_stale` unit-tested exhaustively; stale responses dropped before touching result state. |
| P2.5 | Dispatcher: out-of-band cancel — dispatcher holds `InterruptHandle` clone, calls `.interrupt()` on supersede; drains `Cancelled` before next dequeue ([§0/D4](#)) | P2.3, P2.4, P1.4 | TODO | Headless test (stub engine blocking on a barrier): enqueue id=1, then id=2; assert dispatcher fires interrupt from the **dispatcher thread**, worker emits `Cancelled{1}`, only id=2 surfaces. |
| P2.6 | `src/grid/grid_layout.rs` — pure `layout_grid(rows, &Schema, &GridView) -> GridFrame{header, body, col_x, total_width}`; `col_width.rs`; type-aware alignment; column-granular h-scroll | P1.2 | TODO | Pure fn, no Frame/Terminal/clock; unit-tested widths/alignment/ellipsis/null-glyph; property tests (1 row=1 line; widths fit viewport). |
| P2.7 | `src/grid/grid_render.rs` — thin blit: sticky header row + scrolled body `Paragraph` (reuses jiq vertical-slice) | P2.6 | TODO | `TestBackend` 80x24 + pathological-width `insta` snapshots; header outside scrolled body; body viewport height = inner-1. |
| P2.8 | `src/app/` — shell skeleton (App state, crossterm event loop, focus/mode), retargeted to grid; `app_render.rs` | P2.7, P1.7 | TODO | Synthetic `KeyEvent`s route + assert state; `TestBackend` render snapshot. Crossterm loop is the only terminal edge. |
| P2.9 | `src/query/preprocess.rs` — grammar validate + §2.3 LIMIT-wrap via ONE shared `top_level_tokens` scan (D6) | P2.2 | **DONE** | Pure `String -> String`; table-driven tests incl. trailing `;`, existing `LIMIT`, reject multi-statement/DML. |
| P2.10 | `src/query/error_enhance.rs` — DuckDB error -> friendly message | P2.2 | **DONE** | Pure mapping, golden table tests. |
| P2.11 | Load state machine (`Loading → Ready → Querying`, `LoadError`); async load off UI thread; query bar editable during load | P2.8, P2.3 | TODO | Headless: fake/slow engine drives states; "query typed during load fires on Ready"; `load()` called **exactly once** (counting `FakeEngine`). |

> **Progress (group 1 of 4 done, committed `1a57d9c`):** P2.1/P2.4/P2.9/P2.10 land the pure query primitives. The `/simplify`+`/code-review`+fix-re-review loop ran: code-review found ~10 bugs in the first naive scanners → consolidated to one shared `top_level_tokens` scan (D6, DRY); fix-re-review then caught a SAFETY-CRITICAL statement-smuggling hole (stray `)`/`(` hid a top-level `;`, smuggling DROP/DELETE past the guard) → fixed to fail-closed (`;` is a terminator regardless of paren depth; `)` clamps depth at 0); a `proptest` caught a UTF-8 mid-char-slice panic on `"¡"` → fixed. 97 tests. Remaining P2 groups: (2) worker+channel+dispatcher [P2.2/2.3/2.5], (3) grid [P2.6/2.7], (4) shell+load [P2.8/2.11].

**Phase 2 exit (agent-checkable):** `AppHarness` types `SELECT * FROM t WHERE region='EU' LIMIT 5`, advance `current_time_ms` past 150ms → snapshot shows 5 aligned rows + header, type-aligned; out-of-band cancel test green; debounce coalesces N keystrokes → exactly one `query` call; invalid SQL → error line not crash; CI gates green.
**Human-validation (first):** one scripted smoke — launch `ciq fixture.csv`, type a WHERE, confirm live update + colors render. Scope = §4.7 surface only.

---

## Phase 3 — Schema-aware autocomplete  ·  Status: TODO  ·  Deps: P2

> Context + column + value completion. Framework reused; grammar + sources replaced. Canonical context enum = §5.3 `CursorContext` (S5).

| ID | Task | Deps | Status | Exit criteria |
|---|---|---|---|---|
| P3.1 | `src/autocomplete/sql_lexer.rs` — pure `tokenize(&str) -> Vec<Token>` (tolerant of half-typed input; paren-depth; quote tracking) | P1.1 | TODO | Property: `concat(token.text) == input`; spans cover input; never panics on arbitrary bytes. |
| P3.2 | `src/autocomplete/clause_context.rs` — pure `detect_context(&[Token], cursor) -> CursorContext` (S5 enum: SelectList/FromTable/Predicate/ComparisonOp/ColumnValue/GroupOrderList/Keyword) | P3.1 | TODO | Table-driven (one row per §5.4 mapping line) + property (never panics for any byte offset). Matches §5.3/§5.4 exactly. |
| P3.3 | `src/autocomplete/sql_keywords.rs` — static DuckDB keyword + function + operator tables (one file, NOT duckdb_functions.rs — S4) | P1.1 | TODO | Const data + tests; negative assertion: jq-only names (`to_entries`/`gsub`) absent. |
| P3.4 | `src/schema/` value cache `ValueCache` + `value_source.rs::build_distinct_sql(col, cap)` (cap = `MAX_VALUES_PER_PATH` 10_000, S3) | P1.2, P1.4 | TODO | Pure SQL-string builder unit-tested (incl. identifier quoting) without executing; one integration test runs it on a fixture. |
| P3.5 | `src/autocomplete/candidates.rs` — pure `get_suggestions(query, cursor, &Schema, &OperatorTable, &ValueCache) -> Vec<Suggestion>` | P3.2, P3.3, P3.4 | TODO | Table-driven golden cases over fixed `Schema` + seeded `ValueCache`; **no engine** in unit tests. |
| P3.6 | Reuse autocomplete framework (popup render, fuzzy ranking, insertion); `SuggestionType` + `Keyword`/`Aggregate` variants; `ColumnType` typed hints; SQL identifier quoting on insert | P3.5, P2.8 | TODO | Ranking total-order property; insertion UTF-8/cursor round-trip property; popup `insta` snapshot. |
| P3.7 | Wire value-completion through the worker channel (engine fills `ValueCache`; autocomplete never opens its own connection) | P3.6, P2.5 | TODO | Mock `distinct()` returns fixed set → `WHERE status = ` then `a` suggests `'active'` with correct quoting. |

**Phase 3 exit (agent-checkable):** `detect_context` truth table (§7.4, S5 names) green; column completion ranks correctly; value completion inserts quoted value; function popup shows DuckDB (not jq) builtins; CI gates green.
**Human-validation:** one popup-navigation check (real arrow/Tab/Enter, value popup after `=`) — **folds into the batched P4/P5 gate**, not a separate stop.

---

## Phase 4 — CSV-native UX  ·  Status: TODO  ·  Deps: P2 (non-value parts) + P3 (value-completion parts)

> Column palette (generated-state, D3), facets, schema bar, delimiter detect, output modes. Non-value parts may run parallel with P3; value parts (facets, palette value-suggestion) gated on P3.

| ID | Task | Deps | Status | Exit criteria |
|---|---|---|---|---|
| P4.1 | `src/schema_bar.rs` — pure `layout_schema_bar(&Schema, width, h_col_offset, active_col) -> Vec<Span>` | P2.6 | TODO | Span list `insta`-asserted; alignment matches grid `col_x`; type badges pure lookup. |
| P4.2 | `palette/palette_state.rs` — generated-state machine `{cols: IndexSet, predicates, needle, cursor}` (D3) | P2.8 | TODO | Pure toggle/reorder/filter transitions; unit-tested. |
| P4.3 | `palette/query_emit.rs` — pure `emit(state) -> String` (canonical SELECT; `LIMIT min(k,N)`; identifier + facet-value quoting) (D3) | P4.2 | TODO | Golden tests incl. reorder ordering, identifier quoting, **facet-value quoting/escaping** (`O''Brien`, NULL, numeric vs string, dates). |
| P4.4 | Palette ownership detection (byte-compare bar vs last emitted; disable on hand-typed SQL; "Replace?" affordance) (D3) | P4.3, P2.8 | TODO | Unit test: equal → live; different → disabled+offer. No SQL parsing anywhere. |
| P4.5 | `palette/palette_render.rs` — blit (reuse autocomplete popup chrome) | P4.4, P3.6 | TODO | `TestBackend` 80x24 snapshot. |
| P4.6 | `facets/facet_query.rs` (`build_facet_sql(col, &Schema)`) + `facet_state.rs` + `facet_render.rs` — **value part, gated on P3** | P4.1, P3.7 | TODO | Type-aware SQL golden-tested; histogram bar-width math pure; reuses worker channel + request_id staleness. |
| P4.7 | `ingest/sniff.rs` + `ingest/csv_opts.rs` — delimiter/quote/header detect + `CsvOpts` + `merge(config,cli,sniffed)` (CLI>config>sniffed) + `to_read_csv_sql` | P1.4 | TODO | Pure sniffer over fixture bytes (comma/semicolon/tab/pipe + ambiguous→documented default); precedence unit-tested; emitted `read_csv(...)` SQL golden. **Expand CsvOpts to R5 set** (add `types`/`all_varchar`/`date_format`; unify `sniff_rows` with `sample_size`). |
| P4.8 | `output/emit.rs` — `render_output(rows, &Schema, OutputFormat) -> String` (CSV/TSV/JSON/Markdown); reuse top-level `clipboard::osc52` (no `output/clipboard.rs`) | P2.6 | TODO | Byte-exact golden per format (RFC-4180 quoting; JSON null-vs-"" per Q12); CSV round-trips; `--output` CLI path is a headless integration test. |

**Phase 4 exit (agent-checkable):** palette projection from selection set = exact SQL; facet predicate added to state + regenerated string exact; LIMIT composition (no double-limit); delimiter detect on 4 fixtures + ambiguous; output modes byte-exact; schema bar snapshot; CI gates green.
**Human-validation:** batched gate (shared w/ P5).

---

## Phase 5 — AI NL→SQL + history + polish + docs site  ·  Status: TODO  ·  Deps: P4

| ID | Task | Deps | Status | Exit criteria |
|---|---|---|---|---|
| P5.1 | AI NL→SQL behind `trait Provider` (mockable); prompt grounds on live `Schema`; feeds same `QueryRequest` path | P4 | TODO | Mock provider → canned SQL parsed, validated against fixture engine, → `QueryOutcome::Rows`; prompt embeds schema (names+`ColumnType`); **no network in tests**. |
| P5.2 | Query history (port jiq); in-session + on-disk (with config schema) | P4 | TODO | add/recall/dedupe/navigate over in-memory store. |
| P5.3 | Polish: empty-state, large-result truncation banners, error-message enhancement | P4 | TODO | Golden tests for known DuckDB error → friendly string. |
| P5.4 | `docs/` GitHub Pages site (Jekyll + just-the-docs): features, quick-reference, config | P4 | TODO | Site builds; zero broken internal links; quick-ref includes every shortcut/flag from P3–P5. |
| P5.5 | Resolve deferred correctness items with fixtures: Q3 (col-name policy), Q7 (ragged rows), Q12 (empty-vs-NULL) | P4.7 | TODO | Each decided + fixture-tested; documented in DECISIONS.md. |

**Phase 5 exit:** all exit criteria green; CI green on release build.
**Human-validation:** final batched gate (§4.7 surface only): real-terminal AI popup UX, history nav, clipboard/OSC52, color polarity light/dark, resize reflow. NL→SQL quality spot-check recommended but **not** a blocking gate.

---

## Cross-phase invariants (re-checked every phase)

| Invariant | Asserted by |
|---|---|
| CSV parsed exactly once per session | counting `FakeEngine` on `load()` |
| Every interactive query < 150ms debounce budget | perf test over fixtures (value redacted from snapshots) |
| Stale queries discarded by `request_id`; cancel out-of-band (dispatcher interrupts; worker never self-interrupts) | P2.5 test against blocking stub engine |
| Logic majority headless; human surface = §4.7's exact 6 rows | tiered coverage; render split; tests run with no TTY |
| Determinism: no wall-clock/rand in lib code | `clippy.toml disallowed-methods` (rides clippy gate) |
| jiq conventions | `{name}.rs`/`{name}_tests.rs`, <1000 lines, colors in theme.rs, lib.rs re-exports (review-checked, not CI) |
