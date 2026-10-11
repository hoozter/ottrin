# Search performance evidence

This document records measured behavior of Ottrin's persistent search service. It is not a universal hardware claim or a comparison against another product.

## Incremental update behavior

Filesystem bursts are applied in transactions of at most 500 changed paths. A larger burst stays queued for subsequent transactions instead of triggering a full-root rebuild. The 500 ms coalescing window starts with the first pending event; continuous writes do not postpone updates indefinitely. Maintenance runs on a clock even while events arrive. Failed transactions retain their pending paths for retry. Directory creation and moves still index their included descendants.

Linux AppImage daemon launches use the bundled `ottrin-indexd`, including the installed background service and session indexing. The index process is distinct from the `ottrin` UI process.

## AppImage verification on 2026-10-11

The installed service was consuming 89.3% of one CPU core (about 5.58% of this 16-thread machine). A diagnostic run reproduced the full-root fallback after a burst of 961 changed paths. The appearance overhaul had not changed the search worker.

The repaired package was built from clean tracked revision `13e9e93eadcf89be20bdb29a41b6fa563416ad4d`. The stable artifact is `dist/packages/Ottrin-0.1.0-x86_64.AppImage`, SHA-256 `1c16bbf07666438ca04901ff823585d21c122f87a8a89e0997aece44e1bcd92d`.

| Measurement | One core CPU | Whole machine CPU |
|---|---:|---:|
| Original installed daemon, 10 seconds | 89.3% | 5.58% |
| Exact package, isolated quiet index, 20 seconds | 0.60% | 0.038% |
| Repaired installed service, ongoing home-directory activity, 30 seconds | 7.43% | 0.465% |
| Repaired installed service after the test burst, ongoing activity, 30 seconds | 5.37% | 0.335% |

The quiet measurement used a private 10,000-file index. The live measurements used the existing personal home-directory index, with approximately 154,000 items and other applications still changing files; they are background-activity measurements, not idle claims. CPU percentages use 100% for one logical core, then divide by 16 for the whole-machine column.

Both the private exact-package test and installed service test passed creation of 1,200 files, modification, rename, and deletion. Create propagation took 1.79 seconds in isolation and 2.27 seconds on the live index. The active SQLite generation stayed unchanged across both bursts: no full rebuild was triggered. The private daemon also restarted successfully from its saved index. The live service remained enabled and running as `ottrin-indexd`, with ready filesystem watches and no reported error. Its executable SHA-256 matched the tested package daemon. Personal settings retained their exact pre-test checksum, and the existing index was reused.

Source validation passed 210 search tests, 22 application tests, one daemon test, strict search Clippy, formatting, and the Linux daemon packaging contract. Two existing FTS storage measurement fixtures remained ignored; they do not exercise the changed scheduling behavior. Raw acceptance is retained in `.hermes/evidence/index-service/ACCEPTANCE.json` and copied to `dist/packages/INDEX_ACCEPTANCE.json`. Test profiles, test files, diagnostic processes, and packaging staging were removed after verification.

## Historical acceptance result

Measured on 2026-08-15 with the dedicated `ottrin-indexd` process and a configured home-directory root containing **1,185,806 indexed items**.

| Operation | Result |
|---|---:|
| Full rebuild elapsed | 272.354 s |
| Full rebuild peak RSS | 86,716 KiB |
| Full rebuild peak PSS | 84,595 KiB |
| Full rebuild peak private memory | 84,564 KiB |
| Full rebuild peak RSS + swap | 157,068 KiB |
| Return-to-idle RSS after 20 s | 22,872 KiB |
| Return-to-idle PSS after 20 s | 20,753 KiB |
| Watcher after rebuild | ready, no error |
| Create propagation, three samples | 1,391–1,392 ms |
| Delete propagation, three samples | 1,722–1,723 ms |
| Ten-second idle CPU sample | 0.100% |

A later live revalidation after removing corpus-sized path-FTS maintenance remained in the same propagation range: approximately 1.36 s for create and 1.71 s for delete.

Fresh persisted-query samples from the same 1.18M-item index:

| Query | Total matches | Returned | Wall time | CLI peak RSS |
|---|---:|---:|---:|---:|
| `Cargo.toml` | 566 | 500 | 0.15 s | 11,312 KiB |
| `ottrin` | 857 | 500 | 0.10 s | 11,460 KiB |
| `README` | 6,342 | 500 | 1.03 s | 11,624 KiB |
| absent 28-character term | 0 | 0 | 2.23 s | 10,916 KiB |

The absent-term path remains the slowest sampled query and is a known latency risk. It is not represented as equivalent to common positive-term lookup.

## Test system

- Linux 7.0.0-28-generic, x86-64
- AMD Ryzen 7 5800H, 8 cores / 16 threads
- 13 GiB visible RAM with swap enabled
- ext4 on NVMe-backed root storage
- Rust 1.94.0
- Current debug binaries from the working checkout; these numbers are not release-artifact claims

## Reproduction

Build and start the dedicated daemon from the checkout:

```sh
cargo build -p ottrin-app --bins
./target/debug/ottrin-indexd
```

In another shell, verify that the configured index is ready:

```sh
./target/debug/ottrin-app index status --json
```

Run the controlled full-rebuild sampler:

```sh
python3 .hermes/measure-full-index-memory.py \
  | tee .hermes/evidence/full-index-memory-$(date +%F).json
```

The sampler:

- resolves the checkout from its own path;
- samples daemon RSS, swap, PSS, private memory, CPU time, database size, and WAL size every 200 ms;
- invokes the normal daemon rebuild command rather than deleting or replacing the database;
- requests a controlled shutdown if combined RSS and swap exceed 320 MiB;
- waits 20 seconds after completion before recording return-to-idle memory;
- records final indexed count, watcher readiness, and daemon error state.

Query timing can be reproduced with:

```sh
/usr/bin/time -f 'elapsed=%e maxrss_kib=%M' \
  ./target/debug/ottrin-app search Cargo.toml --json
```

Watcher propagation is measured with a uniquely named disposable file beneath a configured watched root. Measure from durable file creation/removal until `snapshot_revision` advances, then verify query visibility. Remove the fixture after each run.

## Interpretation and limitations

- Corpus composition, exclusions, filesystem cache warmth, concurrent load, storage, and installed extractors can materially change results.
- The corpus above is a real configured home-directory tree, not a standardized public fixture. Its contents are private and are not published.
- The full rebuild used bounded 2,048-item scanner batches, synchronous backpressure, inactive SQLite generation staging, count validation, and an activation fence. Peak memory therefore reflects the bounded pipeline rather than a corpus-sized in-memory snapshot.
- Content extraction was disabled for this live corpus. When enabled, text reads are hard-bounded to 512 KiB per file and run sequentially in the backpressured writer path; extraction-isolation should be reconsidered only if representative enabled-content measurements exceed the accepted budgets.
- SQLite remains canonical for metadata and active-generation authority. Path FTS, content FTS, and content-completeness state share one generation-scoped disposable sibling database. Direct deletes leave stale path postings that are filtered through canonical active-generation rows; the next full replacement atomically activates a complete candidate sibling and retires its predecessor without retaining old FTS segments in canonical storage.
- The 2026-08-15 live full rebuild used the earlier canonical-content layout and grew the database from 9,360,211,968 to 11,676,770,304 bytes while the WAL peaked at 1,922,511,512 bytes. That historical live-cache result establishes the original disk-headroom risk; it is not evidence for the current separate derived-store layout.
- Fresh isolated source verification on 2026-08-16 replaced a 25.17 MiB content generation with equal-sized content and ended at 56.12 MiB aggregate database storage, 354,352 bytes canonical WAL, one active derived database, and zero free pages. A separate 12,000-item equal-sized path-heavy replacement reproduced the prior canonical-path amplification RED at 13,152,256 to 23,715,840 aggregate bytes with a 27,826,512-byte WAL; with path FTS moved into the disposable sibling, GREEN measured 13,144,064 initially, 13,230,080 after one replacement, and 13,299,712 after a repeated replacement, with zero retained WAL bytes, one active sibling, and zero free pages. These are isolated source measurements, not a live-cache migration or exact-package claim.
- No Explorer, Windows Search, Everything, FSearch, or other competitor comparison has yet been accepted. Such a comparison needs the same corpus, query set, cache-state definitions, and freshness checks.

Raw full-rebuild evidence is retained at `.hermes/evidence/full-index-memory-2026-08-15.json`. Direct-delta evidence and historical architecture transitions are retained at `.hermes/evidence/index-direct-delta-2026-08-15.md`. Separate content-store evidence is retained at `.hermes/evidence/fts-separate-store-verification-2026-08-16.md`.
