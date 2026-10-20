# Persistent Windows Copy setup controls

The earlier automatic-deletion and freeze-after-creation designs could discard
unrelated named-stream data. This replacement removes the temporary-file factory,
extended disposition promotion and empty-stage rollback. It uses ordinary private
persistent files and the existing authoritative journal writer/encoder.

A complete first Claim is flushed and checked before no-replace canonical
publication. The central journal is prepared under `.preparing-<transaction>`;
only its complete paired Begin is published as `<transaction>.copy-journal`.
Binding checks use the actual preparation name before publication and the final
name afterward. Payload work starts only after successful constructor return.
A failed live constructor returns its unstarted lease for release and retains all
setup data. Close and process death do not discard added streams.

The exact final source is in `corrected/SOURCE.json`. Native core acceptance is
132 passed, zero failed, six explicit exclusions (423.77s). The separate dispatch
selection passes five tests (11.75s) on the same transferred executable. Four
previous setup checks overlap the final core. An earlier core had 130 passes and
two failures caused by tests expecting the initial journal to be canonical before
setup completed. The corrected tests explicitly prove private-frame preservation
and retain the later-record mismatch/unverified-stage check. The final full core
was rerun. Strict Windows all-target Clippy and formatting pass.

The six exclusions are listed in `ACCEPTANCE.json`: four owned child entry points,
the separate Activity fixture consumer and the owned secondary-NTFS-volume case.
This is not an all-platform, rendered Activity or complete cleanup-module claim.

Both power cases use the exact final compiler executable. `power-prepared` cuts
an ordinary-account owner while a complete first Claim is unpublished. The 1,585
bytes survive intact, then fresh admission reclaims only the verified preparation
and establishes a new reservation. `power-persisted` cuts a published reservation;
its exact canonical bytes survive, and fresh admission safely replaces that
orphan. Both keep source, public-file and foreign-preparation hashes unchanged.
These are ungraceful guest stops; the host and physical storage stayed running.
They do not establish whole-Copy or physical-device interruption acceptance.

Before-cut telemetry copies are exported to the host before destruction. Two
volatile guest JSON copies became NUL bytes after the cut. Application records
and recovery were independently verified. The first export cleanup correctly
stopped on that mismatch. The reusable cleanup now verifies the current completed
owner, keeps exact checks on post-cut exports and accepts only same-length all-NUL
loss for those two volatile reports. It was executed successfully. Pre-cut log
redirection is now scoped inside the owned fixture root, so normal fixture removal
also removes future logs; that small controller path change was source-checked,
not a third power test. See `power-persisted/VOLATILE_REPORTS.json` and
`EXPORT_CLEANUP.json`. No application validation was weakened.

All current fixture roots, payload copies, exports and actor files were removed.
`TEARDOWN.json` verifies original profile, installation and login state, no owned
executable processes/temporary roots and a shut-down guest. The existing
`ottrin-copy-cleanup-qa` guest/overlay is retained for ongoing Ottrin work; no new
VM, overlay or extra disk was created.

Retained current shared test executable: `/home/campbell/Projects/Ottrin/target/x86_64-pc-windows-gnu/debug/deps/ottrin_platform-899e03b7e4facbca.exe`.
Its next consumer is the remaining Windows Copy restart/admission and interference
QA. Its shared target/compiler caches remain useful; no separate Cargo home,
registry, toolchain or target tree was created. `STORAGE.json` records the final
profile session and staging checks. There are no pending task-owned extractions.

No package was rebuilt. Public Windows Copy remains disabled. Unpublished journal
restart admission/review, partial stages, independent cleanup/mutation interference,
resource gates and current platform/package workflows remain open in the roadmap.
