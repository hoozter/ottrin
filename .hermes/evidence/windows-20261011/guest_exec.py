"""Bounded commands in the owned, network-isolated Ottrin QA guest only."""
import base64, json, subprocess, sys, time
DOMAIN='ottrin-qa-20261011'
def qga(command, arguments=None):
    request={'execute':command}
    if arguments is not None: request['arguments']=arguments
    result=subprocess.run(['virsh','-c','qemu:///system','qemu-agent-command',DOMAIN,json.dumps(request),'--timeout','10'],capture_output=True,text=True,check=True)
    value=json.loads(result.stdout)
    if 'error' in value: raise RuntimeError(value['error'])
    return value['return']
def execute(script, timeout=30):
    encoded=base64.b64encode(script.encode('utf-16le')).decode()
    pid=qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid']
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        result=qga('guest-exec-status',{'pid':pid})
        if result.get('exited'):
            for key in ['out-data','err-data']:
                if result.get(key): print(base64.b64decode(result[key]).decode('utf-8',errors='replace'),end='')
            return result.get('exitcode',-1)
        time.sleep(.3)
    raise TimeoutError(f'Guest command still running: pid={pid}; poll guest-exec-status; do not restart it')
if __name__=='__main__': sys.exit(execute(sys.stdin.read()))
