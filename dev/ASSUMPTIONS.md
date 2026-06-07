# ciq — Assumptions Ledger

> Every claim ciq's design rests on that is **not yet verified against reality**. Each has: what we assume, why it matters, how/when it gets validated, and the fallback if it's false. A future session checks this before relying on any assumption — an unvalidated assumption is a latent bug, not a fact.
>
> Cross-refs: `dev/DECISIONS.md` (the decisions these assumptions support), `dev/PLAN.md` §8 (the risk register R1–R8 these refine).

---

## Status legend
- **OPEN** — not yet validated; do not rely on as fact.
- **GATING** — blocks a specific piece of work until closed.
- **VALIDATED** — confirmed; kept for the record with the evidence.
- **FALSE** — disproven; fallback in effect.

---

## A1 — `duckdb` reuse-after-interrupt (✅ VALIDATED 2026-06-07 — no longer blocks `DuckdbEngine`)

> **CLOSED — A1 PASS.** Micro-spike at `../ciq-spike/interrupt-spike/` (see its `FINDINGS.md` + `RESULTS-A1.txt`), real `duckdb 1.10503.1` bundled, 5M-row/368MB fixture. The **same connection is reusable after interrupt** (re-query = 5,000,000 rows, baseline match, across 2 cycles); interrupt latency **0.78 ms** (~200× under the 150 ms budget); `SET threads=4` keeps interactive latency at ~18 ms; `try_clone()` fallback also validated (table survives, no re-parse) but **not needed**. **Decision: `DuckdbEngine` keeps one long-lived connection per session.** The fallback stays documented for a future-version regression only. P0.5 in `TASKS.md` is DONE.

**Assumption (now confirmed):** after `interrupt_handle().interrupt()` aborts an in-flight query, the **same** `Connection` is immediately reusable for the next `prepare()` + `query()` and returns correct rows.

**Why it matters:** D1 (engine trait) and D4 (dispatcher-direct cancellation) both assume one long-lived worker connection that survives interrupts. If a connection is poisoned by an interrupt, the worker's "cancel then re-issue the next keystroke's query" path is wrong.

**Verified so far (crate source, by the deep-dive):**
- `Connection::interrupt_handle() -> Arc<duckdb::InterruptHandle>` exists; the handle is `Send + Sync`. ✅
- Cross-thread interrupt of a blocked query works and fails cleanly with an INTERRUPT error — the crate's own `test_interrupt` proves *this much*. ✅
- BUT `test_interrupt` **stops at the `is_err()` assertion** (crate `lib.rs:1574`) — it never re-queries the connection afterward. And the ciq spike only read peak RSS after interrupt; it never re-queried either. So **reuse is inferred from source reasoning, NOT observed.** ❌ not proven.

**How to validate (the micro-spike, before any `DuckdbEngine` code):**
1. Open in-memory DB, create table `t`, load a large CSV.
2. On a worker thread, fire a heavy aggregate/sort.
3. From another thread, call `handle.interrupt()`.
4. Assert the query returns the INTERRUPT error.
5. **Then run a fresh `prepare()` + `query("SELECT count(*) FROM t")` on the SAME connection and assert correct rows.** ← the unproven step.
6. Also: capture numeric interrupt **latency** under a fanned-out aggregate (must beat 150ms comfortably; RESULTS.md only recorded "yes").

**Fallback if FALSE:** the worker rebuilds via `Connection::try_clone()` — verified (crate `lib.rs:696`) to "create a new connection to the **already-opened** database", so the in-memory table `t` **survives** (no CSV re-parse). It must **NOT** `open_in_memory()` afresh (that loses `t` and re-parses the whole CSV every cancelled keystroke). Either way **the D1 trait surface and D4 thread topology are unchanged** — which is why D1/D4 are locked now despite A1 being open. The R4 test must then also assert `t` is still queryable on the cloned connection.

**Refines:** PLAN.md §2.7 / §8-R4.

---

## A2 — DuckDB thread oversubscription on many-core hosts (OPEN)

**Assumption:** without a thread cap, DuckDB may spawn up to one thread per core per query (the spike ran on a 96-core box), and under rapid keystrokes that oversubscribes badly.

**Why it matters:** interactive latency (North Star 1) degrades if every debounced keystroke launches 96 threads.

**How to validate / act:** set `SET threads = <bounded>` on the connection at load; measure interactive latency with and without on a high-core host. This is needed **regardless** of the cancellation topology.

**Refines:** PLAN.md §8-R4 item 3.

---

## A3 — musl static-link of bundled DuckDB C++ (OPEN)

**Assumption:** a fully-static musl Linux binary can link DuckDB's bundled C++ (needs a C++ stdlib statically linked against musl — the classic libstdc++-vs-musl friction).

**Why it matters:** portable single-file Linux release. The spike confirmed jiq solved the analogous TLS/rustls problem, but DuckDB's C++ is a harder case.

**How to validate:** a CI musl-target job that builds and runs `--version` + a one-shot query under `ldd`-clean conditions. **Fallback:** glibc-only portable release + a musl build that dynamically links libstdc++ (still distributable, not single-file static). DataFusion remains the pure-Rust escape hatch if musl-static is later judged a hard launch requirement.

**Refines:** PLAN.md §8-R1.

---

## A4 — jiq source citations are illustrative, not authoritative (STANDING)

**Assumption (inverted):** do **NOT** trust the specific jiq file/line citations sprinkled through PLAN.md (e.g. `executor.rs:281`, `context.rs:351`, `value_collector.rs:14`). They were written from memory/grep at plan-authoring time and the deep-dive already found several slightly off.

**Why it matters:** porting work that trusts a stale line number wastes time or copies the wrong thing.

**How to act:** when porting any jiq mechanism, **grep the live jiq source** at `/local/home/chahcha/RustProjects/jiq` for the symbol; treat the plan's line number as a hint only. jiq is inspiration, not law (see DECISIONS guiding principle) — also re-judge whether the mechanism even fits ciq's tabular/SQL reality before copying.

---

## A5 — `distinct()` return shape (OPEN — design taste, settle at autocomplete value-completion)

**Assumption:** `distinct()` returning a generic columnar `Table` (wrapped in `QueryOutcome::Rows`) is acceptable, vs a dedicated `QueryOutcome::DistinctValues` variant.

**Trade-off:** generic `Rows` keeps the outcome enum uniform but forces autocomplete to re-extract a flat value list every keystroke; `DistinctValues` is purpose-built but adds an enum arm. **Lean `DistinctValues`.** Decide when the autocomplete value-completion path is built (Phase 3), not now.

**Refines:** D1.

---

## Meta-finding — PLAN.md "self-canonical" pathology (process assumption)

The deep-dive's stress passes found a *systemic* documentation defect, not just isolated contradictions: **nearly every section declares ITSELF the canonical source of truth and brands the others stale**, while disagreeing on the actual value. It hit:
- the engine trait (four-way: name / method / return / watcher-thread),
- the schema home (§3.3 vs §7.2, both "the single decided path"),
- type-name spellings (`SqlType` ~12× vs `ColumnType` ~5×; `ValueCache` vs `ValueIndex`).

**Consequence for the reconciliation work:** fixing D1/D2/D3 in PLAN.md is a **multi-section sweep**, not a one-line edit. **Preventive convention to add to PLAN.md:** *"Single source of truth — cite, don't re-declare."* A decided fact lives in exactly one section (or in `dev/DECISIONS.md`); every other mention *links* to it rather than restating (and possibly contradicting) it. Without this convention the contradiction class regenerates on the next edit.
