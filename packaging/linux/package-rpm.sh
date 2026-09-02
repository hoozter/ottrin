#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
RPM_ARCH="${OTTRIN_RPM_ARCH:-$(uname -m)}"

command -v rpmbuild >/dev/null 2>&1 || {
  printf 'rpmbuild is required to create the native RPM artifact.\n' >&2
  exit 1
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
TOP="$TMP/rpmbuild"
STAGE="$TOP/SOURCES/stage"
mkdir -p "$TOP/BUILD" "$TOP/BUILDROOT" "$TOP/RPMS" "$TOP/SOURCES" "$TOP/SPECS" "$TOP/SRPMS" "$DIST_DIR"
"$ROOT/packaging/linux/stage-native-package.sh" "$STAGE"

cat > "$TOP/SPECS/ottrin.spec" <<EOF
Name: ottrin
Version: $VERSION
Release: 1%{?dist}
Summary: Fast, keyboard-friendly file manager
License: MIT
URL: https://github.com/hoozter/ottrin
BuildArch: $RPM_ARCH
AutoReqProv: yes
Requires: libc.so.6()(64bit)
Requires: libgcc_s.so.1()(64bit)
Requires: libgio-2.0.so.0()(64bit)
Requires: libgobject-2.0.so.0()(64bit)
Requires: libglib-2.0.so.0()(64bit)
Requires: libgstreamer-1.0.so.0()(64bit)
Requires: libgstbase-1.0.so.0()(64bit)
Requires: libgstapp-1.0.so.0()(64bit)

%description
Ottrin is a keyboard-first desktop file manager with Miller, List, and Grid views.

%prep

%build

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/stage/. %{buildroot}/

%files
/usr/bin/ottrin
/usr/bin/ottrin-indexd
/usr/bin/ottrin-legacy-doc-render
/usr/bin/ottrin-media-playback
/usr/libexec/ottrin/ottrin-priv-helper
/usr/share/applications/org.ottrin.Ottrin.desktop
/usr/share/icons/hicolor/scalable/apps/ottrin.svg
/usr/share/icons/hicolor/256x256/apps/ottrin.png
/usr/share/metainfo/org.ottrin.Ottrin.appdata.xml
/usr/share/polkit-1/actions/org.ottrin.filesystem.policy
/usr/share/ottrin/ottrin-index.service.in
/usr/share/doc/ottrin
EOF

rpmbuild --define "_topdir $TOP" -bb "$TOP/SPECS/ottrin.spec"
RPM="$(find "$TOP/RPMS" -type f -name '*.rpm' -print -quit)"
test -n "$RPM"
OUTPUT="$DIST_DIR/$(basename "$RPM")"
install -m 0644 "$RPM" "$OUTPUT"
sha256sum "$OUTPUT" | sed "s|$OUTPUT|$(basename "$OUTPUT")|" > "$OUTPUT.sha256"
printf '%s\n' "$OUTPUT"
