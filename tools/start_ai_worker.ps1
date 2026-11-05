param([string]$Connection = '')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path $PSScriptRoot -Parent
if (!$Connection) { $Connection = Join-Path $taskRoot '.local/connection.json' }
$taskLogs = Join-Path $taskRoot '.local/ai-worker'
New-Item -ItemType Directory -Force -Path $taskLogs | Out-Null
$taskPython = (Get-Command python.exe -ErrorAction Stop).Source
$taskWorker = Join-Path $PSScriptRoot 'ai_worker.py'
$taskRunning = Get-CimInstance Win32_Process -Filter "Name='python.exe'" | Where-Object { $_.CommandLine -and $_.CommandLine.Replace('\', '/').Contains($taskWorker.Replace('\', '/')) }
if ($taskRunning) { Write-Output 'GPU Pocket AI worker is already running'; return }
$taskArguments = @('-X', 'utf8', '-u', ('"' + $taskWorker + '"'), '--connection', ('"' + $Connection + '"'))
$taskProcess = Start-Process -FilePath $taskPython -ArgumentList $taskArguments -WorkingDirectory $taskRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskLogs 'stdout.log') -RedirectStandardError (Join-Path $taskLogs 'stderr.log') -PassThru
Write-Output "GPU Pocket AI worker PID: $($taskProcess.Id)"
