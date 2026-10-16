# Windows Copy private ownership and DACL validation

Source/native component acceptance; public Windows Copy remains disabled.

The format-v2 paired journal records the effective token's default owner SID,
protected private transaction/journal security, and each candidate's owner/group,
DACL bytes and inheritance/protection flags before acknowledging creation. A
private record admits only explicit OWNER RIGHTS full access. Serialized records
are structurally validated as bytes and never applied as Windows descriptors.

Staging checks parent security before child creation, candidate security before
payload acknowledgement and every candidate before terminal verification.
Recovery requires the same effective default owner and verifies exact private
security around its content scan. These checks detect changes; they do not claim
to exclude an independent owner's WRITE_DAC handle or an existing writable map.
Changed ACLs/groups and payloads are retained, never repaired or removed.

Reproduce with the owned no-network COW/limited interactive user helpers from
`windows-copy-links-current`, substituting the unique owner
`ottrin-init-windows-copy-private-security-qa`. Build and check:

```sh
cargo test -p ottrin-platform --target x86_64-pc-windows-gnu --lib --no-run --message-format=json
cargo test -p ottrin-platform --lib
cargo clippy -p ottrin-platform --all-targets -- -D warnings
cargo clippy -p ottrin-platform --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

Run `copy_transaction::windows --nocapture --test-threads=1` and the full platform
suite as the owned ordinary user. Only the explicit audit fixture runs as SYSTEM.
Developer Mode is confined to the disposable overlay for existing link fixtures.
The production primitive keeps its existing rights and never enables privileges.

The first native run exposed a group-drift fixture opening without WRITE_OWNER,
which returned error 5. Rejected logs and the binary pin are retained. The fixture
now opens that right separately only for its controlled group change. Production
staging and recovery rights are unchanged. Microsoft documents the required group
setter right in [SECURITY_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-information).

Final payload security, complete SACL/audit/label drift, Overwrite/publication,
rights acquisition for guarded mutation, central/interrupted recovery, Undo and
exact public package workflows remain required. Passing validation is not deletion
or publication authority. Cleanup checks original profiles/installations and
removes only the owned domain/volume/NVRAM/TPM after normal shutdown.
