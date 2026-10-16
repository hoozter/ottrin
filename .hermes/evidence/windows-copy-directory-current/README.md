# Windows Copy directory payload acceptance

See ACCEPTANCE.json for the precise source-only scope and remaining requirements.
TEST_BINARY.json pins compiler output and the transferred stripped image; retained
PE sections were compared. COPY_* and FULL_* are the final ordinary-user runs.
The explicit audit case uses audit-actor.ps1 under QGA SYSTEM. Its initial file
invocation was blocked by execution policy before any test ran; the same actor
text then passed through encoded-command transport without changing machine policy.

INITIAL_* records the fresh COW prerequisite failure: directory payload tests
passed, while two existing symlink fixture setups needed Developer Mode.
PRELIMINARY_* records that prerequisite correction before the last two tests were
added. INITIAL_TEST_BINARY.json pins that preliminary binary.

FINAL_GUEST.json confirms no owned executable left running and unchanged original
preferences/installation. FINAL_CLEANUP.json records normal shutdown and removal
of the owned COW disk/domain/NVRAM/TPM, preserving backing images and stopped
original domains. Text logs normalize CRLF/trailing whitespace only. Build and
Clippy logs are disposable; test results and source/binary pins are compact evidence.
Public Windows Copy remains disabled and packages were not rebuilt for this work.
