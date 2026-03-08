# Changelog

## [1.0.0](https://github.com/git-ents/git-vendor/compare/v0.0.1...v1.0.0) (2026-03-08)


### ⚠ BREAKING CHANGES

* remove merge commits from `add` and `merge` operations

### Features

* `add` returns the updated `VendorSource` with `base` set ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))
* Add CLI application content ([0b381a8](https://github.com/git-ents/git-vendor/commit/0b381a865a59d0552d8ae5814571dad683dd842d))
* Add executor library ([0b381a8](https://github.com/git-ents/git-vendor/commit/0b381a865a59d0552d8ae5814571dad683dd842d))
* Add_vendor with two-way merge, conflict detection, and MergeOutcome return ([fa197c1](https://github.com/git-ents/git-vendor/commit/fa197c1d69a1bb572a2ddb0e9bd90f3c86ecfeb8))
* Implement vendor tracking and merging in library ([b8213b4](https://github.com/git-ents/git-vendor/commit/b8213b43b9c621a7892e3992ff8ef94b75c2ce17))
* Perform initial merge on `add` ([238649b](https://github.com/git-ents/git-vendor/commit/238649b5798deaf78a39e4fb06342bb1ac08e918))


### Bug Fixes

* `merge_vendor` always updates `base` in `.gitvendors` to the upstream tip, even when conflicts are detected ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))
* Move `.gitvendors` to repository root as intended ([1b159ac](https://github.com/git-ents/git-vendor/commit/1b159ac1e62e67c15d6899ddd1b88d05fe2bb68e))
* Perform two-reference merge on initial add ([fa197c1](https://github.com/git-ents/git-vendor/commit/fa197c1d69a1bb572a2ddb0e9bd90f3c86ecfeb8))


### Code Refactoring

* Remove merge commits from `add` and `merge` operations ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))

## 0.0.1 (2026-03-06)


### Features

* Add CLI, executor, and library separations ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))
* Add merge functionality via `vendor_merge` trait implementation ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))
* Add status checks for vendors ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))
* Implement fetch and reference retrieval trait methods ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))


### Bug Fixes

* Resolve temporary path failure on Windows ([5f05f13](https://github.com/git-ents/git-vendor/commit/5f05f1355e47c4f861be4fc584e3ed958ade567d))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * git-set-attr bumped from 0.0.0 to 0.0.1
