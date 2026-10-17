# Native Copy destination policy checkpoint

Start with ACCEPTANCE.json. SOURCE.json binds the two changed Windows source
files. The single build.jsonl is authoritative for the final executable reused
by the focused, core and cleanup batches. Each TEST_BINARY.json binds the same
transferred image and retained non-debug PE sections.

JOURNAL_LIST.txt is the actual executable's enumeration. JOURNAL_COVERAGE.json
compares all 181 named cases with the disjoint core and cleanup runs: 172 pass,
zero failures, nine ignored. Seven ignored rows are parent-owned crash children;
one exports a fixture for the separate Activity consumer, and one requires an
owned secondary NTFS volume. The latter two are not accepted by this batch.

RED files preserve the native regression against unchanged production code.
FIXTURE_FAILURE files retain the first corrected build's invalid nonempty-folder
policy transition. Final fixtures respect the OS transition rules without
weakening byte preservation or mutation-refusal assertions.

Text receipts are normalized to UTF-8 LF with whitespace cleanup; original byte
hashes are in RAW_RECEIPT_IDENTITIES.json. No runtime binaries or copied private
profiles are retained here. The uploaded executable was removed; TEARDOWN.json
checks original profile/installation/login state and the shut-off owned guest.

OBSOLETE_PACKAGE_CLEANUP_DEFERRED.json records a separate storage inspection. No
old package was removed because complete consumer visibility was unavailable.

RED_REGRESSION.patch.gz preserves the original patch bytes. Its decompressed
patch was applied to 5da46a6 in an owned temporary repository and exactly matched
the recorded RED source hash; that repository was removed afterward.
