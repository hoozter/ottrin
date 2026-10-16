# Search experience design

Status: accepted direction for v1 implementation and packaging

## Goal

Ottrin search must be useful before a user configures a large private index, remain faster and richer than invoking `locate` manually for ordinary filename discovery, and make content extraction a separate explicit privacy/storage choice.

The product has three independent search layers. They must not be presented as one undifferentiated "indexing" switch.

## 1. System filename discovery

On Linux, use the installed plocate/locate database for broad filename and path discovery. The operating system database may be built by root while remaining queryable by ordinary users according to the distribution's database permissions. Ottrin does not need a root-owned copy of its normal SQLite service to obtain this coverage.

For global filename/path queries, Ottrin queries both its native index and an enabled system locate database, merges results, and deduplicates by exact path. System results are not restricted to Ottrin's configured native roots. A current native row wins over a duplicate system row.

When the current user can stat a system result, Ottrin enriches it with type, size, modification time, and normal actions. A path returned by plocate but not currently statable may remain a path-only result with unknown metadata; opening or mutating it still follows normal operating-system authorization. A stale path that can be proven absent is omitted.

Current-folder searches restrict locate results to that folder. Content predicates are never evaluated against plocate.

### Availability and freshness

First-run setup detects:

1. whether plocate/locate is installed;
2. whether its database is queryable;
3. database age;
4. an existing distribution systemd timer or cron maintenance job;
5. whether any maintenance was installed by Ottrin.

A healthy system filename index is offered as the default broad filename source. Existing distribution maintenance is reused and never duplicated. If the database is stale, Ottrin offers **Update now**. If no maintenance exists, Ottrin offers **Keep system filename index current** with an explicit administrator prompt.

Prefer a package-owned, narrowly defined systemd timer on systemd installations; use a fixed cron job only as a compatibility fallback. Setup, immediate update, status, and removal go through a packaged fixed-operation helper/polkit policy. The GUI must not construct arbitrary root shell commands.

Ottrin-owned maintenance is separately removable and affects only updatedb/plocate. It never starts or elevates `ottrin-indexd`.

## 2. Native live metadata index

The normal per-user `ottrin-indexd` remains unprivileged. It indexes configured readable roots and stores at least:

- filename and complete path;
- entry kind/type;
- size where meaningful;
- modification time;
- metadata required by Ottrin filters, ranking, previews, and actions.

It uses direct watcher deltas for ordinary freshness and bounded periodic reconciliation for missed/ambiguous events. SQLite remains the sole canonical Ottrin store.

First-run setup offers sensible readable locations such as Home, Documents, Downloads, Desktop, and Pictures, but keeps the choice explicit. A user may configure no native roots and still use global system filename search through plocate.

## 3. Optional content index

Content indexing is off by default and cannot be implied by enabling native metadata indexing or plocate.

Enabling it requires explicit content roots. A user may index native metadata for Home while extracting content only from Documents. Settings must provide:

- selected content roots;
- selected supported file types or groups;
- content exclusions;
- current extraction size limit;
- clear notice that indexing is slower and the database becomes larger;
- measured database size and rebuild status once available.

Missing or access-denied content-only folders do not invalidate healthy metadata indexing. Watcher health reports the failure and retries registration/extraction when they return. Retries start at 60 seconds and back off after repeated failures; reconnection is not immediate. This does not establish isolation from blocked network filesystem calls.

Protected or system-wide content is not extracted merely because plocate can name a path. Filename/path discovery and content privacy remain separate boundaries.

## Query composition

For a compatible global filename/path query:

```text
native SQLite results ----\
                           +--> exact-path dedupe --> common ranking/sort --> one result list
system plocate results ---/
```

Native rows carry richer metadata and live freshness. System rows extend coverage. Content search adds matches only from explicitly selected content roots, including roots outside native metadata locations. Saved extraction and watcher deltas cover those roots independently. Their shared file records do not become global native filename results unless the metadata policy also admits them.

Result details may identify `Live index`, `System filename index`, or `Content match`, but the main list remains unified rather than split into separate result screens.

Positive literal content queries visit all saved candidates in one generation read transaction and all admitted live matches before final sorting and paging. A source cannot truncate to the UI page size. Exact-path deduplication spans saved/live/filename matches; content expressions do not add unrelated fuzzy filename suggestions. Every sort has a path tie-breaker so static results have the same order across page sizes. The retained result payload is bounded by `offset + limit`; saved content streams one record at a time and live content is merged in batches of 50. Deduplication/provider path sets still scale with matching paths; this is not constant-memory traversal.

The results view exposes Previous/Next and the visible range. The CLI accepts `--limit N` and zero-based `--offset N`. Changing the query/filter/sort starts at the first page; a snapshot refresh retains the requested page or returns to the last nonempty page after deletion. Superseded content queries release SQLite readers and kill/reap the live provider while draining both output pipes. The production client interrupts its authenticated connection on cancellation; the daemon observes disconnect or shutdown and passes cancellation into the query. Queries retain no detached registry job. A client waits at most 60 seconds for a query reply.

This does not establish complete totals for bounded system-filename discovery or filesystem fallback while native indexing is incomplete. Their coverage/completeness reporting, query composition and paging remain separate required work. Live files can change during a query; pages are recomputed, not held by a cursor across filesystem mutations.

## Root execution and escalation

Intentional root use is supported and must not be shamed or rejected. The safety rule is that Ottrin never creates persistent privileged state by surprise.

Launching `sudo ottrin` or running Ottrin from a root login opens a visibly identified **Administrator** session. Browsing, opening, previewing, and editing a permitted file do not require any index to be created. A direct file invocation such as `sudo ottrin /etc/fstab` should open that file in Quick Inspector/editor rather than treating the path as an unknown command.

On the first indexing action in an administrator session, Ottrin offers explicit choices:

1. **Continue without a root Ottrin index** — use direct browsing and available system plocate results.
2. **Index selected locations for this administrator session** — temporary root-owned indexing that ends with the session.
3. **Set up a persistent root index (advanced)** — install and start a separately named root service only after confirmation.

The advanced explanation must state that the ordinary user's index may still be running, so the machine can have two independent Ottrin indexers, databases, watcher sets, storage footprints, and refresh work. It shows the selected root indexing scope and estimated/observed database size. Root content extraction remains separately disabled until explicit roots and file types are selected.

A root profile never connects to or mutates an ordinary user's daemon, socket, configuration, database, or cache, even when `sudo` preserves `HOME`, `XDG_*`, or other user-session environment variables. Persistent root operation uses a deterministic root-owned service name and protected root-owned configuration, state, database, lock, and socket namespace. Ordinary users cannot query that private index. Uninstalling the root profile stops the root service and removes only its generated integration/data according to the user's purge choice.

Merely opening or editing a file as root does not start, install, or retain a root daemon. Persistent service creation is a distinct confirmed action; dismissing the choice leaves no background root process behind.

System updatedb maintenance remains a separate fixed operation. Enabling it does not enable the root Ottrin index, and enabling the root Ottrin index does not silently change the system plocate schedule.

### Privileged text editing

The administrator editor uses the same fail-closed text-save contract as ordinary editing: complete bounded UTF-8 regular files only, no symlink following, identity/generation conflict detection, atomic publication, file and parent-directory durability, and preservation of supported owner/group/mode metadata. Files with hard links, special mode bits, unsupported extended metadata, sparse layout, excessive size, or an intervening mutation remain read-only or fail without replacing the source.

The title bar and save confirmation surface identify the administrator session, but routine saves do not use scolding language or add repetitive confirmation prompts solely because the user intentionally launched Ottrin as root.

A privileged Ottrin index is still unnecessary for the default broad filename use case because plocate normally provides that coverage. It exists for users who deliberately want richer root-visible metadata or content over explicitly selected protected locations, not merely to duplicate plocate automatically.

## Existing-user migration

Do not silently overwrite an existing user's search choices. Existing configurations keep their native roots and content policy. Settings reports detected plocate health and offers the combined system filename source. Fresh installations may enable a healthy queryable system database during the visible setup flow without requesting elevation; scheduling or running updatedb always requires an explicit action.

## Acceptance gates

- Global filename search works through plocate with zero native roots.
- Global plocate results are not filtered to native roots.
- Native and system duplicates produce one result, preferring native metadata.
- Inaccessible but database-visible paths have truthful unknown metadata and retain normal authorization boundaries.
- Current-folder scope cannot leak results outside its selected root.
- Content queries never use plocate and never cross configured content roots.
- Existing distro updatedb maintenance is detected and not duplicated.
- Ottrin-managed maintenance can be installed, inspected, run, and completely removed.
- A deliberate administrator profile is isolated from every ordinary-user config/database/socket/cache namespace, explains that two independent indexers may run, and creates session or persistent root indexing only after the corresponding explicit choice.
- Direct root file launch/editing works without enabling an index; administrator text saves retain the same fail-closed identity, metadata, atomicity, and durability contract.
- Setup and health UI explain the three layers without calling plocate a last-resort fallback.
