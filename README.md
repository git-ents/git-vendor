# 🎁 `git-vendor`

*An in-source vendoring alternative to submodules and subtrees.*

<!-- rumdl-disable MD013 -->
[![CI](https://github.com/git-ents/git-vendor/actions/workflows/CI.yml/badge.svg)](https://github.com/git-ents/git-vendor/actions/workflows/CI.yml)
[![CD](https://github.com/git-ents/git-vendor/actions/workflows/CD.yml/badge.svg)](https://github.com/git-ents/git-vendor/actions/workflows/CD.yml)
<!-- rumdl-enable MD013 -->

> [!CAUTION]
> This project is being actively developed!
> Despite this, semantic versioning rules will be respected.
> Expect frequent updates.

## About

To support a more expansive usage of the Git object database — as is the goal for other projects within the [`git-ents`](https://github.com/git-ents) organization — new tooling is needed.
This project provides a command that allows users to fetch and merge remote content.

You may see the terms *porcelain* and *plumbing* used across this project.
These are [borrowed from Git itself](https://git-scm.com/book/en/v2/Git-Internals-Plumbing-and-Porcelain): porcelain refers to user-facing commands, and plumbing refers to the lower-level libraries and commands they are built on.

## Crates

| Crate | Description | API |
|---|---|----|
| [`git-vendor`](crates/git-vendor/) | An in-source vendoring alternative to submodules and subtrees. | Porcelain |
| [`git-set-attr`](crates/git-set-attr/) | Set Git attributes programmatically. | Plumbing |
