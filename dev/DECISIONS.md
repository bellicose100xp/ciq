# ciq — Decision Log (ADR)

> Append-only log of decisions that shape the build. Each entry: what was open, what we decided, why, and what it supersedes in `PLAN.md`. A future session reads this to understand **why** the plan says what it says, without re-deriving it. Newest decisions are appended; never rewrite history — correct with a new entry.

> **✅ D1–D5 VALIDATED & FINAL (2026-06-07).** They were first answered quickly in a clarifying Q&A, then independently re-derived from scratch by a dynamic-workflow deep-dive (12 agents: per-decision investigator → adversarial red-team → synthesis) that checked the engine decisions against the **real `duckdb` crate source**, and finally confirmed by the user. Verdicts: D1 REVISE, D2 CONFIRM, D3 CONFIRM, D4 CONFIRM, D5 REVISE→tiered. The deep-dive's key correction: there is **no `Connection::interrupt()` method** — the real API is `Connection::interrupt_handle() -> Arc<duckdb::InterruptHandle>` (verified `Send + Sync`). One live code spike (reuse-after-interrupt) gates D1/D4 before any `DuckdbEngine` code — see `dev/ASSUMPTIONS.md` A1.

> **Meta-finding to honor going forward (from the deep-dive's stress passes):** `PLAN.md` has a systemic *"every section declares ITSELF canonical and brands the others stale"* pathology — it hit the engine trait (four-way), the schema home, and type-name spellings (`SqlType` vs `ColumnType`, `ValueCache` vs `ValueIndex`). Reconciling D1/D2/D3 means **multi-section sweeps, not one-line edits**. Add a *"single source of truth — cite, don't re-declare"* convention to the plan, or this contradiction class regenerates.

## Guiding principle (applies to every decision below)

**jiq is inspiration, not law.** ciq starts from jiq's shell, but its domain is fundamentally different (tabular CSV vs JSON; in-process DuckDB vs external `jq`; SQL grammar vs jq paths). Every reuse decision is justified on **ciq's own merits** — "does this fit tabular/DuckDB/SQL reality?" — not "jiq does it this way." Where ciq should consciously diverge from jiq, we say so. jiq source line-number citations in `PLAN.md` are **illustrative** (grep to confirm, don't trust the number).

**When in doubt, deep-dive.** Any non-trivial design fork or material risk gets a dynamic-workflow investigation (adversarial, multi-agent) before a call is made — not a first-instinct guess.

---

## How this log relates to the other docs

- `dev/PLAN.md` — the full spec. The canonical decisions below are folded back into it so it states each once. When PLAN.md and this log ever drift, **this log wins for the decision itself**; PLAN.md wins for surrounding design detail.
- `dev/ASSUMPTIONS.md` — unverified assumptions each decision rests on, with how/when they get validated.
- `dev/TASKS.md` — the dependency-ordered build plan that executes these decisions.

---

## D1 — Engine trait name & signature

**Status:** FINAL 2026-06-07 (Q&A → deep-dive **REVISE**, high confidence → user-confirmed). Verified against the real `duckdb` crate source.
**Was open because:** three sections each declared a different "canonical" engine trait — `QueryEngine::run(sql, cancel) -> QueryOutcome` (§7), `CsvEngine::query(sql, cancel) -> Result<QueryOutput, EngineError>` (§3/§8), plus mixed `QueryEngine::query` variants (§2.6/§4.1). This is the type every layer compiles against, so it was genuinely blocking.

**Decision:** the engine is

```rust
trait QueryEngine {
    fn load(&mut self, path: &Path, opts: &CsvOpts) -> Result<Schema, EngineError>;
    fn query(&self, sql: &str) -> QueryOutcome;
    fn distinct(&self, col: &str, limit: usize) -> QueryOutcome; // returns QueryOutcome, NOT Vec<String>
    fn schema(&self) -> &Schema;
    fn interrupt_handle(&self) -> InterruptHandle;
}

enum QueryOutcome {
    Rows(Table),                              // Table is COLUMNAR: Vec<Column>, with a cheap row-view for the grid
    Error { message: String, sql: String },
    Cancelled,
}

// InterruptHandle is a thin newtype over Arc<duckdb::InterruptHandle> (the real return type of
// Connection::interrupt_handle(), verified Send + Sync). It is Clone via the Arc. There is NO
// method named Connection::interrupt(); you call .interrupt() ON the handle.
struct InterruptHandle(std::sync::Arc<duckdb::InterruptHandle>);
```

Production impl: `DuckdbEngine`. Test impl: `FakeEngine` (deterministic, in-memory, no DuckDB dependency).

**Why (on ciq's own merits — all engine facts verified in crate source):**
- **No cancel arg on `query()`.** Cancellation is out-of-band (see D4): the worker blocks in a synchronous `query()` and cannot watch a token mid-call (the call is stuck inside DuckDB C++), so a cancel parameter on the hot path would be dead weight. The interrupt is delivered through the separate `InterruptHandle`, and `Cancelled` comes back as a first-class outcome.
- **`QueryOutcome` enum, not `Result<_, EngineError>`, for the hot path.** A cancelled query and a SQL error are both *normal, expected* results of live-typing against half-written SQL — not exceptional failures. Modeling them as enum arms (rather than error variants) makes the worker→dispatcher mapping to `QueryResponse::{ProcessedSuccess, Error, Cancelled}` a total, exhaustive, compiler-checked match with no error-type smuggling. `EngineError` is reserved for `load()`, where a genuine failure (file unreadable, OOM) *is* exceptional.
- **`Table` is COLUMNAR (`Vec<Column>`).** Every consumer in ciq is column-oriented: the grid's per-column widths/alignment, type-aware autocomplete, facets. DuckDB hands back typed columns. Carrying a row-major table would force every consumer to transpose. A cheap row-view adapter serves the grid's by-row iteration.
- **`distinct()` returns `QueryOutcome`** (not `Vec<String>`) so *all* engine output flows through one handling/cancellation path. (Open taste call: a typed `QueryOutcome::DistinctValues` variant vs a generic `Rows` the autocomplete re-extracts each keystroke — lean `DistinctValues`; settle when autocomplete value-completion is built.)
- **`&self` on `query()` is sound:** DuckDB's `Connection` uses interior mutability — `Connection::prepare` is `&self`, each call makes a fresh owned `Statement` borrowed `&mut` locally. Verified in crate source.

**Supersedes:** §2.4, §2.6, §3.3, §3.4, §4.1, §7.0, §7.2, §8.2-preamble wherever they say `CsvEngine`, `run()`, `execute()`, a cancel arg on `query()`, or a `Result<…, EngineError>` hot-path return. All defer to D1. **Note:** PLAN.md's §8 (the section the doc itself brands "canonical, supersedes everything") picks the *wrong* options on all four axes (name/method/return/watcher) — reconciling D1 is a multi-section sweep, and §8's "canonical" self-label must be demoted.

**Gating spike (shared with D4):** reuse-after-interrupt — see `dev/ASSUMPTIONS.md` A1. Safe to lock the trait now because the fallback (`try_clone()`) leaves the trait surface unchanged.

---

## D2 — Schema type location

**Status:** FINAL 2026-06-07 (Q&A → deep-dive **CONFIRM**, high confidence → user-confirmed).
**Was open because:** §3.3/§5 declared top-level `src/schema/` canonical; §7.2 declared `src/engine/schema.rs` "the single decided path." §6 muddied it further by citing "§7.1" (the spike section, which says nothing about schema). Every `use` statement depends on this.

**Decision:** `Schema`, `ColumnMeta`, `SqlType`/`ColumnType` live in a **top-level `src/schema/`** module (sibling of `engine/`):

```
src/schema/
  schema.rs   // Schema { columns: Vec<ColumnMeta { name, ty, .. }> }
  types.rs    // ColumnType (Int/Float/Date/Text/Bool/…) mirroring DuckDB sniffing
```

Import path everywhere: `crate::schema::Schema`.

**Why (on ciq's own merits):** `Schema` is consumed by **both** `engine/` (which produces it at load) and `autocomplete/`, `grid/`, `schema_bar/`, `facets/`, `config/` (which only read it). Putting it *inside* `engine/` would force every non-engine consumer to import `crate::engine::schema::…`, coupling the whole app's type graph to the engine module — exactly the coupling the swappable-engine-box design (D1) is trying to avoid. A plain top-level data module that the engine *fills* keeps `Schema` a pure owned value with no back-reference to the live connection, which is also what lets the autocomplete candidate generator stay a pure function.

**Supersedes:** §7.2's "under engine/" statement; §6.3/§6.6/§6.8's "home settled in §7.1" cross-references (repoint to §3.3 / this entry). **Cleanup is a multi-section sweep, not one line:** repoint the five "§7.1" schema-home references (§7.1 is the spike section, decides nothing), delete §6.8's "owned by the engine layer" assertion, rewrite §7.2's `engine/schema.rs` row, and in the same pass settle the **type-name inconsistency** the plan already carries (`SqlType` used ~12× vs `ColumnType` ~5× — **pick one**; recommend `ColumnType`) and the **`ValueCache` vs `ValueIndex`** naming inconsistency. The provisional also invented `ColumnType::from_duckdb` (appears nowhere in the plan).

**Design tension to resolve (not a runtime spike):** the DuckDB-type→`ColumnType` mapping. If it only handles trivial cases (DATE/BIGINT/DOUBLE/VARCHAR) a pure helper in `schema/` is fine. If it must parse DuckDB's full type grammar (`DECIMAL(p,s)`, `STRUCT`, `LIST`, `MAP`, nested types the sniffer emits), that mapping becomes a DuckDB-dialect parser sitting in the supposedly engine-agnostic leaf — re-coupling it to DuckDB, and `from_duckdb` ages badly under a DataFusion/Arrow swap. **Decide:** is the mapping a neutral helper in `schema/`, or owned by each engine impl (recommended — engine owns its own type→ColumnType conversion, hands `schema/` a finished `Schema`)? Also consider making `Schema`'s owned/`Send`/`'static` nature a compile-checked property so a future contributor can't stash a live `Connection` on it (the facets/value-completion path is where that temptation lives).

---

## D3 — Column palette behavior

**Status:** FINAL 2026-06-07 (Q&A → deep-dive **CONFIRM**, high confidence → user-confirmed).
**Was open because:** §6.2 designed a `select_writer` that *parses and splices into the user's hand-typed SQL* (locate the projection, rewrite it, byte-preserve the tail, round-trip via `parse_selected`). §7.5 said the palette *never parses user text* — it owns a ciq-generated query and is disabled when the user has typed SQL. Two different modules, different tests, different UX.

**Decision:** the palette **owns a ciq-generated query state** and emits a canonical `SELECT <projection> FROM t [WHERE …] [ORDER BY …] [LIMIT …]` from structured state (`{ cols: IndexSet, predicates: Vec<…> }`). When the user has hand-typed SQL in the bar, palette/facet actions are **disabled**, optionally offering "replace with generated query?". The palette **never parses or splices into user-typed text.**

**Why (on ciq's own merits):**
- **Stays inside the "tokenizer, not a parser" boundary** the plan commits to (§5.3). Splicing into arbitrary user SQL — even "restricted" SQL with CTEs, subqueries, reserved-word identifiers — needs real parse-tree reasoning the plan deliberately declines to build. Generated-state emission needs none.
- **Tests are pure `state -> String`**, fully deterministic, no parser to get subtly wrong, no `parse_selected ∘ apply_projection == identity` round-trip to maintain. This maximizes the headless-testable surface (North Star 2).
- **The UX cost is acceptable:** the common case (open a fresh file, pick columns) is fully served; the "I typed complex SQL and also want the palette to edit its projection" case degrades to an explicit, safe "replace?" rather than a risky silent rewrite.

**How "is the palette live?" is decided without parsing:** compare the bar text to the last string the palette emitted (**byte equality**). Match → palette owns it, stays live. Differs → user hand-edited; offer the soft "Replace query with column selection?". No parser needed.

**Supersedes / removes:** §6.2's entire `select_writer` / `parse_shape` / `apply_projection` / `parse_selected` / `ProjectionShape` design and §6.1's "round-trip parse of an explicit SELECT into checkmarks" test row. The `palette/select_writer.rs` module is dropped; replaced by a generated-query emitter over palette state. **Delete-sweep must hit ALL references** (grep to zero): the mermaid diagram node, the §6.8 module tree + prose, and the §8/R6 wording — not just the three obvious spots.

**Refinements from the deep-dive:**
- **Don't name the emitter `palette/emit.rs`** — it collides with `output/emit.rs` (the CSV/JSON serializer). Use `palette/query_emit.rs`, or fold the tiny fn into `palette_state.rs`.
- **LIMIT:** defer to the plan's existing `LIMIT min(k, N)` rule verbatim, not a simplified `LIMIT <viewport>`.
- **Two correctness surfaces to golden-test (not one):** (a) identifier quoting in the generated SELECT, **and** (b) facet-predicate VALUE quoting/escaping — `region = 'O''Brien'` (embedded quote), NULL handling, numeric `5` vs string `'5'`, dates. The provisional under-weighted (b).
- **Add an exit criterion that column REORDER emits in the chosen order** (the one palette action with ordering semantics — currently untested).
- **`query_emit`'s byte format is a compatibility/identity surface** (the ownership check compares against it), not a free internal choice — treat its formatting as stable.

**Deferred human-UX check (Phase 4/5 gate):** the soft "Replace query with column selection?" transition — specifically, a user who typed `SELECT id,name FROM t WHERE region='EU'` and opens the palette to add a column: accepting Replace throws away their `WHERE` and snaps to `SELECT *` (correct-by-construction for generated-state, but a real UX cliff). Test *that transition*, not just "does the text read clearly."

---

## D4 — Cancellation: which thread issues `interrupt()`

**Status:** FINAL 2026-06-07 (Q&A → deep-dive **CONFIRM**, high confidence → user-confirmed). Threading topology locked; one engine behavior **gated on the A1 spike**.
**Was open because:** §3.1 put a worker-side "interrupt watcher" helper thread that the dispatcher only signals via a `CancellationToken`; §2.4/§7/§8-R4 had the dispatcher call `interrupt()` directly. Both claimed canonical.

**Decision:** the **dispatcher (App) thread calls `.interrupt()` directly** on its clone of the `InterruptHandle` (the `Arc<duckdb::InterruptHandle>` newtype from D1) when a newer `request_id` supersedes the in-flight query. The worker thread only ever blocks inside `engine.query(sql)` and returns `QueryOutcome::Cancelled` when interrupted. Two threads total (dispatcher + worker); **no interrupt-watcher thread.**

**Why (on ciq's own merits — verified in crate source):**
- **DuckDB's `Connection` is `Send` but `!Sync`; the interrupt handle is `Send + Sync`.** That split makes "worker owns the `Connection`, dispatcher holds a cloned handle" the *only* clean lock-free partition — no `Mutex` on the hot path. A watcher thread adds a concurrency surface for zero behavioral gain.
- **The cancel is a performance optimization, not a correctness requirement.** Correctness comes from `request_id` stale-discard (a late result from a superseded query is dropped regardless). The table is read-only, so a mis-timed interrupt can waste CPU but can never show wrong data. So the simplest topology wins.

**Refinement the deep-dive surfaced — `interrupt()` is NOT request-scoped.** `.interrupt()` aborts *whatever query is currently running* on the connection, not a specific `request_id`. A late interrupt can therefore nick the *newer* query, briefly showing an empty/stale grid until the next keystroke (bounded, self-healing — never corruption). **Required invariant:** the dispatcher only interrupts while it believes a specific request is in-flight, and the worker drains a `Cancelled` before dequeuing the next request.

**Fallback if reuse-after-interrupt fails (A1):** the worker rebuilds via `Connection::try_clone()` (verified crate source: "creates a new connection to the **already-opened** database" — so the in-memory table `t` survives, no CSV re-parse). It must **NOT** `open_in_memory()` afresh, which would lose `t` and silently re-parse the whole CSV on every cancelled keystroke. Either way the trait surface and thread topology are unchanged — which is why D4 is safe to lock now with A1 still open.

**Supersedes / removes:** §3.1's interrupt-watcher thread, its mermaid, and the §3.2/§3.3/§3.4 references to "the interrupt watcher of §3.1." Also corrects the spike's loose `Connection::interrupt()` wording to `interrupt_handle().interrupt()`.

---

## D5 — Coverage gate (TIERED)

**Status:** FINAL 2026-06-07 (Q&A flat-95%-warn → deep-dive **REVISE→tiered**, medium confidence → user-confirmed tiered + kept 95%). **Overrides §4.6 / §4.0.**
**Was open because:** §4.0/§4.6 deliberately refused a fixed coverage percentage ("we do not assert a fixed LOC percentage… false precision"), relying instead on a marker-enforced shell-exemption containment rule. The user wants a maintained 95%.

**Decision — three tiers:**

| Tier | Gate | Rule |
|---|---|---|
| **Pure-core** (explicit allowlist: SQL-context analyzer, ranking, grid-layout math, schema inference, scroll/search math, candidate generation) | **HARD — blocks build** | coverage of the allowlisted modules must stay ≥ floor. Use **branch** coverage, not just line. |
| **Project-wide** | **WARN — build passes** | `cargo tarpaulin` reports overall %; **< 95%** emits a warning annotation, never fails. |
| **Shell containment** | **HARD — blocks build** | a `// ciq:shell-exempt` marker on any file *not* in the §4.7 list fails CI (unchanged from §4.6). |

**Why tiered (on ciq's own merits):**
- **Pure-core functions can't be coverage-padded.** They're data-in/data-out with no I/O, so "cover every branch" and "write a real behavior test" are the *same act* — a hard floor here is **free of the gaming failure mode** that makes blanket gates bad, and it's the highest-value place to harden (a wrong SQL cursor-context silently corrupts *every* autocomplete suggestion; that should turn the build red, not whisper a warning). ciq's architecture makes this core unusually large and cheap to cover.
- **The project-wide blanket number stays warn-only** precisely to avoid test-padding and WIP-blocking — which matters doubly for the AI build→test→fix loop (a hard blanket gate gets gamed, not satisfied).
- **95%, not 90%.** The deep-dive's investigator suggested lowering to ~90%; the synthesis overruled it and the user confirmed: warn-only *already* absorbs the "false-precision" concern (a warning is an aspiration, not a lie), so lowering the number contradicts the stated 95% for no gain. Ratchet later from real data if 95%-warn proves noisy.

**Open risks / spike items (Phase-1 tooling — all net-new; jiq has none of these):**
1. **Branch vs line.** The core floor must use *branch* coverage, else an uncovered `match` arm (e.g. an unhandled `CursorContext` variant) passes at high line-%, defeating the rationale. tarpaulin's branch support is historically weak — **verify it works** or the floor is softer than it looks.
2. **Core/seam boundary is prose-defined.** A dev under floor pressure could reclassify a hard-to-cover branch as "seam" to dodge the floor (padding-by-reclassification). The core tier is an **explicit maintained allowlist**, not "everything not-shell-not-seam" — auditable in review.
3. **Floor calibration.** Early on the core denominator is tiny; one half-finished pure module can drag the aggregate under. Pick the floor % with that in mind.
4. **Allowlist depends on D1/D2 paths.** Build the core allowlist *after* D1 (method names) and D2 (schema home) are reconciled in PLAN.md, or a moved file silently drops out of the floor's scope (counted as excluded, not failed — a silent hole).
5. Confirm tarpaulin emits per-file cobertura a post-step can aggregate against the allowlist, and that Codecov status can be non-blocking while the core-floor is a separate blocking check. (jiq's `fail_ci_if_error` Codecov upload can hard-fail on a transient error — "build passes below target" isn't free today.)

**Supersedes:** §4.6's "no fixed percentage" stance and §4.0's "we deliberately do not assert a fixed LOC percentage." The shell containment gate is retained verbatim.

---

## Self-resolved (no user call needed — fixed with documented defaults, justified on ciq's merits)

These were real inconsistencies the audit confirmed, but they have an obviously-correct resolution; recording so the reasoning is durable.

- **S1 — CI gate set = 4 gates** (test / tarpaulin-coverage / fmt / clippy), no standalone "build" job, no separate "binary" gate. The determinism `disallowed-methods` rule **rides inside the existing clippy gate** via `clippy.toml` (no new job). §4.4's "7th gate alongside 6 (incl. build/binary)" framing is stale and corrected to match §7.2. *(Coverage gate is warn-not-block per D5 but is still one of the four jobs.)*
- **S2 — Human-test surface = §4.7's exact six rows** (glyph+color/polarity, kbd+mouse, **bracketed-paste framing**, clipboard/OSC52, resize, perf-feel). §8/R7 must mirror these verbatim — it had dropped paste-framing and invented a standalone color-polarity item. §4.7 is the single source of truth; R7 only summarizes.
- **S3 — Per-column DISTINCT cap = `MAX_VALUES_PER_PATH` (10_000).** Already consistent across §2.5/§5.5/§8-R6; §5.5's "§2.5 must change" note is **stale** (§2.5 already says this) — delete the note, mark resolved.
- **S4 — DuckDB keywords+functions live in `sql_keywords.rs`** (one combined static-table file), not a separate `duckdb_functions.rs`. §7.4's `duckdb_functions.rs` is the lone outlier and is corrected. *(On ciq's merits: keywords, functions, and the operator table are all static position-filtered candidate data; one file is simpler and well under the 1000-line split rule.)*
- **S5 — Autocomplete context enum = §5.3's `CursorContext`** `{ SelectList, FromTable, Predicate, ComparisonOp, ColumnValue, GroupOrderList, Keyword }` (the only fully-specified, internally-consistent version, matching §5.4's mapping table). §3.2/§4.1/§7.4's variant names are illustrative and realigned to it — including §7.4's Phase-3 exit-criteria assertions.
- **S6 — `ProcessedResult` added fields:** carries structured grid data as `grid: GridLayout` plus `rows` + `schema` (per §3.2), **not** a flat `column_widths` (§7.3's outlier). One authoritative field list; §7.3 corrected. The §3.2 note "corrects §7.2" is repointed to §7.3 (where `ProcessedResult` is actually discussed).

---

## Deferred to implementation (decide at CSV-ingest time, Phases 2/4 — NOT now)

These carry real correctness weight but do not gate Phase 1. Each gets decided **with a default and fixture tests** when we build ingest. Tracked here so they are not forgotten.

- **Q3 — Column-name normalization policy.** Default: keep raw header names, auto-`"quote"` on emit when they contain spaces/special chars/reserved words; document DuckDB quoting. Open: dedupe/slugify policy for duplicate/empty headers.
- **Q7 — Ragged-row policy** (rows with wrong column count). Default: lean on DuckDB's detector; decide error-vs-pad-vs-skip during impl, fixture-tested.
- **Q12 — Empty-cell vs SQL NULL semantics.** Default: render NULL distinctly from empty string; ingest empties per DuckDB default; `null_string` knob is the user lever. Must be decided + fixture-tested before launch (affects `WHERE col IS NULL` vs `= ''`).
- **CsvOpts ↔ CLI-flag inventory.** §6.6's struct (`delimiter, quote, escape, header, null_string, sample_size`) must be expanded to cover R5's required overrides: add `types` (`--types`), `all_varchar` (`--all-varchar`), `date_format` (`--date-format`); unify `--sniff-rows` with the existing `sample_size` under one name. Final field/flag/config-key names decided when the config schema (Q5) is frozen.
