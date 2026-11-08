# MSIX registry transaction probe

Build the probe on the host and transfer it to a disposable Windows guest with
the development MSIX installed for the ordinary test user.

```sh
x86_64-w64-mingw32-gcc -std=c11 -Wall -Wextra -Werror -municode .hermes/evidence/msix-registry-diagnostic/probe.c -lktmw32 -ladvapi32 -o /tmp/ottrin-msix-registry-probe.exe
```

Run the same executable twice from the ordinary interactive user. Use a fresh
owned directory with no spaces for the inputs and result files. For example,
after placing the executable at `C:\OttrinRegistryQA\probe.exe`:

```powershell
$family = (Get-AppxPackage -Name '<installed development package name>').PackageFamilyName
& C:\OttrinRegistryQA\probe.exe unpackaged > C:\OttrinRegistryQA\outside.txt
Invoke-CommandInDesktopPackage -PackageFamilyName $family -AppId Ottrin -Command "$env:windir\System32\cmd.exe" -PreventBreakaway -Args '/d /c C:\OttrinRegistryQA\probe.exe packaged > C:\OttrinRegistryQA\inside.txt 2>&1'
```

Wait for both processes to exit and compare package identities, user SIDs and
elevation flags. The probe refuses the wrong package context or an elevated
token. Compare paired ordinary and transacted return codes for each key and
access mask. A missing key remains a missing-key observation. A difference
isolates package context without identifying the resource manager mechanism.
The command produces a diagnostic token with package identity, which can differ
from the token of an ordinary launched app. Check proposed fixes from the
actual installed app.

The probe requests query access and closes each uncommitted transaction. The
strict cross-compile and PE import check passed. Imports include the expected
read and identity APIs and exclude registry mutation and transaction commit
APIs. Native execution remains pending the Windows guest storage gate.
Remove the probe executable and temporary guest files after recording results.

[Microsoft's package-context diagnostic command](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-debug)
