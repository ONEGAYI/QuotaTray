$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$cleanScript = Join-Path $PSScriptRoot "clean.ps1"
$sandboxes = [System.Collections.Generic.List[string]]::new()

function Add-TestFile {
    param(
        [Parameter(Mandatory)] [string] $Root,
        [Parameter(Mandatory)] [string] $RelativePath
    )

    $path = Join-Path $Root $RelativePath
    $parent = Split-Path -Parent $path
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    [System.IO.File]::WriteAllText($path, "test")
}

function New-CleanSandbox {
    $root = Join-Path ([System.IO.Path]::GetTempPath()) (
        "quotatray-clean-test-{0}" -f [System.Guid]::NewGuid().ToString("N")
    )
    New-Item -ItemType Directory -Path $root | Out-Null
    $sandboxes.Add($root)

    foreach ($relative in @(
        "Cargo.toml",
        "apps/quota-desktop/package.json",
        "target/debug/incremental/cache.bin",
        "target/debug/deps/keep.bin",
        "target/release/incremental/cache.bin",
        "target/release/bundle/keep.exe",
        "apps/quota-desktop/node_modules/.vite/cache.bin",
        "apps/quota-desktop/node_modules/.vite-temp/cache.bin",
        "apps/quota-desktop/node_modules/pkg/keep.js",
        "apps/quota-desktop/dist/index.html",
        "apps/quota-desktop/src-tauri/gen/schemas/schema.json",
        "apps/quota-desktop/src/index.css",
        ".DevApiKey.json",
        ".zcode/keep.json"
    )) {
        Add-TestFile -Root $root -RelativePath $relative
    }
    return $root
}

function Assert-PathState {
    param(
        [Parameter(Mandatory)] [string] $Root,
        [Parameter(Mandatory)] [string] $RelativePath,
        [Parameter(Mandatory)] [bool] $Exists
    )

    $actual = Test-Path -LiteralPath (Join-Path $Root $RelativePath)
    if ($actual -ne $Exists) {
        throw "断言失败：$RelativePath 预期存在=$Exists，实际=$actual"
    }
}

function Assert-ProtectedFiles {
    param([Parameter(Mandatory)] [string] $Root)
    foreach ($relative in @(
        "Cargo.toml",
        "apps/quota-desktop/package.json",
        "apps/quota-desktop/src/index.css",
        ".DevApiKey.json",
        ".zcode/keep.json"
    )) {
        Assert-PathState -Root $Root -RelativePath $relative -Exists $true
    }
}

try {
    # 根入口不显式传 WorkspaceRoot 时也必须能解析脚本所在仓库。
    & $cleanScript -Level 3 -WhatIf | Out-Null

    # Level 1：只清增量/Vite 缓存和生成物，保留编译依赖与安装依赖。
    $root = New-CleanSandbox
    & $cleanScript -Level 1 -WorkspaceRoot $root -Confirm:$false | Out-Null
    foreach ($relative in @(
        "target/debug/incremental",
        "target/release/incremental",
        "apps/quota-desktop/node_modules/.vite",
        "apps/quota-desktop/node_modules/.vite-temp",
        "apps/quota-desktop/dist",
        "apps/quota-desktop/src-tauri/gen/schemas"
    )) {
        Assert-PathState -Root $root -RelativePath $relative -Exists $false
    }
    Assert-PathState -Root $root -RelativePath "target/debug/deps/keep.bin" -Exists $true
    Assert-PathState -Root $root -RelativePath "target/release/bundle/keep.exe" -Exists $true
    Assert-PathState -Root $root -RelativePath "apps/quota-desktop/node_modules/pkg/keep.js" -Exists $true
    Assert-ProtectedFiles -Root $root

    # Level 2：删除整个 debug，保留 release 与 node_modules。
    $root = New-CleanSandbox
    & $cleanScript -Level 2 -WorkspaceRoot $root -Confirm:$false | Out-Null
    Assert-PathState -Root $root -RelativePath "target/debug" -Exists $false
    Assert-PathState -Root $root -RelativePath "target/release/incremental" -Exists $false
    Assert-PathState -Root $root -RelativePath "target/release/bundle/keep.exe" -Exists $true
    Assert-PathState -Root $root -RelativePath "apps/quota-desktop/node_modules/pkg/keep.js" -Exists $true
    Assert-ProtectedFiles -Root $root

    # Level 3：删除完整 Rust 目标目录与前端依赖，仍不得碰开发者文件。
    $root = New-CleanSandbox
    & $cleanScript -Level 3 -WorkspaceRoot $root -Confirm:$false | Out-Null
    Assert-PathState -Root $root -RelativePath "target" -Exists $false
    Assert-PathState -Root $root -RelativePath "apps/quota-desktop/node_modules" -Exists $false
    Assert-PathState -Root $root -RelativePath "apps/quota-desktop/dist" -Exists $false
    Assert-PathState -Root $root -RelativePath "apps/quota-desktop/src-tauri/gen/schemas" -Exists $false
    Assert-ProtectedFiles -Root $root

    # WhatIf：完整列出计划但不删除。
    $root = New-CleanSandbox
    & $cleanScript -Level 3 -WorkspaceRoot $root -WhatIf | Out-Null
    Assert-PathState -Root $root -RelativePath "target/debug/incremental/cache.bin" -Exists $true
    Assert-PathState -Root $root -RelativePath "apps/quota-desktop/node_modules/pkg/keep.js" -Exists $true

    # 非仓库目录必须在任何删除前拒绝。
    $invalidRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
        "quotatray-clean-test-{0}" -f [System.Guid]::NewGuid().ToString("N")
    )
    New-Item -ItemType Directory -Path $invalidRoot | Out-Null
    $sandboxes.Add($invalidRoot)
    $rejected = $false
    try {
        & $cleanScript -Level 1 -WorkspaceRoot $invalidRoot -Confirm:$false | Out-Null
    }
    catch {
        $rejected = $true
    }
    if (-not $rejected) {
        throw "缺少仓库标记的目录应被拒绝"
    }

    Write-Host "clean 契约测试通过：三级清理、WhatIf、受保护文件与仓库边界"
}
finally {
    $tempPrefix = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    foreach ($sandbox in $sandboxes) {
        $resolved = [System.IO.Path]::GetFullPath($sandbox)
        $leaf = Split-Path -Leaf $resolved
        if (
            $resolved.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase) -and
            $leaf.StartsWith("quotatray-clean-test-", [System.StringComparison]::Ordinal)
        ) {
            Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
