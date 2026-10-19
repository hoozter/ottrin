# Temporary control lifetime: rejected guarded candidate

Application baseline: `9f01fd04`. No application replacement or package is accepted
by these controls. Public Windows Copy stays disabled.

The first six scopes in `SUMMARY.json` examine creation rights, permission
restoration and deletion disposition under the ordinary `OttrinCopyQA` account.
The `pending-control` sequence passed tests made **after** freezing permissions:
creator writes remained possible, later writer/EA/attribute/owner/DACL opens were
refused, and cancel-before-restoring permissions preserved post-promotion streams.
Those observations do not prove the creation interval is safe.

`preheld-permission/read-control` demonstrates the missing interval. A second
handle is acquired with WRITE_DAC and READ_CONTROL while the initial descriptor
allows it. Freezing the descriptor on the creator succeeds. The pre-held handle
then restores full permissions, creates and flushes 17 bytes of unrelated named
stream data, and classic pending deletion succeeds. Closing the handles removes
both the control and that stream. All relevant API calls succeed, and neither
file nor stream survives. This is a deliberately scheduled native interleave,
not a probabilistic race or an application acceptance test. It uses the proposed
security/disposition sequence, not the complete Rust constructor.

The first `preheld-permission` control omitted READ_CONTROL. Its permission
restoration failed with access denied, so it did **not** reproduce the attack.
Its shared observer also rejected the missing `mode` output field after downloading
and hashing the raw output. That receipt is retained as an unsuccessful diagnostic;
the corrected child scope has `mode: 0` and completes the shared observer.

The Rust candidate is archived, not installed, in
`../windows-guarded-control-current/rejected-candidate.patch.gz` with source hashes.
The initial native run passed one of five tests. After correcting a malformed
stream-open test and pending-link validation, two primitive tests passed while
three journal initialization tests failed. A diagnostic run identified that
Windows removes inheritance flags when applying a DACL to a regular file.
A corrected promotion build succeeded but was not dispatched: the independently
observed pre-held permission capability already invalidated its safety model.
The candidate application and harness edits were removed from the worktree.
These failed/partial runs are not production acceptance.

The next implementation must avoid automatically deleting an object while an
unrelated data or permission capability can exist. Checking the DACL or streams
again just before enabling deletion does not revoke an already-open handle.
An uncertain empty/partial preparation may be retained for review; a filename,
age or incomplete frame cannot authorize its deletion. Any persistent-publication
replacement must keep canonical records complete and preserve restart admission.
This finding does not change previous narrower successful receipts.

Per-scope DISPATCH/RESULTS, hashed raw output and source identify the observations.
`TEARDOWN.json` records original profile/install/login preservation, absent owned
processes/temporary roots and the stopped isolated guest. `HOST_CLEANUP.json`
records removal of consumed probe executables after their consumers stopped.
