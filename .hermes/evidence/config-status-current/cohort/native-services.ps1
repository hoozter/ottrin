 # Existing per-user service actions, exercised through native controls. This
 # does not claim reboot/login or the missing direct startup-toggle interface.
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
 function ServiceButton($window,$name){
  $deadline=[DateTime]::UtcNow.AddSeconds(60)
  do{$nodes=@(Nodes $window $name|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button-and$_.Current.IsEnabled});if($nodes.Count-eq1){return $nodes[0]};Pump}while([DateTime]::UtcNow-lt$deadline)
  throw ('Service action is not enabled: '+$name)
 }
 function ServiceText($window,$name){
  $deadline=[DateTime]::UtcNow.AddSeconds(60)
  do{if(@(VisibleText $window $name).Count-eq1){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  [PickerNative]::Shot($window,(Join-Path $out 'service-state-missing.png'));throw ('Native service state not visible: '+$name)
 }
 function InvokeService($window,$name){(ServiceButton $window $name).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Pump}
 function ObserveService($window,$case,$installed,$enabled,$running){
  ServiceText $window $(if($running){'Running '+[char]0x2014+' index ready'}else{'Stopped'})
  $setup=if($installed){'Update / reinstall service'}else{'Enable automatic startup'};$null=ServiceButton $window $setup
  Require (@(Nodes $window $(if($installed){'Enable automatic startup'}else{'Update / reinstall service'})).Count-eq0) 'Setup action does not match registration'
  Require ((@(Nodes $window 'Uninstall service').Count-eq1)-eq$installed) 'Uninstall action does not match registration'
  $expectedLogin=if(-not$installed){'Automatic startup at sign-in: Off'}elseif($enabled){'Automatic startup at sign-in: Enabled at login'}else{'Automatic startup at sign-in: Installed but disabled'};ServiceText $window $expectedLogin
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do{$task=OwnedIndexTask;if(($null-ne$task)-eq$installed-and(-not$installed-or$task.Enabled-eq$enabled)){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require (($null-ne$task)-eq$installed-and(-not$installed-or$task.Enabled-eq$enabled)) 'Actual task registration differs from UI'
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do{$daemonCount=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count;$instances=if($task){$task.GetInstances(0).Count}else{0};if((($daemonCount-gt0)-eq$running)-and(-not($installed-and$enabled-and$running)-or$instances-gt0)-and($running-or$instances-eq0)){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require ((($daemonCount-gt0)-eq$running)-and(-not($installed-and$enabled-and$running)-or$instances-gt0)-and($running-or$instances-eq0)) 'Actual daemon/task runtime state differs from UI'
  $report.service_matrix.states+=@{case=$case;installed=$installed;login_enabled=$enabled;running=$running;native_setup_action=$setup;native_login=$expectedLogin;daemon_processes=$daemonCount;task_name=if($task){$task.Name}else{$null};task_enabled=if($task){$task.Enabled}else{$null};task_instances=if($task){$task.GetInstances(0).Count}else{0}};Save
 }
 function RunServicesAudit($settings){
  Phase 'Native service registration/runtime action matrix'
  $report.service_matrix=@{status='running';states=@();scope='Ordinary-user current NSIS, existing Task Scheduler service controls. No reboot/login, MSIX startup, Linux polkit or full U04 acceptance.';known_interface_gap='Installed-but-disabled ordinary service has no direct in-app login enable/disable control; update preserves disabled intent. Track separately for implementation.'};Save
  [PickerNative]::Focus($settings);(Node $settings 'Services' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  [PickerNative]::Wheel($settings,2400);Pump;[PickerNative]::Wheel($settings,2400);Pump
  ObserveService $settings 'session running; login not installed' $false $false $true
  [PickerNative]::Shot($settings,(Join-Path $out 'services-session-running.png'))
  $preferencesBefore=(Get-FileHash $pref).Hash
  InvokeService $settings 'Enable automatic startup';ObserveService $settings 'installed enabled running' $true $true $true
  $task=OwnedIndexTask;$security=$task.GetSecurityDescriptor(7)
  [PickerNative]::Shot($settings,(Join-Path $out 'services-installed-running.png'))
  InvokeService $settings 'Update / reinstall service';ObserveService $settings 'reinstalled enabled running' $true $true $true;Require ((OwnedIndexTask).GetSecurityDescriptor(7)-eq$security) 'Reinstall changed task permissions'
  InvokeService $settings 'Stop indexing';ObserveService $settings 'installed enabled stopped' $true $true $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-installed-stopped.png'))
  InvokeService $settings 'Start / restart';ObserveService $settings 'installed enabled restarted' $true $true $true
  # Simulate the owner's supported external Windows startup choice. Repair must
  # retain it rather than silently opting back into automatic startup.
  $task=OwnedIndexTask;$task.Enabled=$false;InvokeService $settings 'Refresh status';ObserveService $settings 'installed disabled but running' $true $false $true
  InvokeService $settings 'Update / reinstall service';ObserveService $settings 'disabled registration repaired; runtime resumed' $true $false $true;Require ((OwnedIndexTask).GetSecurityDescriptor(7)-eq$security) 'Disabled repair changed task permissions'
  [PickerNative]::Shot($settings,(Join-Path $out 'services-disabled-running.png'))
  InvokeService $settings 'Stop indexing';ObserveService $settings 'installed disabled stopped' $true $false $false
  InvokeService $settings 'Start / restart';ObserveService $settings 'disabled login; session restarted' $true $false $true
  InvokeService $settings 'Uninstall service';ObserveService $settings 'uninstalled and stopped' $false $false $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-uninstalled.png'))
  Require ((Get-FileHash $pref).Hash-eq$preferencesBefore) 'Service actions changed main settings'
  Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Service actions changed source'
  $persisted=(Run $app 'index status --json' 'service-persisted-index').stdout|ConvertFrom-Json
  Require ($persisted.indexed_items-eq1-and-not$persisted.watcher_ready) 'Service removal did not keep the saved index or claimed a live watcher'
  $report.service_matrix.status='pass existing native service action/state scope';$report.service_matrix.source_and_preferences_preserved=$true;$report.service_matrix.saved_index_items=$persisted.indexed_items;Save
 }
