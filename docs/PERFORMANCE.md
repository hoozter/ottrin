# Performance Budget and Benchmark Plan

## Startup Budget (Target)
1. Warm start (config cached): <= 120 ms to first frame.
2. Cold start (fresh process): <= 250 ms to first frame.

## Interaction Budgets
1. Directory switch command should start worker request in <= 16 ms on UI thread.
2. Basic pane render for 500 entries should stay under frame budget (16 ms target).

## Measurement Plan
1. Record startup ms from built-in startup status line.
2. Capture folder-load timings for:
- small folder (~100 entries)
- medium folder (~2k entries)
- large folder (10k+ entries)
3. Track regressions in CI by preserving benchmark logs as artifacts (future step).

## Optimization Priorities
1. Keep filesystem operations off UI thread.
2. Avoid rendering unbounded item counts in one frame.
3. Cache preview results by path and invalidate on explicit refresh.
