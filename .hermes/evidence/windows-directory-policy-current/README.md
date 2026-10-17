# Native directory case-policy preservation

Start with ACCEPTANCE.json. SOURCE.json and the single build.jsonl bind the final
source and executable reused by the focused test and all three disjoint batches.
JOURNAL_LIST.txt is the actual executable enumeration; COVERAGE.json accounts
for all 182 named cases: 173 pass, zero fail, nine explicitly ignored. Exact
ignored reasons remain in that file. This is component acceptance, not public
Windows Copy or GUI/package readiness.

The focused parent covers eight empty/populated source/parent combinations,
nested opposite policies, distinct case-variant bytes, publication, Undo and
refusal after retained-policy drift. RED files preserve the actual failure
against unchanged production. Applying the decompressed RED patch to 4294ff4
reproduced all recorded RED source hashes; the temporary repository was removed.

Text receipts are normalized to UTF-8 LF with trailing-whitespace cleanup; raw
identities are preserved in RAW_RECEIPT_IDENTITIES.json. The uploaded executable
and private runtime profiles were removed. TEARDOWN.json records the shut-off
owned guest and unchanged original profile, installation and login state.
