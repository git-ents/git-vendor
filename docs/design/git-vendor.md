# git-vendor Design

## Overview

git-vendor vendors upstream repository files into a local repository as regular committed files.
It replaces git submodules and git subtrees with a simpler model: vendored files are first-class citizens in the local repo, with three-way merge support for upstream updates, full rename tracking, and selective file vendoring.

## Why Not Submodules or Subtrees

Submodules are a separate repository with separate state.
They require `--recursive` on clone, break CI silently via detached HEAD, and overwrite local modifications on update.
Users cannot rename or reorganize submodule files.

Subtrees embed files as regular commits, but track nothing.
There is no record of what is vendored, from where, or at what version.
The subtree merge strategy uses heuristics to align paths, which can guess wrong.
Updates require remembering the remote and prefix every time.
History is either fully interleaved (noisy) or squashed (no attribution).

git-vendor gives you committed files (like subtrees), explicit configuration and ownership tracking (unlike either), selective file vendoring (unlike either), and rename support for vendored files (unlike either).

## Configuration

Single file: `.gitvendors`, tracked in the repository, using git-config syntax.

```ini
[vendor "lua"]
    url = https://github.com/lua/lua.git
    ref = v5.4
    base = a1b2c3d4e5f6
    pattern = src/**:third_party/lua/
    pattern = LICENSE:third_party/lua/
    authorship = replay
    license = MIT

[vendor "sqlite"]
    url = https://github.com/sqlite/sqlite.git
    ref = 3.45.0
    base = f6e5d4c3b2a1
    pattern = src/sqlite3.c:third_party/sqlite/
    pattern = src/sqlite3.h:third_party/sqlite/
```

### Fields

- `url` — upstream repository URL.
  Human-edited.
- `ref` — upstream ref to track.
  Accepts a branch name, tag, or SHA.
  Human-edited.
- `pattern` — one or more pattern mappings (see Patterns below).
  Human-edited.
- `base` — the upstream commit SHA that the current vendored state corresponds to.
  **Tool-managed.**
  **Never hand-edited.**
- `authorship` — optional.
  One of `squash` (default), `linear`, or `replay`.
  See Authorship Modes below.
- `license` — optional.
  Records the upstream license identifier for visibility and audit.
  Human-edited.

`base` is the only field the tool writes.
Everything else is human-edited.

## Patterns

Patterns define how upstream paths map to local paths on add and update.
Syntax: `<upstream-glob>:<local-prefix>`.

The glob matches files in the upstream tree.
The literal text before the colon is stripped from the matched path.
The text after the colon is prepended.

Examples:

| Pattern | Upstream path | Local path |
|---|---|---|
| `src/**:third_party/lua/` | `src/lapi.c` | `third_party/lua/lapi.c` |
| `src/**:third_party/lua/` | `src/core/vm.c` | `third_party/lua/core/vm.c` |
| `LICENSE:third_party/lua/` | `LICENSE` | `third_party/lua/LICENSE` |
| `*:lib/` | `foo.c` | `lib/foo.c` |

If no colon is present, the CLI `path` argument is used as the local prefix.

A default pattern of `*:<path>/` vendors the entire upstream repo into a single directory.
This is the simple case and should not require explicit pattern configuration.

Unmatched upstream files are excluded.

The prefix before the colon must be literal (not a glob).
Only the first segment is a glob.

**Patterns are only used for upstream-to-local remapping when building trees from upstream.**
They define where new files land.
They do not track files after placement.
If a user does `git mv` on a vendored file, patterns are not involved — rename detection and attributes handle it.

## Three Separate Mechanisms

The design relies on three mechanisms with no overlap:

1. **Patterns** — remap upstream paths to local paths.
   Used when building base and theirs trees from upstream commits.
   Forward direction only.
2. **Attributes** — track which local files are owned by which vendor.
   Survive renames.
   Authoritative ownership record.
3. **Rename detection** — git's built-in rename detection in `merge_trees` handles files that moved locally since the last vendor update.
   Upstream changes follow the rename automatically.

## Ownership Tracking

Each vendored file gets a `vendor=<n>` entry in `.gitattributes`.
This is the authoritative record of which local files belong to which vendor.

```text
third_party/lua/lapi.c vendor=lua
third_party/lua/lapi.h vendor=lua
third_party/sqlite/sqlite3.c vendor=sqlite
```

### Why Attributes

- Attributes follow files through renames. `git check-attr` gives ownership at any path, regardless of how files moved since they were placed.
- `git log -p -- .gitattributes` and `git blame .gitattributes` show when ownership changed and in which commit.
  Full audit trail.
- After initial placement, users can `git mv` vendored files freely.
  Ownership survives because attributes travel with the file.

### Maintenance

A `refresh_vendor_attrs` operation rewrites the `vendor=` lines in `.gitattributes` after every add/update.
It is surgical: only `vendor=` lines are touched; all other attribute lines are preserved.
`.gitattributes` is committed atomically with the vendored file changes.

## Refs

- `refs/vendor/<n>/head` — latest fetched upstream tip.
- `refs/vendor/<n>/base` — the upstream commit corresponding to the current vendored state (matches `base` in `.gitvendors`).

Both are local-only, never pushed.
They are cache.
The `base` value in `.gitvendors` is the source of truth and can be used to refetch if refs are lost.

## Tree Pipeline

Building a remapped tree from upstream is a two-step pipeline:

1. **git-filter-tree** — filter the upstream tree by glob, attribute, or user-provided predicate.
   Returns a filtered tree containing only the selected files.
2. **git-vendor remap** — apply pattern path transformations to the filtered tree (strip upstream prefix, prepend local prefix).
   Returns a remapped tree with local paths.

The remap step lives in git-vendor, not in git-filter-tree. git-filter-tree is a general-purpose tree filtering library.
Path remapping is vendor-specific logic.

The remapped tree is then fed into `merge_trees` for three-way merge.

## Operations

### fetch

1. Resolve `ref` from `.gitvendors`.
2. Fetch from `url` into `refs/vendor/<n>/head`.

### add

First-time vendoring of an upstream dependency.

1. Fetch.
2. Read tree at `refs/vendor/<n>/head`.
3. Filter tree via git-filter-tree using configured glob patterns.
4. Remap filtered tree paths using pattern prefix transformations.
5. Merge remapped tree into HEAD with an empty ancestor tree as the base.
   Any pre-existing file at a destination path surfaces as an add/add conflict.
6. If any output path collides with an existing non-vendored file, error before proceeding.
7. Write `vendor=<n>` attributes for all vendored files via `refresh_vendor_attrs`.
8. Set `base` in `.gitvendors` to the fetched upstream commit.
9. Create a single commit containing: vendored files, `.gitvendors`, and `.gitattributes`.

### update

Update a previously vendored dependency to a newer upstream state.

1. Fetch.
2. Build three trees:
   - **base** — filter and remap upstream tree at the recorded `base` commit.
   - **ours** — full HEAD tree.
     Not filtered.
     Rename detection handles files that moved from their original vendored location.
   - **theirs** — filter and remap upstream tree at `refs/vendor/<n>/head`.
3. Three-way merge via `merge_trees`: base/ours/theirs.
   - Git's rename detection applies.
     If a user did `git mv` on a vendored file, upstream changes to that file follow the rename.
   - Non-vendored files in HEAD pass through untouched because base and theirs only contain vendored paths — the diff is already scoped.
4. On success:
   - Update `vendor=` attributes via `refresh_vendor_attrs` (adds new files, removes deleted files).
   - Update `base` in `.gitvendors` to the new upstream commit.
   - Commit according to the authorship mode (see Authorship Modes below).

### update --no-commit

Perform the merge and stage the result, but do not commit.
Write the commit message (including author trailers) to `.git/VENDOR_MSG`.
The user can then:

- `git commit -F .git/VENDOR_MSG` to commit with the generated message.
- `git commit` to write their own message, using `.git/VENDOR_MSG` as reference.
- Restructure, amend, or cherry-pick before committing.

This gives full control over the final commit(s) while still getting the merge result and attribution data from the tool.

`--no-commit` works with `linear` and `squash` modes.
It does not apply to `replay`, which creates commits as part of its core operation.

Note: `base` in `.gitvendors` is staged but not committed.
It updates only when the user finalizes the commit.
If the user aborts (`git reset`), `base` reverts to its previous value.

### check

Report which vendors have unmerged upstream changes.

1. For each vendor, compare recorded `base` against `refs/vendor/<n>/head`.
2. Same OID: up to date.
3. `base` is an ancestor of head: update available.
4. `base` is not an ancestor of head: upstream force-pushed.
   Warn loudly.

### check --licenses

Scan vendored directories for the presence of license files.
Report any vendor missing one.
This is audit tooling, not enforcement — it does not block operations.

## Authorship Modes

Three modes, configured per vendor via the `authorship` field.
Increasing traceability from linear to squash to replay.

### linear

Single-parent commit.
The commit message includes author trailers summarizing who contributed to the upstream changes:

```text
Vendor update: lua v5.4 (a1b2c3d..f6e5d4c)

Upstream-Author: Roberto Ierusalimschy <roberto@lua.org> (14 commits)
Upstream-Author: Luiz Henrique de Figueiredo <lhf@lua.org> (3 commits)
```

Clean, linear history.
No DAG link to upstream.
`base` in `.gitvendors` is the only structural connection to the upstream commit.

### squash (default)

Merge commit with two parents.
First parent is HEAD.
Second parent is a synthetic squash commit whose tree is the remapped upstream tree and whose message summarizes the upstream log from `base..head`.
The commit message also includes author trailers.

The upstream history is not pulled into the local DAG, but the merge structure is visible in `git log --merges` and the synthetic squash commit is reachable by walking parents.
This matches the model used by `git subtree` with `--squash`.

### replay

Replay each upstream commit from `base..head` as a separate synthetic commit with original authorship.

1. Walk upstream commits from `base..refs/vendor/<n>/head`.
2. For each commit, filter and remap the upstream tree.
3. Create a synthetic commit with:
   - Original `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_AUTHOR_DATE` from the upstream commit.
   - Committer set to the local user.
   - Tree is the remapped result.
   - Parent is the previous synthetic commit (or current HEAD for the first).
4. The final tree is identical to what squash or linear would produce.

This makes `git blame` on vendored files show the original upstream authors.
Most expensive option — use when blame attribution matters.

`--no-commit` does not apply to replay.

## .git/VENDOR_MSG

Written by `--no-commit` and available for any workflow that needs the generated commit message or trailers without the tool committing.

Format:

```text
Vendor update: lua v5.4 (a1b2c3d..f6e5d4c)

Updated 17 files. 3 added, 2 removed, 12 modified.

Upstream-Author: Roberto Ierusalimschy <roberto@lua.org> (14 commits)
Upstream-Author: Luiz Henrique de Figueiredo <lhf@lua.org> (3 commits)
```

The file is overwritten on each `update --no-commit`.
It is not tracked in the repository.
It is deleted on a successful `git vendor commit` or can be manually removed.

## Licensing

git-vendor provides visibility into upstream licenses without interpreting or enforcing them.

- `license` field in `.gitvendors` records the upstream license identifier per vendor.
- `check --licenses` scans vendored directories for the presence of license files (LICENSE, COPYING, etc.) and reports any vendor missing one.
- If a vendor's upstream repo has a license file at the root and no pattern includes it, warn on add.
  Do not block — some licenses don't require the file to travel — but make the user acknowledge it.
- Optional `notice` command generates a `NOTICE` or `THIRD_PARTY_LICENSES` file aggregating license info from all vendors.
  For compliance in large projects.

## Invariants

1. `base` in `.gitvendors` is updated only after a successful commit.
   If the process is interrupted or the user aborts after `--no-commit`, `base` reverts to its previous value.
2. `.gitvendors`, `.gitattributes`, and vendored file changes are always in the same commit.
   No partial state.
3. Overlapping output paths across vendors are a hard error.
   Two vendors cannot claim the same local path.
4. Collision with existing non-vendored files on add is a hard error.
   The user must resolve the conflict explicitly.
5. `.gitattributes` is the sole ownership authority.
   No glob fallback.
   If attributes are missing or inconsistent, operations error before proceeding.

## Not In Scope

- Subtree extraction (`vendored_subtree`).
  Remove or do not expose until implemented.
- Partial updates (vendoring a subset of a vendor's files in a single operation).
- Contribute-back workflow (generating patches from local modifications against upstream).
  The inverse remap operation (local paths to upstream paths) supports this conceptually, but the CLI command is not yet defined.
  Note: contribute-back for renamed files requires resolving the current local path back to the original upstream path, which attributes alone cannot do — this needs further design work.
