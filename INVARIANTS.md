# Turbolite Invariant Registry

This document maps the adversarial-review invariant annotations (A*, B*, C*,
D*, E*, F*) to their enforcement points, regression tests, and Quint/DST
models. It is meant to be updated whenever a new invariant is added or an
existing one changes shape.

Legend:
- **Enforced in** — file/function/struct that implements the rule.
- **Tested in** — Rust test that exercises the regression.
- **Modeled in** — Quint spec (if any) that captures the contract.

---

## A — Manifest / handle consistency

| ID | Rule | Enforced in | Tested in | Modeled in |
|---|---|---|---|---|
| A1 | `set_len` must keep BTree-aware `group_pages` / `page_index` consistent. | `src/tiered/handle.rs::TurboliteHandle::set_len` | `tests/set_len_btrees.rs::set_len_truncate_and_regrow_keeps_btree_mapping_consistent` | — |
| A2 | In-bounds pages with no backend object must error, not zero-fill. | `src/tiered/handle.rs::TurboliteHandle::read_exact_at`, `decode_and_cache_group` | `src/tiered/test_handle.rs::test_remote_read_missing_group_errors`, `tiered_read_releases_claim_when_page_group_key_missing` | — |
| A3 | Cache must drop stale group state on manifest/key changes. | `src/tiered/disk_cache.rs::DiskCache::invalidate_group`, `invalidate_all_groups`; `src/tiered/vfs.rs::TurboliteVfs::set_manifest` | HA/follower tests | — |
| A4 | Nonce sidecar must not tear between read and ciphertext. | `src/tiered/handle.rs::TurboliteHandle::read_exact_at`, `write_all_at` | `tests/integration.rs::test_passthrough_encryption_nonce_sidecar` | — |
| A5 | Lock-downgrade flush errors must propagate. | `src/tiered/handle.rs::TurboliteHandle::lock` | `src/tiered/test_handle.rs::test_lock_downgrade_flush_error_propagated` | — |
| A6 | Speculative file growth must stay local until `sync()`. | `src/tiered/handle.rs::TurboliteHandle::pending_page_count`, `effective_page_count`, `close_local_commit`, `write_all_at`; `src/tiered/vfs.rs` close path | `src/tiered/test_handle.rs::test_speculative_page_count_not_shared` | `specs/flush_ordering.qnt` (local-vs-shared ordering) |

## B — Disk-cache concurrency / sizing

| ID | Rule | Enforced in | Tested in | Modeled in |
|---|---|---|---|---|
| B1 | `group_states` must grow beyond initial page-count-derived capacity. | `src/tiered/disk_cache.rs::try_claim_group`, `mark_group_present`, `ensure_group_capacity` | `src/tiered/test_disk_cache.rs::test_group_states_resize_on_claim_and_mark`, `test_mark_pages_present_resizes_group_states`, `test_mark_all_pages_present_resizes_group_states` | — |
| B2 | `mem_cache` capacity must be budget-based, not page-count-based. | `src/tiered/disk_cache.rs::DiskCache` construction, `ensure_mem_cache_capacity` | `src/tiered/test_disk_cache.rs::test_mem_cache_sized_to_budget_not_page_count`, `test_mem_cache_grows_lazily_for_high_page_numbers` | `specs/mem_cache_epoch.qnt` |
| B3 | `mem_cache` slots must be immutable once published. | `src/tiered/disk_cache.rs::install_mem_cache_page` | `src/tiered/test_disk_cache.rs::test_mem_cache_no_torn_reads` | — |
| B4 | Eviction and page install must not leave a set bit over a hole. | `src/tiered/disk_cache.rs::eviction_lock`, `write_page`, `clear_pages_from_disk`; `src/tiered/handle.rs::decode_and_cache_group` | `src/tiered/test_disk_cache.rs::test_evict_install_no_zero_pages` | — |
| B5 | Deferred frees must not expose freed/corrupted buffers to readers. | `src/tiered/disk_cache.rs` crossbeam-epoch reclamation | `src/tiered/test_disk_cache.rs::test_deferred_frees_concurrent_readers_while_evicting` | `specs/mem_cache_epoch.qnt` |
| B6 | After eviction, bitmap and sub-chunk tracker must agree. | `src/tiered/disk_cache.rs::evict_to_budget` | `src/tiered/test_disk_cache.rs::test_evict_to_budget_tracker_bitmap_consistent` | — |
| B7 | Truncate must evict truncated pages from `mem_cache`. | `src/tiered/disk_cache.rs::truncate_to_page_count` | `src/tiered/test_disk_cache.rs::test_truncate_to_page_count_invalidates_mem_cache` | — |
| B8 | Bulk/scattered write paths must reject short data. | `src/tiered/disk_cache.rs::write_pages_bulk`, `promote_bulk_to_mem_cache`, `promote_scattered_to_mem_cache` | `src/tiered/test_disk_cache.rs::test_write_pages_bulk_rejects_short_data`, `test_promote_bulk_rejects_short_data`, `test_promote_scattered_rejects_short_data` | — |
| B9 | Tainted cache must reject all writes. | `src/tiered/disk_cache.rs::check_tainted` | `src/tiered/test_disk_cache.rs::test_tainted_rejects_write_paths` | — |

## C — Cross-writer / flush / manifest safety

| ID | Rule | Enforced in | Tested in | Modeled in |
|---|---|---|---|---|
| C1 | Manifest upload must reject stale overwrites. | `src/tiered/storage.rs::put_manifest` | cross-writer HA tests | `specs/manifest_cas.qnt` |
| C2/C3 | Missing/evicted pages must not be encoded as zeros during flush. | `src/tiered/flush.rs::flush_inner` | `tests/flush_missing_pages.rs::local_then_flush_does_not_encode_missing_pages_as_zeros`, `local_then_flush_growth_after_import_does_not_lose_pages` | `specs/dirty_page_read.qnt` |
| C4 | `sync()` local flushes must serialize with background flushes. | `src/tiered/handle.rs::TurboliteHandle::flush_lock`, `set_flush_lock`, `sync` | concurrent flush tests | — |
| C7 | Manifest decoder must reject out-of-range fields. | `src/tiered/manifest.rs::validate_field_ranges`, `decode_manifest_bytes` | `src/tiered/test_manifest.rs` validate tests; `tests/integration.rs::wire_decode_rejects_invalid_page_size` | — |

## D — Prefetch pool liveness / lock ordering

| ID | Rule | Enforced in | Tested in | Modeled in |
|---|---|---|---|---|
| D1 | Pool teardown must not hang on stuck remote I/O. | `src/tiered/prefetch.rs::PrefetchPool::drop` | `src/tiered/test_prefetch.rs::prefetch_pool_drop_does_not_hang_on_stuck_remote_io` | — |
| D2 | Frame-batch submit/check locks must not deadlock. | `src/tiered/prefetch.rs::submit_frame_batch`, `pending_frame_indices_for_gid`, `is_frame_pending` | `src/tiered/test_prefetch.rs::submit_frame_batch_lock_order_no_deadlock` | — |
| D3 | `coalesced_frame_runs` must reject overflows. | `src/tiered/prefetch.rs::coalesced_frame_runs` | `src/tiered/test_prefetch.rs::coalesced_frame_runs_rejects_overflowing_entries`, `coalesced_frame_runs_rejects_overflowing_run_length`, `coalesced_frame_runs_still_coalesces_valid_contiguous_frames` | — |
| D4/D5 | End-of-query must clear prefetch state. | `src/tiered/handle.rs::TurboliteHandle::read_exact_at` | `src/tiered/test_handle.rs::end_of_query_clears_prefetch_state` | `specs/cloud_scan_prefetch.qnt` |
| D7 | Pool drop must not hang on stuck remote I/O. | `src/tiered/prefetch.rs::PrefetchPool::drop` | `src/tiered/test_prefetch.rs::prefetch_pool_drop_does_not_hang_on_stuck_remote_io` | — |

## E — Crash durability / decompression bombs

| ID | Rule | Enforced in | Tested in | Modeled in |
|---|---|---|---|---|
| E1 | Capped decompressor must reject oversized declared output. | `src/compress.rs::decompress_capped` | `src/compress.rs::decompress_capped_rejects_bomb_for_active_codec` | — |
| E2 | Decompression must not allocate unbounded memory. | `src/compress.rs::decompress_capped`; `src/local/file_format.rs::decompress_zstd`, `decode_page`; `src/tiered/disk_cache.rs::read_page_compressed` | `src/local/file_format.rs::decode_rejects_zstd_decompression_bomb` | — |
| E3 | Sidecar parent directory must survive a crash. | `src/tiered/handle.rs::PassthroughNonceMap::append_record` / `fsync_parent` | `tests/integration.rs::test_passthrough_sidecar_reloads_and_compacts_across_restart` | — |
| E4 | Nonce must not be persisted before ciphertext. | `src/tiered/handle.rs::PassthroughNonceMap::append_record`, `read_exact_at`, `write_all_at` | `tests/integration.rs::test_passthrough_encryption_nonce_sidecar` | — |
| E5 | Key rotation must clear local cache before publishing rotated manifest. | `src/tiered/rotation.rs::rotate_encryption_key` | `tests/rotation_crash.rs::rotation_clears_local_cache_before_manifest_commit` | — |

## F — Crypto / encoding / cache correctness

| ID | Rule | Enforced in | Tested in | Modeled in |
|---|---|---|---|---|
| F1/F2 | Page rewrite must not reuse a nonce/keystream. | `src/compress.rs::encrypt_gcm_random_nonce` | `tests/property_encryption.rs::gcm_page_rewrite_differs_and_both_decrypt` | — |
| F3 | GCM page-group blobs must be slot-bound via AAD. | `src/tiered/test_encoding.rs` encode/decode with `keys::aad_page_group` | `src/tiered/test_encoding.rs::test_page_group_aad_binds_to_slot` | — |
| F10 | Recovery/shrink must clear pages at or above committed count. | `src/tiered/disk_cache.rs::mark_all_pages_present`, `clear_pages_at_or_above` | `src/tiered/test_disk_cache.rs::test_mark_all_pages_present_shrink_clears_bits_above_count`, `test_clear_pages_at_or_above` | — |

---

## Specs index

| Spec | Invariants covered | Location |
|---|---|---|
| `cursor_chain.qnt` | Promotion, chain anchors, writer/lease fencing, delta application | `specs/cursor_chain.qnt` |
| `cursor_chain_liveness.qnt` | Temporal progress of follower catch-up | `specs/cursor_chain_liveness.qnt` |
| `cloud_scan_prefetch.qnt` | D4/D5 prefetch state clearing, scheduler permits | `specs/cloud_scan_prefetch.qnt` |
| `manifest_cas.qnt` | C1 manifest upload CAS | `specs/manifest_cas.qnt` |
| `flush_ordering.qnt` | A6 local-vs-shared ordering, flush persistence | `specs/flush_ordering.qnt` |
| `dirty_page_read.qnt` | C2/C3 missing/evicted pages must error, not zero-fill | `specs/dirty_page_read.qnt` |
| `mem_cache_epoch.qnt` | B2/B5 epoch-safe mem_cache reclamation | `specs/mem_cache_epoch.qnt` |

---

## How to update this file

1. When adding a new invariant annotation, add a row to the appropriate
   letter section.
2. If the invariant is modeled in Quint, add the spec to the index and link
   it in the row.
3. If an invariant changes shape (e.g. B5 moved from a 64-generation ring to
   crossbeam-epoch), update the enforcement point and add a note.
