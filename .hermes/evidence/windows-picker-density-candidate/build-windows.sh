#!/usr/bin/env bash
set -euo pipefail
repo="$(pwd -P)"
ev="$repo/.hermes/evidence/windows-picker-density-candidate"
work="$repo/.hermes/worktrees/windows-picker-density-source"
stage="$repo/.hermes/packaging-tmp/.windows-picker-density-stage"
scratch="$(mktemp -d "$repo/.hermes/packaging-tmp/.windows-picker-density-build.XXXXXX")"
owned_work=false
cleanup() {
    if "$owned_work"; then git -C "$repo" worktree remove --force "$work"; fi
    python3 -c 'import pathlib, shutil, sys; p=pathlib.Path(sys.argv[1]); assert p.name.startswith(".windows-picker-density-build.") and p.parent.name == "packaging-tmp"; shutil.rmtree(p)' "$scratch"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
test ! -e "$work" && test ! -e "$stage"
du -sh "$repo/target/debug/"{deps,incremental} "$repo/target/x86_64-pc-windows-gnu/debug/"{deps,incremental} "$repo/target/x86_64-pc-windows-gnu/release/deps" "$ev"
source_revision="$(jq -er .source_revision "$ev/BUILD_SOURCE.json")"
git -C "$repo" worktree add --detach "$work" "$source_revision"
owned_work=true
test -z "$(git -C "$work" status --porcelain)"
mapfile -t changed_runtime < <(git -C "$repo" diff --name-only f62ec5e4 "$source_revision" -- crates Cargo.toml Cargo.lock vendor assets packaging)
expected_runtime=(crates/ottrin-ui/src/file_row.rs crates/ottrin-ui/src/picker.rs)
test "${changed_runtime[*]}" = "${expected_runtime[*]}"
ln -s "$repo/target" "$work/target"
export CARGO_TARGET_DIR="$repo/target"
export OTTRIN_DIST_DIR="$scratch"
export CARGO_NET_OFFLINE=true
export CARGO_BUILD_JOBS=1
(
 cd "$work"
 cargo build --locked --offline --release --target x86_64-pc-windows-gnu -p ottrin-app
)
"$work/scripts/windows-gui-subsystem-smoke.sh" "$repo/target/x86_64-pc-windows-gnu/release/ottrin-app.exe"
payload="$scratch/windows-payload/Ottrin-0.1.0-windows-x64"
git -C "$work" diff --exit-code f62ec5e4 "$source_revision" -- packaging/windows packaging/package-lifecycle.sh packaging/package-search-runtime.sh packaging/search-runtime.json packaging/search-runtime crates/ottrin-ui/assets/THIRD_PARTY_NOTICES.md
previous="$repo/dist/windows-candidate/Ottrin-0.1.0-windows-x64-setup.exe"
7z x -y "-o$scratch/previous" "$previous" >/dev/null
python3 - "$scratch" "$payload" "$work" "$repo/target/x86_64-pc-windows-gnu/release" "$ev" "$previous" <<'PY2'
import hashlib,json,shutil,sys
from pathlib import Path
scratch,payload,work,build,ev,installer=map(Path,sys.argv[1:])
def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
old=json.loads((ev.parent/'windows-picker-preferences-delivery/HOST_PAYLOAD.json').read_text())
assert sha(installer)==old['artifacts'][installer.name]
prior=scratch/'previous/$INSTDIR.ottrin-setup/new'
for name,digest in old['payload_hashes'].items():
    assert sha(prior/name)==digest,name
    dst=payload/name
    dst.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(prior/name,dst)
for name in ('ottrin-app.exe',):
    source=build/name
    stem=source.stem
    assert str(work) in (build/(stem+'.d')).read_text(),name
    shutil.copy2(source,payload/'bin'/('ottrin.exe' if name=='ottrin-app.exe' else name))
for name in ('README.md','LICENSE','docs/USER_GUIDE.md','docs/DEVELOPMENT.md','docs/ROADMAP.md','docs/RELEASE_NOTES.md','docs/RELEASE_CHECKLIST.md','assets/icon.svg','assets/icon.png','assets/icon.ico','assets/logo.svg'):
    shutil.copy2(work/name,payload/name)
PY2
source "$work/packaging/package-lifecycle.sh"
ottrin_write_source_revision "$work" "$payload/SOURCE_REVISION.txt" 0.1.0
bash "$work/packaging/tests/verify-windows-opengl-loader-contract.sh" "$payload"
(cd "$(dirname "$payload")" && zip -qr "$scratch/Ottrin-0.1.0-windows-x64-portable.zip" "$(basename "$payload")")
"$work/packaging/windows/package-installer.sh" "$payload"
7z x -y "-o$scratch/extracted" "$scratch/Ottrin-0.1.0-windows-x64-setup.exe" > /dev/null
python3 - "$scratch" "$payload" "$stage" "$ev" "$source_revision" "$previous" <<'PY'
import hashlib,json,os,shutil,sys,zipfile
from pathlib import Path
tmp, payload, stage, ev = map(Path,sys.argv[1:5])
source_revision, previous = sys.argv[5:]
name='Ottrin-0.1.0-windows-x64-setup.exe'
zip_name='Ottrin-0.1.0-windows-x64-portable.zip'
def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
files={str(p.relative_to(payload)):sha(p) for p in payload.rglob('*') if p.is_file()}
assert len(files)==59
assert f'Git revision: {source_revision}' in (payload/'SOURCE_REVISION.txt').read_text()
with zipfile.ZipFile(tmp/zip_name) as z:
    entries={x.filename.split('/',1)[1]:hashlib.sha256(z.read(x)).hexdigest() for x in z.infolist() if not x.is_dir()}
assert entries==files
extracted=tmp/'extracted/$INSTDIR.ottrin-setup/new'
assert extracted.is_dir()
extras=set()
for p in extracted.rglob('*'):
    if p.is_file():
        key=str(p.relative_to(extracted))
        if key in files:
            assert sha(p)==files[key],key
        else:
            extras.add(key)
assert all((extracted/n).is_file() for n in files)
assert extras=={'Uninstall.exe'},extras
artifacts={key:sha(tmp/key) for key in (name,zip_name)}
sidecar=(tmp/(name+'.sha256')).read_text().split()[0]
assert sidecar==artifacts[name]
stage.mkdir()
try:
    (tmp/(zip_name+'.sha256')).write_text(f'{artifacts[zip_name]}  {zip_name}\n')
    for key in (name,name+'.sha256',zip_name,zip_name+'.sha256'):
        os.replace(tmp/key,stage/key)
    receipt={'status':'pass Windows stable-channel candidate ZIP and NSIS staged',
             'source_revision':source_revision,'stage':str(stage),'artifacts':artifacts,
             'payload_files':len(files),'payload_hashes':files,'installer_matches_portable':True,
             'native_installed_acceptance_pending':True,
             'administrator_copy_capability':'Desktop administrator Copy retained; MSIX package identity keeps its existing capability boundary',
             'provider_recovery':'Verified the preceding installed package and all 59 member hashes. Provider pins and packaging code match. The app executable was rebuilt. Verified supporting payloads are reused from f62ec5e4.',
             'provider_source_installer_sha256':sha(Path(previous)),
             'scope':'Local stage for normal Windows candidate. Stable Windows and Linux packages remain outside this build.'}
    (ev/'HOST_PAYLOAD.json').write_text(json.dumps(receipt,indent=2)+'\n')
except BaseException:
    for p in stage.iterdir():p.unlink()
    stage.rmdir()
    raise
print(f'Staged Windows installer SHA256 {artifacts[name]}')
PY
