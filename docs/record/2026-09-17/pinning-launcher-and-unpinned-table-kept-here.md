# Pinning launcher and unpinned table kept here

`claude/gate6-pcore-pinning` is deleted with this item, and what it held that `main` did
not is below. Two record items cite that branch by name
(`gate-6-noise-floor-and-core-pinning.md` sections 1 and 2, and
`2026-09-16/gate-6-benchmarks-pinned-to-performance-cores.md`, which was only ever on the
branch), and a record cannot be edited once merged, so this item is where those citations
now lead. Nothing here is wired into a workflow, and by D-62 nothing will be: gate 6 never
enforces its threshold on `gungnir-rtx-5060ti`, so a launcher that pins its benchmarks has
no job to do.

**Why it is here rather than on a branch.** A branch is not an archive. Nothing on `main`
keeps it alive, no check reads it, and a citation to one is a citation to something anybody
can delete. Keeping the evidence as a record item is the difference between saying the
measurement exists and having it.

## The unpinned table, which lived only on the branch

Three dispatches of `8e03a78` against the baseline saved from `8e03a78`: identical code,
so every ratio is noise. This is the "before" the pinned table in
`gate-6-noise-floor-and-core-pinning.md` section 2 compares against.

| benchmark | run 1 | run 2 | run 3 | range |
|---|---|---|---|---|
| ekf | 1.082 | 1.382 | 1.908 | 0.83 |
| hungarian 100x100 | 0.957 | 1.435 | 1.447 | 0.49 |
| hungarian 100x400 | 1.028 | 1.277 | 1.362 | 0.33 |
| hungarian 200x200 | 0.925 | 1.481 | 1.146 | 0.56 |
| journal_append | 0.804 | 0.914 | 0.838 | 0.11 |
| node_ingest | 0.945 | 1.433 | 0.969 | 0.49 |
| node_journal sync | 1.005 | 1.030 | 1.044 | 0.04 |
| phd | 1.356 | 1.108 | 1.068 | 0.29 |
| is_healthy | 1.094 | 1.585 | 0.972 | 0.61 |
| tracks | 0.942 | 1.164 | 0.978 | 0.22 |
| startup | 0.921 | 1.104 | 0.911 | 0.19 |
| tick | 1.077 | 1.297 | 1.265 | 0.23 |

Ten of twelve crossed 10 percent at least once. Note that four of these benchmarks were
measuring the timer at the time (`ekf`, `phd`, `tracks`, `is_healthy`), which #125 then
fixed; `2026-09-16/sub-nanosecond-benchmarks-replaced-with-real-work.md` has that, and the
re-measurement on real benchmarks did not narrow the spread either.

## What the launcher did, and what it cost to get right

It read the performance cores from Windows rather than assuming them, pinned itself, and
ran the command, which inherited the affinity. On this host the performance cores are
logical 0, 1, 6, 7, 8, 9, 18 and 19, mask `0xC03C3` -- not contiguous, so a fixed "first
eight" mask would have put six of the eight slots on efficiency cores. It refused on a host
reporting a single efficiency class rather than run unpinned under a pinned name.

Three local checks passed before it was used: a grandchild process reported affinity
787395, a child's exit code 7 came back as 7, and arguments arrived exactly, including
`--` and one containing a space. Two Windows PowerShell traps were found on the way, and
they are the part most likely to be useful again:

1. Under `powershell -File`, a script-level `param()` with `ValueFromRemainingArguments`
   refuses the wrapped command's own words ("A positional parameter cannot be found that
   accepts argument 'cmd'"). An unbound `$args` passes everything through.
2. Splatting that array as `@rest` makes Windows PowerShell 5.1 read `--version` as
   parameter syntax and hand the child `-` and `-version`. Passing the array as one value
   (`& $exe $rest`) does not.

Measured with it, 8 of 12 benchmarks varied *more* than unpinned, because whole runs move
together: the noise is other work on the machine, and pinning the benchmarks to eight cores
leaves everything else free to use those same eight.

## The script, verbatim

If a dedicated benchmark runner ever exists, this is a starting point rather than a thing
to restore: it was never wired into a green Gate 6 run on `main`, and the workflow edit that
went with it is not reproduced here because `bench-regression.yml` has moved on twice since
(#133, #136).

```powershell
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

# Run a command restricted to this host's performance cores (gate 6, GAP-093).
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File run_on_performance_cores.ps1 <command> [args...]
#
# Why: the self-hosted runner is a hybrid CPU, and three comparisons of identical code
# against one fixed baseline on 2026-09-16 moved by up to 0.83 in ratio -- ten of twelve
# benchmarks crossed the 10 percent threshold at least once. A benchmark the scheduler
# moves between performance and efficiency cores measures the scheduler.
#
# The performance cores are read from Windows at run time (GetSystemCpuSetInformation's
# EfficiencyClass: the highest class is the performance class), never assumed. On this
# host they are logical processors 0, 1, 6, 7, 8, 9, 18 and 19 -- not contiguous, so
# "the first eight" would have pinned six of eight slots to efficiency cores.
#
# The affinity is set on this process before the command starts, and Windows gives
# every child process its parent's affinity at creation, so `cargo` and every benchmark
# binary it runs inherit it. The command's exit code is this script's exit code.
#
# A host with a single efficiency class has no performance cores to prefer, and this
# refuses rather than running unpinned: an unpinned measurement reported as pinned is
# the kind of claim this workspace does not make.
#
# No `param()` block: under `-File`, a script-level parameter declared with
# ValueFromRemainingArguments refused the command's own words ("A positional parameter
# cannot be found that accepts argument 'cmd'"), and a declared parameter would also claim
# any argument spelled like its name. The unbound `$args` passes everything through.
$Command = @($args)
if ($Command.Count -eq 0) { Write-Error 'usage: run_on_performance_cores.ps1 <command> [args...]'; exit 2 }
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class GungnirCpuSets {
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetSystemCpuSetInformation(IntPtr info, uint length, out uint returned, IntPtr process, uint flags);
    // Each row: logical processor index, efficiency class.
    public static List<int[]> Read() {
        uint length;
        GetSystemCpuSetInformation(IntPtr.Zero, 0, out length, IntPtr.Zero, 0);
        IntPtr buffer = Marshal.AllocHGlobal((int)length);
        var rows = new List<int[]>();
        try {
            if (!GetSystemCpuSetInformation(buffer, length, out length, IntPtr.Zero, 0)) {
                throw new InvalidOperationException("GetSystemCpuSetInformation failed: " + Marshal.GetLastWin32Error());
            }
            int offset = 0;
            while (offset < length) {
                int size = Marshal.ReadInt32(buffer, offset);
                // SYSTEM_CPU_SET_INFORMATION: Size, Type, Id (4 each), Group (2),
                // LogicalProcessorIndex, CoreIndex, LastLevelCacheIndex, NumaNodeIndex,
                // EfficiencyClass (1 each).
                rows.Add(new int[] { Marshal.ReadByte(buffer, offset + 14), Marshal.ReadByte(buffer, offset + 18) });
                offset += size;
            }
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
        return rows;
    }
}
'@

$rows = [GungnirCpuSets]::Read()
$classes = $rows | ForEach-Object { $_[1] } | Sort-Object -Unique
if (@($classes).Count -lt 2) {
    Write-Error "this host reports a single efficiency class ($classes); there are no performance cores to pin to, and running unpinned would not be the measurement gate 6 asks for"
    exit 2
}
$performance = ($classes | Measure-Object -Maximum).Maximum
$cores = @($rows | Where-Object { $_[1] -eq $performance } | ForEach-Object { $_[0] } | Sort-Object)
if ($cores | Where-Object { $_ -ge 64 }) {
    Write-Error "a performance core has logical index 64 or above, which one affinity mask cannot address"
    exit 2
}
[long]$mask = 0
foreach ($c in $cores) { $mask = $mask -bor ([long]1 -shl $c) }

$self = [System.Diagnostics.Process]::GetCurrentProcess()
$self.ProcessorAffinity = [IntPtr]$mask
Write-Host ("pinned to performance cores: logical {0} of {1} (efficiency class {2}, mask 0x{3:X})" -f ($cores -join ','), $rows.Count, $performance, $mask)

$exe = $Command[0]
$rest = if ($Command.Count -gt 1) { $Command[1..($Command.Count - 1)] } else { @() }
Write-Host ("running: {0} {1}" -f $exe, ($rest -join ' '))
# `$rest` as one array, not splatted as `@rest`: Windows PowerShell 5.1 reads a splatted
# `--version` as parameter syntax and handed cargo `-` and `-version` instead.
& $exe $rest
exit $LASTEXITCODE
```
