# Native temporary lifetime and stream isolation

The latest eight-mode sweep uses the private OWNER RIGHTS descriptor, native
create-new, sharing, reparse and synchronous options used by the current Windows
Copy temporary-file factory. Its desired access includes WRITE_DAC. The earlier
`generic-access/` sweep omitted WRITE_DAC and is retained separately; the final
sweep repeats the findings with that right present. `initial-seven/` predates the
regular-file stream case. These are primitive probes, not the Rust wrapper or a
completed Copy workflow.

- An empty delete-on-close directory disappears on normal close.
- A temporary child closed before its directory permits directory removal.
- Closing the directory first leaves it behind after the temporary child closes.
- A hard-killed process also leaves an empty directory behind in this fixture.
- An ordinary retained child and its exact bytes prevent directory deletion.
- A second handle can create a named stream on the open directory; directory
  close deletes it despite the creator's read-only sharing mode.
- The same stream insertion and loss occur on an ordinary temporary file.

Do not solve unfinished-stage recovery by making its directory delete-on-close.
The current temporary-file primitive also needs isolation before public Windows
Copy can be enabled. Empty/base-stream ownership does not authorize losing a
newly added stream. Read sharing is not an exclusion of every stream writer.

## Access-control probes

The intermediate `restricted/mask-control/` descriptor removes WRITE_DATA and
APPEND_DATA while retaining other full-access rights. The creating handle writes
successfully; a second stream writer gets error 5. Clearing delete-on-close and
then restoring the normal private descriptor permits a new stream, whose bytes
survive creator close. This is a useful narrow control, not accepted application
security: it still grants other metadata/permission rights.

The stronger `restricted/` read-only OWNER RIGHTS descriptor also permits the
initial creator to write and denies the second stream writer. However restoring
full private security returns error 5. It is not an accepted replacement. The
normal PowerShell fixture cleanup also fails under that descriptor. A subsequent
ordinary-user DeleteFile call, guarded by the exact fixture hash, removes it;
that cleanup is separately recorded. The permission restoration cause is not yet
established.

## Next implementation work

Add a native Rust regression against the actual temporary-file factory, then
establish a guarded ephemeral-file state that excludes unrelated stream/metadata
writers while retaining the creator's required capabilities. Clear automatic
deletion before permitting other writers. Integrate that state with the exact
private-security evidence used by claim preparation and paired journal setup;
`PrivateSecurity` currently accepts only its full-access template. Do not broaden
serialized cleanup authority merely to make a descriptor pass validation.

Re-run process exit, ordinary errors, unknown streams, named-stream writer
attempts, promotion failures, competing publishers and a real guest-power case.
Unregistered stages and partial root anchors still require independent native
ownership proof; the directory probe gives no restart cleanup authority.

Public Copy remains gated. These probes touched only disposable owned fixtures.
No application behavior or package changed in this investigation.
