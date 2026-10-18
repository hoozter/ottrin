# Next construction check: unpublished preparations

Research only; no production change or native acceptance in this checkpoint.

`NtCreateFile(FILE_DELETE_ON_CLOSE)` could cover the gap between file creation and the first complete record. Promotion must preserve a published claim and continuous writer exclusion. Do not assume the creation flag can simply be cleared: Microsoft's current guidance says the original create flag is permanent, and ordinary FileDispositionInformation(FALSE) does not undo it.

- [Microsoft delete-on-close guidance](https://devblogs.microsoft.com/oldnewthing/20260706-00/?p=112506/)
- [FileDispositionInformation](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information)
- [Extended disposition flags](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex)

The extended ON_CLOSE flag documents setting/clearing on-close state, but its interaction with a create-time delete-on-close handle and held-handle rename needs a bounded ordinary-account native probe before any design relies on it. Test pre-publication process exit, promotion, post-promotion exit, and competing writers; distinguish process exit from physical-power behavior. Do not introduce managed hard-link aliases or infer cleanup authority from preparation names/age. Existing private preparations remain preserved until a proven ownership-safe lifecycle replaces them.
