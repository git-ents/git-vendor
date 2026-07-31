# Changelog

## [1.0.0-alpha.1](https://github.com/git-ents/git-vendor/compare/git-vendor-v1.0.0-alpha.1...git-vendor-v1.0.0-alpha.1) (2026-07-31)


### Features

* `add` stages vendored files and sets MERGE_HEAD rather than auto-committing; user runs `git commit` to finalize ([4d0b229](https://github.com/git-ents/git-vendor/commit/4d0b22955f1ff735cf03f3c5ad10d9e539086e57))
* Add --dry-run to add and update ([03d8deb](https://github.com/git-ents/git-vendor/commit/03d8deba67e90e454d5b810e4389685a5d1c1dbc))
* Add `ConflictStages` carrying `[base, ours, theirs]` stage versions ([0993c27](https://github.com/git-ents/git-vendor/commit/0993c27e42e976f10921cd7c8acea678a5e26077))
* Add `Error::Conflict` ([395b29c](https://github.com/git-ents/git-vendor/commit/395b29c42ceee1d136bb0348bc2468161767a5b6))
* Add `Error::InvalidName` ([e535fcf](https://github.com/git-ents/git-vendor/commit/e535fcfc62f9cf2a6e64c7ed0e7d2ee1a47e2eba))
* Add `Error::InvalidUrl` and `Error::Fetch` variants ([b9d4407](https://github.com/git-ents/git-vendor/commit/b9d4407e3bce2b0373dd43958d477bcc6a656ecd))
* Add `From<gix::repository::merge_base::Error>` to `Error` ([73df0c5](https://github.com/git-ents/git-vendor/commit/73df0c558ae7674d6b78658688523b207e1599e5))
* Add `From<merge_trees::Error>` and `From<tree_merge_options::Error>` to `Error` ([31d98a9](https://github.com/git-ents/git-vendor/commit/31d98a9254c03cd5f629ec306383055811621db8))
* Add `mode` config key with `squash` integration strategy ([b6d305e](https://github.com/git-ents/git-vendor/commit/b6d305e300e26dcd818985cf0dd4dee19da92c0f))
* Add `vendor_overlay` to `VendorRepository` ([4799cb0](https://github.com/git-ents/git-vendor/commit/4799cb0c601a2d8f3d629d9952aabbd9e9c1dc78))
* Add `VendorMode` enum replacing `VendorEntry::squash` ([b6d305e](https://github.com/git-ents/git-vendor/commit/b6d305e300e26dcd818985cf0dd4dee19da92c0f))
* Add `VendorName` validated newtype ([e535fcf](https://github.com/git-ents/git-vendor/commit/e535fcfc62f9cf2a6e64c7ed0e7d2ee1a47e2eba))
* Add `VendorWorktree::checkout_vendor_conflicted` ([395b29c](https://github.com/git-ents/git-vendor/commit/395b29c42ceee1d136bb0348bc2468161767a5b6))
* Add apply subcommand ([d4e7285](https://github.com/git-ents/git-vendor/commit/d4e7285d580becc91ebc4001f51b4df5243d2398))
* Add CLI subcommands add, update, status, remove, list ([f2b6e46](https://github.com/git-ents/git-vendor/commit/f2b6e46ea3ecd21e3d0d10dc98d8460aecf21068))
* Add committer, author, and message params to `commit_vendor` ([cc46448](https://github.com/git-ents/git-vendor/commit/cc464483f24e0e5f5646b16d6881e5ac1f170e5d))
* Add DepotRepository trait and VendorEntry ([24f21a7](https://github.com/git-ents/git-vendor/commit/24f21a7a3bdb455a6f6a9cc0b9157d38be3bd0a6))
* Add Error type with thiserror and impl_gix_from! macro ([24f21a7](https://github.com/git-ents/git-vendor/commit/24f21a7a3bdb455a6f6a9cc0b9157d38be3bd0a6))
* Add Error::InvalidPath ([f2b6e46](https://github.com/git-ents/git-vendor/commit/f2b6e46ea3ecd21e3d0d10dc98d8460aecf21068))
* Add exe module with VendorWorktree trait (checkout/track/untrack/sync) ([1da7741](https://github.com/git-ents/git-vendor/commit/1da774161074166a9f06e9d52953691ee33640c8))
* Add explicit `ours` commit parameter to `VendorRepository::vendor_paths` ([a5c94e6](https://github.com/git-ents/git-vendor/commit/a5c94e6a117100120d6f860fe733537dea8a05db))
* Add gix index-from-tree and attribute-stack error conversions to `Error` ([098de11](https://github.com/git-ents/git-vendor/commit/098de11cfdc52699c98d8ed679f9743303fb88d0))
* Add gix tree-editor and traverse error conversions to `Error` ([f7fe174](https://github.com/git-ents/git-vendor/commit/f7fe174c07c846afe03055165df6e4c6e2b6484e))
* Add paths to vendor content filter in working-copy .gitattributes ([83f7038](https://github.com/git-ents/git-vendor/commit/83f7038690fa96a027b954793ffd79ac2a0016cf))
* Add PatternMapping, VendorStatus, VendorMerge, VendorConfig types ([1da7741](https://github.com/git-ents/git-vendor/commit/1da774161074166a9f06e9d52953691ee33640c8))
* Add prepare_merge to VendorWorktree trait ([f2b6e46](https://github.com/git-ents/git-vendor/commit/f2b6e46ea3ecd21e3d0d10dc98d8460aecf21068))
* Add VendorConfig::open_from_bytes ([33cd3ba](https://github.com/git-ents/git-vendor/commit/33cd3ba7fc61ae461d653a1913e2006a1e4ddab4))
* Add visible aliases rm and ls ([03d8deb](https://github.com/git-ents/git-vendor/commit/03d8deba67e90e454d5b810e4389685a5d1c1dbc))
* Carry merge-stage versions on `VendorMerge` conflicts ([0993c27](https://github.com/git-ents/git-vendor/commit/0993c27e42e976f10921cd7c8acea678a5e26077))
* Checkout_vendor returns full overlay tree OID ([f2b6e46](https://github.com/git-ents/git-vendor/commit/f2b6e46ea3ecd21e3d0d10dc98d8460aecf21068))
* Default vendored files to vendor/&lt;name&gt;/ when no patterns given ([03d8deb](https://github.com/git-ents/git-vendor/commit/03d8deba67e90e454d5b810e4389685a5d1c1dbc))
* Define DepotRepository trait and Error type ([24f21a7](https://github.com/git-ents/git-vendor/commit/24f21a7a3bdb455a6f6a9cc0b9157d38be3bd0a6))
* Derive `Error::Config` for invalid vendor glob patterns ([f7fe174](https://github.com/git-ents/git-vendor/commit/f7fe174c07c846afe03055165df6e4c6e2b6484e))
* Enable gix `blocking-network-client` feature ([b9d4407](https://github.com/git-ents/git-vendor/commit/b9d4407e3bce2b0373dd43958d477bcc6a656ecd))
* Enable gix `worktree-mutation` feature ([5bb39fb](https://github.com/git-ents/git-vendor/commit/5bb39fb45ed2d3303aee3cb4362240ec1d8c2cd2))
* Enforce vendor name invariant via validated `VendorName` newtype ([e535fcf](https://github.com/git-ents/git-vendor/commit/e535fcfc62f9cf2a6e64c7ed0e7d2ee1a47e2eba))
* Filter and remap upstream blobs into local path space in `upstream_tree` ([f7fe174](https://github.com/git-ents/git-vendor/commit/f7fe174c07c846afe03055165df6e4c6e2b6484e))
* Friendlier CLI UX ([03d8deb](https://github.com/git-ents/git-vendor/commit/03d8deba67e90e454d5b810e4389685a5d1c1dbc))
* Implement `base_tree` as `upstream_tree` at the recorded base ([e1c0593](https://github.com/git-ents/git-vendor/commit/e1c05931bb2b1d6314f4101ef4fcdc7d12e7c161))
* Implement `checkout_vendor_conflicted` ([951eb6b](https://github.com/git-ents/git-vendor/commit/951eb6bde8f057a39b082a4ece7cd937ebb15ce8))
* Implement `checkout_vendor` working-copy projection ([5bb39fb](https://github.com/git-ents/git-vendor/commit/5bb39fb45ed2d3303aee3cb4362240ec1d8c2cd2))
* Implement `commit_vendor` ([b8da2b3](https://github.com/git-ents/git-vendor/commit/b8da2b3c32fc9fbaa13a2f6096d2a047e8a1f049))
* Implement `commit_vendor` using `gix::objs::Commit` and `write_object` ([b8da2b3](https://github.com/git-ents/git-vendor/commit/b8da2b3c32fc9fbaa13a2f6096d2a047e8a1f049))
* Implement `fetch_vendor` for `gix::Repository` ([b9d4407](https://github.com/git-ents/git-vendor/commit/b9d4407e3bce2b0373dd43958d477bcc6a656ecd))
* Implement `fetch_vendor` with anonymous remote fetch and ref projection ([b9d4407](https://github.com/git-ents/git-vendor/commit/b9d4407e3bce2b0373dd43958d477bcc6a656ecd))
* Implement `merge_vendor` in `VendorRepository for gix::Repository` ([31d98a9](https://github.com/git-ents/git-vendor/commit/31d98a9254c03cd5f629ec306383055811621db8))
* Implement `merge_vendor` three-way merge in local path space ([31d98a9](https://github.com/git-ents/git-vendor/commit/31d98a9254c03cd5f629ec306383055811621db8))
* Implement `ours_tree` attribute-filtered local snapshot ([098de11](https://github.com/git-ents/git-vendor/commit/098de11cfdc52699c98d8ed679f9743303fb88d0))
* Implement `track_vendor` ([83f7038](https://github.com/git-ents/git-vendor/commit/83f7038690fa96a027b954793ffd79ac2a0016cf))
* Implement `untrack_vendor` ([c760880](https://github.com/git-ents/git-vendor/commit/c760880acc1d962b6b303bc71714892735bd2e27))
* Implement `upstream_tree` pattern filtering and remapping ([f7fe174](https://github.com/git-ents/git-vendor/commit/f7fe174c07c846afe03055165df6e4c6e2b6484e))
* Implement `vendor_paths`, resolving the local selection from `.gitattributes` ([a7bd9d2](https://github.com/git-ents/git-vendor/commit/a7bd9d279dd26502afa80e6f203f8905b1ff1f28))
* Implement `vendor_status` ([73df0c5](https://github.com/git-ents/git-vendor/commit/73df0c558ae7674d6b78658688523b207e1599e5))
* Implement `vendor_tip` via `try_find_reference` ([e1c0593](https://github.com/git-ents/git-vendor/commit/e1c05931bb2b1d6314f4101ef4fcdc7d12e7c161))
* Implement CLI, extend VendorWorktree, and refactor gitattributes helpers ([f2b6e46](https://github.com/git-ents/git-vendor/commit/f2b6e46ea3ecd21e3d0d10dc98d8460aecf21068))
* Implement PatternMapping and VendorConfig ([33cd3ba](https://github.com/git-ents/git-vendor/commit/33cd3ba7fc61ae461d653a1913e2006a1e4ddab4))
* Implement VendorConfig and PatternMapping with tests ([33cd3ba](https://github.com/git-ents/git-vendor/commit/33cd3ba7fc61ae461d653a1913e2006a1e4ddab4))
* Make name optional in add, defaulting to URL leaf ([5136a64](https://github.com/git-ents/git-vendor/commit/5136a64bec06be4c5ced748b2b223d81c1831676))
* Normalize --prefix and stage-only add ([4d0b229](https://github.com/git-ents/git-vendor/commit/4d0b22955f1ff735cf03f3c5ad10d9e539086e57))
* Preserve non-UTF-8 `ours` paths byte-for-byte in `ours_tree` ([098de11](https://github.com/git-ents/git-vendor/commit/098de11cfdc52699c98d8ed679f9743303fb88d0))
* Project conflicted merges onto the working copy ([951eb6b](https://github.com/git-ents/git-vendor/commit/951eb6bde8f057a39b082a4ece7cd937ebb15ce8))
* Record squashed upstream OID as a `Squashed-upstream` git trailer ([b6d305e](https://github.com/git-ents/git-vendor/commit/b6d305e300e26dcd818985cf0dd4dee19da92c0f))
* Refuse to commit conflicted vendor merges and stub conflict checkout ([395b29c](https://github.com/git-ents/git-vendor/commit/395b29c42ceee1d136bb0348bc2468161767a5b6))
* Reject conflicted `VendorMerge` in `commit_vendor` ([395b29c](https://github.com/git-ents/git-vendor/commit/395b29c42ceee1d136bb0348bc2468161767a5b6))
* Remove `sync_attributes` method ([faa5cf3](https://github.com/git-ents/git-vendor/commit/faa5cf3119738777259c31a25fe6ceb47fa0013c))
* Remove paths from vendor content filter in working-copy .gitattributes ([c760880](https://github.com/git-ents/git-vendor/commit/c760880acc1d962b6b303bc71714892735bd2e27))
* Rename --ref-name to --ref ([03d8deb](https://github.com/git-ents/git-vendor/commit/03d8deba67e90e454d5b810e4389685a5d1c1dbc))
* Select `vendor=<name>` attributed paths from the ours commit tree in `ours_tree` ([098de11](https://github.com/git-ents/git-vendor/commit/098de11cfdc52699c98d8ed679f9743303fb88d0))
* Wire `vendor_paths` to `resolve_vendor_paths` ([a7bd9d2](https://github.com/git-ents/git-vendor/commit/a7bd9d279dd26502afa80e6f203f8905b1ff1f28))


### Bug Fixes

* .gitvendors included in commit tree and staged alongside .gitattributes ([4e50374](https://github.com/git-ents/git-vendor/commit/4e50374361cd724df409a8df775bf225af018660))
* Allow local (file://) transport for non-bare repos ([9e1afb5](https://github.com/git-ents/git-vendor/commit/9e1afb5f8a8f60bf80dd9f9edb4534e3d3ad9d66))
* Append trailing / to --prefix so it is always treated as a directory ([4d0b229](https://github.com/git-ents/git-vendor/commit/4d0b22955f1ff735cf03f3c5ad10d9e539086e57))
* Avoid lossy UTF-8 conversion when writing .gitattributes ([ee6fc03](https://github.com/git-ents/git-vendor/commit/ee6fc03001a1744c9ab8858c0b16a6c6a09cda54))
* Build the merge commit's tree by overlaying `result_tree` onto the parent ([881d96c](https://github.com/git-ents/git-vendor/commit/881d96cca6aab7a7f40d867438e117d17e3dff67))
* Cmd_add always auto-commits ([4e50374](https://github.com/git-ents/git-vendor/commit/4e50374361cd724df409a8df775bf225af018660))
* Cmd_remove stages index deletions for removed vendor files ([4e50374](https://github.com/git-ents/git-vendor/commit/4e50374361cd724df409a8df775bf225af018660))
* Conflict path writes MERGE_HEAD and saves config before exiting ([4e50374](https://github.com/git-ents/git-vendor/commit/4e50374361cd724df409a8df775bf225af018660))
* Correct git-vendor CLI orchestration edge cases ([4d9d57b](https://github.com/git-ents/git-vendor/commit/4d9d57b72fdd30e54b19fe5ae60abc521dc3a5e9))
* Correct several CLI correctness bugs found in adversarial review ([4e50374](https://github.com/git-ents/git-vendor/commit/4e50374361cd724df409a8df775bf225af018660))
* Error when all configured patterns are invalid instead of silently emptying the tree ([6561041](https://github.com/git-ents/git-vendor/commit/656104187478faba9ec4c89ccd20b8b9cb7d6318))
* Escape glob metacharacters when writing .gitattributes patterns ([1b4bd84](https://github.com/git-ents/git-vendor/commit/1b4bd8482c6b679f5cc68b5128f8fa6499cdfc46))
* Force `refs/vendor/<name>` to a direct ref after fetch ([329f141](https://github.com/git-ents/git-vendor/commit/329f141d0e7afa3e77158f5fe6e5b61cdd651218))
* Gate `raw_paths` helper on `#[cfg(unix)]` ([48f6f51](https://github.com/git-ents/git-vendor/commit/48f6f515e403c444ae8d805aea15c611dd8c1777))
* Guard against silently resolving a .gitattributes merge conflict ([867038d](https://github.com/git-ents/git-vendor/commit/867038ddce827db9c8b505ad2472024161fdf252))
* Handle unborn HEAD with no `.git/index` in `checkout_vendor` ([5bb39fb](https://github.com/git-ents/git-vendor/commit/5bb39fb45ed2d3303aee3cb4362240ec1d8c2cd2))
* Harden `upstream_tree` glob matching and path remapping ([3caa4ac](https://github.com/git-ents/git-vendor/commit/3caa4acf6fc818f74a6ce81a48af9a92e389fa6d))
* Harden upstream_tree pattern indexing, path safety, and traversal ([6561041](https://github.com/git-ents/git-vendor/commit/656104187478faba9ec4c89ccd20b8b9cb7d6318))
* Harden vendor worktree/attribute layer ([381ebb4](https://github.com/git-ents/git-vendor/commit/381ebb4b39458d3da7c3e54675b26114731f7d2b))
* Import `ByteSlice` trait in `non_utf8_filename_is_preserved` ([48f6f51](https://github.com/git-ents/git-vendor/commit/48f6f515e403c444ae8d805aea15c611dd8c1777))
* Make `remove` work on an unborn HEAD ([a796f3c](https://github.com/git-ents/git-vendor/commit/a796f3ca2d56beedf838e5d1e2a1cb438ce2480d))
* Make advance_head a compare-and-swap on HEAD ([a75768a](https://github.com/git-ents/git-vendor/commit/a75768ad4aa5f86370970417f7b7d1bf13d58314))
* Pair compiled globs with their PatternMapping to prevent index desync ([6561041](https://github.com/git-ents/git-vendor/commit/656104187478faba9ec4c89ccd20b8b9cb7d6318))
* Peel fetched OID to a commit unconditionally ([f895e2b](https://github.com/git-ents/git-vendor/commit/f895e2b5c6975b39958b477908a0179b7f97b995))
* Propagate `vendor_paths` errors in `update`/`apply` ([9ce6bb3](https://github.com/git-ents/git-vendor/commit/9ce6bb3de7e413300a2b379f270ca76bd6043057))
* Prune empty ancestor directories after remove ([aa96571](https://github.com/git-ents/git-vendor/commit/aa965715a8991c033e31f1b638ffa0c1aefa9460))
* Read upstream OID from outcome.ref_map rather than local vendor ref ([9e1afb5](https://github.com/git-ents/git-vendor/commit/9e1afb5f8a8f60bf80dd9f9edb4534e3d3ad9d66))
* Read upstream OID from refmap to avoid HEAD-symref misresolution ([9e1afb5](https://github.com/git-ents/git-vendor/commit/9e1afb5f8a8f60bf80dd9f9edb4534e3d3ad9d66))
* Refuse local transports ([23a6e4e](https://github.com/git-ents/git-vendor/commit/23a6e4e3d4201aa1a7157a96bf0e4c345b026697))
* Reject `add <url> <existing-name>` instead of silently corrupting merge state ([1b5fd0c](https://github.com/git-ents/git-vendor/commit/1b5fd0c21ab9a77cf5a56068ba5b9f88f7907c43))
* Reject `add` of a vendor name that already exists ([1b5fd0c](https://github.com/git-ents/git-vendor/commit/1b5fd0c21ab9a77cf5a56068ba5b9f88f7907c43))
* Reject escaping or empty synthesized local paths via gix::validate::path::component ([6561041](https://github.com/git-ents/git-vendor/commit/656104187478faba9ec4c89ccd20b8b9cb7d6318))
* Remove stale vendor symlinks during `checkout_vendor` ([5bb39fb](https://github.com/git-ents/git-vendor/commit/5bb39fb45ed2d3303aee3cb4362240ec1d8c2cd2))
* Report `Error::Config` when a pattern maps to an empty local path ([3caa4ac](https://github.com/git-ents/git-vendor/commit/3caa4acf6fc818f74a6ce81a48af9a92e389fa6d))
* Splice vendored content into the full tree on `commit_vendor` ([881d96c](https://github.com/git-ents/git-vendor/commit/881d96cca6aab7a7f40d867438e117d17e3dff67))
* Split_attr_line returns None for comment and whitespace-only lines ([4e50374](https://github.com/git-ents/git-vendor/commit/4e50374361cd724df409a8df775bf225af018660))
* Stage rewritten `.gitvendors` on `remove` ([5d8ab0b](https://github.com/git-ents/git-vendor/commit/5d8ab0b7fa6407b94128d8541b42c303b6a5704b))
* Stop checkout_vendor from clobbering staged index entries ([a8d4d84](https://github.com/git-ents/git-vendor/commit/a8d4d840f494b0ff933de67b7494d7be94bcc441))
* Stop multi-vendor `update` from clobbering earlier vendors' pending MERGE_HEAD ([1b5fd0c](https://github.com/git-ents/git-vendor/commit/1b5fd0c21ab9a77cf5a56068ba5b9f88f7907c43))
* Track vendor paths when `add`/`update` conflicts ([32a39d8](https://github.com/git-ents/git-vendor/commit/32a39d85ca3c23fb7e6352eee39ade0257065823))
* Untrack .gitattributes on `remove --keep-files` ([1784c17](https://github.com/git-ents/git-vendor/commit/1784c1703556405b280affc12a380448169043a7))
* Use gitignore-like glob semantics in `upstream_tree` so `*` no longer crosses `/` ([3caa4ac](https://github.com/git-ents/git-vendor/commit/3caa4acf6fc818f74a6ce81a48af9a92e389fa6d))
* Validate trackable paths before checkout mutates state ([cdb481c](https://github.com/git-ents/git-vendor/commit/cdb481c74f62f5ec43631ae8ab34b261394cf6d5))

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
