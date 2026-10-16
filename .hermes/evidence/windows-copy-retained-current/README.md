# Windows Copy retained candidate validation

This is source/component acceptance. Public Windows Copy is disabled.

The retained-stage reader uses the shared paired-journal decoder, opens the
transaction/journals and candidate entries with read-sharing only, and requires
full staging verification before scanning. It compares native identities, raw
UTF-16 directory membership, payload metadata, link records and stream hashes.
Its handles are not write/delete authority. It neither repairs nor cleans up
interrupted, changed or foreign entries. Private/final security, rights acquisition
for guarded mutation, Overwrite/publication, central discovery and Undo remain
required before public Copy can be enabled.

Reproduction uses the owned no-network Windows overlay/ordinary-user actor helpers
at `windows-copy-links-current`, with the unique owner name
`ottrin-init-windows-copy-retained-qa`. Build commands:

```sh
cargo test -p ottrin-platform --target x86_64-pc-windows-gnu --lib --no-run --message-format=json
cargo test -p ottrin-platform --lib
cargo clippy -p ottrin-platform --all-targets -- -D warnings
cargo clippy -p ottrin-platform --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

Run `copy_transaction::windows --nocapture --test-threads=1` and the full platform
suite as the owned limited interactive account. Only the explicit audit fixture
runs as SYSTEM. Developer Mode is enabled only inside this disposable overlay for
the existing link fixtures. No source access or presence is needed by validation.
The crash parent also checks retained validation after each real child exit;
only a fully verified paired terminal record admits the complete retained tree.

Source and transferred test-binary hashes, actual counts, limitations and cleanup
are in `ACCEPTANCE.json`. Logs distinguish focused Copy, full native platform,
Linux platform and separate audit runs. Original profile/installation hashes are
checked before normal shutdown and disposal of only the owned overlay.

The first native candidate was rejected: ordinary directory/file reads changed
access timestamps, causing two validation tests to fail before the intended
content check or on retry after cancellation. `REJECTED_COPY_*` and the rejected
binary pin retain this causal evidence. The repair keeps strict access-time
checks in final staging while recovery permits read-owned access-time updates;
creation/write times and the full payload/namespace checks remain enforced.
