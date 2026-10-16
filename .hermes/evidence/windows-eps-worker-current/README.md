# Production Windows EPS worker repair

The exact portable GUI check rejected source 7f497e9: direct rendering and the
thumbnail/full-preview utilities worked, but the shared asynchronous preview
worker returned no pixels on Windows. Its Linux-only branch and platform-coupled
error binding were the cause. The worker now admits Windows; the two existing
main/Inspector routing tests run there and a real worker test requires the
expected green/red rendered drawing, not just successful request submission.

Linux full UI passes 379 tests (one existing exclusion). Native ordinary-user
Windows UI passes 343 with two required public Copy failures and no exclusions.
Both all-target UI Clippy lanes pass. The external Ghostscript runtime is required
in this native lane; source bytes stay unchanged. Earlier renderer and sandbox
acceptance remains separately scoped to windows-eps-current. This checkpoint
does not accept the rejected packages or claim that source tests prove delivery.
The owned COW is retained for rebuilding/exact-package work, then disposed.
