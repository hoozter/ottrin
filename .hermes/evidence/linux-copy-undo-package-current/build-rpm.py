"""Finish RPM with the retained pinned tools,sharing the completed release payload."""
from pathlib import Path
import json,os,subprocess,tempfile
root=Path.cwd();p=Path(__file__).resolve().parent;build=json.loads((p/'BUILD_SOURCE.json').read_text());stage=Path(build['stage'])
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==build['source_revision']
assert all((stage/name).is_file() for name in ['Ottrin-0.1.0-x86_64.AppImage','Ottrin-0.1.0-linux-x86_64-portable.tar.gz','ottrin_0.1.0_amd64.deb'])
assert not (stage/'ottrin-0.1.0-1.x86_64.rpm').exists()
with tempfile.TemporaryDirectory(prefix='.rpm-tool-',dir=p) as directory:
 wrapper=Path(directory)/'rpmbuild';tool=root/'.hermes/tools/rpm'
 wrapper.write_text('#!/bin/sh\nexport LD_LIBRARY_PATH="'+str(tool/'usr/lib/x86_64-linux-gnu')+'${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\nexport RPM_CONFIGDIR="'+str(tool/'usr/lib/rpm')+'"\nexec "'+str(tool/'usr/bin/rpmbuild')+'" "$@"\n');wrapper.chmod(0o700)
 env={**os.environ,'PATH':directory+':'+os.environ['PATH'],'CARGO_BUILD_JOBS':'1','OTTRIN_DIST_DIR':str(stage),'OTTRIN_SKIP_BUILD':'1'}
 with (p/'rpm-build.log').open('w') as log:subprocess.run(['bash','packaging/linux/package-rpm.sh'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
(p/'RPM_BUILD_SCOPE.json').write_text(json.dumps({'status':'pass','source_revision':build['source_revision'],'compiler_reused':True,'temporary_tool_wrapper_removed':True,'scope':'Default host PATH has no rpmbuild;the three-format build-all result was completed with the retained private RPM runtime and the same release files. No repeated compiler or additional target configuration.'},indent=2)+'\n')
