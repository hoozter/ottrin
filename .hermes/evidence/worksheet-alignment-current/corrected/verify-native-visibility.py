"""Verify real Windows package visibility and Ctrl+L captures, never filesystem state alone."""
from pathlib import Path
import argparse,hashlib,json
from PIL import Image,ImageChops
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--case',type=Path,required=True);e=parser.parse_args().case.resolve();assert e.parent==Path(__file__).resolve().parent and e.name in {'upgrade-enabled-native-focus','upgrade-disabled-console'}
runtime=json.loads((e/'RUNTIME_RESULT.json').read_text());assert runtime['status']=='pass',runtime.get('error');a=runtime['updated_actor']['native_visibility_observation'];assert a['fixture_entries_preserved'] and a['source_bytes_preserved'];attrs={x['name']:x['attributes'] for x in a['attributes']};assert set(attrs)=={'V.txt','H.txt','S.txt','HD','SD'}
for name in ['H.txt','HD']:assert attrs[name]&2
for name in ['S.txt','SD']:assert attrs[name]&4
assert not attrs['V.txt']&6
out=e/'results';assert out.is_dir();proofs={}
for name in ['visibility-address-input.png','visibility-default.png','visibility-show-all.png','visibility-restored.png','updated-inspector-before-normal-space.png']:
 p=out/name;assert p.is_file();proofs[name]={'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
review=json.loads((e/'VISIBILITY_VISUAL_REVIEW.json').read_text());assert review['status']=='reviewed'
expected={'visibility-default.png':['V.txt'],'visibility-show-all.png':['HD','SD','H.txt','S.txt','V.txt'],'visibility-restored.png':['V.txt']}
images={name:Image.open(out/name).convert('RGB') for name in expected}
for name,entries in expected.items():
 assert images[name].size==(1024,680)
 assert review['captures'][name]['sha256']==proofs[name]['sha256']
 assert review['captures'][name]['visible_entries']==entries
 assert images[name].getpixel((60,200))==images['visibility-default.png'].getpixel((60,200))
 background=images[name].getpixel((60,200));counts=[sum(pixel!=background for pixel in images[name].crop((84+94*i,89,124+94*i,129)).getdata()) for i in range(5)]
 assert [count>500 for count in counts]==([True]*5 if name=='visibility-show-all.png' else [True,False,False,False,False]),(name,counts)
 proofs[name]['native_icon_slot_ink_pixels']=counts
assert ImageChops.difference(images['visibility-default.png'].crop((44,72,989,627)),images['visibility-restored.png'].crop((44,72,989,627))).getbbox() is None
receipt={'status':'pass','source_revision':runtime['source_revision'],'package_input':json.loads((e/'RUNTIME_INPUT.json').read_text()),'scope':'Actual ordinary Windows MSIX GUI native Ctrl+L to a separate fixture and Hidden/System file/directory toggle. Default/restored show exactly the ordinary proof file; show-all renders all five known entries. Native attrs/source bytes independently recorded. Physical desktop/native GPU remain separate.','native_attributes':attrs,'captures':proofs,'rendered_verification':'Hash-bound direct review of all three original native screenshots plus independent icon-slot counts and exactly restored file-view pixels.','restored_file_view_pixels_identical':True,'native_fixture_preserved':True,'verification_driver_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
(e/'NATIVE_VISIBILITY_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
