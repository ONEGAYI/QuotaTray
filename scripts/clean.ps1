[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = "Medium")]
param(
    [Parameter(Position = 0)]
    [ValidateRange(0, 3)]
    [int] $Level = 0,

    # 测试与多工作树复用入口；日常由 clean.cmd 固定传入当前仓库。
    [Parameter(DontShow = $true)]
    [string] $WorkspaceRoot = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($WorkspaceRoot)) {
    $WorkspaceRoot = Split-Path -Parent $PSScriptRoot
}

function Resolve-WorkspaceRoot {
    param([Parameter(Mandatory)] [string] $Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "工作区不存在：$Path"
    }
    $resolved = (Get-Item -LiteralPath $Path -Force).FullName.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    foreach ($marker in @("Cargo.toml", "apps/quota-desktop/package.json")) {
        if (-not (Test-Path -LiteralPath (Join-Path $resolved $marker) -PathType Leaf)) {
            throw "拒绝清理：目录缺少 QuotaTray 工作区标记 $marker"
        }
    }
    return $resolved
}

function Resolve-CleanTarget {
    param(
        [Parameter(Mandatory)] [string] $Root,
        [Parameter(Mandatory)] [string] $RelativePath
    )

    $target = [System.IO.Path]::GetFullPath((Join-Path $Root $RelativePath))
    $prefix = $Root + [System.IO.Path]::DirectorySeparatorChar
    if (
        $target -eq $Root -or
        -not $target.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)
    ) {
        throw "拒绝清理工作区外路径：$target"
    }
    return $target
}

function Select-CleanLevel {
    Write-Host "QuotaTray 开发目录清理"
    Write-Host "  1 轻量：增量/Vite 缓存 + 前端与 Tauri 生成物"
    Write-Host "  2 标准：轻量项 + 完整 target/debug（保留 release）"
    Write-Host "  3 深度：完整 target + node_modules + 所有生成物"
    $selection = Read-Host "请选择级别 [1-3]"
    if ($selection -notmatch "^[1-3]$") {
        throw "清理级别必须是 1、2 或 3"
    }
    return [int] $selection
}

$root = Resolve-WorkspaceRoot -Path $WorkspaceRoot
if ($Level -eq 0) {
    $Level = Select-CleanLevel
}

$generated = @(
    "apps/quota-desktop/dist",
    "apps/quota-desktop/src-tauri/gen/schemas",
    "apps/quota-desktop/src-tauri/generated"
)
$relativeTargets = switch ($Level) {
    1 {
        @(
            "target/debug/incremental",
            "target/release/incremental",
            "apps/quota-desktop/node_modules/.vite",
            "apps/quota-desktop/node_modules/.vite-temp"
        ) + $generated
    }
    2 {
        @(
            "target/debug",
            "target/release/incremental",
            "apps/quota-desktop/node_modules/.vite",
            "apps/quota-desktop/node_modules/.vite-temp"
        ) + $generated
    }
    3 {
        @(
            "target",
            "apps/quota-desktop/node_modules"
        ) + $generated
    }
    default { throw "未支持的清理级别：$Level" }
}

$levelNames = @("", "轻量", "标准", "深度")
Write-Host ("开始 {0}清理（Level {1}）：{2}" -f $levelNames[$Level], $Level, $root)

$removed = 0
$skipped = 0
foreach ($relative in $relativeTargets) {
    $target = Resolve-CleanTarget -Root $root -RelativePath $relative
    if (-not (Test-Path -LiteralPath $target)) {
        Write-Host "  跳过（不存在）：$relative"
        $skipped++
        continue
    }

    if ($PSCmdlet.ShouldProcess($target, "递归删除可再生开发文件")) {
        $item = Get-Item -LiteralPath $target -Force
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            Remove-Item -LiteralPath $target -Force
        }
        else {
            Remove-Item -LiteralPath $target -Recurse -Force
        }
        Write-Host "  已清理：$relative"
        $removed++
    }
}

if ($WhatIfPreference) {
    Write-Host "预览完成：未删除任何文件。"
}
else {
    Write-Host "清理完成：已删除 $removed 项，跳过 $skipped 项。"
    if ($Level -eq 3) {
        Write-Host "深度清理后请先在 apps/quota-desktop 执行 pnpm install。"
    }
}
