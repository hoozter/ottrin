# EPS preview source checkpoint

Windows now uses an optional separately installed Ghostscript console executable
through the same AppContainer/Job boundary as installed preview providers. The
UI thumbnail, full-preview and cache-capability paths use this renderer. Source
admission creates a bounded identity-checked snapshot; source redirection and
oversized inputs/PNG outputs are rejected. Only the first page is requested.
Windows output arguments escape literal percent signs for Ghostscript's filename
pattern syntax. No shell, DLL linking, runtime download or automatic installation
was added to Ottrin.

`ACCEPTANCE.json`, suite logs and transferred-binary hashes record the source
checks. Linux all 201 preview and 378 UI checks pass (one existing UI exclusion).
Windows all 199 preview checks, actual EPS render, ten native boundary checks and
340 UI checks pass. Two required public Copy UI failures remain. The three job
fixture entry points are deliberately invoked as children by their parent tests.
The real EPS and UI cache tests ran under an ordinary limited interactive account
with the external runtime required and a percent/space temporary path.

The first UI run lacked the pinned content-search helper beside the test exe;
INITIAL_* retains that failure. Restoring its exact bundled rg.exe input cleared
paging on a complete rerun. No positive test was excluded. The Ghostscript /S
installer attempt was stopped after its controller timed out with the installer
still live. Official source forces interactive mode in .onInit. The test fixture
uses the pinned official runtime files in their normal layout; it does not claim
Ghostscript installer acceptance or include those files in Ottrin deliverables.
Host extraction/archive inputs were removed after the guest hash checks. The
owned COW guest is retained for immediate compiler/package consumers; original
profiles/installation remain unchanged and no source-test processes remain.

Primary references:
- https://ghostscript.readthedocs.io/en/latest/Use.html
- https://ghostscript.readthedocs.io/en/latest/Install.html
- https://github.com/ArtifexSoftware/ghostpdl/blob/master/psi/nsisinst.nsi
- https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation

Source/native tests are not exact-package acceptance. Stable deliverables remain
at their recorded earlier revision until the next cohort is built and checked.
