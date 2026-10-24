# Owned Windows desktop workflow checks

This driver uses the retained `ottrin-copy-cleanup-qa` guest through the existing
QEMU guest-agent transport. It does not control the host desktop. It creates one
ordinary temporary `OttrinWindowQA` account and portable fixtures under
`C:\OttrinWindowQA`. Start with the guest off, record its persistent definition
and an `OWNERSHIP.json` containing `archive`, `package_source`, `archive_sha256`
and `payload_manifest` (the package cohort's verified `PAYLOAD.json`), then boot it.
The guest must have no application work or existing autologon password to replace.

Use `python3 scripts/windows-desktop-qa/desktop.py --evidence-dir PATH OP`.
An explicit owned evidence directory is required. Use a new directory for a
different package; preserve the earlier receipt's scope.

1. `prepare` verifies the archive/manifest/source agree before any guest mutation,
   saves protected restoration data, creates the account, verifies and
   extracts the exact candidate. Reboot the guest once into the temporary account.
2. Finish the temporary account's first-login onboarding before launching Ottrin.
   JSON operations `prepare`, `launch`, `observe`, `probe`, `click`, `key`, `nav`, `verify`
   and `close` drive/check that account. Window input checks the target process;
   inspect screenshots before choosing coordinates. `probe` reports native point
   ownership without sending input. First-login onboarding input
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

For cross-volume Move acceptance, after entering the temporary account and before
case preparation, use `scripts/windows-cross-move-qa/volume.ps1 -Action prepare
-Actor OttrinWindowQA` through the guest agent. Copy its verified descriptor to
`C:\OttrinWindowQA\secondary-volume.json`. The actor validates NTFS volume ID,
label, ownership SID and marker on every action. `secondary\destination` and
`secondary\moved` address only that owned volume; ordinary paths stay in the
private C: fixture. Retained completion checks require source-removed for cross-
volume journals and published for same-volume journals. Close the owned app,
remove the volume with the same volume helper, remove its descriptor, then run
normal account/fixture cleanup. The helper refuses live QA tasks or payloads;
verify the original disk inventory and persistent VM definition after teardown.

For containing-parent lifecycle checks, the actor accepts `fixture-mkdir`,
`fixture-write` (new files only), `fixture-rename` (no replacement) and
`fixture-remove` (ordinary files with an exact SHA-256). Paths use the same
relative fixture/secondary-volume mapping as navigation. Mutation refuses the
fixture roots and reparse ancestors. These actions model external namespace
changes between actual packaged UI operations; they are not product operations.

To seed an interrupted cross-volume Move, close the packaged GUI and transfer
its exact source revision's native platform test executable as `native-tests.exe`.
Place its verified `source_commit`, original `compiler_sha256`, and transferred
`transferred_sha256` in `native-seeder.json`. Debug stripping must match the
previous native verification receipt and preserve every non-debug PE section.
`seed-move-recovery` accepts relative `source`/`destination`, a `Prepared`, `Staged`
or `Published` phase, and a lowercase `tag` for diagnostic exports. It invokes
the production Move engine through the fixed native interruption test and requires
exit 87 at the selected central journal boundary. Inspect the resulting native
journal/payload evidence before reopening the package. This is fixture creation;
only subsequent real Activity input and verified file/journal state establish
packaged recovery acceptance. Remove the test executable and exports during normal
owned-root teardown.

`nav` types the fixture path as Unicode input without replacing the native Cut
clipboard. `submit: false` captures the editor before Enter. Native Unicode
input follows [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
and [KEYBDINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput);
held modifiers and incomplete insertion refuse the action.
`inspect-moves` captures read-only journal/completion metadata in the owned profile.
`dismiss-shell` sends Escape only to the observed temporary-account Start menu or
Search host, with foreground PID and exact executable/owner checks.
