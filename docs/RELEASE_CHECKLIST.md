# V1 Release Checklist

## Stability
- [ ] `cargo check` and tests pass on Linux and Windows
- [ ] Core file operations verified on both OSes
- [ ] Preview behavior verified for text/image/pdf/office-basic

## UX
- [ ] Mode switch behavior verified (Miller, Dual, Hybrid)
- [ ] Sidebar persistence and active-pane open behavior verified
- [ ] Keyboard shortcuts verified (`Ctrl+C/X/V`, `Delete`, `Shift+Delete`, `Space`)

## Performance
- [ ] Startup timing captured on reference machines
- [ ] Large directory behavior tested (10k+ entries)
- [ ] Transfer queue behavior tested on large copy/move jobs

## Packaging
- [ ] Linux AppImage built and smoke-tested
- [ ] Windows portable build created and smoke-tested
- [ ] Windows installer candidate built and uninstall path verified

## Documentation
- [ ] User quickstart
- [ ] Known limitations and fallback behavior documented
- [ ] Upgrade/migration notes documented
