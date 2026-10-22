# Search performance evidence

This document records measured behavior of Ottrin's persistent search service. It is not a universal hardware claim or a comparison against another product.

## Incremental update behavior

Filesystem bursts are applied in transactions of at most 500 changed paths. A larger burst stays queued for subsequent transactions instead of triggering a full-root rebuild. The 500 ms coalescing window starts with the first pending event; continuous writes do not postpone updates indefinitely. Maintenance runs on a clock even while events arrive. Failed transactions retain their pending paths for retry. Directory creation and moves still index their included descendants.

Linux AppImage daemon launches use the bundled `ottrin-indexd`, including the installed background service and session indexing. The index process is distinct from the `ottrin` UI process.

## Initial AppImage verification on 2026-10-11

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

## Sustained-activity follow-up on 2026-10-11

The initial short samples above missed a second failure. A later 20-second live
sample measured **69.5% of one core (4.34% of this 16-thread machine)** even
though the daemon reported Ready. A controlled burst of 2,000 short-lived
folders reproduced failed watch registration on already removed descendants,
followed by recovery of the entire configured home root. That recovery also
rewrote the index; an unchanged active SQLite generation alone therefore does
not prove absence of whole-root work.

Watch discovery now tolerates descendants removed between enumeration and
registration, including a queued creation whose directory is already gone.
Path-specific backend errors for absent or inaccessible directories do not
invalidate the accessible root. Failures on existing directories, configured
root watch failures, and pathless backend errors such as event queue overflow
retain recovery behavior. Regression tests cover the disappearance race and
preservation of real watch registration failures.

A second live trace found that a metadata notification for a busy application
data directory classified **73,952 descendants** as changed. The pending event
queue now retains whether work concerns one entry or a subtree. Metadata/data
updates refresh only the entry and preserve indexed children; creation, moves,
and removals retain subtree handling. Coalescing preserves subtree discovery
when a later metadata event arrives for the same path. Ordinary file updates
also avoid deleting and re-inserting unchanged path-FTS postings.

Daemon replacement now shares one graceful shutdown wait in the daemon client.
It allows 30 seconds for large watch sets and persisted work to be released,
instead of reporting failure after five seconds while shutdown is still
finishing. The application and service installer use that same implementation.

### Final installed-package acceptance

The final AppImage was built from clean tracked revision `c82d3b65956c2f4ad0f540d74d669d2f3cee5837`.
Its stable path is `dist/packages/Ottrin-0.1.0-x86_64.AppImage`, SHA-256
`fc000ef6cf723e7f5f04f58a9e559b394cb33d188f2fe4899ecc6fc19f8afad1`. The installed `ottrin-indexd` executable hash matched
the bundled release binary. Service update succeeded, the saved index loaded
on restart, and the service remains enabled and active.

| Live measurement | One core CPU | Whole machine CPU |
|---|---:|---:|
| Before controlled burst, 30 seconds | 2.967% | 0.1854% |
| After burst, 30 seconds | 4.467% | 0.2792% |
| Later sustained sample, 30 seconds | 2.367% | 0.1479% |

These are live background-activity measurements on the same 16-thread machine,
with other applications continuing to write to the configured home tree;
they are not claims of idle CPU use. All three samples are substantially below
the sustained 69.5% one-core / 4.34% whole-machine failure measured earlier.

The exact installed package passed creation of 1,200 files (1.886 seconds),
modification, rename, deletion, repeated directory metadata changes preserving
indexed children, and creation/removal of 2,000 short-lived directories. The
normal full rebuild replaced the previous 3.7 GiB derived database with roughly
157 MiB; the subsequent controlled burst grew that derived store by only
0.00 MiB. Personal settings retained their exact checksum.

Validation passed 213 search tests, 22 application tests, one daemon test, and
strict Clippy. Two existing storage benchmark fixtures remained ignored.
Current raw acceptance is `.hermes/evidence/index-service/WATCH_RACE_ACCEPTANCE.json`
and `dist/packages/INDEX_ACCEPTANCE.json`. The earlier partial package was
rejected after the longer sample exposed directory metadata subtree work;
its compact diagnostic receipt is retained as `WATCH_RACE_PARTIAL.json`.
Task-owned profiles, fixtures, extraction copies, diagnostic processes, and
packaging worktrees were removed.

## Current appearance-package integration on 2026-10-11

The installed service was upgraded again to the current AppImage, built from
clean tracked revision `a3a76ac1d26a88c3cde026faaf74d934e25faad5`, SHA-256
`2aca0c0376dd6f99e235fbb386e79982b856c15fbe2ba58d99cb062d070ae8ac`.
The process is `ottrin-indexd`; its executable checksum matches the bundled
release daemon. The existing saved index loaded successfully, filesystem
watches are ready, and the service is enabled and active without reported errors.

Two consecutive 30-second live samples after replacement measured 7.933% and
1.167% of one core, or 0.4958% and 0.0729% of this 16-thread machine. The first
includes recent startup and test cleanup; the second is the later settled
sample. Both include concurrent home-directory activity rather than a
controlled idle workload. Personal settings retained their exact checksum.
An actual disposable file became searchable in 0.921 seconds and disappeared
from search in 0.499 seconds after deletion; the fixture was removed.
The integration receipt is
`.hermes/evidence/index-service/CURRENT_PACKAGE_INTEGRATION.json`.
The earlier controlled-burst receipt above remains evidence for the index
fixes; these fresh measurements verify their integration into the newer package.

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

## Prior packaged populated-index sample — 2026-10-15 (af78252)

Exact stable AppImage source `af78252`, SHA-256
`dc9496042ce54d4df373de2ce8a25ab121fadc7491a79272116d9056892b25d9`,
passes a private metadata-only corpus of 10,000 files in 100 directories.
No GUI, login service or content extraction participates. Quiet samples contain
no IPC clients or filesystem changes; percentages use one logical CPU core.

| Check | Observed result |
|---|---:|
| Populated quiet CPU, 30.034 s | 0.067% of one core |
| RSS at end of quiet sample | 9,084 KiB |
| Burst | 500 rename + 500 delete + 500 modify + 500 create |
| All metadata paths/sizes and returned query results correct | 2,975.613 ms from mutation start |
| Full rescan / generation replacement during burst | Neither occurred |
| After-burst quiet CPU, 30.032 s | 0.067% of one core |
| RSS at end of after-burst sample | 11,892 KiB |
| Restart with complete metadata/query results and active watcher | 255.323 ms |
| Full scan / generation replacement on restart | Neither occurred |

The canonical file set and sizes must exactly match the filesystem. Query totals
and returned paths/sizes are checked for renamed, newly created and remaining
seed files, including the modified-file sample. Restart uses the same persisted
index; a fresh process must restore its generation without starting a scan.
Both daemon instances shut down normally and remove their private socket. The
runner tears down its processes and profile on failure, timeout and cancellation.
Your existing live service/profile is not used or changed.

Reproduce from the repository root:

```sh
python3 scripts/index-populated-qa.py \
  dist/packages/Ottrin-0.1.0-x86_64.AppImage \
  .hermes/evidence/index-populated-current/ACCEPTANCE.json
```

The runner verifies the exact artifact checksum and embedded clean provenance,
requires 10,000 actual indexed files before measurement, and rejects quiet CPU
at or above 0.5% of one core. Receipt:
`.hermes/evidence/index-populated-current/ACCEPTANCE.json`.
These observed timings are not latency guarantees. Longer live-home sampling,
content-enabled work, larger/adversarial bursts, Windows and competitor comparison
remain separate gates. No zero-CPU or broad daily-driver claim is inferred.

A live cancellation check sends SIGTERM during the populated quiet phase. The
runner exits 143, stops its private daemon, removes its profile/extraction and
produces no passing receipt. Evidence:
`.hermes/evidence/index-populated-current/CANCELLATION_ACCEPTANCE.json`.


## Current packaged offline freshness and return to idle — 2026-10-15

Clean 554591d AppImage SHA-256:
`3758bae8aabda470eb224b7b2889cff6018b480ccbcb2f2fe8dd893e9db5ed55`.
The previous saved-snapshot/no-scan startup could not see changes made while the
daemon was stopped. Source now keeps cached queries available during watcher
setup and performs one background reconciliation. Cached query availability is
covered by the blocked-watcher source regression. This exact package verifies
completed freshness against the filesystem and IPC query results.

| Private metadata-only 10,000-file case | Observed result |
|---|---:|
| Initial quiet CPU, 30.034 s | 0.033% of one core |
| Ordinary 2,000-change burst, complete metadata/query freshness | 3,052.432 ms |
| Ordinary burst replaces generation / starts full scan | Neither |
| After-burst quiet CPU, 30.033 s | 0.100% of one core |
| Offline create/edit/rename/delete: completed startup freshness | 508.305 ms |
| After-startup-validation quiet CPU, 30.037 s | 0.067% of one core |
| After-startup-validation RSS | 9,024 KiB |

Startup validation performs a scan even when the saved plan matches, since Linux
filesystem watches cannot report offline changes. Ordinary live events keep the
incremental path. These are observed cases, not latency guarantees, content-enabled
or live-home measurements. The personal running service/profile is unchanged.
Receipt: `.hermes/evidence/index-populated-current/OFFLINE_FIXED_PACKAGE.json`.

```sh
python3 scripts/index-populated-qa.py \
  dist/packages/Ottrin-0.1.0-x86_64.AppImage \
  .hermes/evidence/index-populated-current/OFFLINE_FIXED_PACKAGE.json \
  --offline-changes
```

The default runner now requires startup validation completion; it does not retain
the earlier no-scan criterion. `--quiet-seconds 0` skips CPU samples for a focused
correctness repro and must not be treated as resource acceptance. Shutdown and
failure cleanup release owned processes and disposable profiles. Larger corpus,
content-enabled, live-home and native Windows acceptance remain separate gates.


## Windows current portable offline freshness — 2026-10-15

The candidate from clean revision `4a0c13b084080fccf18d839bbd43bac80b66a399`
contains the startup reconciliation repair. Portable SHA-256:
`e7e018f47406f30868e27e1edd2b5b2aeb97f37488fb1c6034e1eea12d8fcaf0`.
A native, non-administrator interactive Windows account indexed a private corpus
of 10,000 files in 100 directories, with content indexing disabled and scheduled
refresh disabled. The directly started `ottrin-indexd.exe` remained the daemon
used by the CLI; no background task was installed.

| Measurement | Result |
|---|---:|
| Quiet CPU before stop, 15 seconds | 0.208% of one core |
| Create/edit/rename/delete while stopped, visible after restart | 1,806.709 ms |
| Quiet CPU after startup reconciliation, 15 seconds | 0.312% of one core |

Stopping session indexing exited zero and retained the database. After offline
mutations, read-only queries found the created and renamed entries, excluded the
old/deleted names and returned the modified file's new size, without a manual
update. Watches were ready and the startup scan completed. Packaged cleanup
stopped the restarted daemon, exited zero and removed the private database.

Raw results, payload identity and teardown are recorded in
`.hermes/evidence/windows-20261011/WINDOWS_OFFLINE_INDEX_ACCEPTANCE.json`,
`WINDOWS_OFFLINE_CANDIDATE_PAYLOAD.json` and `WINDOWS_OFFLINE_INDEX_CLEANUP.json`.
The test task exited zero; its fixtures, extracted packages and compiler runtime
were removed, account preferences/source hashes were preserved, and the owned
VM was shut down with backing-image content mtime unchanged. An earlier harness
stall came from deep PowerShell serialization of extended string metadata;
plain .NET file reads allowed the same test to complete. Timed-out attempts were
not accepted.

This is private metadata-only portable acceptance, not whole-home or
content-enabled performance acceptance. Current MSI/NSIS native lifecycle,
login/reboot persistence and GUI acceptance remain separate. All three candidate
formats were rebuilt and their embedded payloads checked; public Windows
downloads have not been replaced.

## Current Linux package with content enabled — 2026-10-22

The stable AppImage is from clean `3ed1b303`, SHA-256
`f320f2253392437f08acfb9854441f2e38d9fcde25c940595072e5ed98e1c5e6`.
A private corpus contains 10,000 small plain-text files in 100 directories, with
body indexing explicitly enabled for that corpus and scheduled refresh disabled.
The exact extracted daemon indexes every body. Read-only SQLite comparisons check
all persisted bodies; five content-query groups check exact totals and the first
500 returned paths, alongside metadata and filename-query comparisons.

| Observed case | Result |
| --- | ---: |
| Populated quiet CPU, 30.068 s | 0.100% of one core |
| After-burst quiet CPU, 30.066 s | 0.100% of one core |
| After-restart quiet CPU, 30.071 s | 0.133% of one core |
| RSS at the ends of those samples | 9,552 / 13,216 / 11,140 KiB |
| 2,001 live changes through complete verification | 10,844.914 ms |
| Restart with offline changes through complete verification | 2,115.944 ms |

The live burst renames 500 files, deletes 500, edits 501 and creates 500. One edit
preserves both size and modification time; an offline edit does the same. Cached
bodies and query results converge in both cases. Live changes preserve the active
generation and do not start a full scan. Restart completes the required background
reconciliation. Timing includes the harness's complete comparisons and sequential
query groups; it is not GUI startup time or a storage/latency guarantee. Quiet
samples send no IPC and make no corpus changes. They retain the existing limit
of less than 0.5% of one core. RSS is sampled at interval ends, not a peak-memory
claim. Larger files, provider extraction, live-home content indexing, native
Windows content indexing and whole-application resource acceptance remain open.

The shared runner now exports observations and teardown even when cancelled or
failing. An actual SIGTERM during a live 600-second quiet sample verifies its
daemon exits and private profile is removed. A metadata-only control also passes
with CPU sampling explicitly skipped. Evidence:
`.hermes/evidence/index-content-package-current/ACCEPTANCE.json`.

```sh
python3 scripts/index-populated-qa.py \
  dist/packages/Ottrin-0.1.0-x86_64.AppImage \
  .hermes/evidence/index-content-package-current/CONTENT.json \
  --content --offline-changes
```

A separate read-only 30-second observation of the existing personal service
measures 0.100% of one core with 121,934 indexed entries and a ready watcher;
the snapshot revision advances from 10,238 to 10,241. That is mixed background
activity, not isolated idle. Content indexing is disabled in that personal
profile, and its running binary differs from the current package; the observation
does not accept the new package against the user's home. Configuration and the
running service remain unchanged.
