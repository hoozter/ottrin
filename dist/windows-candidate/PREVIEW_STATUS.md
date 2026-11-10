# Current Windows preview

Source `68b145fd` is available as a portable ZIP, an NSIS installer and
unsigned development MSIX 37. The app defers loading bundled Mesa until
OpenGL compatibility uses it. Native picker tests passed in both WGPU and
OpenGL from the portable ZIP and the installed EXE. Portable cold startup
remains under investigation. Administrator Copy is available in the desktop formats.

MSIX passed upgrade from revision 36 with automatic indexing enabled, real
sign-in, Explorer folder actions and removal.

See ../WINDOWS_CANDIDATE_ACCEPTANCE.json for scope and artifact hashes.
