# Current Windows preview

Source `381ef193` is available as a portable ZIP, an NSIS installer and
unsigned development MSIX 30. The installed Save picker asks again when
the file targeted for replacement has changed.

Native checks covered NSIS upgrade, reinstall and locked-cache recovery.
Development MSIX passed fresh deployment and upgrade from revision 29 with
both sign-in choices, Explorer folder actions and removal. Windows keeps the
current default folder app when Ottrin is installed through MSIX.

See ../WINDOWS_CANDIDATE_ACCEPTANCE.json for scope and artifact hashes.
