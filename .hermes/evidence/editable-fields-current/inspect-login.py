from pathlib import Path
import base64,importlib.util,json,subprocess,time,sys
r=Path.cwd();case=Path(__file__).resolve().parent/(sys.argv[1] if len(sys.argv)>1 else 'windows-fields');owner=json.loads((case/'OWNER.json').read_text());assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',owner['domain']],text=True).strip()==owner['uuid']
s=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(s);s.loader.exec_module(g);g.DOMAIN=owner['domain']
code=r'''$ErrorActionPreference='Stop';$o=Get-Content 'C:\OttrinMsiCleanClientResults\OWNER.json' -Raw|ConvertFrom-Json;$u=Get-LocalUser -SID $o.sid;$w=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon';$result=$null;$path=Join-Path $o.root 'results\RESULT.json';if(Test-Path $path){$result=Get-Content $path -Raw|ConvertFrom-Json};@{actor_result=$result;fixture=$o;user_description=$u.Description;console_user=(Get-CimInstance Win32_ComputerSystem).UserName;default_user=$w.DefaultUserName;auto_admin_logon=$w.AutoAdminLogon;auto_logon_count=$w.AutoLogonCount;password_present=([string]$w.DefaultPassword).Length-gt0;boot_ticks=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().Ticks;product_processes=@(Get-Process ottrin,ottrin-indexd -ErrorAction SilentlyContinue).Count}|ConvertTo-Json -Depth 8 -Compress'''
pid=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',base64.b64encode(code.encode('utf-16-le')).decode()],'capture-output':True})['pid'];deadline=time.monotonic()+30
while True:
 d=g.qga('guest-exec-status',{'pid':pid})
 if d.get('exited'):break
 assert time.monotonic()<deadline;time.sleep(.2)
assert d['exitcode']==0,base64.b64decode(d.get('err-data','')).decode(errors='replace')
v=json.loads(base64.b64decode(d['out-data']).decode('utf-8-sig'));(case/'LOGIN_STATE.json').write_text(json.dumps(v,indent=2)+'\n');print(json.dumps({'console_user':v['console_user'],'actor_result':v.get('actor_result')}))
