# Windows Copy ordinary tree staging

ACCEPTANCE.json records source/component scope, source hashes, current native
binary pins, results and explicit remaining requirements. TEST_BINARY.json binds
compiler output to the stripped transferred image through retained PE sections.
COPY_* and FULL_* are final ordinary-user runs; audit.* and AUDIT_RESULTS.json
record the explicitly selected SYSTEM fixture. Specialized exclusions are retained,
not claimed as passing ordinary tests.

INITIAL_* and SECOND_* record fixture setup failures, not product fixes: the first
used a reparse leaf instead of a directory under an ancestor alias; the second
left parent delete-child permissions and therefore did not deny source DELETE.
Both were corrected without relaxing production guards or the source assertions.
The corrected ten native tree cases pass. Source discovery has a caller entry cap;
completed staging retains two handles per entry plus transient/caller/root handles.

The no-NIC COW guest ran Windows 11 IoT Enterprise LTSC build 26100 on NTFS.
Developer Mode was enabled only there for native symlink fixtures. FINAL_GUEST.json
verifies original preferences/installation preservation and no owned test process;
FINAL_CLEANUP.json records normal shutdown and owned domain/disk/NVRAM/TPM removal.
Original stopped guests and backing storage remain intact. Text logs normalize
CRLF and trailing whitespace only; bulky build/Clippy logs are disposable.

The source also moves directory enumeration/parser responsibility out of Move
into windows_fs. Ordinary required-type Move/text-save opens keep the same native
flags; only Copy source discovery uses an unconstrained type before identity and
reparse admission. Full native platform regressions pass. Existing packages remain
unchanged, and public Windows Copy stays disabled pending complete transaction,
metadata/security, links, recovery/Undo and exact public package acceptance.
