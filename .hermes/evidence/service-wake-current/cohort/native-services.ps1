 # Actual per-user service and independent login controls through native UI.
 # Reboot/login activation, MSIX and Linux polkit remain separate scopes.
 function OwnedIndexTask {
  $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect()
  try{$task=$scheduler.GetFolder('\').GetTask(('Ottrin Index '+$ExpectedSid))}
  catch{if($_.Exception.GetBaseException().HResult-eq-2147024894){return $null};throw}
  $principal=$task.Definition.Principal.UserId
  if(-not$principal.StartsWith('S-1-')){$principal=([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier]).Value}
  Require ($principal-eq$ExpectedSid-and$task.Definition.Principal.LogonType-eq3-and$task.Definition.Principal.RunLevel-eq0) 'Task owner/logon privilege differs'
  Require ($task.Definition.Actions.Item(1).Path-eq(Join-Path $install 'bin\ottrin-indexd.exe')-and$task.Definition.Actions.Item(1).Arguments-eq'--index-daemon') 'Task does not target the current daemon'
  return $task
 }
 # The original60second in-flight failure is retained. This longer diagnostic
 # observation measures terminal behavior on the cold software VM; it does not
 # accept60second responsiveness or change production deadlines.
 function ServicePending($window,$expected,$clock){
  if($clock.Elapsed.TotalSeconds-lt60){return}
  if(@($report.service_pending|Where-Object {$_.expected-eq$expected}).Count){return}
  $rows=@(Get-CimInstance Win32_Process|Where-Object {$_.ParentProcessId-eq$p.Id-or$_.ProcessId-eq$p.Id}|ForEach-Object {@{name=$_.Name;pid=$_.ProcessId;parent=$_.ParentProcessId;session=$_.SessionId;created=[string]$_.CreationDate;register_task=([string]$_.CommandLine).Contains('RegisterTaskDefinition');query_task=([string]$_.CommandLine).Contains('$task.Xml');stop_task=([string]$_.CommandLine).Contains('Stop-OttrinIndexTask')}})
  $name='service-pending-'+@($report.service_pending).Count+'.png';[PickerNative]::Shot($window,(Join-Path $out $name))
  $report.service_pending+=@{expected=$expected;seconds=$clock.Elapsed.TotalSeconds;processes=$rows;capture=$name};Save
 }
 function ServiceButton($window,$name){
  $deadline=[DateTime]::UtcNow.AddSeconds(180);$clock=[Diagnostics.Stopwatch]::StartNew()
  do{$nodes=@(Nodes $window $name|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button-and$_.Current.IsEnabled});if($nodes.Count-eq1){return $nodes[0]};ServicePending $window $name $clock;Pump}while([DateTime]::UtcNow-lt$deadline)
  throw ('Service action is not enabled: '+$name)
 }
 function ServiceText($window,$name){
  $deadline=[DateTime]::UtcNow.AddSeconds(180);$clock=[Diagnostics.Stopwatch]::StartNew()
  do{if(@(VisibleText $window $name).Count-eq1){return};ServicePending $window $name $clock;Pump}while([DateTime]::UtcNow-lt$deadline)
  [PickerNative]::Shot($window,(Join-Path $out 'service-state-missing.png'));throw ('Native service state not visible: '+$name)
 }
 function InvokeService($window,$name){(ServiceButton $window $name).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Pump}
 function ObserveService($window,$case,$installed,$enabled,$running,$scheduled=$null){
  ServiceText $window $(if($running){'Running '+[char]0x2014+' index ready'}else{'Stopped'})
  $setup=if($installed){'Update / reinstall service'}else{'Enable automatic startup'};$null=ServiceButton $window $setup
  Require (@(Nodes $window $(if($installed){'Enable automatic startup'}else{'Update / reinstall service'})).Count-eq0) 'Setup action does not match registration'
  Require ((@(Nodes $window 'Uninstall service').Count-eq1)-eq$installed) 'Uninstall action does not match registration'
  Require ((@(Nodes $window 'Start index automatically at sign-in').Count-eq1)-eq$installed) 'Login checkbox does not match registration'
  $expectedLogin=if(-not$installed){'Automatic startup at sign-in: Off'}elseif($enabled){'Automatic startup at sign-in: Enabled at login'}else{'Automatic startup at sign-in: Installed but disabled'};ServiceText $window $expectedLogin
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do{$task=OwnedIndexTask;if(($null-ne$task)-eq$installed-and(-not$installed-or$task.Enabled-eq$enabled)){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require (($null-ne$task)-eq$installed-and(-not$installed-or$task.Enabled-eq$enabled)) 'Actual task registration differs from UI'
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do{$daemonCount=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count;$instances=if($task){$task.GetInstances(0).Count}else{0};if((($daemonCount-gt0)-eq$running)-and($running-or$instances-eq0)-and($null-eq$scheduled-or(($instances-gt0)-eq$scheduled))){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require ((($daemonCount-gt0)-eq$running)-and($running-or$instances-eq0)-and($null-eq$scheduled-or(($instances-gt0)-eq$scheduled))) 'Actual daemon/task runtime state differs from UI'
  $report.service_matrix.states+=@{case=$case;installed=$installed;login_enabled=$enabled;running=$running;native_setup_action=$setup;native_login=$expectedLogin;daemon_processes=$daemonCount;task_name=if($task){$task.Name}else{$null};task_enabled=if($task){$task.Enabled}else{$null};task_instances=if($task){$task.GetInstances(0).Count}else{0}};Save
 }
 function ToggleLogin($window,$case,$enabled,$running){
  $before=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|ForEach-Object {$_.Id}|Sort-Object)
  $task=OwnedIndexTask;$instances=$task.GetInstances(0).Count;$security=$task.GetSecurityDescriptor(7)
  $choice=Node $window 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)
  $choice.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle();Pump
  ObserveService $window $case $true $enabled $running
  $after=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|ForEach-Object {$_.Id}|Sort-Object)
  Require (($before-join',')-eq($after-join',')) 'Login choice started/stopped/replaced the current daemon'
  $task=OwnedIndexTask;Require ($task.GetInstances(0).Count-eq$instances-and$task.GetSecurityDescriptor(7)-eq$security) 'Login choice changed task runtime or permissions'
  $state=(Node $window 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)).GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState
  Require (($state-eq[Windows.Automation.ToggleState]::On)-eq$enabled) 'Native login checkbox does not match actual task flag'
  $report.service_matrix.login_toggle_checks+=@{case=$case;enabled=$enabled;running=$running;daemon_pids_preserved=$after;task_instances_preserved=$instances;permissions_preserved=$true};Save
 }
 function RunServicesAudit($settings){
  Phase 'Native service registration/runtime and independent login checkbox matrix'
  $report.service_pending=@();$report.service_matrix=@{status='running';states=@();login_toggle_checks=@();scope='Ordinary-user current NSIS, existing Task Scheduler service controls. No reboot/login, MSIX startup, Linux polkit or full U04 acceptance.'};Save
  [PickerNative]::Focus($settings);(Node $settings 'Services' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  [PickerNative]::Wheel($settings,2400);Pump;[PickerNative]::Wheel($settings,2400);Pump
  ObserveService $settings 'session running; login not installed' $false $false $true
  [PickerNative]::Shot($settings,(Join-Path $out 'services-session-running.png'))
  $preferencesBefore=(Get-FileHash $pref).Hash
  InvokeService $settings 'Enable automatic startup';ObserveService $settings 'installed enabled running' $true $true $true $true
  $task=OwnedIndexTask;$security=$task.GetSecurityDescriptor(7)
  [PickerNative]::Shot($settings,(Join-Path $out 'services-installed-running.png'))
  ToggleLogin $settings 'login disabled while task stays running' $false $true
  [PickerNative]::Shot($settings,(Join-Path $out 'services-disabled-running.png'))
  ToggleLogin $settings 'login reenabled while task stays running' $true $true
  InvokeService $settings 'Update / reinstall service';ObserveService $settings 'reinstalled enabled running' $true $true $true $true;Require ((OwnedIndexTask).GetSecurityDescriptor(7)-eq$security) 'Reinstall changed task permissions'
  InvokeService $settings 'Stop indexing';ObserveService $settings 'installed enabled stopped' $true $true $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-installed-stopped.png'))
  ToggleLogin $settings 'login disabled while index stays stopped' $false $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-disabled-stopped.png'))
  ToggleLogin $settings 'login reenabled while index stays stopped' $true $false
  InvokeService $settings 'Start / restart';ObserveService $settings 'installed enabled restarted' $true $true $true $true
  # Simulate the owner's supported external Windows startup choice. Repair must
  # retain it rather than silently opting back into automatic startup.
  $task=OwnedIndexTask;$task.Enabled=$false;InvokeService $settings 'Refresh status';ObserveService $settings 'installed disabled but running' $true $false $true
  InvokeService $settings 'Update / reinstall service';ObserveService $settings 'disabled registration repaired; runtime resumed' $true $false $true;Require ((OwnedIndexTask).GetSecurityDescriptor(7)-eq$security) 'Disabled repair changed task permissions'
  [PickerNative]::Shot($settings,(Join-Path $out 'services-disabled-running.png'))
  InvokeService $settings 'Stop indexing';ObserveService $settings 'installed disabled stopped' $true $false $false
  InvokeService $settings 'Start / restart';ObserveService $settings 'disabled login; session restarted' $true $false $true
  ToggleLogin $settings 'login enabled while existing session stays running' $true $true
  [PickerNative]::Shot($settings,(Join-Path $out 'services-enabled-session-running.png'))
  ToggleLogin $settings 'login disabled again while session stays running' $false $true
  InvokeService $settings 'Uninstall service';ObserveService $settings 'uninstalled and stopped' $false $false $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-uninstalled.png'))
  Require ((Get-FileHash $pref).Hash-eq$preferencesBefore) 'Service actions changed main settings'
  Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Service actions changed source'
  $persisted=(Run $app 'index status --json' 'service-persisted-index').stdout|ConvertFrom-Json
  Require ($persisted.indexed_items-eq1-and-not$persisted.watcher_ready) 'Service removal did not keep the saved index or claimed a live watcher'
  $report.service_matrix.status='pass native service and independent login state scope';$report.service_matrix.source_and_preferences_preserved=$true;$report.service_matrix.saved_index_items=$persisted.indexed_items;Save
 }
