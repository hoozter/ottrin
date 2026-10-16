# Windows recursive folder-size checkpoint

The Windows folder preview used to return an unknown zero lower bound for every
recursive scan. The new scanner observes entry lengths through directory handles
and opens child directories relative to those handles with no-follow semantics.
It never opens regular-file contents and skips reparse/device entries. The shared
16,384-entry and 64-level limits remain; unreadable/replaced children, malformed
native records and exhausted limits produce a partial lower bound. Like the Unix
scanner, this is a bounded metadata observation, not an atomic tree snapshot.
Synchronous filesystem calls do not promise a wall-clock bound on remote storage.

Native tests cover nested totals, file symlinks, junctions and root junction
refusal, a child replaced by a symlink, root-path redirection after opening,
multiple enumeration pages, ill-formed UTF-16 names, depth/entry limits, denied
child listing and malformed native directory records. Existing required EPS
routing and public Windows Copy tests are not excluded or weakened.

The first full native run is retained as INITIAL_*: six symlink-fixture creations
failed with Windows error 1314 before the assertions could exercise the scanner;
Windows EPS routing also failed. Developer Mode is enabled only in this owned
COW guest for fixture creation, then the full ordinary-user suite is rerun.
Microsoft documents this fixture requirement at:
https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createsymboliclinkw

API references used for the handle scan:
https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex
https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile

Source/native-test evidence is separate from package acceptance. Existing stable
Linux deliverables are unchanged; the internal Windows cohort remains at its
recorded source revision until rebuilt and checked. No release or daily-driver
readiness is claimed. Task-owned guest fixtures and configuration are discarded
with the COW volume, NVRAM and TPM after normal shutdown; original guests,
installation and user preferences remain preserved.
