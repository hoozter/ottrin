# Terminal Copy reservation audit

Scope: source at 680ec9c, followed by the terminal-Undo lease correction in this checkpoint. This is not release or package acceptance.

## Current authority

- `State::claim_terminal` is true only after complete Undo (including original restoration) or durable cleanup start. Forward completion alone cannot relinquish a destination that may still be undone.
- `Snapshot::load_control` opens the physical claim before full paired journal revalidation. `Snapshot::revalidate` checks that claim, both journal bytes, security, native identities and canonical bindings.
- `Journal::from_snapshot` revalidates, reopens the claim, and keeps both journal handles in the returned writer while releasing a terminal claim.
- The explicit Activity name-release action retains those same paired handles through the writer handoff.
- `cleanup_dependencies::references` includes all five claim-control identities in the cleanup dependency graph, in addition to journal and retained payload identities.
- The completed-Undo fast path was an exception: it explicitly dropped the stage and both paired journals before `release_after_terminal`. A concurrent writer could therefore change the evidence after validation but before the release was durable.

## Correction and proof

Keep the completed-Undo snapshot alive through `release_after_terminal`, using Rust ownership rather than another authority path. A one-shot test callback runs at the existing release writer boundary. It attempts write and delete access to each journal without modifying either file. All four attempts must fail with the native sharing violation, and the exact claim release, paired bytes and withdrawn destination must remain correct. The baseline reproduced successful write opens for both journals. Its delete probes omitted SYNCHRONIZE and returned invalid-parameter (87), so they do not establish a baseline delete-access result. The final probe corrects that flag and requires sharing-violation (32) for all four opens. Raw baseline results remain separately labeled as an expected failed regression, not acceptance.

## Further lifecycle work

The physical claim currently remains a required snapshot/dependency identity even for terminal records. Removing the historical append path therefore requires a coherent change to those two contracts; deleting released ledgers alone would make retained recovery unreadable. That redesign remains actionable, together with interrupted construction, abandoned private preparation collection and the remaining roadmap gates. This checkpoint fixes an actual authority gap without claiming to finish those larger outcomes.
