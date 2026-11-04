"""Reuse accepted owned-display/bootstrap helpers; replace the exercised workflow."""
from pathlib import Path
r=Path.cwd();e=Path(__file__).resolve().parent
s=(r/'.hermes/evidence/gui-idle-refresh-current/native-actor.ps1').read_text()
s=s[:s.index(" foreach($renderer in @('Native','OpenGL'))")]
a=s.index(" Add-Type @'");b=s.index(' Add-Type -ReferencedAssemblies',a);s=s[:a]+s[b:]
a=s.index(' function MeasureGuiIdle(');b=s.index(" Phase 'prepare native large display'",a);s=s[:a]+s[b:]
a=s.index(' function AliasRows(');s=s[:a]
s=s.replace("cases=@();gui_idle_baseline=@()", "cases=@()")
a=s.index("scope='");b=s.index("';cases=@()",a);s=s[:a]+"scope='Exact current NSIS/ordinary user, Dark144DPI Native and OpenGL; malformed settings refusal, retained native theme draft after save failure, repaired retry and truthful Search health. Other appearance/MSIX/physical scopes separate"+s[b:]
s=s.replace("$final='failed native DPI/layout audit'", "$final='failed native settings preservation workflow';$readonlyConfig=$null;$originalAttributes=$null")
s=s.replace('function Run($exe,$arguments,$name){','function Run($exe,$arguments,$name,$expectedExit=0){').replace('Require ($v.exit-eq0)', 'Require ($v.exit-eq$expectedExit)')
s+=(e/'native-services.ps1').read_text()+(e/'native-workflow.ps1').read_text()
(e/'native-actor.ps1').write_text(s)
