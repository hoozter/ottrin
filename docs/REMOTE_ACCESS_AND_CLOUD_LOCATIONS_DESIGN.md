# Remote access and cloud locations — deferred architecture

Status: deferred design record; **not current roadmap work**
Researched: 2026-09-02
Revisit only after the current Ottrin roadmap and release acceptance are complete.

## Decision summary

Ottrin should treat these as three distinct products:

1. **Live Ottrin-to-Ottrin remote access** — browse a folder that remains on another online computer. The host is authoritative; this is not synchronization.
2. **Cloud-backed locations** — browse Google Drive, Dropbox, OneDrive, WebDAV, and similar providers. Prefer an existing OS-visible folder where it provides honest filesystem semantics; add provider adapters only where the OS does not.
3. **Cloud synchronization** — retain replicated local/offline copies and reconcile concurrent changes. This is useful, but it is a separate future feature with substantially larger conflict, storage, recovery, and trust obligations.

For live remote access, use a small Cloudflare control plane and a peer data plane:

- **Cloudflare Worker** for the authenticated API and short-lived connection grants;
- **D1** for accounts, device public keys, explicit folder grants, revocations, and bounded audit metadata;
- **hibernating Durable Object WebSockets** for online presence and WebRTC signalling only;
- **WebRTC data channels** for the file protocol, using direct ICE paths when possible;
- **Cloudflare STUN** for direct-path discovery and **Cloudflare TURN** only as an encrypted NAT-traversal fallback;
- **Tailscale/direct LAN** as an optional transport that bypasses WebRTC signalling/relay when both devices already have a route.

Cloudflare must never receive file encryption keys, plaintext file contents, a browsable filename index, or arbitrary host paths. Remote search executes on the host and returns only authorized results.

## Does the account system need a database?

An account-based service does. It needs durable answers to questions that cannot safely live only in an ephemeral WebSocket:

- which external identity owns an Ottrin account;
- which device public keys belong to that account;
- which device shared which folder capability with which peer;
- whether a device, session, or grant has been revoked;
- which short-lived connection grant has already been consumed;
- enough security-event metadata to investigate abuse without logging filenames or file contents.

A database is **not** needed to store the files. The host computer remains the sole file store in the live-access model.

A pairing-only prototype could avoid conventional accounts by exchanging capability material directly through a QR code, but revocation, lost-device recovery, multiple devices, and cross-network discovery still need a small durable directory. Avoiding the word “account” would not remove that state; it would merely hide it.

## Recommended Cloudflare control plane

### Worker API

The Worker should expose only compact control operations:

- begin/complete OIDC sign-in;
- register, name, rotate, and revoke a device key;
- create/revoke an explicit folder grant;
- announce device presence;
- exchange authenticated WebRTC offers, answers, and ICE candidates;
- issue short-lived TURN credentials after both peers and the grant are authorized;
- retrieve the caller's devices, grants, and recent security events.

It must not proxy ordinary file reads/writes. Keeping file traffic off Workers avoids body/CPU limits and prevents the control plane from becoming an accidental cloud file service.

Cloudflare's Workers Free plan currently provides 100,000 requests/day with 10 ms CPU per invocation. That is ample for a small, indexed control API, but authentication and authorization handlers must remain deliberately small.[1]

### D1

Use one D1 database with indexed account/device/grant tables. D1 is the better first choice than Neon because:

- it is directly bound to Workers, so there is no external database credential or network hop;
- it scales to zero and is operationally small;
- the Free plan currently includes 5 million rows read/day, 100,000 rows written/day, and 5 GB total storage; free-limit exhaustion fails requests rather than creating D1 overage charges;[2]
- a free D1 database is limited to 500 MB and the account to ten databases, still far beyond the metadata needed for an early deployment;[3]
- Time Travel is always enabled, with seven days of point-in-time recovery on the Free plan;[4]
- an EU jurisdiction can be selected when the database is created.[5]

Use proper indexes on account ID, device ID, grant ID, host device ID, grantee account ID, and expiry. D1 charges rows scanned, not just rows returned, so unindexed lookups are both slower and waste the free allowance.[2]

Suggested minimum schema:

```text
accounts(
  id, oidc_issuer, oidc_subject, created_at, disabled_at
)

devices(
  id, account_id, display_name, identity_public_key,
  created_at, last_seen_at, revoked_at
)

shares(
  id, host_device_id, grantee_account_id,
  opaque_host_capability_id, permission_set,
  created_at, expires_at, revoked_at
)

session_refresh_tokens(
  id, account_id, device_id, token_hash,
  created_at, expires_at, revoked_at
)

connection_grants(
  id, share_id, host_device_id, client_device_id,
  nonce_hash, created_at, expires_at, consumed_at
)

security_events(
  id, account_id, device_id, event_kind,
  coarse_network_metadata, created_at
)
```

Do not store source paths, filenames, directory listings, preview text, search indexes, file hashes, file bytes, device private keys, raw refresh tokens, or TURN long-term keys in D1.

### Durable Objects

Use one short-lived coordination object per account or host device for presence and signalling. The hibernation WebSocket API keeps clients connected while allowing an idle object to leave memory; Cloudflare explicitly recommends it for this shape.[6]

Do not put durable account truth in the signalling object's memory. On wake, authorize from D1 and reconstruct only the minimum connection state. Do not route file chunks through the Durable Object.

The Workers Free plan currently includes Durable Objects, with 100,000 requests/day, 13,000 GB-s/day, and SQLite-backed storage allowances. Exceeding a free dimension causes that operation to fail.[7] Hibernation is essential: a non-hibernating connected WebSocket continues consuming duration.[7]

### Authentication

For the first account implementation:

- use standard browser-based OIDC Authorization Code + PKCE;
- start with one or two mainstream identity providers rather than storing passwords;
- bind every Ottrin installation to an independently generated device identity key;
- place the device private key in the OS credential/key storage, never D1;
- issue short-lived API access tokens and rotating refresh tokens;
- store only a strong hash of each refresh token;
- require a fresh account authentication or an already trusted device to add/recover a device;
- show every device and folder grant in a revocation screen.

Email address is display/contact data, not identity. The stable key is `(issuer, subject)`. Account deletion must revoke sessions/grants first, delete control-plane metadata, and leave host files untouched.

Passkeys are a sensible later authentication option, not a reason to design a password database now.

## Data plane and protocol

### Transport order

Attempt transports in this order:

1. existing direct LAN route;
2. explicitly configured Tailscale route;
3. direct WebRTC ICE path using STUN;
4. WebRTC through TURN.

The UI should state which path is active: `Local network`, `Tailscale`, `Direct encrypted connection`, or `Encrypted relay`. “Peer-to-peer” must not be shown when TURN is carrying the packets.

Tailscale remains an optional fast path, not a required Ottrin dependency or an Ottrin account substitute. Its Personal plan is currently free for up to six users,[8] but requiring users to join one tailnet would not be a credible general-consumer design.

### TURN and the zero-cost requirement

Cloudflare's STUN service is documented as free and unlimited. Realtime TURN currently includes the first 1,000 GB/month, then charges $0.05/GB of egress.[9] With WebRTC, DTLS encrypts data channels end-to-end; Cloudflare sees connection metadata but cannot read the channel payload.[9]

That makes TURN technically suitable, but **the published free allowance is not a hard spending cap**. Cloudflare budget alerts are informational, evaluate accumulated spend, and can arrive after usage has crossed the threshold.[10] Therefore:

- direct/Tailscale paths may be offered under an absolute free-tier requirement;
- TURN must remain disabled in a strict no-spend deployment unless Cloudflare provides a verified hard cap;
- if TURN is later enabled, Ottrin's server must maintain a conservative internal byte budget, stop issuing credentials well below the allowance, revoke active credentials at the budget boundary, and still describe this as risk reduction rather than a provider-enforced zero-cost guarantee;
- TURN credentials must be generated server-side, short-lived, scoped to an authorized connection, and revocable. The long-term TURN key never ships in Ottrin.[11]

A connection that cannot obtain a permitted route must say `Remote computer unreachable` or `Relay unavailable`; it must not show an empty folder.

### Application protocol

Run a versioned Ottrin file protocol over the selected encrypted channel. Every request includes:

- protocol version;
- connection-grant ID and monotonically increasing request ID;
- opaque share ID and provider-relative item ID;
- declared operation and bounded parameters;
- expected generation/version for any mutation.

Start with:

```text
Capabilities
ListChildren(parent_id, cursor, page_size)
Stat(item_id)
ReadRange(item_id, generation, offset, length)
Download(item_id, generation)
Search(share_id, query, cursor, page_size)
```

Add only after read-only acceptance:

```text
BeginUpload(parent_id, name, expected_parent_generation, length)
WriteUpload(upload_id, offset, bytes)
CommitUpload(upload_id, content_hash)
Rename(item_id, expected_generation, new_name)
Move(item_id, expected_generation, destination_id)
Trash(item_id, expected_generation)
```

Uploads use host-local staging and atomic publication. Mutations fail on generation mismatch and return a visible conflict; they never silently overwrite. Search runs at the host and is constrained to the exact granted root.

WebDAV is useful precedent because it specifies collections, properties, locking, copy/move, and authoring semantics over HTTP.[12] Ottrin-to-Ottrin should not expose a generic WebDAV server, however: a capability-scoped protocol can preserve Ottrin's typed errors, source-generation checks, and narrow share boundary without exposing arbitrary URL paths.

## Authorization and host safety

A share grant is a capability rooted at one directory selected by the host user. It contains no reusable host path and cannot escape upward.

Required rules:

- default permission is read-only;
- `download`, `write`, `rename/move`, `trash`, and `search` are separate capabilities;
- permanent delete, chmod, symlink creation, executable launch, shell access, and privileged-helper forwarding are never remotely available in the initial design;
- resolve every item beneath an already-open root descriptor/handle and reject symlink/reparse-point escapes;
- never translate a remote request into `PrivilegedCommand`;
- apply byte, item-count, depth, duration, request-rate, concurrent-stream, preview, and search-result limits;
- bind each response to the requested generation so replaced files cannot masquerade as the earlier object;
- log security events, not content or filenames;
- revoke immediately on host request, device removal, account disablement, or suspicious replay.

## Why this needs an Ottrin location abstraction first

Current file operations encode local filesystem paths directly in every `FileCommand` variant (`crates/ottrin-core/src/lib.rs:1197-1238`). Linux copy recursively invokes `std::fs` on local `Path` values (`crates/ottrin-platform/src/lib.rs:3451-3522`). Pretending a remote object is a `PathBuf` would leak local-only assumptions into navigation, preview, mutation, Undo, indexing, and error handling.

Before remote providers, introduce concepts equivalent to:

```text
LocationId
ProviderId
ItemId { location_id, opaque_provider_id }
ItemGeneration
LocationCapabilities
LocationAvailability { online, offline, authorization_required, unavailable }
```

Then route operations through a provider interface rather than teaching individual UI actions to special-case URLs. Local filesystem behavior remains one provider and must not be weakened to fit the remote case.

Ottrin already preserves unavailable storage locations instead of presenting them as silently empty (`docs/STORAGE_LOCATIONS_STATUS.md:14-21`). Remote locations should inherit that product truth.

## Cloud accounts such as Google Drive, Dropbox, OneDrive, and WebDAV

### First preference: navigate what the OS already exposes

When a provider's desktop client presents an ordinary or virtual filesystem location with sufficient native semantics, Ottrin should discover and browse it like any other location. That avoids owning OAuth tokens, provider API drift, synchronization, local cache policy, and conflict resolution.

This works well for many Windows/macOS users. Google Drive for desktop exposes a virtual drive, but Google's own requirements page says Drive for desktop is unavailable on Linux.[13] Therefore “use the installed folder” is a good first path, not a cross-platform completion claim.

Ottrin must also distinguish:

- fully local files;
- placeholders/online-only files that may hydrate on open;
- provider-offline locations;
- provider errors and quota/authentication failures.

It must not mistake a hydration delay for an empty or corrupted file.

### Second preference: provider adapters, still not sync

After the general location abstraction exists, add direct adapters only where users lack a reliable OS mount:

1. **WebDAV** first: open standard, broad server support, clear collection/read/write operations. Nextcloud documents WebDAV access across Linux, macOS, Windows, and mobile clients and recommends app passwords for third-party clients.[14]
2. **Google Drive** next if Linux demand warrants it. Use OAuth and request the narrowest practical scope; Google explicitly recommends narrowly focused scopes.[15]
3. Dropbox/OneDrive after demand and API/permission review.

Provider adapters browse remote objects and stream/download on demand. They are not synchronization engines and should be labelled `Cloud location`, including explicit online/offline and placeholder states.

### Cloud sync remains separate

A real sync feature requires its own accepted design for:

- durable local manifests and provider cursors;
- offline edits and two-sided conflicts;
- rename/move/delete detection;
- atomic downloads/uploads and crash recovery;
- placeholder semantics and storage budgets;
- case sensitivity, Unicode, illegal-name, timestamp, and permission mismatches;
- encryption model and recovery;
- background scheduling, battery/network policy, and truthful status.

Do not let a provider adapter quietly become half a sync engine.

## Failure and privacy model

The interface must distinguish at least:

- host offline;
- account authorization expired;
- device revoked;
- share revoked or expired;
- direct path failed, relay unavailable;
- host denied operation;
- item changed since listing;
- source unreadable or disappeared;
- quota/rate/resource limit;
- partial transfer, safely resumable;
- protocol/version incompatible.

No one of these should render as an empty directory.

Control-plane retention should be short and documented. Security logs should use coarse IP/network metadata only where necessary for abuse response, with an explicit deletion period. Folder names and paths should remain end-to-end data, not account-directory metadata.

## Alternatives considered

### Neon/Postgres

Neon is credible when Ottrin outgrows D1 or needs portable Postgres semantics. Its current Free plan provides 100 CU-hours/project, 0.5 GB storage/project, 5 GB public transfer, scale-to-zero after five minutes, and no SLA.[16] It adds an external credential, network hop, pool/connection concerns, and a second vendor without solving a present control-plane requirement. Do not start there.

Define a small repository/service boundary around account and grant persistence so D1 can later be migrated to Postgres without rewriting authorization policy.

### Cloudflare Tunnel

Tunnel creates outbound-only connections and is available on all plans.[17] It is useful for a self-hosted/advanced configuration, but making Ottrin provision public host routes under one central Cloudflare account creates broader origin, abuse, access-policy, hostname, and product-coupling obligations. It is not the default consumer data plane.

### Central file relay in Workers/Durable Objects/R2

Rejected for live access. It creates cloud storage/retention semantics, consumes provider limits, enlarges the breach surface, and muddies the promise that the file stays on the host. R2 may be appropriate only for a separately approved encrypted cloud-sync product.

### Generic direct-IP listener

Keep as an advanced LAN/manual option only. A default unauthenticated or globally reachable inbound listener is unacceptable. Any direct listener still requires device authentication, capability-scoped shares, replay protection, and clear firewall guidance.

## Deferred implementation sequence

Nothing below begins before the current roadmap is complete.

1. **Architecture seam:** introduce typed locations/items/capabilities while preserving all local behavior.
2. **LAN/Tailscale read-only prototype:** explicit pairing, one shared root, list/stat/range-read/download, host-online truth, no cloud account requirement.
3. **Cloudflare control plane:** OIDC accounts, device keys, D1 grants/revocation, hibernating signalling, direct WebRTC.
4. **TURN decision gate:** verify Rust WebRTC interoperability, throughput, provider hard-cost controls, credential rotation, and the zero-cost policy before enabling relay.
5. **Read-only product acceptance:** hostile path tests, revocation, reconnect, large-file streaming, generation replacement, offline UX, privacy review.
6. **Controlled writes:** staged upload, generation conflicts, atomic publication, bounded rename/move/trash; no remote privilege operations.
7. **Cloud locations:** OS-visible locations first, then WebDAV, then demand-led provider adapters.
8. **Cloud sync decision:** only as a separately scoped roadmap outcome.

## Acceptance gates for the future remote feature

- A host shares one selected folder without exposing siblings or parent paths.
- Revocation terminates existing sessions and rejects replayed grants.
- Direct and relayed paths have identical authorization and file semantics.
- Relay/provider operators cannot read file/channel payloads.
- Offline/unreachable never appears empty.
- Replacement during read either completes against one generation or fails explicitly.
- Interrupted uploads never publish partial destination files.
- No remote operation reaches the privileged helper.
- The host can see active sessions and terminate them.
- Free-tier exhaustion fails closed and does not silently incur cost.
- Documentation states connection route, metadata exposure, limitations, and retention honestly.

## Sources

1. Cloudflare, “Workers pricing,” 100,000 requests/day and 10 ms CPU on Free: <https://developers.cloudflare.com/workers/platform/pricing/>
2. Cloudflare, “D1 pricing,” Free rows/storage allowances and exhaustion behavior: <https://developers.cloudflare.com/d1/platform/pricing/>
3. Cloudflare, “D1 limits,” per-database/account Free limits: <https://developers.cloudflare.com/d1/platform/limits/>
4. Cloudflare, “D1 Time Travel and backups”: <https://developers.cloudflare.com/d1/reference/time-travel/>
5. Cloudflare, “D1 data location,” including EU jurisdiction: <https://developers.cloudflare.com/d1/configuration/data-location/>
6. Cloudflare, “Use WebSockets,” Durable Object hibernation behavior: <https://developers.cloudflare.com/durable-objects/best-practices/websockets/>
7. Cloudflare, “Durable Objects pricing”: <https://developers.cloudflare.com/durable-objects/platform/pricing/>
8. Tailscale, “Free pricing plans and discounts”: <https://tailscale.com/docs/account/manage-plans/free-plans-discounts>
9. Cloudflare, “Realtime TURN FAQ,” pricing, STUN, encryption, and metadata: <https://developers.cloudflare.com/realtime/turn/faq/>
10. Cloudflare, “Budget alerts,” notifications rather than enforcement: <https://developers.cloudflare.com/billing/manage/budget-alerts/>
11. Cloudflare, “Generate TURN credentials,” server-held long-term key and expiring credentials: <https://developers.cloudflare.com/realtime/turn/generate-credentials/>
12. IETF RFC 4918, WebDAV collections, properties, locking, copy, and move: <https://datatracker.ietf.org/doc/rfc4918/>
13. Google, “Drive for desktop compatibility,” including no Linux desktop client: <https://support.google.com/drive/answer/2375082?co=GENIE.Platform%3DDesktop&hl=en-GB>
14. Nextcloud, “Accessing Nextcloud files using WebDAV”: <https://docs.nextcloud.com/server/stable/user_manual/en/files/access_webdav.html>
15. Google, “Choose Google Drive API scopes”: <https://developers.google.com/workspace/drive/api/guides/api-specific-auth>
16. Neon, “Neon plans”: <https://neon.com/docs/introduction/plans>
17. Cloudflare, “Cloudflare Tunnel”: <https://developers.cloudflare.com/tunnel/>
