# Windows Copy whole-tree destination security intent

Source/native component acceptance. Public Windows Copy remains disabled.

The same native DACL derivation now handles real destination parents and projected
final parents. One held token is used to build the complete permission graph while
all candidates remain private. Native descriptors stay owned; capture/validation
never apply permissions. Evidence records creator, root parent/security, raw UTF-16
names, graph indexes, identities, container roles and private/final security.

The live owner appends this complete intent to both journals after payload/tree
verification. Both flush before preparation succeeds. Repeating the same plan is
idempotent; changed parent permissions or a different interpretation are refused.
Readonly retained recovery rederives native inheritance and compares it with the
record. Serialized descriptors are structurally checked cache data, never native
permission authority. A test changes a cached ACL, recomputes valid hashes and
writes matching mirrors; native rederivation still rejects it without repair.

Native tests compare every projected node with real kernel-created reference trees
under six parent ACL configurations: inherited allow, no inheritance/token defaults,
creator owner, deny entries, NO_PROPAGATE, and distinct file/container inheritance.
Actual kernel-created file/directory links with absent targets are compared too.
Graph errors, cancellation, parent/private-permission/membership drift, source-free
retained planning, idempotence and interrupted plan appends are covered. Existing
crash parents now exercise ten process exits, including both plan-flush boundaries;
original data stays unpublished/private. Source removal after terminal plan exit
still permits readonly native rederivation.

Reproduce with the owned no-network overlay/ordinary-user actor helpers from
`windows-copy-links-current`, using owner `ottrin-init-windows-copy-tree-security-qa`:

```sh
cargo test -p ottrin-platform --target x86_64-pc-windows-gnu --lib --no-run --message-format=json
cargo test -p ottrin-platform --lib
cargo clippy -p ottrin-platform --all-targets -- -D warnings
cargo clippy -p ottrin-platform --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

Run `copy_transaction::windows --nocapture --test-threads=1` and the full platform
suite as the owned limited user. The full actor records its runtime binary hash.
SYSTEM runs only the explicit audit case. Developer Mode is confined to the COW
for existing symlink fixtures; production never enables privileges or changes it.

This does not apply final DACLs or provide permission-transition recovery,
publication, Overwrite, guarded deletion/Undo or public package acceptance.
Complete SACL/label drift, privilege/impersonation workflows and other storage/
metadata gates remain required. Source hashes, exact binary pins, actual results
and guarded normal-shutdown cleanup are in `ACCEPTANCE.json`.

Native derivation follows [CreatePrivateObjectSecurityEx](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createprivateobjectsecurityex),
including container roles, creator-owner substitution, token defaults and owned
descriptor lifetime. Kernel-created references establish the tested behavior.
