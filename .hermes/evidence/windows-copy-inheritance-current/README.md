# Windows Copy candidate inheritance

Source/component evidence only. See ACCEPTANCE.json for scope, counts and source
hashes; TEST_BINARY.json pins the native compiler and stripped transferred image.
The source is not wired to public Windows Copy, which remains disabled.

INITIAL_* logs reproduce the nested-candidate NO_PROPAGATE label/audit failures.
COPY_* and FULL_* record final ordinary-user runs. audit.* and AUDIT_RESULTS.json
record the explicitly selected SYSTEM audit case. The test alone enables audit
privilege. FINAL_GUEST.json verifies original profile/installation preservation;
FINAL_CLEANUP.json verifies normal shutdown and removal of the owned no-NIC COW,
NVRAM and TPM while all original domains remain stopped.

Build/Clippy logs are disposable and not part of the compact committed receipt.
Native actors and transport helpers are retained to make this bounded run
reviewable. Fixture source paths and source/binary hashes do not imply release,
installed GUI or complete Copy acceptance.

Committed text evidence normalizes CRLF and trailing whitespace only.
