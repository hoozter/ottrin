# Current Windows preview

Source `7992ad1e` is available as a portable ZIP, an NSIS installer and
unsigned development MSIX 28. The installed Save picker keeps keyboard
shortcuts inside the Replace dialog until it closes.

Native checks covered NSIS upgrade, reinstall and locked-cache recovery.
Development MSIX passed fresh deployment and enabled upgrade from revision 27,
Explorer folder actions, real sign-in startup and removal. Windows keeps the
current default folder app when Ottrin is installed through MSIX.

See ../WINDOWS_CANDIDATE_ACCEPTANCE.json for scope and artifact hashes.
