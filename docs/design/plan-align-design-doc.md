# Plan: Align Implementation with Design Doc

Source of truth: `docs/design/git-vendor.md`.

## Gap Analysis

| Design | Current | Action |
|---|---|---|
| `ref` field in `.gitvendors` | `branch` field | Rename |
| `history` field in `.gitvendors` | `commit` field | Rename |
| `update` command | `merge` command | Rename |
| `check` command | `status` command | Rename |
| `check --licenses` | not implemented | Add |
| `--no-commit` writes `.git/VENDOR_MSG` | unverified | Verify / add |
| `subtree` extraction | unknown | Find and remove or hide |

Commands not in the design (`list`, `rm`, `track`, `untrack`, `pull`, `prune`) are not listed as "not in scope" — keep them.

---

## Phase 1 — Config field renames

### 1a. `branch` → `ref`

- `lib.rs`: rename `VendorSource.branch` → `ref_name` (`ref` is a Rust keyword)
- Config read: accept `ref` key; stop accepting `branch`
- Config write: emit `ref` key
- Update all callsites

### 1b. `commit` → `history`

- `lib.rs`: rename `VendorSource.commit` → `VendorSource.history`
- Rename type `CommitMode` → `History`
- Config read: accept `history` key; stop accepting `commit`
- Config write: emit `history` key
- Update all callsites and tests

---

## Phase 2 — Command renames

### 2a. `merge` → `update`

- `cli.rs`: rename subcommand `merge` → `update`
- `exe.rs`: rename handler and internal functions accordingly
- `pull` internally calls merge — update that call site
- Update help text to match design language

### 2b. `status` → `check`

- `cli.rs`: rename subcommand `status` → `check`
- `exe.rs`: rename handler function
- Verify output matches design's three states:
  - Same OID → "up to date"
  - `base` is ancestor of head → "update available"
  - `base` is not an ancestor → "upstream force-pushed" (loud warning)

---

## Phase 3 — `--no-commit` and `.git/VENDOR_MSG`

Design specifies that `update --no-commit`:

- Stages all changes (vendored files, `.gitvendors`, `.gitattributes`)
- Writes commit message including `Upstream-Author` trailers to `.git/VENDOR_MSG`
- Does **not** commit; `base` in `.gitvendors` is staged but not committed
- If user runs `git reset`, `base` reverts

Verify the current `--no-commit` path does all of this.
If `.git/VENDOR_MSG` is not written, add it.

`--no-commit` does not apply to `replay` history mode — verify this is enforced or error clearly.

---

## Phase 4 — `check --licenses`

Add `--licenses` flag to the `check` subcommand.

Behavior:

- For each vendor, resolve its local vendored paths from `.gitattributes`
- Scan those paths for a license file: `LICENSE`, `LICENSE.*`, `COPYING`, `COPYING.*`, `LICENSE`, `LICENSE.*`
- Report any vendor with no license file present
- Audit only — no blocking, no hard errors

Also: on `add`, if the upstream tree root contains a license file and no configured pattern would include it, emit a warning (do not block).

---

## Phase 5 — `add` collision invariant

Design invariant 4: collision with an existing non-vendored file on `add` is a hard error.

Verify `add_vendor()`:

- Errors before any write if any output path collides with a file not owned by another vendor
- The user must resolve explicitly; the tool does not overwrite silently

---

## Phase 6 — Remove out-of-scope items

Search for any `subtree`, `vendored_subtree`, or contribute-back command/function.
Remove or leave unexposed (no CLI entry point, no public API).

---

## Suggested commit sequence

```text
refactor: rename branch→ref, commit→history in VendorSource and config
refactor: rename merge subcommand → update, status → check
feat: write .git/VENDOR_MSG on --no-commit
feat: add check --licenses
fix: hard-error on non-vendored file collision in add
chore: remove or hide out-of-scope subtree extraction
```

Each commit should leave tests passing.
