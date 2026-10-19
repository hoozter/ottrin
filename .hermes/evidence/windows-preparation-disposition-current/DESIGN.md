# Temporary claim creation and promotion

This checkpoint changes only newly created Windows Copy claim preparations. It does not scan, adopt or delete existing preparation files, or alter payload deletion authority.

## Native API observation

Five normal-account cases use a single held writer, rename through that handle, attempt a competing writer, then close. The normal-file control survives. A delete-on-close file disappears both without a clear call and after legacy FileDispositionInfo(FALSE), even though that call reports success. Extended FileDispositionInfoEx with ON_CLOSE and without DELETE preserves the file, whether applied before or after rename. All five cases block competing write/delete access with sharing violation 32. Source/binary/actor hashes and raw observations are recorded separately from the Rust suite.

These observations establish the API choice on this VM. They do not establish physical-power durability or a general filesystem/platform guarantee.

## Production lifetime

The shared native opener retains its ordinary behavior. The private factory alone can request create-time FILE_DELETE_ON_CLOSE, with the existing private descriptor and exclusive writer established in the same NtCreateFile operation. Existing ordinary and per-object ACL factories pass the non-temporary mode.

The claim constructor writes, flushes, decodes and validates the first record while it is temporary. It atomically publishes the same handle with no replacement, flushes both namespaces, then clears on-close deletion with the verified extended API. The writer never closes or weakens its share mode during promotion. Only after persistence and final bound-metadata validation may a Lease be issued, so no Copy journal/payload authority can refer to an unpersisted preparation.

On an ordinary pre-persistence failure, closing that exact newly created handle performs cleanup; native identity observation distinguishes an owned residue from a later owner, and cleanup/flush failures remain visible. An error after persistence retains the complete unstarted canonical claim for the existing guarded orphan recovery. Unknown legacy preparations and colliding foreign files remain untouched.

Native fault seams cover creation, complete preparation, publication, directory flushing and persisted ownership. An additional creation prefix models death after only three bytes. A real hard-kill parent stops a writer held at the preparation barrier. Concurrent prepared publishers still use native no-replace under both case policies. The tests verify source/public/foreign-preparation preservation and usable subsequent admission.

## Remaining boundaries

The change covers ordinary errors, explicit process exit and TerminateProcess. Sudden VM/physical power, partial root/stage construction, retirement-specific interruptions/races, resource/alias/ancestor and final workflow/package acceptance remain separate. Public Windows Copy stays gated, and application packages are unchanged.
