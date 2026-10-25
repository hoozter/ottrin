@echo off
whoami /user /groups > C:\OttrinOwnedFolderQA\results\ordinary-token.txt 2>&1
C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\OttrinOwnedFolderQA\actor.ps1 > C:\OttrinOwnedFolderQA\results\ordinary-startup.stdout 2> C:\OttrinOwnedFolderQA\results\ordinary-startup.stderr
exit /b %ERRORLEVEL%
