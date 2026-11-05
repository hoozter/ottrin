"""Owned actual AppImage Services startup/error-control acceptance with registered guest teardown."""
from pathlib import Path
import hashlib,json,os,pwd,secrets,shutil,subprocess,sys,tarfile,time,traceback
base=Path('/home/ottrinqa');out=base/'results';out.mkdir()
assert os.geteuid()==0 and os.uname().nodename=='ottrin-native-ubuntu-desktop-qa'
account=pwd.getpwnam('ottrinqa');uid=account.pw_uid
report={'status':'running','package_source':sys.argv[1]}
try:
 assert 'Git revision: '+sys.argv[1] in Path('/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text()
 source=json.loads((base/'APPIMAGE_INPUT.json').read_text())
 image=base/source['name']
 assert image.name==source['name'] and hashlib.sha256(image.read_bytes()).hexdigest()==source['sha256']
 image.chmod(0o755)
 fuse=subprocess.run(['env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','install','libfuse2t64','x11-utils'],capture_output=True,text=True,timeout=180)
 (out/'fuse-install.log').write_text(fuse.stdout+fuse.stderr);assert fuse.returncode==0
 subprocess.run(['modprobe','fuse'],check=True);assert Path('/dev/fuse').exists()
 subprocess.run(['chpasswd'],input='ottrinqa:'+secrets.token_urlsafe(24)+'\n',text=True,check=True)
 fixture=base/'fixtures';fixture.mkdir()
 import struct,zlib
 (fixture/'00-WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW.txt').write_text('wide glyphs\n')
 (fixture/'01-iiiiiiiiiiiiiiiiiiii.txt').write_text('narrow glyphs\n')
 def chunk(kind,data):return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data)&0xffffffff)
 pixels=b''.join(b'\x00'+bytes([30,95,180])*64 for _ in range(48))
 png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',64,48,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(pixels))+chunk(b'IEND',b'')
 (fixture/'02-preview.png').write_bytes(png)
 folder=fixture/('03-folder-'+'W'*32);folder.mkdir();(folder/'nested.txt').write_text('nested original\n')
 originals={str(p.relative_to(fixture)):hashlib.sha256(p.read_bytes()).hexdigest() for p in fixture.rglob('*') if p.is_file()}
 pref=base/'.config/ottrin/config.json';pref.parent.mkdir(parents=True,exist_ok=True)
 pref.write_text(json.dumps({'theme':'Dark','list_columns':['Name','Kind','Size','Modified'],'command_frame_always_visible':False,'show_hidden_files':False,'target':{'current':None,'pinned':[],'has_ever_set':False,'recent':[]},'default_view_mode':'Miller','search_setup_completed':True,'window_size':[1100,800],'search':{'include_roots':[str(fixture)],'content_indexing_enabled':False}}))
 for p in [base,out,fixture,*fixture.rglob('*'),pref,*pref.parents]:
  if p==base or base in p.parents:os.chown(p,uid,account.pw_gid)
 subprocess.run(['systemctl','stop','lightdm'],check=True)
 Path('/usr/share/xsessions/ottrin-qa.desktop').write_text('[Desktop Entry]\nName=Owned Ottrin UI QA\nType=Application\nExec=/usr/bin/env LIBGL_ALWAYS_SOFTWARE=1 GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0 /usr/bin/python3 /home/ottrinqa/worker.py\n')
 config=Path('/etc/lightdm/lightdm.conf.d/90-ottrin-owned-qa.conf');config.parent.mkdir(parents=True,exist_ok=True)
 config.write_text('[Seat:*]\nautologin-user=ottrinqa\nautologin-user-timeout=0\nuser-session=ottrin-qa\nautologin-session=ottrin-qa\n')
 subprocess.run(['systemctl','start','lightdm'],check=True)
 deadline=time.monotonic()+300
 while time.monotonic()<deadline and not (out/'WORKER_RESULT.json').exists():time.sleep(.5)
 worker=json.loads((out/'WORKER_RESULT.json').read_text());report['worker']=worker
 assert worker['status']=='pass',worker
 assert all(hashlib.sha256((fixture/name).read_bytes()).hexdigest()==digest for name,digest in originals.items())
 report.update(status='pass',appimage_input=source,original_fixture_hashes=originals,sources_preserved=True)
except BaseException as error:report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
finally:
 subprocess.run(['systemctl','stop','lightdm'],capture_output=True)
 subprocess.run(['systemctl','stop',f'user@{uid}.service'],capture_output=True)
 (out/'lightdm.log').write_text(subprocess.run(['journalctl','-u','lightdm','--no-pager'],capture_output=True,text=True).stdout)
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
 with tarfile.open(base/'results.tar.gz','w:gz') as archive:archive.add(out,arcname='results')
 os.chown(base/'results.tar.gz',uid,account.pw_gid)
print(json.dumps(report,indent=2));sys.exit(0 if report['status']=='pass' else 1)
