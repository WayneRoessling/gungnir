# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md
# Render every PlantUML (.puml) and Mermaid (.mmd) source under docs/architecture/uaf
# to SVG under docs/architecture/uaf/rendered/, mirroring the folder structure.
#
# PlantUML: uses `plantuml` on PATH if present, else the plantuml/plantuml container
# through Docker, pinned to the version the committed renders came from (D-79): the
# layout engine is part of what a render looks like, and every SVG records the
# PlantUML version that drew it (`<?plantuml ...?>`). Mermaid: uses `mmdc` on PATH if
# present, else `npx @mermaid-js/mermaid-cli`. Exits non-zero if any diagram fails.
$plantumlImage = "plantuml/plantuml:1.2026.8"
$ErrorActionPreference = "Continue"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$out = Join-Path $here "rendered"
$failed = 0

function Render-Puml($src) {
    $rel = [System.IO.Path]::GetRelativePath($here, $src)
    $dir = Join-Path $out (Split-Path -Parent $rel)
    New-Item -ItemType Directory -Force $dir | Out-Null
    if (Get-Command plantuml -ErrorAction SilentlyContinue) {
        & plantuml -tsvg -o $dir $src
    } elseif (Get-Command docker -ErrorAction SilentlyContinue) {
        $relDir = (Split-Path -Parent $rel) -replace "\\", "/"
        $relFile = $rel -replace "\\", "/"
        & docker run --rm -v "${here}:/work" -w /work $plantumlImage -tsvg -o "/work/rendered/$relDir" $relFile
    } else {
        Write-Error "no plantuml or docker available for $rel"; $script:failed = 1; return
    }
    if ($LASTEXITCODE -ne 0) { $script:failed = 1 }
}

function Render-Mmd($src) {
    $rel = [System.IO.Path]::GetRelativePath($here, $src)
    $dir = Join-Path $out (Split-Path -Parent $rel)
    New-Item -ItemType Directory -Force $dir | Out-Null
    $target = Join-Path $dir ([System.IO.Path]::GetFileNameWithoutExtension($src) + ".svg")
    if (Get-Command mmdc -ErrorAction SilentlyContinue) {
        & mmdc -i $src -o $target
    } elseif (Get-Command npx -ErrorAction SilentlyContinue) {
        & npx -y @mermaid-js/mermaid-cli -i $src -o $target
    } else {
        Write-Error "no mmdc or npx available for $rel"; $script:failed = 1; return
    }
    if ($LASTEXITCODE -ne 0) { $script:failed = 1 }
}

Get-ChildItem -Path $here -Recurse -Filter *.puml | Where-Object { $_.FullName -notlike "$out*" } | ForEach-Object { Render-Puml $_.FullName }
Get-ChildItem -Path $here -Recurse -Filter *.mmd | Where-Object { $_.FullName -notlike "$out*" } | ForEach-Object { Render-Mmd $_.FullName }
exit $failed
