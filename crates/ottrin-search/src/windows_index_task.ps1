# Embedded by the search crate; task parameters arrive as process environment data.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$TaskName = 'Ottrin Index'
$interval = [uint32]$env:OTTRIN_TASK_INTERVAL_HOURS
if ($interval -lt 1 -or $interval -gt 744) { throw 'Refresh interval must be between 1 and 744 hours.' }
$executable = $env:OTTRIN_TASK_EXECUTABLE
if (-not [IO.File]::Exists($executable)) { throw 'The Ottrin executable does not exist.' }
$user = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$service = New-Object -ComObject Schedule.Service
$service.Connect()
$folder = $service.GetFolder('\')
$existing = $null
try { $existing = $folder.GetTask($TaskName) }
catch {
    if ($_.Exception.GetBaseException().HResult -ne -2147024894) { throw }
}
$enabled = $true
$security = $null
if ($null -ne $existing) {
    if ($existing.Definition.Principal.LogonType -ne 3) { throw 'The existing index task uses an unsupported logon type; setup was not changed.' }
    $principal = $existing.Definition.Principal.UserId
    $owner = if ($principal.StartsWith('S-1-')) {
        [Security.Principal.SecurityIdentifier]::new($principal)
    } else {
        ([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier])
    }
    if ($owner.Value -ne $user) { throw 'The existing index task belongs to another user; setup was not changed.' }
    $enabled = $existing.Enabled
    $security = $existing.GetSecurityDescriptor(7)
}
$definition = $service.NewTask(0)
$definition.RegistrationInfo.Description = 'Ottrin background index refresh'
$definition.Principal.UserId = $user
$definition.Principal.LogonType = 3 # Interactive token, no stored password.
$definition.Principal.RunLevel = 0 # Least privilege.
$definition.Settings.Enabled = $enabled
$definition.Settings.AllowDemandStart = $true
$definition.Settings.MultipleInstances = 2 # Ignore a trigger while indexing runs.
$trigger = $definition.Triggers.Create(1) # Time trigger; repeat without a duration limit.
$trigger.StartBoundary = [DateTime]::Now.AddMinutes(1).ToString('s')
$trigger.Repetition.Interval = "PT${interval}H"
$trigger.Enabled = $true
$action = $definition.Actions.Create(0)
$action.Path = $executable
$action.Arguments = 'index update'
# Preserve the disabled state atomically and retain the existing permissions.
# CREATE_OR_UPDATE | DONT_ADD_PRINCIPAL_ACE | IGNORE_REGISTRATION_TRIGGERS.
$null = $folder.RegisterTaskDefinition($TaskName, $definition, 54, $null, $null, 3, $security)
if ($enabled) { "Background Ottrin index refresh enabled every ${interval}h with Windows Task Scheduler." }
else { 'Background Ottrin index task updated; scheduled refresh remains disabled.' }
