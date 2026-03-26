# Changelog

## [1.0.0-alpha.1](https://github.com/git-ents/git-vendor/compare/git-vendor-v0.2.0...git-vendor-v1.0.0-alpha.1) (2026-03-26)


### ⚠ BREAKING CHANGES

* combine list and check into a single status command
* draft v1 specification
* VendorStatus.upstream_oid replaced by VendorStatus.state (VendorState enum)

### Features

* Add --no-commit flag to merge and pull ([6bacd29](https://github.com/git-ents/git-vendor/commit/6bacd29d9a45c296b82f9e7d7c54caf69bd0d1a0))
* Add `vendor status` command ([f912866](https://github.com/git-ents/git-vendor/commit/f91286631fd6586705f3ec87d6590c6b2d766d2a))
* Add `VendorSource::base_ref()` ([a4a545c](https://github.com/git-ents/git-vendor/commit/a4a545cd7ca440b00343fad39eabe6421d7477f1))
* Add `VendorSource::path` field and `vendor.<name>.path` config key ([5cb353c](https://github.com/git-ents/git-vendor/commit/5cb353c5bf758c0c7b3c412678b5b05ce28ac7c8))
* Add CommitMode enum (squash/linear/replay) to VendorSource ([6bacd29](https://github.com/git-ents/git-vendor/commit/6bacd29d9a45c296b82f9e7d7c54caf69bd0d1a0))
* Auto-commit on vendor add with configurable history mode ([68f4a5e](https://github.com/git-ents/git-vendor/commit/68f4a5e277d4f8eb2da38d546e2bdc765438a0c7))
* Defer base write until after successful commit or staging ([5899c16](https://github.com/git-ents/git-vendor/commit/5899c16251601abf101ff65fc6e63407c930026e))
* Enforce invariants from section 8 ([5899c16](https://github.com/git-ents/git-vendor/commit/5899c16251601abf101ff65fc6e63407c930026e))
* Error on collision with existing non-vendored files on add ([5899c16](https://github.com/git-ents/git-vendor/commit/5899c16251601abf101ff65fc6e63407c930026e))
* Error on overlapping output paths across vendors on add ([5899c16](https://github.com/git-ents/git-vendor/commit/5899c16251601abf101ff65fc6e63407c930026e))
* Extend status output with force-push detection ([6bacd29](https://github.com/git-ents/git-vendor/commit/6bacd29d9a45c296b82f9e7d7c54caf69bd0d1a0))
* Implement authorship modes for vendor merges ([60ac1f3](https://github.com/git-ents/git-vendor/commit/60ac1f37217c17ca00591c32a96b5a38e9658234))
* Linear mode creates single-parent commit on HEAD ([60ac1f3](https://github.com/git-ents/git-vendor/commit/60ac1f37217c17ca00591c32a96b5a38e9658234))
* Pub fn vendor_ref(name: &str) -&gt; String in lib.rs ([5ee296c](https://github.com/git-ents/git-vendor/commit/5ee296c5e673f6978688a1c67547aca91e5d4352))
* Replay mode replays upstream commits with original author identity ([60ac1f3](https://github.com/git-ents/git-vendor/commit/60ac1f37217c17ca00591c32a96b5a38e9658234))
* Squash mode creates synthetic second-parent + merge commit ([60ac1f3](https://github.com/git-ents/git-vendor/commit/60ac1f37217c17ca00591c32a96b5a38e9658234))
* Tests/add_vendor.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* Tests/commit_mode.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* Tests/fetch.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* Tests/merge_vendor.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* Tests/rm.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* Tests/status.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* Tests/track_vendor_pattern.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* Tests/track.rs ([baa5e16](https://github.com/git-ents/git-vendor/commit/baa5e161849390ba86e8c170c503843bb3b5c329))
* VENDOR_MSG conflict variant directs user to git commit -e -F .git/VENDOR_MSG ([60ac1f3](https://github.com/git-ents/git-vendor/commit/60ac1f37217c17ca00591c32a96b5a38e9658234))
* VENDOR_MSG includes Updated N files, added/removed/modified counts ([60ac1f3](https://github.com/git-ents/git-vendor/commit/60ac1f37217c17ca00591c32a96b5a38e9658234))
* Write .gitattributes to dest subdirectory when --path is used ([bac50e5](https://github.com/git-ents/git-vendor/commit/bac50e529a1b9537a8645ea431366a24daccf86a))
* Write .gitattributes to destination subdirectory for --path vendors ([bac50e5](https://github.com/git-ents/git-vendor/commit/bac50e529a1b9537a8645ea431366a24daccf86a))


### Bug Fixes

* Check gitattributes path relative to workdir, not CWD ([0a4f674](https://github.com/git-ents/git-vendor/commit/0a4f67472fe9331be66a5c14fa481b434ecd00ec))
* Don't introduce unattributed upstream files via merge_vendor ([dd8eb64](https://github.com/git-ents/git-vendor/commit/dd8eb6403e6e221c089ec1def536fbbedfcc9bf1))
* Hard-error on non-vendored file collision in add ([7b8fc0e](https://github.com/git-ents/git-vendor/commit/7b8fc0e509792f66ea49c48965f6a2890668fdb3))
* Merged_index filtered to attributed paths before checkout_and_stage ([dd8eb64](https://github.com/git-ents/git-vendor/commit/dd8eb6403e6e221c089ec1def536fbbedfcc9bf1))
* Refresh_vendor_attrs uses theirs_tree instead of merged_index ([dd8eb64](https://github.com/git-ents/git-vendor/commit/dd8eb6403e6e221c089ec1def536fbbedfcc9bf1))
* Remove unused imports and suppress dead code warning in tests ([8144ad3](https://github.com/git-ents/git-vendor/commit/8144ad37dd1e90d953a6586ed6f2150f8e21bce3))
* Resolve clippy warnings ([0290a68](https://github.com/git-ents/git-vendor/commit/0290a68ba784102c783b221c80cd3809d61bf97b))
* Stage refreshed .gitattributes before filtering merged index ([c2a6ec7](https://github.com/git-ents/git-vendor/commit/c2a6ec79a91409b407a5d2d667de3bc93d8fc484))
* Track_patterns writes and stages attrs for new patterns ([7dc7e14](https://github.com/git-ents/git-vendor/commit/7dc7e146b52d6bbd5f9f1dc3eec6eca64b93fd74))
* Use three-way merge in replay mode instead of bare tree replacement ([d47dba8](https://github.com/git-ents/git-vendor/commit/d47dba84da2c22bcb8716a975566491b1cab9970))
* Write vendor attributes after checkout to avoid clobbering ([c440437](https://github.com/git-ents/git-vendor/commit/c4404370dd3c39b55c11e53d2e6743d03b9e5e95))


### Code Refactoring

* Combine list and check into a single status command ([f912866](https://github.com/git-ents/git-vendor/commit/f91286631fd6586705f3ec87d6590c6b2d766d2a))
* Draft v1 specification ([9324022](https://github.com/git-ents/git-vendor/commit/932402235bbd7a9b680630da91dbf2f60a72a838))

## [0.2.0](https://github.com/git-ents/git-vendor/compare/git-vendor-v0.1.1...git-vendor-v0.2.0) (2026-03-11)


### Features

* Add --path flag as default destination prefix for all patterns ([24c9c0b](https://github.com/git-ents/git-vendor/commit/24c9c0b6254b19b6216fec3243d58d04cce7d80c))
* Add pattern-to-path mapping via colon syntax ([24c9c0b](https://github.com/git-ents/git-vendor/commit/24c9c0b6254b19b6216fec3243d58d04cce7d80c))
* Place new upstream files at computed mapped paths during merge ([24c9c0b](https://github.com/git-ents/git-vendor/commit/24c9c0b6254b19b6216fec3243d58d04cce7d80c))


### Bug Fixes

* Omit summary when only one vendor is configured ([9c9f012](https://github.com/git-ents/git-vendor/commit/9c9f012f2d2252ec0422dc718cc04071e92cb8f8)), closes [#16](https://github.com/git-ents/git-vendor/issues/16)
* Resolve --path relative to CWD within workdir ([30b78bc](https://github.com/git-ents/git-vendor/commit/30b78bcbacd801ccf0ad7c14befa57bedc7620cd)), closes [#19](https://github.com/git-ents/git-vendor/issues/19)
* Sort gitattributes entries by pattern for deterministic ordering ([8fbe454](https://github.com/git-ents/git-vendor/commit/8fbe45474f94aad87d7ad96315d9545578477981)), closes [#18](https://github.com/git-ents/git-vendor/issues/18)


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * git-set-attr bumped from 0.1.1 to 0.1.2

## [0.1.1](https://github.com/git-ents/git-vendor/compare/git-vendor-v0.1.0...git-vendor-v0.1.1) (2026-03-08)


### Documentation

* Publish with docs.rs metadata ([ca93b0a](https://github.com/git-ents/git-vendor/commit/ca93b0a53aeaa6e4cd84b6529311672d44c989db))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * git-set-attr bumped from 0.1.0 to 0.1.1

## [0.1.0](https://github.com/git-ents/git-vendor/compare/git-vendor-v0.0.2...git-vendor-v0.1.0) (2026-03-08)


### Features

* Add `pull` subcommand (fetch + merge) ([7640e17](https://github.com/git-ents/git-vendor/commit/7640e1718bd52bd30de00f08542edc3b664ce908))
* Define initial release ([a84e912](https://github.com/git-ents/git-vendor/commit/a84e912f8a5c5e06f1988444a6c8ed304a485fa1))


### Bug Fixes

* Add missing crate metadata for crates.io publish ([1079f88](https://github.com/git-ents/git-vendor/commit/1079f88102f546c8bd8e56637463f20b0cfd19cc))
* Skip merge when base already matches latest vendor ref ([42f089d](https://github.com/git-ents/git-vendor/commit/42f089d634f9dc772e20a97b35be7fab2b3d732c))
* Strip "./" prefix from gitattributes patterns and filter base tree ([0f8afa2](https://github.com/git-ents/git-vendor/commit/0f8afa26a8851fd34481e96c0aa5a4cd90e4926f))

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
