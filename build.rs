//! Build script — the single job is to satisfy a Windows-only link dependency of bundled DuckDB.
//!
//! DuckDB's amalgamation (compiled into the binary by the `duckdb` crate's `bundled` feature)
//! calls the Windows Restart Manager API (`RmStartSession` / `RmEndSession` /
//! `RmRegisterResources` / `RmGetList`) from its file-locking diagnostics. Those symbols live in
//! `rstrtmgr.lib`, which `libduckdb-sys` does not add to the MSVC link line, so the release build
//! failed with `LNK2019: unresolved external symbol RmStartSession` (and three siblings). Linking
//! it here fixes the target without patching the upstream crate. No-op on every non-Windows
//! target (macOS / Linux link cleanly).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=dylib=rstrtmgr");
    }
}
