import pathlib,tempfile,subprocess,os,hashlib,json
base=pathlib.Path(__file__).resolve().parent
repo=pathlib.Path.cwd();dist=repo/'dist/packages'
def run(args,**kw):return subprocess.run(args,check=True,**kw)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
appimage=json.loads((dist/'Ottrin-0.1.0-x86_64.AppImage.provenance.json').read_text())
revision=appimage['revision']
assert appimage['built_from_tracked_clean_source']
assert sha(dist/appimage['artifact'])==appimage['sha256']
names={'portable':'Ottrin-0.1.0-linux-x86_64-portable.tar.gz','deb':'ottrin_0.1.0_amd64.deb','rpm':'ottrin-0.1.0-1.x86_64.rpm'}
roles={'ottrin-app':'ottrin','ottrin-indexd':'ottrin-indexd','ottrin-media-playback':'ottrin-media-playback','ottrin-legacy-doc-render':'ottrin-legacy-doc-render','ottrin-priv-helper':'ottrin-priv-helper'}
receipt={'date':'2026-10-15','package_source_revision':revision,'binary_source_revision':subprocess.check_output(['git','rev-parse','51a56e4'],text=True).strip(),'appimage':appimage,'packages':{}}
with tempfile.TemporaryDirectory(prefix='ottrin-payload-qa-') as temp:
 root=pathlib.Path(temp);image=root/'image';image.mkdir()
 run([str(dist/'Ottrin-0.1.0-x86_64.AppImage'),'--appimage-extract'],cwd=image,stdout=subprocess.DEVNULL)
 for src,exe in roles.items():
  rel='usr/libexec/ottrin/'+exe if src=='ottrin-priv-helper' else 'usr/bin/'+exe
  assert sha(image/'squashfs-root'/rel)==sha(repo/'target/release'/src),(src,'release differs from accepted AppImage')
 for kind,name in names.items():
  stage=root/kind;stage.mkdir()
  if kind=='portable':
   run(['tar','-xzf',str(dist/name),'-C',str(stage)]);stage=stage/'Ottrin-0.1.0-linux-x86_64'
  elif kind=='deb':run(['dpkg-deb','-x',str(dist/name),str(stage)])
  else:
   env=os.environ.copy();env['LD_LIBRARY_PATH']=str(repo/'.hermes/tools/rpm/usr/lib/x86_64-linux-gnu')
   p=subprocess.Popen([str(repo/'.hermes/tools/rpm/usr/bin/rpm2cpio'),str(dist/name)],stdout=subprocess.PIPE,env=env)
   run(['cpio','-idm','--quiet'],stdin=p.stdout,cwd=stage);p.stdout.close();assert p.wait()==0
  record=stage/('SOURCE_REVISION.txt' if kind=='portable' else 'usr/share/doc/ottrin/SOURCE_REVISION.txt')
  assert 'Git revision: '+revision in record.read_text()
  assert ('Working tree: clean' if kind=='portable' else 'Tracked tree: clean') in record.read_text().splitlines()
  hashes={}
  for src,exe in roles.items():
   if kind=='portable':rel=('bin/' if src=='ottrin-priv-helper' else 'libexec/ottrin/')+exe
   else:rel=('usr/libexec/ottrin/' if src=='ottrin-priv-helper' else 'usr/bin/')+exe
   file=stage/rel;expected=repo/'target/release'/src
   assert os.access(file,os.X_OK)
   if kind!='rpm':assert sha(file)==sha(expected),(kind,rel)
   else:
    for section in ['.text','.rodata']:
     a=root/'a.section';b=root/'b.section'
     run(['objcopy','--dump-section',section+'='+str(a),str(file),str(root/'dump-copy')])
     run(['objcopy','--dump-section',section+'='+str(b),str(expected),str(root/'dump-copy')])
     assert sha(a)==sha(b),(kind,rel,section)
   hashes[rel]=sha(file)
  item={'artifact':name,'sha256':sha(dist/name),'binary_sha256':hashes,'source_record':record.read_text(),'payload_identity':'whole-file match' if kind!='rpm' else '.text and .rodata match; standard RPM stripping changes whole-file hashes'}
  receipt['packages'][kind]=item
  prov={'artifact':name,'revision':revision,'binary_revision':appimage['revision'],'sha256':item['sha256'],'built_from_tracked_clean_source':True,'build_strategy':'Reuse verified release binaries matching current accepted AppImage; runtime source unchanged since binary revision'}
  (dist/(name+'.provenance.json')).write_text(json.dumps(prov,indent=2)+'\n')
(base/'PAYLOAD_VERIFICATION.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('All three payloads match accepted AppImage release code and clean package revision; extraction removed')
