//! `DuckdbEngine` — the production `QueryEngine`, backed by embedded DuckDB.
//!
//! Canonical per `dev/PLAN.md` §0/D1 + the A1 spike (`dev/ASSUMPTIONS.md`):
//! - **Parse once** at `load()` into a resident in-memory table `t`; re-query it per call.
//! - **One long-lived `Connection`** owned by this engine for the whole session — proven
//!   reusable after interrupt by the A1 spike, so no per-interrupt rebuild is needed.
//! - **`SET threads = <bounded>`** at load, so a query on a many-core host doesn't spawn one
//!   thread per core under rapid keystrokes (A2).
//! - **Out-of-band cancel (§0/D4):** `interrupt_handle()` returns an `InterruptHandle` over
//!   the connection's `Arc<duckdb::InterruptHandle>`; the dispatcher calls `.interrupt()` on
//!   it from its thread. An interrupted query surfaces as a DuckDB error whose text contains
//!   "INTERRUPT", which we map to `QueryOutcome::Cancelled`.
//!
//! Interior mutability: DuckDB's `Connection` issues queries through `&self` (it prepares a
//! fresh owned `Statement` per call), so `query`/`distinct` take `&self` while the dispatcher
//! independently holds an `InterruptHandle` clone.

use std::path::Path;
use std::sync::Arc;

use duckdb::types::{Type, ValueRef};
use duckdb::{Connection, InterruptHandle as DuckInterruptHandle};

use crate::engine::types::{Cell, Column, InterruptHandle, Interruptible, QueryOutcome, Table};
use crate::engine::{CsvOpts, QueryEngine};
use crate::error::EngineError;
use crate::schema::{ColumnMeta, ColumnType, Schema};

/// Default cap on DuckDB's per-query thread fan-out (A2). Bounded so rapid keystrokes on a
/// many-core box don't oversubscribe; the A1 spike measured ~18 ms interactive latency at 4.
const DEFAULT_THREADS: u64 = 4;

/// The resident table name every interactive query targets.
pub const TABLE: &str = "t";

pub struct DuckdbEngine {
    conn: Connection,
    schema: Schema,
}

impl DuckdbEngine {
    /// Open an in-memory DuckDB and load `path` once into table `t`, returning the engine.
    /// This is the constructor used in tests; the `QueryEngine::load` path mutates an
    /// already-constructed engine. Both go through [`Self::open_and_load`].
    pub fn open(path: &Path, opts: &CsvOpts) -> Result<Self, EngineError> {
        Self::open_and_load(path, opts)
    }

    fn open_and_load(path: &Path, _opts: &CsvOpts) -> Result<Self, EngineError> {
        let conn = Connection::open_in_memory().map_err(EngineError::Duckdb)?;
        conn.execute_batch(&format!("SET threads = {DEFAULT_THREADS};"))
            .map_err(EngineError::Duckdb)?;

        let path_str = path.to_string_lossy();
        // sample_size = -1 scans the whole file for type inference (correctness over a fast
        // guess — paid once). CsvOpts overrides (delimiter/header/types) wire in later.
        let create = format!(
            "CREATE TABLE {TABLE} AS SELECT * FROM read_csv_auto('{}', sample_size = -1);",
            escape_sql_literal(&path_str)
        );
        conn.execute_batch(&create).map_err(|e| EngineError::Load {
            path: path_str.to_string(),
            source: e,
        })?;

        let schema = introspect_schema(&conn)?;
        Ok(Self { conn, schema })
    }

    /// Run `sql` and collect the full result into a columnar [`Table`], or map an interrupt
    /// to `Cancelled` / any other error to `Error`.
    fn run(&self, sql: &str) -> QueryOutcome {
        let mut stmt = match self.conn.prepare(sql) {
            Ok(s) => s,
            Err(e) => return classify_err(e, sql),
        };
        let mut rows = match stmt.query([]) {
            Ok(r) => r,
            Err(e) => return classify_err(e, sql),
        };

        // `query()` has executed the statement, so column names are available now. Capture
        // them into an owned Vec in a scoped borrow so it doesn't clash with the mutable
        // `rows.next()` borrow in the loop below.
        let names: Vec<String> = match rows.as_ref() {
            Some(stmt) => stmt.column_names(),
            None => Vec::new(),
        };
        let n_cols = names.len();
        let mut col_cells: Vec<Vec<Cell>> = vec![Vec::new(); n_cols];
        let mut col_types: Vec<Option<ColumnType>> = vec![None; n_cols];

        loop {
            match rows.next() {
                Ok(Some(row)) => {
                    for i in 0..n_cols {
                        match row.get_ref(i) {
                            Ok(vr) => {
                                if col_types[i].is_none() && !matches!(vr, ValueRef::Null) {
                                    col_types[i] = Some(map_type(vr.data_type()));
                                }
                                col_cells[i].push(value_ref_to_cell(vr));
                            }
                            Err(e) => return classify_err(e, sql),
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => return classify_err(e, sql),
            }
        }

        let columns = names
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let ty = col_types[i].clone().unwrap_or(ColumnType::Text);
                Column::new(name, ty, std::mem::take(&mut col_cells[i]))
            })
            .collect();

        QueryOutcome::Rows(Table::new(columns))
    }
}

impl QueryEngine for DuckdbEngine {
    fn load(&mut self, path: &Path, opts: &CsvOpts) -> Result<Schema, EngineError> {
        // Re-load replaces the resident engine state. (In practice load happens once per
        // session; this keeps the trait honest if a caller re-loads.)
        let fresh = Self::open_and_load(path, opts)?;
        self.conn = fresh.conn;
        self.schema = fresh.schema;
        Ok(self.schema.clone())
    }

    fn query(&self, sql: &str) -> QueryOutcome {
        self.run(sql)
    }

    fn distinct(&self, col: &str, limit: usize) -> QueryOutcome {
        // Quote the identifier defensively; ordering by frequency desc is the value-autocomplete
        // shape (PLAN §5.5). The full builder lives in autocomplete later; this is the engine path.
        let sql = format!(
            "SELECT \"{}\", count(*) AS n FROM {TABLE} WHERE \"{}\" IS NOT NULL GROUP BY 1 ORDER BY n DESC LIMIT {}",
            col.replace('"', "\"\""),
            col.replace('"', "\"\""),
            limit
        );
        self.run(&sql)
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn interrupt_handle(&self) -> InterruptHandle {
        let duck: Arc<DuckInterruptHandle> = self.conn.interrupt_handle();
        InterruptHandle::new(Arc::new(DuckHandle(duck)))
    }
}

/// Wraps DuckDB's interrupt handle as our `Interruptible`.
struct DuckHandle(Arc<DuckInterruptHandle>);

impl Interruptible for DuckHandle {
    fn interrupt(&self) {
        self.0.interrupt();
    }
}

/// Map a DuckDB error to `Cancelled` (if it was an interrupt) or `Error`.
fn classify_err(e: duckdb::Error, sql: &str) -> QueryOutcome {
    let msg = e.to_string();
    if msg.to_uppercase().contains("INTERRUPT") {
        QueryOutcome::Cancelled
    } else {
        QueryOutcome::Error {
            message: msg,
            sql: sql.to_string(),
        }
    }
}

/// Map a DuckDB `Type` to ciq's neutral `ColumnType`. The engine owns this mapping (D2), so
/// `schema/` stays free of any DuckDB type grammar.
fn map_type(t: Type) -> ColumnType {
    match t {
        Type::Boolean => ColumnType::Bool,
        Type::TinyInt
        | Type::SmallInt
        | Type::Int
        | Type::BigInt
        | Type::HugeInt
        | Type::UTinyInt
        | Type::USmallInt
        | Type::UInt
        | Type::UBigInt => ColumnType::Int,
        Type::Float | Type::Double | Type::Decimal => ColumnType::Float,
        Type::Date32 => ColumnType::Date,
        Type::Timestamp | Type::Time64 => ColumnType::Timestamp,
        Type::Text => ColumnType::Text,
        other => ColumnType::Other(format!("{other:?}")),
    }
}

/// Convert a borrowed DuckDB value into an owned `Cell`. Non-primitive/temporal/decimal
/// values are rendered to text (ciq does not re-parse them; the engine's string form is the
/// display form).
fn value_ref_to_cell(vr: ValueRef<'_>) -> Cell {
    match vr {
        ValueRef::Null => Cell::Null,
        ValueRef::Boolean(b) => Cell::Bool(b),
        ValueRef::TinyInt(i) => Cell::Int(i as i64),
        ValueRef::SmallInt(i) => Cell::Int(i as i64),
        ValueRef::Int(i) => Cell::Int(i as i64),
        ValueRef::BigInt(i) => Cell::Int(i),
        ValueRef::UTinyInt(i) => Cell::Int(i as i64),
        ValueRef::USmallInt(i) => Cell::Int(i as i64),
        ValueRef::UInt(i) => Cell::Int(i as i64),
        ValueRef::HugeInt(i) => Cell::Text(i.to_string()),
        ValueRef::UBigInt(i) => Cell::Text(i.to_string()),
        ValueRef::Float(f) => Cell::Float(f as f64),
        ValueRef::Double(f) => Cell::Float(f),
        ValueRef::Text(bytes) => Cell::Text(String::from_utf8_lossy(bytes).into_owned()),
        other => Cell::Text(format!("{other:?}")),
    }
}

/// Introspect the resident table's schema once at load via `DESCRIBE`.
fn introspect_schema(conn: &Connection) -> Result<Schema, EngineError> {
    let mut stmt = conn
        .prepare(&format!("DESCRIBE {TABLE}"))
        .map_err(EngineError::Duckdb)?;
    let mut rows = stmt.query([]).map_err(EngineError::Duckdb)?;
    let mut cols = Vec::new();
    while let Some(row) = rows.next().map_err(EngineError::Duckdb)? {
        // DESCRIBE columns: column_name, column_type, null, key, default, extra
        let name: String = row.get(0).map_err(EngineError::Duckdb)?;
        let type_str: String = row.get(1).map_err(EngineError::Duckdb)?;
        cols.push(ColumnMeta::new(name, map_type_str(&type_str)));
    }
    Ok(Schema::new(cols))
}

/// Map a DuckDB type *string* (from `DESCRIBE`) to a `ColumnType`. Handles the common base
/// types; parameterized forms (`DECIMAL(p,s)`, `VARCHAR(n)`) match on their prefix.
fn map_type_str(s: &str) -> ColumnType {
    let up = s.to_uppercase();
    let base = up.split('(').next().unwrap_or(&up).trim();
    match base {
        "BOOLEAN" | "BOOL" => ColumnType::Bool,
        "TINYINT" | "SMALLINT" | "INTEGER" | "INT" | "BIGINT" | "HUGEINT" | "UTINYINT"
        | "USMALLINT" | "UINTEGER" | "UBIGINT" => ColumnType::Int,
        "FLOAT" | "REAL" | "DOUBLE" | "DECIMAL" | "NUMERIC" => ColumnType::Float,
        "DATE" => ColumnType::Date,
        "TIMESTAMP" | "TIMESTAMPTZ" | "TIME" | "DATETIME" => ColumnType::Timestamp,
        "VARCHAR" | "CHAR" | "TEXT" | "STRING" => ColumnType::Text,
        _ => ColumnType::Other(s.to_string()),
    }
}

/// Escape a single-quoted SQL string literal (double any embedded single quotes).
fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}
