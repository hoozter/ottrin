# Linux repository setup and recovery

GitHub Releases is the manual download location. The planned update origin is
`https://updates.ottrin.com` on the `ottrin` Cloudflare R2 bucket. APT uses
`/apt`, with suite `stable` and component `main`. DNF uses `/rpm`.
The signed native installers can register these sources for subsequent
updates through the system package manager.

APT scopes the embedded key to the Ottrin source. Fedora DNF can ask to
confirm the same signing fingerprint twice on first use. It keeps a
repository-specific key for metadata and imports the package verification key
into the RPM database. Ottrin's DNF source is restricted to the `ottrin`
package. Uninstall removes the source and the packaged certificate file.
The package manager owns its imported keys.

## Before enabling publication

- Set up a dedicated OpenPGP release signing identity. Keep the primary
  certification key offline and export only the signing subkey to the
  `LINUX_SIGNING_PRIVATE_KEY` Actions secret. Set
  `LINUX_SIGNING_KEY_FINGERPRINT` to the approved primary fingerprint.
  The current unattended workflow needs an importable signing subkey that
  can sign without an interactive passphrase prompt. Select the signing
  subkey by its full fingerprint with
  `gpg --armor --export-secret-subkeys "SIGNING_SUBKEY_FINGERPRINT!"`.
  Both workflow jobs check that the approved primary secret is absent
  and exactly one signing subkey is available. The fingerprint variable
  continues to identify the primary key.
- Back up an encrypted export of the full secret key, its public key, and
  revocation certificate to the Enpass vault. Record the fingerprint and
  keep the export passphrase recoverable. Restore into a separate GnuPG home
  on the replacement machine, compare the fingerprint, and verify a fresh
  signature against the backed-up public key before replacing the CI secret.
- Attach the update hostname to the bucket as an R2 custom domain. Confirm
  anonymous HTTPS reads and configure Cloudflare to respect origin cache
  headers for the update hostname. Bypass edge caching for mutable metadata
  and the release receipt. Packages and content-addressed indexes can be
  cached. Use a bucket-scoped Object Read & Write token. Actions uses
  `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` as secrets and
  `R2_ACCOUNT_ID` as a repository variable.
- Set `LINUX_REPOSITORY_PUBLIC_ORIGIN` to the HTTPS origin used by the
  publisher. Enable `LINUX_REPOSITORY_STAGING_ENABLED` to build and inspect
  signed snapshots in Actions. Enable `LINUX_REPOSITORY_PUBLISH_ENABLED`
  only after verifying publication and native updates in isolation. Set
  `LINUX_REPOSITORY_URL` for the official package builds only after the
  production endpoint is verified. That separate switch adds the update
  source to new DEB and RPM packages.

## Release and recovery

The release workflow builds signed artifacts for a version tag and creates a
GitHub draft. Review and publish that draft on GitHub to start the repository
job. The job stages exact assets from the latest published stable release.
The publishing step uploads and verifies versioned packages and immutable
index files before replacing the signed channel metadata. Keep old objects
available for clients that fetched older metadata. Do not run a second
publisher or upload directly to live channel paths.

The `/linux-release.json` receipt says `publishing` when the metadata switch
has started and `published` after the public checks pass. If the job fails,
inspect the failed step and manually run the release workflow on the default
branch with the current published stable tag. Retries verify existing package
bytes, refuse collisions, and prevent older tags from taking over. APT's
inline signature is switched last. RPM uses two separate objects for its
index and signature. An interruption between those writes can cause a
temporary signature rejection until a retry completes.

Treat key rotation as a planned release migration. Distribute and verify the
replacement trust material through the existing trusted channel before
switching signatures. Test upgrade, rollback protection, and trust behavior
on both package managers. The current publisher intentionally rejects
changes to the existing public key objects. Hosting can move behind the
same HTTPS hostname by copying all immutable and live objects, checking the
new origin, then switching DNS while keeping the old origin available until
clients and caches have transitioned.
