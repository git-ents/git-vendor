# 📦 `git-vendor`

*An in-source vendoring alternative to Git submodules and subtrees.*

<!-- rumdl-disable MD013 -->
[![CI](https://github.com/git-ents/git-vendor/actions/workflows/CI.yml/badge.svg)](https://github.com/git-ents/git-vendor/actions/workflows/CI.yml)
[![CD](https://github.com/git-ents/git-vendor/actions/workflows/CD.yml/badge.svg)](https://github.com/git-ents/git-vendor/actions/workflows/CD.yml)
<!-- rumdl-enable MD013 -->

> [!CAUTION]
> This project is in active development.
> There are surely bugs and misbehaviors that have not yet been discovered.
> Please file a [new issue] for any misbehaviors you find!

[new issue]: https://github.com/git-ents/git-vendor/issues/new

## Overview

Git has long supported vendoring content from external repositories via submodules, and more recently subtrees.
These two options have different tradeoffs.
Submodules are easy to update for maintainers with the `update --remote` command, but require contributors to take extra steps to retrieve all repository content.
Subtrees are easy for contributors (the files are right there in the worktree) but require extra steps for maintainers: maintainers must specify the remote URL and reference each time.

The `git-vendor` project aims to bring the best of both worlds.
Vendored content is stored directly in the worktree, so the remote source of content is opaque to contributors.
Still, the source information is versioned controlled and readily available for maintainers in a new `.gitvendors` file.

## Installation

### CLI

The `git-vendor` plumbing command can be installed with `cargo install`.

```shell
cargo install --locked git-vendor
```

If `~/.cargo/bin` is on your `PATH`, you can invoke the command with `git`.

```shell
git vendor -h
```

### Library

The `git-vendor` library can be added to your Rust project via `cargo add`.

```shell
cargo add git-vendor
```
