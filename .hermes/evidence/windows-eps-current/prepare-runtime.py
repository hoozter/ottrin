from pathlib import Path
import sys,json,urllib.request,hashlib,subprocess,zipfile,shutil,tempfile
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
with tempfile.TemporaryDirectory(prefix='eps-external-runtime-',dir='.hermes/packaging-tmp') as temporary:
    root=Path(temporary)
    pin=json.loads((p/'GHOSTSCRIPT_QA_INPUT.json').read_text());installer=root/'gs10080w64.exe'
    with urllib.request.urlopen(pin['url'],timeout=45) as source,installer.open('wb') as target:
        shutil.copyfileobj(source,target)
    assert hashlib.sha256(installer.read_bytes()).hexdigest()==pin['sha256']
    stage=root/'runtime';stage.mkdir()
    subprocess.run(['7z','x','-y',str(installer),'-o'+str(stage),'bin/*','lib/*','Resource/*','iccprofiles/*'],check=True,stdout=subprocess.DEVNULL)
    files={f.relative_to(stage).as_posix():hashlib.sha256(f.read_bytes()).hexdigest() for f in stage.rglob('*') if f.is_file()}
    assert files==json.loads((p/'QA_RUNTIME_FILES.json').read_text())
    with zipfile.ZipFile(root/'runtime.zip','w',zipfile.ZIP_DEFLATED,compresslevel=1) as archive:
        for f in stage.rglob('*'):
            if f.is_file():archive.write(f,f.relative_to(stage))
    n.upload(root/'runtime.zip','runtime.zip')
    n.upload(p/'QA_RUNTIME_FILES.json','runtime-files.json')
    s=r"""$root='C:\OttrinContentQA';$target='C:\Program Files\gs\gs10.08.0'
    if(Test-Path $target){throw 'Refuse existing runtime directory'}
    Expand-Archive "$root\runtime.zip" $target
    $files=Get-Content "$root\runtime-files.json" -Raw|ConvertFrom-Json
    foreach($file in $files.PSObject.Properties){if((Get-FileHash (Join-Path $target $file.Name)).Hash.ToLowerInvariant()-cne$file.Value){throw "Runtime input differs: $($file.Name)"}}
    $renderer=Join-Path $target 'bin\gswin64c.exe'
    @{status='pass';scope='Official external runtime files in normal layout, owned COW only; not installer acceptance';path=$renderer;sha256=(Get-FileHash $renderer).Hash.ToLowerInvariant();files=@($files.PSObject.Properties).Count}|ConvertTo-Json|Set-Content "$root\results\RENDERER_INSTALL.json"
    New-Item 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' -Force|Out-Null
    New-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' -Name AllowDevelopmentWithoutDevLicense -Value 1 -PropertyType DWord -Force|Out-Null
    Remove-Item "$root\runtime.zip"
    if(Test-Path "$root\gs10080w64.exe"){Remove-Item "$root\gs10080w64.exe"}
    """
    pid=n.start(s);(p/'RUNTIME_PREPARATION_PID.json').write_text(json.dumps({'pid':pid,'domain':n.guest.DOMAIN})+'\n');n.wait(pid,seconds=120)
    n.download('results\\RENDERER_INSTALL.json',p/'RENDERER_INSTALL.json')
    assert n.execute("Start-Process shutdown.exe -ArgumentList '/r /t 2 /f'")==0
