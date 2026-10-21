# Owned Windows desktop workflow checks

This driver uses the retained `ottrin-copy-cleanup-qa` guest through the existing
QEMU guest-agent transport. It does not control the host desktop. It creates one
ordinary temporary `OttrinWindowQA` account and portable fixtures under
`C:\OttrinWindowQA`. Start with the guest off, record its persistent definition
and an `OWNERSHIP.json` containing the exact archive/source/SHA-256, then boot it.
The guest must have no application work or existing autologon password to replace.

Use `python3 scripts/windows-desktop-qa/desktop.py [--evidence-dir PATH] OP`.
The default evidence directory is the original
`.hermes/evidence/windows-packaged-workflows-current` checkpoint. Use a new owned
evidence directory for a different package; preserve the earlier receipt's scope.

1. `prepare` saves protected restoration data, creates the account, verifies and
   extracts the exact candidate. Reboot the guest once into the temporary account.
2. JSON operations `prepare`, `launch`, `observe`, `click`, `key`, `nav`, `verify`
   and `close` drive/check that account. Window input checks the target process;
   inspect screenshots before choosing coordinates. First-login onboarding input
   additionally requires the observed foreground PID and the owned `wwahost.exe`.
   Native regression execution requires the transferred executable's SHA-256.
3. An input observer timeout exits 75: use `resume` to observe the same guest PID.
   Do not dispatch a second action because observation expired. For preparation or
   cleanup timeouts, poll the saved `observer.pid` through the transport before
   proceeding with dependent steps; do not repeat a mutation.
4. On completion, failure or cancellation, close the owned application and run
   `cleanup`. It restores login values, removes the temporary credential, task,
   account and profile, then retires fixtures and requests guest shutdown. If
   account teardown succeeded but fixture retirement failed, use `retire` after
   diagnosing the failure. Fixture retirement refuses reparse traversal and never
   resets permissions outside the exact owned root.
5. Verify the guest is off and its persistent definition is unchanged. Remove
   transferred host test archives/executables and retain compact receipts,
   assertions, relevant logs and selected screenshots. Do not retire this guest.

Never infer a successful workflow from dispatch, exit status alone, a visible
window handle, or an unverified screenshot. Check exact fixture bytes and native
test counts, and keep source corrections separate from packaged GUI acceptance.
