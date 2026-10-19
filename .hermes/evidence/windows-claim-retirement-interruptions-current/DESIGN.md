# Released-claim retirement interruption and publisher handoff

Scope: no-op production fault boundaries plus native tests of the existing retirement algorithm. This checkpoint does not change namespace operations, claim/journal formats or payload authority.

## Interruption matrix

Four boundaries cover the held released ledger before deletion, after the POSIX deletion mark, after closing/unlinking its handle, and after the bucket flush. Each boundary is exercised with an ordinary injected I/O error and a subprocess exit without Rust destruction: eight cases. Before deletion the complete released ledger must remain byte-identical; after the mark/close it must be absent. All paired journals, edited retained files, public siblings and a foreign preparation remain unchanged. The original source is removed before the test. Fresh admission must succeed with a different native control identity afterward.

## Independent publisher

At each boundary the retiring process waits for a separate publisher process to attempt admission and report its opaque binding atomically. The live old writer must exclude a publisher before deletion. Once unlink/flush finishes, the new publisher must win and the retiring process must fail no-replace publication. The deletion-mark transition may yield either safe winner, but never both or neither.

A winning child retains its strong writer while the parent validates the exact new binding, verifies further admission is excluded, checks its bytes and inspects old terminal recovery. A losing child must have received sharing violation or delete-pending; access-denied is permitted only at the explicitly tagged Marked boundary. The initial native run observed that Windows status and failed its narrower expectation (34 pass, 1 fail). The raw source, binary and failed result are preserved separately. Final acceptance uses a fresh dispatch and executable after the phase-scoped correction.

Tests retain/reap every child on ordinary failure, assertion failure and timeout. Test-only callbacks are consumed before invocation and reset on unwind. No test owns user files outside its private fixture. Physical-power, root/stage construction, resource/alias/ancestor and final application/package gates remain separate.
