# Current Windows preview

Source `7aabcc56` includes the portable ZIP, NSIS and unsigned development MSIX 26.
The installed Save confirmation follows the theme in Dark and Light. Native
Save returns the requested Unicode path without changing the original file.
The NSIS upgrade preserves the index database and preferences. Reinstall
preserves startup intent, and uninstall recovers after a held cache lock.

MSIX 26 fresh deployment and upgrade from 25 passed Explorer folder background,
multi-selection and drive commands, ordinary runtime, reboot and login startup,
and removal. The separate default-folder registry transaction remains open.
See ../WINDOWS_CANDIDATE_ACCEPTANCE.json for exact evidence.
