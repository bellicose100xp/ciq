# ciq

**CSV Interactive Query** — type DuckDB SQL, watch an aligned grid update live, against an
in-memory columnar table parsed once at startup. `jiq` for CSV.

## Installation

No prerequisites: DuckDB is embedded, so the installed `ciq` is a single self-contained binary.

### Install via script (macOS/Linux)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/bellicose100xp/ciq/releases/latest/download/ciq-installer.sh | sh
```

### Install from source

Building from source needs a C++ compiler at build time (it compiles the bundled DuckDB;
the first build takes a few minutes).

```bash
git clone https://github.com/bellicose100xp/ciq.git
cd ciq
cargo install --path .
```

## Documentation

- **Documentation:** the [docs site](docs/index.md) (features, quick reference, configuration).
- **Full spec and design:** [`dev/PLAN.md`](dev/PLAN.md).
