#Requires -Version 7
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md
<#
.SYNOPSIS
Run one dispatch of a self-hosted workflow on this workstation, on a runner registered
for that one job.

.DESCRIPTION
`gpu-fusion.yml` and `bench-regression.yml` run on the owner's workstation, by dispatch
only (D-10, D-62). The runner is started by hand when it is needed, never installed as a
service (D-63). Until 2026-10-29 it was a standing registration started with `run.cmd`;
GitHub removes a self-hosted runner that has not connected for fourteen days, and by
2026-10-28 it had, so every dispatch queued for a runner that no longer existed.

This script replaces the standing registration with a just-in-time one. It asks GitHub
for a runner configuration valid for a single job, dispatches the workflow, runs the
runner until that job is done, and leaves nothing registered behind. Nothing expires,
because nothing stands; no runner credential stays on disk; and GitHub still records the
log that docs/release-governance.md's evidence package is made of.

Needs `gh`, signed in as an account with admin rights on the repository, and the runner
installed at -RunnerDir (https://github.com/actions/runner/releases, Windows x64 zip
extracted there; no `config.cmd` step).

.EXAMPLE
pwsh .github/scripts/run_self_hosted.ps1 gpu-fusion.yml

.EXAMPLE
pwsh .github/scripts/run_self_hosted.ps1 bench-regression.yml -Ref main
#>
[CmdletBinding()]
param(
    # The workflow file to dispatch. Only the two that run on this machine.
    [Parameter(Mandatory, Position = 0)]
    [ValidateSet('gpu-fusion.yml', 'bench-regression.yml')]
    [string] $Workflow,

    # The branch the workflow runs from. bench-regression writes its baseline only on main.
    [string] $Ref = 'main',

    [string] $Repo = 'WayneRoessling/gungnir',

    [string] $RunnerDir = 'C:\actions-runner',

    # The workflows' `runs-on` name these labels, and the name is what their comments and
    # the record cite, so both stay what the standing runner had.
    [string] $RunnerName = 'gungnir-rtx-5060ti',

    # How long to wait for the job to reach the runner before giving up.
    [int] $PickupMinutes = 10
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$labels = @('self-hosted', 'Windows', 'X64', 'gpu')
$listener = Join-Path $RunnerDir 'bin\Runner.Listener.exe'
if (-not (Test-Path $listener)) {
    throw "no runner at $RunnerDir; extract the Windows x64 runner release there"
}

function Invoke-Gh {
    $out = & gh @args
    if ($LASTEXITCODE -ne 0) { throw "gh $($args -join ' ') exited with $LASTEXITCODE" }
    $out
}

function Remove-Runner([long] $Id) {
    & gh api -X DELETE "repos/$Repo/actions/runners/$Id" 2>$null | Out-Null
}

# A runner of the same name left by an interrupted run, or the old standing registration,
# would make the new registration fail; only an offline one is removed.
$existing = Invoke-Gh api "repos/$Repo/actions/runners" --paginate --jq ".runners[] | select(.name == `"$RunnerName`")" |
    ForEach-Object { $_ | ConvertFrom-Json }
foreach ($r in @($existing)) {
    if ($r.status -ne 'offline') {
        throw "runner $RunnerName (id $($r.id)) is $($r.status); is another run in progress?"
    }
    Write-Host "removing offline runner $RunnerName (id $($r.id))"
    Remove-Runner $r.id
}

$labelArgs = $labels | ForEach-Object { '-f'; "labels[]=$_" }
$jit = Invoke-Gh api -X POST "repos/$Repo/actions/runners/generate-jitconfig" `
    -f "name=$RunnerName" -F runner_group_id=1 -f work_folder=_work @labelArgs |
    ConvertFrom-Json
$runnerId = $jit.runner.id
Write-Host "registered $RunnerName (id $runnerId) for one job, labels $($labels -join ', ')"

# The runner writes the one-job configuration into these files and marks them hidden,
# and .NET refuses to overwrite a hidden file, so any left behind -- by the standing
# registration this replaced, or by a runner stopped mid-job -- stop it at start. They
# belong to a registration that no longer exists; they are moved aside, not deleted.
$aside = Join-Path $RunnerDir '_previous-config'
foreach ($name in '.runner', '.credentials', '.credentials_rsaparams') {
    $path = Join-Path $RunnerDir $name
    if (Test-Path -LiteralPath $path) {
        New-Item -ItemType Directory -Force -Path $aside | Out-Null
        $file = Get-Item -LiteralPath $path -Force
        $file.Attributes = 'Normal'
        Move-Item -LiteralPath $path -Destination (Join-Path $aside $name) -Force
        Write-Host "moved a previous registration's $name to $aside"
    }
}

$runner = $null
$run = $null
try {
    $listRuns = {
        Invoke-Gh run list --repo $Repo --workflow $Workflow --branch $Ref `
            --event workflow_dispatch --limit 10 --json 'databaseId,url' | ConvertFrom-Json
    }
    $before = @(& $listRuns | ForEach-Object { $_.databaseId })
    $said = Invoke-Gh workflow run $Workflow --repo $Repo --ref $Ref

    # gh prints the new run's URL when GitHub returns it. Otherwise the new run is the
    # dispatch that was not there before: matched by id, not by time, because this
    # machine's clock and GitHub's differ by seconds, which was enough to miss it.
    if ("$said" -match '/actions/runs/(\d+)') {
        $id = [long] $Matches[1]
        $run = [pscustomobject] @{ databaseId = $id; url = "https://github.com/$Repo/actions/runs/$id" }
    }
    for ($i = 0; $i -lt 30 -and -not $run; $i++) {
        Start-Sleep -Seconds 2
        $run = & $listRuns | Where-Object { $_.databaseId -notin $before } | Select-Object -First 1
    }
    if (-not $run) { throw "the dispatch of $Workflow on $Ref did not appear; check its runs and cancel any left queued" }
    Write-Host "dispatched: $($run.url)"

    # The configuration is passed on the command line and never written out by this
    # script; the runner deletes its own copy when the job is done.
    $runner = Start-Process -FilePath $listener -ArgumentList 'run', '--jitconfig', $jit.encoded_jit_config `
        -WorkingDirectory $RunnerDir -NoNewWindow -PassThru

    $deadline = (Get-Date).AddMinutes($PickupMinutes)
    $picked = $false
    while (-not $runner.HasExited) {
        Start-Sleep -Seconds 10
        if (-not $picked) {
            $jobs = Invoke-Gh run view $run.databaseId --repo $Repo --json jobs --jq '[.jobs[] | select(.status != "queued")] | length'
            $picked = [int] $jobs -gt 0
            if (-not $picked -and (Get-Date) -gt $deadline) {
                throw "the job did not reach the runner within $PickupMinutes minutes"
            }
        }
    }
    Write-Host "runner exited with $($runner.ExitCode)"
}
finally {
    if ($runner -and -not $runner.HasExited) { Stop-Process -Id $runner.Id -Force }
    # An ephemeral runner removes itself after its job; this covers every other way out.
    Remove-Runner $runnerId
    # The runner leaves the one-job configuration on disk (seen 2026-10-29); its
    # registration is gone, so the credential in it is dead, and nothing keeps it.
    foreach ($name in '.runner', '.credentials', '.credentials_rsaparams') {
        $path = Join-Path $RunnerDir $name
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
    }
    # Left alone, a run whose runner is gone waits a day in the queue, and the next
    # dispatch's runner would pick it up instead of its own. Whatever the way out, a run
    # still waiting for a runner here never gets one.
    if ($run) {
        $status = & gh run view $run.databaseId --repo $Repo --json status --jq .status 2>$null
        if ($status -in 'queued', 'pending', 'waiting', 'requested') {
            & gh run cancel $run.databaseId --repo $Repo 2>$null | Out-Null
            Write-Host "cancelled $($run.url), which no runner took"
        }
    }
}

# The runner exits as soon as its job ends; the run is marked complete a moment later.
for ($i = 0; $i -lt 12; $i++) {
    $result = Invoke-Gh run view $run.databaseId --repo $Repo --json 'status,conclusion' | ConvertFrom-Json
    if ($result.status -eq 'completed') { break }
    Start-Sleep -Seconds 5
}
Write-Host "$Workflow on ${Ref}: $($result.status) $($result.conclusion) -- $($run.url)"
exit ($result.conclusion -eq 'success' ? 0 : 1)
