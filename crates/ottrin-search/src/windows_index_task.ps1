# Embedded by the search crate; task parameters arrive as process environment data.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$TaskName = Get-OttrinIndexTaskName
$executable = $env:OTTRIN_TASK_EXECUTABLE
if (-not [IO.File]::Exists($executable)) { throw 'The Ottrin executable does not exist.' }
$user = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$service = New-Object -ComObject Schedule.Service
$service.Connect()
$folder = $service.GetFolder('\')
$existing = Get-OttrinIndexTask $folder
$superseded = Get-OttrinSupersededTask $folder
$prior = if ($null -ne $existing) { $existing } else { $superseded }
$existingRecord = Get-OttrinTaskRecord $TaskName $existing
$supersededRecord = Get-OttrinTaskRecord 'Ottrin Index' $superseded
$enabled = $true
$security = $null
if ($null -ne $prior) {
    Assert-OttrinTaskOwner $prior
    $enabled = $prior.Enabled
    $security = $prior.GetSecurityDescriptor(7)
}
$definition = $service.NewTask(0)
$definition.RegistrationInfo.Description = 'Ottrin continuous background index service'
$definition.Principal.UserId = $user
$definition.Principal.LogonType = 3 # Interactive token, no stored password.
$definition.Principal.RunLevel = 0 # Least privilege.
$definition.Settings.Enabled = $enabled
$definition.Settings.AllowDemandStart = $true
$definition.Settings.MultipleInstances = 2 # Ignore a trigger while indexing runs.
$definition.Settings.ExecutionTimeLimit = 'PT0S' # Persistent daemon, no runtime limit.
$definition.Settings.DisallowStartIfOnBatteries = $false
$definition.Settings.StopIfGoingOnBatteries = $false
$trigger = $definition.Triggers.Create(9) # Start once when this user logs in.
$trigger.UserId = $user
$trigger.Enabled = $true
$action = $definition.Actions.Create(0)
$action.Path = $executable
$action.Arguments = '--index-daemon'
# Preserve the disabled state atomically and retain the existing permissions.
# CREATE_OR_UPDATE | DONT_ADD_PRINCIPAL_ACE | IGNORE_REGISTRATION_TRIGGERS.
if ($null -ne $existing) { Stop-OttrinIndexTask $existing }
$registered = $folder.RegisterTaskDefinition($TaskName, $definition, 54, $null, $null, 3, $security)
if ($null -ne $superseded) {
    try { $superseded.Enabled = $false; Stop-OttrinIndexTask $superseded; $folder.DeleteTask('Ottrin Index', 0) }
    catch {
        # A failed retirement must not leave two scheduled writers active.
        $registered.Enabled = $false
        Restore-OttrinTaskRecord $folder $supersededRecord
        Restore-OttrinTaskRecord $folder $existingRecord
        throw
    }
}
if ($enabled) { 'Background Ottrin index service enabled at login with Windows Task Scheduler.' }
else { 'Background Ottrin index task updated; automatic startup remains disabled.' }
