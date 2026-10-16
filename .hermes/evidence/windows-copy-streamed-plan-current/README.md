# Windows Copy streamed permission intent

Source/native component acceptance; public Windows Copy remains disabled.

The monolithic permission-plan record is replaced with one header, ordered per-entry
records and a terminal count/digest seal. Partial prefixes remain explicit and
cannot report `security_planned`. Recovery compares each prefix with fresh native
inheritance; it never applies cached descriptors. Complete plans still require
native rederivation. Old monolithic events are not interpreted by a compatibility
path. Existing frame/log budgets remain 16 MiB/64 MiB.

Digest serialization writes directly to a SHA-256 sink. Entry frames are bounded
individually. The codec test accepts a synthetic 2,001-node recorded graph whose
aggregate plan exceeds the old frame budget while each frame and the total log
stay within current bounds. It does not open or copy 2,001 real files, apply their
ACLs or claim broad large-directory filesystem/resource acceptance.

Staging validation now uses replay-built object/name indexes rather than repeated
full-tree alias searches. Hard-linked sources remain allowed; candidate aliases
and repeated names remain refused. Cancellation is checked between plan records.
A safely cancelled prefix can resume without duplicate records; uncertain writes
still poison their owner and cannot be retried or repaired automatically.

Native coverage includes malformed/reordered/missing/duplicate entries and invalid
seals, partial inspection/rederivation without source files, cancelled-prefix
resume, all plan-append flush boundaries and eighteen real process exits. Existing
kernel-tree/link ACL reference comparisons, content/ownership checks and forged
cached-ACL rejection remain in the same suite.

Reproduce using the no-network COW/limited-user helpers from
`windows-copy-links-current`, with owner `ottrin-init-windows-copy-streamed-plan-qa`:

```sh
cargo test -p ottrin-platform --target x86_64-pc-windows-gnu --lib --no-run --message-format=json
cargo test -p ottrin-platform --lib
cargo clippy -p ottrin-platform --all-targets -- -D warnings
cargo clippy -p ottrin-platform --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

Run `copy_transaction::windows --nocapture --test-threads=1` and the complete
platform suite under the owned ordinary user. Only the explicit audit case runs
as SYSTEM. The full actor records its runtime executable hash. Developer Mode
exists only in this disposable overlay for link fixtures; production does not
change it or enable privileges. Normal-shutdown cleanup verifies original
profiles/installations and removes only the owned VM/volume/NVRAM/TPM.

Final ACL application and permission-transition recovery, publication, Overwrite,
Undo/cleanup, real large-tree/storage resource gates and exact public packages
remain required. No current deliverable is replaced by this internal work.
