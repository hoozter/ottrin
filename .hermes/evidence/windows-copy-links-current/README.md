# Windows Copy known links and junctions

Source/component acceptance only; public Windows Copy remains disabled.
ACCEPTANCE.json records source hashes, exact native binary pins and remaining
requirements. TEST_BINARY.json compares retained PE sections before transfer;
FINAL_BINARY.json confirms the same executable after the policy reboot.
COPY_* and FULL_* record final ordinary-user Developer Mode-enabled runs.
Audit is explicitly selected under SYSTEM; POLICY_* and JUNCTION_POLICY_* record
one exact ordinary-user case each after Developer Mode was disabled and Windows
rebooted. POLICY_READY.json binds the new boot, configured policy and ordinary
login. Both policy receipts were additionally checked for the exact test name,
one passing test, zero ignored/failures, ordinary role, exit zero and no temp data.
The policy driver now asserts that narrow result itself; this adds harness
validation without rerunning or claiming a different native job.

INITIAL_* pins the first six-form capture/install test before tree integration.
INTEGRATED_* pins the first integrated tree run. ACL_PROBE_* captures the expanded
suite's fixture failure; ACL_BACKTRACE_* isolates its security-update helper.
WRITE_DAC alone was insufficient for fixture security maintenance. RC/WD restores
that ability while target data/listing-denial assertions remain. No production
permission check, target-read denial or positive test was removed.

The component preserves exact known reparse bytes and a link's own named data,
never copies target contents, never substitutes a junction for a symbolic link,
and does not enable privileges or change Developer Mode. Native fixtures prove
unreadable targets, raw UTF-16, Unicode/empty/large ADS, dangling links, junctions,
retarget detection and default-policy refusal with data retention. Microsoft
reparse/EA restrictions are respected; ordinary non-reparse EA coverage remains.

FINAL_GUEST.json verifies original preferences/installation hashes and no owned
executable left running. FINAL_CLEANUP.json records normal shutdown and removal
of the owned no-NIC COW domain/disk/NVRAM/TPM. Backing storage and stopped original
guests remain intact. Text evidence normalizes CRLF and trailing whitespace only.
Bulky build/Clippy logs are disposable. Existing packages are unchanged.
