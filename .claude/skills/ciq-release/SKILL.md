---
name: ciq-release
description: Drive the full ciq release flow — sync, gate, version bump, changelog, tag, and the cargo-dist Release workflow. Use when the user says "release ciq", "ship ciq", "publish a new version of ciq", "do the ciq release", "release patch/minor", "tag a new release", or has a TUI-validated change ready to ship.
---

# ciq-release

Run only after the user has validated any real-terminal change in the TUI (CLAUDE.md → Pre-Commit
Requirements step 8). ciq's release is deliberately simpler than jiq's: **shell installer only**,
built by cargo-dist. There is **no crates.io publish, no Homebrew tap, and no live docs site** —
do not add those steps.

## Argument: `patch` | `minor`

Pre-1.0 versioning (CLAUDE.md → Versioning & Releasing):

- `patch` (default) — bug fixes, refactors, polish, docs → `0.minor.Y+1`
- `minor` — new features **and** breaking changes alike → `0.X+1.0`
- **No `major` / `1.0.0`** until the user explicitly declares ciq 1.0-ready. If asked for a major
  bump, stop and confirm.

If unspecified, infer from the change set (step 4).

## Rules

- No `--no-verify`, no force-push, no `git reset --hard origin` on a branch with unpushed work, no
  destructive ops. Remote history is immutable.
- Commit style: lowercase Conventional Commits, single line, no body, no issue refs.
- **Pre-release gate** — the eight CLAUDE.md Pre-Commit Requirements must already be green and the
  change committed locally. If any is unverified, stop and run it before tagging:
  1. Implementation-detail comments stripped
  2. New logic ships with tests (pure-core: ~100% line+branch)
  3. `cargo fmt --all --check` (zero diffs)
  4. `cargo clippy --all-targets --all-features -- -D warnings` (zero; includes the determinism gate)
  5. `cargo build --release` (zero warnings)
  6. `cargo build` (debug; zero warnings)
  7. `cargo test --all-features -- --test-threads=1` (full suite, **never** `--lib`)
  8. TUI validation for any §4.7 shell-surface change — STOP-and-wait for the user
- **User-visible feature / shortcut / config change** → update docs in the same release:
  `docs/features/*.md`, `docs/quick-reference.md`, `docs/configuration.md` (if config). Bug
  fixes / refactors / perf → docs untouched.
- **CHANGELOG.md is updated every release** (step 5).
- ciq develops directly on `main` in this repo (the mouse work landed via a fast-forward). If the
  user is on a feature branch, fast-forward `main` to it first; only branch + PR if they ask for
  review. Don't invent a PR step the user didn't request.

---

## 1. Sync with remote

```sh
git fetch origin
git checkout main
git pull --ff-only origin main
```

If local `main` has committed work ahead of origin, that's expected (ciq commits locally per the
pre-commit flow). If a feature branch holds the work, fast-forward main to it:

```sh
git merge --ff-only <feature-branch>
```

If it won't fast-forward, stop and report — do not force anything.

## 2. Confirm the gate is green

Re-run the eight checks from the Rules section if anything changed since they last passed. Do not
proceed to tagging until `fmt`, `clippy`, both builds, and the full test suite are green.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo build
cargo test --all-features -- --test-threads=1
```

## 3. Push main

```sh
git push origin main
```

Pushing `main` does **not** trigger a release — only a `vX.Y.Z` tag does. The `CI` workflow
(test / lint / coverage / shell-containment) runs on the push; let it settle and confirm it's
green before tagging:

```sh
gh run list --repo bellicose100xp/ciq --workflow=CI --limit 1
```

## 4. Pick the version bump

If the user passed `patch` / `minor`, use it. Otherwise read the current `version` in `Cargo.toml`
and infer per the pre-1.0 table in the Argument section. Never jump to `1.0.0` without explicit
authorization.

## 5. Update CHANGELOG.md

Move the accumulated notes out of `## [Unreleased]` into a new dated section, and refresh the
compare links at the bottom:

```markdown
## [Unreleased]

## [X.Y.Z] - <YYYY-MM-DD>

### Added | Fixed | Changed
- **<short title>** — <one line of concrete user-visible behavior, not implementation detail>.
```

```
[Unreleased]: https://github.com/bellicose100xp/ciq/compare/vX.Y.Z...HEAD
[X.Y.Z]: https://github.com/bellicose100xp/ciq/releases/tag/vX.Y.Z
```

Match the voice of existing entries — concrete and specific, no hedging. Pass the date in (ciq's
determinism rules forbid ambient clocks in code, but this is a doc edit — use the real date from
the environment context).

## 6. Bump Cargo.toml + rebuild

Edit `version = "X.Y.Z"` in `Cargo.toml`, then rebuild so `Cargo.lock` picks it up:

```sh
cargo build --release
grep -A1 '^name = "ciq"$' Cargo.lock   # confirm the new version
```

## 7. Update docs (conditional)

Per the Rules section: only when the release changes a user-visible feature / shortcut / config.

## 8. Commit + push the release

```sh
git add CHANGELOG.md Cargo.toml Cargo.lock
# add any docs/features/*.md, docs/quick-reference.md, docs/configuration.md you changed
git commit -m "release vX.Y.Z"
git push origin main
```

## 9. Tag + push the tag

The tag push is what triggers the `Release` workflow. Match the existing lightweight-tag style
(`git cat-file -t v0.1.0` → `commit`):

```sh
git tag vX.Y.Z
git push origin vX.Y.Z
```

## 10. Watch the Release workflow

```sh
gh run list --repo bellicose100xp/ciq --workflow=Release --limit 1
gh run watch <RUN_ID> --repo bellicose100xp/ciq --exit-status
```

The workflow builds every target in `dist-workspace.toml` and, **only if all legs succeed**,
publishes a GitHub Release with the tarballs + `ciq-installer.sh`. cargo-dist's `host` job requires
**all** `build-local-artifacts` legs green — a single target failure blocks the Release from
publishing.

### 10a. On a target-build failure

DuckDB is bundled (compiled from C++ per target), so the less-common targets can break where the
mainstream ones don't — `x86_64-unknown-linux-musl` (static libstdc++) and `x86_64-pc-windows-msvc`
are the usual suspects. When a leg fails:

1. Read the failing job's log (`gh run view --repo bellicose100xp/ciq --job <ID> --log`, available
   once the run completes).
2. If it's a musl/Windows toolchain issue and the mainstream macOS/Linux-gnu targets are green,
   the fastest unblock is to **drop the failing target** from `dist-workspace.toml`, re-run
   `dist generate` to regenerate `.github/workflows/release.yml`, commit, and re-tag a patch. Only
   keep a target we can actually build.
3. Never delete a pushed tag to "retry" — cut a new patch tag instead.

## 11. Verify the release

```sh
gh release view vX.Y.Z --repo bellicose100xp/ciq
```

Confirm the assets include `ciq-installer.sh` and a tarball per target. The install one-liner is:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/bellicose100xp/ciq/releases/latest/download/ciq-installer.sh | sh
```

## 12. Final summary

Report to the user:
- Release commit SHA on `main`
- Tag pushed
- Release workflow outcome (published, or which target blocked it)
- The install one-liner, if the Release published
