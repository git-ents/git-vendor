# ✍️ `git-set-attr`

*Set Git attributes via code, or from the command-line!*

## Motivation

Git provides a low-level *plumbing* command for reading attributes for a given path: the `check-attr` command.
Previously, no such command existed for *setting* attributes.
This makes sense.
Setting attributes is more complicated than reading attributes.
When reading attributes, multiple sources are referenced: the Git installation, user settings, repository files, and local storage settings under `.git/`.
If you want to write an attribute, which of those sources should you pick?

We can benefit from a simple `set-attr` command by limiting the problem statement: if you are writing an attribute, we assume you are writing it to be shared by other users.
The only source that is shareable to other users is in-source `.gitattributes` files.
The `git set-attr` *plumbing* command, and the accompanying `git_set_attr` library, writes attribute content to the closest `.gitattributes` file while walking up the directory tree from the current directory to the repository's root directory.

The primary application of this [project](/), `git-vendor`, uses this functionality to track metadata for vendored content as attributes.
Storing this metadata in-source allows all users to fetch and merge the latest vendored content if they so choose without limiting users by requiring extra commands after `git clone`.

## Installation

### CLI

The `git-set-attr` plumbing command can be installed with `cargo install`.

```shell
cargo install --git https://github.com/git-ents/git-vendor git-set-attr
```

If `~/.cargo/bin` is on your `PATH`, you can invoke the command with `git`.

```shell
git set-attr -h
```

### Library

The `git-set-attr` library can be added to your Rust project via `cargo add`.

```shell
cargo add --git https://github.com/git-ents/git-vendor git-set-attr
```

