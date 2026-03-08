# Changelog

## [1.0.0](https://github.com/git-ents/git-vendor/compare/git-vendor-v0.1.0...git-vendor-v1.0.0) (2026-03-08)


### ⚠ BREAKING CHANGES

* add `rm` and `prune` commands, rename `check` to `status`
* remove merge commits from `add` and `merge` operations

### Features

* `add` returns the updated `VendorSource` with `base` set ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))
* Add `git vendor prune` to clean up orphaned `refs/vendor/*` refs ([f1f9b03](https://github.com/git-ents/git-vendor/commit/f1f9b03ce83dcbe0cb52b1f7975569d753c92b64))
* Add `git vendor rm` to remove a vendor and its refs/attributes ([f1f9b03](https://github.com/git-ents/git-vendor/commit/f1f9b03ce83dcbe0cb52b1f7975569d753c92b64))
* Add `pull` subcommand (fetch + merge) ([7640e17](https://github.com/git-ents/git-vendor/commit/7640e1718bd52bd30de00f08542edc3b664ce908))
* Add `rm` and `prune` commands, rename `check` to `status` ([f1f9b03](https://github.com/git-ents/git-vendor/commit/f1f9b03ce83dcbe0cb52b1f7975569d753c92b64))
* Add CLI application content ([0b381a8](https://github.com/git-ents/git-vendor/commit/0b381a865a59d0552d8ae5814571dad683dd842d))
* Add CLI, executor, and library separations ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))
* Add executor library ([0b381a8](https://github.com/git-ents/git-vendor/commit/0b381a865a59d0552d8ae5814571dad683dd842d))
* Add merge functionality via `vendor_merge` trait implementation ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))
* Add status checks for vendors ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))
* Add track and untrack commands ([c937754](https://github.com/git-ents/git-vendor/commit/c93775445690076588b00d909867cc4eded1c7d2))
* Add_vendor with two-way merge, conflict detection, and MergeOutcome return ([fa197c1](https://github.com/git-ents/git-vendor/commit/fa197c1d69a1bb572a2ddb0e9bd90f3c86ecfeb8))
* Define initial release ([a84e912](https://github.com/git-ents/git-vendor/commit/a84e912f8a5c5e06f1988444a6c8ed304a485fa1))
* Expand globs to per-file gitattributes entries by default ([885f2d9](https://github.com/git-ents/git-vendor/commit/885f2d927a6dfd39868b3ae9761aa2ff9653c12a))
* Implement fetch and reference retrieval trait methods ([08f7d2d](https://github.com/git-ents/git-vendor/commit/08f7d2dabc2ca33b71ddb181094b4d907c422a30))
* Implement vendor tracking and merging in library ([b8213b4](https://github.com/git-ents/git-vendor/commit/b8213b43b9c621a7892e3992ff8ef94b75c2ce17))
* Layer vendor config like git config ([443903a](https://github.com/git-ents/git-vendor/commit/443903a877e4a3adac7bdd33245c78362e738a22))
* Perform initial merge on `add` ([238649b](https://github.com/git-ents/git-vendor/commit/238649b5798deaf78a39e4fb06342bb1ac08e918))
* Persist vendor patterns in .gitvendors ([ac73b92](https://github.com/git-ents/git-vendor/commit/ac73b9243b01ad946adc83356c0a67780e110d74))
* Support multiple patterns in `vendor add` ([7021db4](https://github.com/git-ents/git-vendor/commit/7021db49af939d3cb5e0a9ecf5e525bc3888614d))
* Use stored patterns to filter upstream in merge ([330a772](https://github.com/git-ents/git-vendor/commit/330a7722e9d6d71725b40ce6c8c320f08a2cadbe))


### Bug Fixes

* `merge_vendor` always updates `base` in `.gitvendors` to the upstream tip, even when conflicts are detected ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))
* Add missing crate metadata for crates.io publish ([1079f88](https://github.com/git-ents/git-vendor/commit/1079f88102f546c8bd8e56637463f20b0cfd19cc))
* Bump git-filter-tree to 0.3.1 for forward-slash predicate paths ([b6cfdc4](https://github.com/git-ents/git-vendor/commit/b6cfdc4ed1fe96b5edb0588c40bd99480040b95a))
* Include glob directory in gitattributes pattern ([81614d6](https://github.com/git-ents/git-vendor/commit/81614d6a720ddeacfd273f5326975eff8ca3c987))
* Move `.gitvendors` to repository root as intended ([1b159ac](https://github.com/git-ents/git-vendor/commit/1b159ac1e62e67c15d6899ddd1b88d05fe2bb68e))
* Normalize git tree paths to forward slashes on Windows ([b6cfdc4](https://github.com/git-ents/git-vendor/commit/b6cfdc4ed1fe96b5edb0588c40bd99480040b95a))
* Omit vendor-prefix attribute when prefix is empty ([2b0bc44](https://github.com/git-ents/git-vendor/commit/2b0bc44f469a5bd1c477c2f16e2e719a91079ccb))
* Perform two-reference merge on initial add ([fa197c1](https://github.com/git-ents/git-vendor/commit/fa197c1d69a1bb572a2ddb0e9bd90f3c86ecfeb8))
* Remove vendor-prefix gitattribute ([19ca16b](https://github.com/git-ents/git-vendor/commit/19ca16b6654a012f2f64d0687cc15a6fa4440525))
* Resolve temporary path failure on Windows ([5f05f13](https://github.com/git-ents/git-vendor/commit/5f05f1355e47c4f861be4fc584e3ed958ade567d))
* Skip merge when base already matches latest vendor ref ([42f089d](https://github.com/git-ents/git-vendor/commit/42f089d634f9dc772e20a97b35be7fab2b3d732c))
* Strip "./" prefix from gitattributes patterns and filter base tree ([0f8afa2](https://github.com/git-ents/git-vendor/commit/0f8afa26a8851fd34481e96c0aa5a4cd90e4926f))
* Write vendor/** pattern for directory globs ([6cfdc16](https://github.com/git-ents/git-vendor/commit/6cfdc16220f431c027f1ae5e30474f8f21d0fd2e))


### Code Refactoring

* Remove merge commits from `add` and `merge` operations ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * git-set-attr bumped from 0.1.0 to 0.2.0

## [0.1.0](https://github.com/git-ents/git-vendor/compare/git-vendor-v0.0.2...git-vendor-v0.1.0) (2026-03-08)


### Features

* Add `pull` subcommand (fetch + merge) ([7640e17](https://github.com/git-ents/git-vendor/commit/7640e1718bd52bd30de00f08542edc3b664ce908))
* Define initial release ([a84e912](https://github.com/git-ents/git-vendor/commit/a84e912f8a5c5e06f1988444a6c8ed304a485fa1))


### Bug Fixes

* Skip merge when base already matches latest vendor ref ([42f089d](https://github.com/git-ents/git-vendor/commit/42f089d634f9dc772e20a97b35be7fab2b3d732c))
* Strip "./" prefix from gitattributes patterns and filter base tree ([0f8afa2](https://github.com/git-ents/git-vendor/commit/0f8afa26a8851fd34481e96c0aa5a4cd90e4926f))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * git-set-attr bumped from 0.0.1 to 0.1.0

## [0.0.2](https://github.com/git-ents/git-vendor/compare/v0.0.1...v0.0.2) (2026-03-08)


### ⚠ BREAKING CHANGES

* add `rm` and `prune` commands, rename `check` to `status`
* remove merge commits from `add` and `merge` operations

### Features

* `add` returns the updated `VendorSource` with `base` set ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))
* Add `git vendor prune` to clean up orphaned `refs/vendor/*` refs ([f1f9b03](https://github.com/git-ents/git-vendor/commit/f1f9b03ce83dcbe0cb52b1f7975569d753c92b64))
* Add `git vendor rm` to remove a vendor and its refs/attributes ([f1f9b03](https://github.com/git-ents/git-vendor/commit/f1f9b03ce83dcbe0cb52b1f7975569d753c92b64))
* Add `rm` and `prune` commands, rename `check` to `status` ([f1f9b03](https://github.com/git-ents/git-vendor/commit/f1f9b03ce83dcbe0cb52b1f7975569d753c92b64))
* Add CLI application content ([0b381a8](https://github.com/git-ents/git-vendor/commit/0b381a865a59d0552d8ae5814571dad683dd842d))
* Add executor library ([0b381a8](https://github.com/git-ents/git-vendor/commit/0b381a865a59d0552d8ae5814571dad683dd842d))
* Add track and untrack commands ([c937754](https://github.com/git-ents/git-vendor/commit/c93775445690076588b00d909867cc4eded1c7d2))
* Add_vendor with two-way merge, conflict detection, and MergeOutcome return ([fa197c1](https://github.com/git-ents/git-vendor/commit/fa197c1d69a1bb572a2ddb0e9bd90f3c86ecfeb8))
* Expand globs to per-file gitattributes entries by default ([885f2d9](https://github.com/git-ents/git-vendor/commit/885f2d927a6dfd39868b3ae9761aa2ff9653c12a))
* Implement vendor tracking and merging in library ([b8213b4](https://github.com/git-ents/git-vendor/commit/b8213b43b9c621a7892e3992ff8ef94b75c2ce17))
* Layer vendor config like git config ([443903a](https://github.com/git-ents/git-vendor/commit/443903a877e4a3adac7bdd33245c78362e738a22))
* Perform initial merge on `add` ([238649b](https://github.com/git-ents/git-vendor/commit/238649b5798deaf78a39e4fb06342bb1ac08e918))
* Persist vendor patterns in .gitvendors ([ac73b92](https://github.com/git-ents/git-vendor/commit/ac73b9243b01ad946adc83356c0a67780e110d74))
* Support multiple patterns in `vendor add` ([7021db4](https://github.com/git-ents/git-vendor/commit/7021db49af939d3cb5e0a9ecf5e525bc3888614d))
* Use stored patterns to filter upstream in merge ([330a772](https://github.com/git-ents/git-vendor/commit/330a7722e9d6d71725b40ce6c8c320f08a2cadbe))


### Bug Fixes

* `merge_vendor` always updates `base` in `.gitvendors` to the upstream tip, even when conflicts are detected ([862b340](https://github.com/git-ents/git-vendor/commit/862b340bca5b50cc1cec88ef1dd21da5b5f8671e))
* Bump git-filter-tree to 0.3.1 for forward-slash predicate paths ([b6cfdc4](https://github.com/git-ents/git-vendor/commit/b6cfdc4ed1fe96b5edb0588c40bd99480040b95a))
* Include glob directory in gitattributes pattern ([81614d6](https://github.com/git-ents/git-vendor/commit/81614d6a720ddeacfd273f5326975eff8ca3c987))
* Move `.gitvendors` to repository root as intended ([1b159ac](https://github.com/git-ents/git-vendor/commit/1b159ac1e62e67c15d6899ddd1b88d05fe2bb68e))
* Normalize git tree paths to forward slashes on Windows ([b6cfdc4](https://github.com/git-ents/git-vendor/commit/b6cfdc4ed1fe96b5edb0588c40bd99480040b95a))
* Omit vendor-prefix attribute when prefix is empty ([2b0bc44](https://github.com/git-ents/git-vendor/commit/2b0bc44f469a5bd1c477c2f16e2e719a91079ccb))
* Perform two-reference merge on initial add ([fa197c1](https://github.com/git-ents/git-vendor/commit/fa197c1d69a1bb572a2ddb0e9bd90f3c86ecfeb8))
* Remove vendor-prefix gitattribute ([19ca16b](https://github.com/git-ents/git-vendor/commit/19ca16b6654a012f2f64d0687cc15a6fa4440525))
* Write vendor/** pattern for directory globs ([6cfdc16](https://github.com/git-ents/git-vendor/commit/6cfdc16220f431c027f1ae5e30474f8f21d0fd2e))


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
