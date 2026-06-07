# ciq

**CSV Interactive Query** — an interactive terminal UI that gives CSV files what [`jiq`](../jiq) gives JSON: type a DuckDB-style SQL query in a bar and watch an aligned result grid update live as you type, querying an in-memory columnar table parsed once at startup.

> Status: **planning**. See [`PLAN.md`](PLAN.md) for the full project plan.

## Why

- **Most performant in-memory CSV CLI.** Parse the CSV once into a columnar table (embedded DuckDB), then re-query in-process per debounced edit — interactive queries land in single-digit-to-low-tens of milliseconds even on multi-million-row files.
- **Live SQL as you type.** Schema-aware autocomplete (columns + types + distinct cell values + SQL keywords), a column palette to filter/select columns without hand-writing SQL, and instant column facets.
- **AI-testable by construction.** The vast majority of the code is pure, headless, and deterministic — exercisable by an automated build -> test -> fix loop — with only a small, explicitly-enumerated TUI surface that requires human validation.

## Relationship to jiq

ciq is a standalone sibling of [`jiq`](../jiq). It reuses jiq's proven TUI shell, debouncer, worker/channel model, and autocomplete framework (copied, not shared as a crate), and replaces the engine (embedded DuckDB instead of piping to `jq`), the autocomplete grammar (SQL instead of jq paths), and the renderer (tabular grid instead of pretty-printed JSON).

## Engine decision

Chosen via a benchmark spike (DuckDB vs Polars vs DataFusion on a 5M-row / 368MB CSV). **Embedded DuckDB** won on interactive query latency, exact SQL dialect, type/date sniffing, and — critically — mid-query cancellation (`Connection::interrupt()`), which Polars lacks. See [`PLAN.md`](PLAN.md) section 2 for the full table and rationale.
