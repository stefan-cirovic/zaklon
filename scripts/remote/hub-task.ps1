# Runs ON the Windows test machine. Registers (or refreshes) a scheduled task
# that starts the Zaklon hub at logon, fixes the firewall rules, and starts it now.
#   powershell -NoProfile -ExecutionPolicy Bypass -File hub-task.ps1 -Root E:\zaklon
param([Parameter(Mandatory = $true)][string]$Root)

$ErrorActionPreference = 'Continue'
$exe = Join-Path $Root 'bin\zaklon-hub.exe'
$data = Join-Path $Root 'data'
$task = 'Zaklon_Hub'

Stop-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue
Stop-Process -Name 'zaklon-hub' -Force -ErrorAction SilentlyContinue
# Library engine processes left behind by hub versions before the job-object fix.
Get-Process -Name 'kiwix-serve' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($Root, [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force -ErrorAction SilentlyContinue

# Firewall: one clean rule per port, replacing anything left from earlier attempts.
Get-NetFirewallRule | Where-Object { $_.DisplayName -like '*Zaklon*' } | Remove-NetFirewallRule -ErrorAction SilentlyContinue
New-NetFirewallRule -DisplayName 'Zaklon hub (app, TLS 8484)' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8484 -Profile Private,Public | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon hub (install page 8480)' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8480 -Profile Private,Public | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon hub (discovery 8485)' -Direction Inbound -Action Allow -Protocol UDP -LocalPort 8485 -Profile Private,Public | Out-Null
# Program rules too, so Windows never shows the "blocked some features" prompt (which creates block rules when dismissed).
New-NetFirewallRule -DisplayName 'Zaklon hub (program TCP)' -Direction Inbound -Action Allow -Protocol TCP -Program $exe -Profile Private,Public | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon hub (program UDP)' -Direction Inbound -Action Allow -Protocol UDP -Program $exe -Profile Private,Public | Out-Null

$action = New-ScheduledTaskAction -Execute $exe -Argument ('--root "' + $data + '"') -WorkingDirectory (Join-Path $Root 'bin')
$trigger = New-ScheduledTaskTrigger -AtLogOn
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $task -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $task
Start-Sleep -Seconds 3
$p = Get-Process -Name 'zaklon-hub' -ErrorAction SilentlyContinue
if ($p) { "hub running (pid $($p.Id))" } else { "hub did not start" }
