# Change only this installation's future login trigger. Do not recreate the
# registration, replace its permissions or start/stop any running instance.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($env:OTTRIN_TASK_ENABLED -notin @('true', 'false')) { throw 'Invalid automatic startup choice.' }
$service = New-Object -ComObject Schedule.Service
$service.Connect()
$task = Get-OttrinIndexTask ($service.GetFolder('\'))
if ($null -eq $task) { throw 'Install the background index service before changing login startup.' }
Assert-OttrinTaskOwner $task
if ($task.Definition.Principal.RunLevel -ne 0) { throw 'The index task has unexpected administrator privileges; startup was not changed.' }
if ($task.Definition.Actions.Count -ne 1) { throw 'The index task has unexpected actions; startup was not changed.' }
$action = $task.Definition.Actions.Item(1)
if ($action.Path -ne $env:OTTRIN_TASK_EXECUTABLE -or $action.Arguments -ne '--index-daemon') {
    throw 'The index task belongs to another installation. Repair its service setup before changing login startup.'
}
$enabled = $env:OTTRIN_TASK_ENABLED -eq 'true'
$task.Enabled = $enabled
if ($task.Enabled -ne $enabled) { throw 'Windows did not retain the automatic startup choice.' }
