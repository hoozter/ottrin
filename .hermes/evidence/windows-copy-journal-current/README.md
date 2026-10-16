# Windows Copy paired staging journals

Source-only native acceptance; public Windows Copy remains disabled.

The compiled test executable is pinned in `TEST_BINARY.json`; its debug-stripped
transport preserves every retained PE section. `ACCEPTANCE.json` pins source
files and separates component, full-platform and special audit scopes.
`COPY_platform.stdout` includes real exit-child launches; their parent asserts
exit 86 and inspects retained journals/source bytes. The 22 injected flush errors
exercise all 11 record sequences in both mirrors. No successful inspection grants
candidate deletion authority.

Reproduction: reuse the owned no-network overlay and ordinary-user actor helpers
from the prior `windows-copy-links-current` checkpoint, substitute the unique
`ottrin-init-windows-copy-journal-qa` owner name, and build with:

```sh
cargo test -p ottrin-platform --target x86_64-pc-windows-gnu --lib --no-run --message-format=json
cargo test -p ottrin-platform --lib
cargo clippy -p ottrin-platform --all-targets -- -D warnings
cargo clippy -p ottrin-platform --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

Run `copy_transaction::windows --nocapture --test-threads=1` as the owned limited
interactive user, then the full platform test executable under the same account.
The ignored process-exit child is selected only by its parent. SYSTEM executes
only the explicit audit fixture, never the ordinary-user suite. Developer Mode
is a disposable-fixture capability, not a production change. Original profile and
installation hashes are checked before normal shutdown and exact overlay disposal.

Remaining publication/recovery/Undo, metadata/storage and package gates are in
`docs/WINDOWS_COPY_DESIGN.md` and the current roadmap checkpoint.
