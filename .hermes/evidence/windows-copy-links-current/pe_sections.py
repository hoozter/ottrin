"""Verify every retained PE section before uploading stripped native tests."""
import struct,hashlib
def sections(path):
 with path.open('rb') as f:
  f.seek(0x3c);pe=struct.unpack('<I',f.read(4))[0];f.seek(pe);assert f.read(4)==b'PE\0\0';header=f.read(20)
  count=struct.unpack_from('<H',header,2)[0];symbol_table,symbols=struct.unpack_from('<II',header,8);optional=struct.unpack_from('<H',header,16)[0]
  table=pe+24+optional;result={}
  for index in range(count):
   f.seek(table+40*index);row=f.read(40);name=row[:8].rstrip(b'\0')
   if name.startswith(b'/'):
    f.seek(symbol_table+symbols*18+int(name[1:]));name=f.read(256).split(b'\0')[0]
   name=name.decode('ascii');size,address,raw_size,offset=struct.unpack_from('<IIII',row,8);flags=struct.unpack_from('<I',row,36)[0]
   if name.startswith('.debug'):continue
   f.seek(offset);digest=hashlib.sha256();remaining=raw_size
   while remaining:
    block=f.read(min(1024*1024,remaining));assert block;digest.update(block);remaining-=len(block)
   result[name]={'virtual_size':size,'virtual_address':address,'flags':flags,'raw_sha256':digest.hexdigest()}
  return result
