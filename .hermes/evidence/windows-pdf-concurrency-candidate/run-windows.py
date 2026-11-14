from pathlib import Path
import os,sys
e=Path(__file__).resolve().parent
root=Path.cwd()
os.execv(sys.executable,[sys.executable,str(e/'windows-driver.py'),
    '--application-nsis-stage',str(root/'.hermes/packaging-tmp/.windows-pdf-concurrency-stage'),
    '--application-actor',str(e/'actor.ps1'),'--use-stock-base',
    '--application-timeout-seconds','1080','--output',str(e/'windows-native')])
