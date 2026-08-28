# 一键发布资产打包：x64 NSIS/Portable + ARM64 Preview/Portable。
#
# 用法（仓库根）：
#   .\package.cmd                    # x64 全部资产（setup + portable）
#   .\package.cmd -Flavor portable   # 仅便携 zip
#   .\package.cmd -SkipBuild         # 复用 target/release 既有产物（重组装）
#   .\package.cmd -Arch arm64        # WoA 双 Preview zip（普通 + 便携）
#
# 契约（AGENTS.md 发布惯例）：
# - 版本号唯一来源是 workspace Cargo.toml，本脚本不做版本改写；
# - 包内 GUI/CLI 的 PE 架构必须与资产名称一致（打包时逐个断言）；
# - 便携 zip 必含 portable.marker 与中英两份说明（「便携版说明.txt」为固定
#   安全提示中文原文；PORTABLE-README.txt 为对齐 README.en.md 的英文版）。
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet("setup", "standalone", "portable", "all")]
    [string] $Flavor = "all",

    [ValidateSet("x64", "arm64")]
    [string] $Arch = "x64",

    # 测试与多工作树复用入口；日常由 package.cmd 固定传入当前仓库。
    [Parameter(DontShow = $true)]
    [string] $WorkspaceRoot = "",

    # 跳过构建，直接用 target/release 既有产物组装（迭代 zip 内容用）
    [switch] $SkipBuild
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($WorkspaceRoot)) {
    # scripts/ 的上一级即仓库根（与 clean.ps1 同款默认）
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
            throw "拒绝打包：目录缺少 QuotaTray 工作区标记 $marker"
        }
    }
    return $resolved
}

function Get-WorkspaceVersion {
    param([Parameter(Mandatory)] [string] $Root)

    # 版本唯一来源：[workspace.package] 的 version 字段（成员 crate 全部继承）
    $cargoToml = Get-Content -LiteralPath (Join-Path $Root "Cargo.toml") -Raw -Encoding UTF8
    if ($cargoToml -notmatch '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') {
        throw "无法从 workspace Cargo.toml 解析 [workspace.package] version"
    }
    return $Matches[1]
}

function Get-ExpectedAssetName {
    param(
        [Parameter(Mandatory)] [string] $Version,
        [Parameter(Mandatory)] [string] $Arch,
        [Parameter(Mandatory)] [ValidateSet("SetupExe", "StandaloneZip", "PortableZip")] [string] $FlavorKind
    )

    # 与 core::update::expected_asset_name 同契约：ARM64 在完成真实 WoA
    # 验收前统一带 -preview 段（AGENTS.md ARM64 Preview 声明）
    $archTag = if ($Arch -eq "arm64") { "arm64-preview" } else { $Arch }
    switch ($FlavorKind) {
        "SetupExe" { return "QuotaTray_${Version}_${archTag}-setup.exe" }
        "StandaloneZip" { return "QuotaTray_${Version}_${archTag}.zip" }
        "PortableZip" { return "QuotaTray_${Version}_${archTag}-portable.zip" }
    }
}

function Get-PackageKinds {
    param(
        [Parameter(Mandatory)] [ValidateSet("x64", "arm64")] [string] $Arch,
        [Parameter(Mandatory)] [ValidateSet("setup", "standalone", "portable", "all")] [string] $Flavor
    )

    if ($Arch -eq "arm64" -and $Flavor -eq "setup") {
        throw "ARM64 在真实 WoA 完整验收前不发布安装包；请使用 standalone、portable 或 all"
    }
    if ($Arch -eq "x64" -and $Flavor -eq "standalone") {
        throw "x64 普通分发使用 NSIS setup；standalone 仅用于 ARM64 Preview"
    }

    if ($Arch -eq "x64") {
        if ($Flavor -in @("setup", "all")) { "SetupExe" }
    }
    else {
        if ($Flavor -in @("standalone", "all")) { "StandaloneZip" }
    }
    if ($Flavor -in @("portable", "all")) { "PortableZip" }
}

function Get-PEMachine {
    param([Parameter(Mandatory)] [string] $Path)

    # 读 PE 头 Machine 字段：0x014c = i386，0x8664 = AMD64，0xAA64 = ARM64
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $reader = New-Object System.IO.BinaryReader($stream)
        $stream.Seek(0x3C, [System.IO.SeekOrigin]::Begin) | Out-Null
        $peOffset = $reader.ReadInt32()
        $stream.Seek($peOffset + 4, [System.IO.SeekOrigin]::Begin) | Out-Null
        return $reader.ReadUInt16()
    }
    finally {
        $stream.Dispose()
    }
}

function Assert-PEArch {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [string] $Arch
    )

    $expected = if ($Arch -eq "x64") { 0x8664 } elseif ($Arch -eq "arm64") { 0xAA64 } else { 0 }
    $machine = Get-PEMachine -Path $Path
    if ($machine -ne $expected) {
        throw ("PE 架构不符：{0} 的 Machine=0x{1:X4}，期望 {2}（0x{3:X4}）" -f $Path, $machine, $Arch, $expected)
    }
}

function Get-PortableReadme {
    # 便携包内说明：固定安全提示为 AGENTS.md「Portable 固定安全提示」
    # 原文，不得改写；用法段为补充说明。
    return @'
QuotaTray 便携版说明
====================

解压到任意可写目录后运行 QuotaTray.exe 即可；数据（配置、历史、主密钥）
全部保存在旁边的 Data/ 目录，不写注册表、不留系统残留。命令行工具
quota.exe 与 GUI 共用同一份数据。

⚠️ **便携版安全提示**：便携版会将用于解密凭据的主密钥保存在 `Data/portable.key`。虽然配置中的凭据仍以 AES-GCM 密文存储，但密钥与密文位于同一便携目录，因此整个 `Data/` 目录的保密级别等同明文凭据。请勿将其上传网盘、提交版本库或交给他人；若存储介质遗失或目录泄露，请立即轮换其中使用的全部 API Key。

首次运行会弹出安全确认；删除整个目录即完成卸载。更新请从 GitHub
Releases 下载新的便携 zip，退出应用后解压覆盖（Data/ 数据不受影响）。
'@
}

function Get-PortableReadmeEn {
    # 便携包内英文说明：固定安全提示与 README.en.md 逐字一致，
    # 结构对齐中文版（用法段为补充说明）。
    return @'
QuotaTray Portable Notes
========================

Extract the zip into any writable folder and run QuotaTray.exe. All data
(config, history, master key) stays in the Data/ folder next to the
executables - no registry entries, no system leftovers. quota.exe (the CLI)
shares the same data with the GUI.

⚠️ **Portable security notice**: the portable build stores the master key that decrypts your credentials in `Data/portable.key`. Although credentials remain AES-GCM encrypted in the configuration, the key and the ciphertext live in the same portable directory, so the entire `Data/` folder must be treated as plaintext credentials. Do not upload it to cloud drives, commit it to version control, or share it with others; if the medium is lost or the folder leaks, rotate every API key it contained immediately.

The first run asks for a security confirmation. Delete the whole folder
to uninstall. To update, download the new portable zip from GitHub
Releases, quit the app, and extract it over the folder (Data/ is
untouched).
'@
}

function New-ZipStaging {
    param(
        [Parameter(Mandatory)] [string] $Staging,
        [Parameter(Mandatory)] [string] $GuiExe,
        [Parameter(Mandatory)] [string] $CliExe,
        [switch] $Portable
    )

    if (Test-Path -LiteralPath $Staging) {
        Remove-Item -LiteralPath $Staging -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Staging | Out-Null
    Copy-Item -LiteralPath $GuiExe -Destination (Join-Path $Staging "QuotaTray.exe")
    Copy-Item -LiteralPath $CliExe -Destination (Join-Path $Staging "quota.exe")

    if ($Portable) {
        New-Item -ItemType File -Path (Join-Path $Staging "portable.marker") | Out-Null
        Get-PortableReadme | Set-Content -LiteralPath (Join-Path $Staging "便携版说明.txt") -Encoding UTF8
        Get-PortableReadmeEn | Set-Content -LiteralPath (Join-Path $Staging "PORTABLE-README.txt") -Encoding UTF8
    }
}

$root = Resolve-WorkspaceRoot -Path $WorkspaceRoot
$version = Get-WorkspaceVersion -Root $root
$packageKinds = @(Get-PackageKinds -Arch $Arch -Flavor $Flavor)
$targetDir = Join-Path $root "target/release"
$distDir = Join-Path $targetDir "dist"

Write-Host "QuotaTray $version 打包：Flavor=$Flavor Arch=$Arch"

if (-not $SkipBuild) {
    Push-Location (Join-Path $root "apps/quota-desktop")
    try {
        # tauri build 同时产出裸 GUI exe 与 CLI（beforeBuildCommand），
        # NSIS 在其上叠加——便携 zip 无需第二次构建
        if ($Arch -eq "arm64") {
            # Preview 阶段不产 ARM64 NSIS，只链接裸 GUI/CLI 后组装双 zip。
            pnpm tauri build --no-bundle --target aarch64-pc-windows-msvc
        }
        elseif ($packageKinds -contains "SetupExe") {
            pnpm tauri build
        }
        else {
            pnpm tauri build --no-bundle
        }
        if ($LASTEXITCODE -ne 0) {
            throw "tauri build 失败（退出码 $LASTEXITCODE）"
        }
    }
    finally {
        Pop-Location
    }
}

$artifacts = @()

# 产物根目录（各 flavor 分支共用；须在分支外定义——
# 单跑 zip 时 setup 分支不执行，分支内定义会让 StrictMode
# 在下方引用处直接报未定义变量）
# --target 交叉时 tauri 产物整体落在 target/<triple>/release/ 下
# （NSIS bundle 与裸 exe 同根）。ARM64 构建钩子与 build.rs 共同保证
# CLI 从同一 target triple 取得；Assert-PEArch 再守最终资产边界。
$archTargetDir = if ($Arch -eq "arm64") {
    Join-Path (Join-Path $root "target") "aarch64-pc-windows-msvc/release"
}
else {
    $targetDir
}

if ($packageKinds -contains "SetupExe") {
    $setupName = Get-ExpectedAssetName -Version $version -Arch $Arch -FlavorKind "SetupExe"
    $nsisDir = Join-Path $archTargetDir "bundle/nsis"
    $setupPath = Join-Path $nsisDir $setupName
    if (-not (Test-Path -LiteralPath $setupPath -PathType Leaf)) {
        throw "安装包产物缺失：$setupPath（先完整执行一次不带 -SkipBuild 的打包）"
    }
    $artifacts += $setupPath
}

if (($packageKinds -contains "StandaloneZip") -or ($packageKinds -contains "PortableZip")) {
    $guiExe = Join-Path $archTargetDir "quota-desktop.exe"
    $cliExe = Join-Path $archTargetDir "quota.exe"
    foreach ($exe in @($guiExe, $cliExe)) {
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
            throw "zip 构建产物缺失：$exe（先完整执行一次不带 -SkipBuild 的打包）"
        }
        # AGENTS.md 门禁：包内 GUI/CLI 的 PE 架构必须与资产名称一致
        Assert-PEArch -Path $exe -Arch $Arch
    }

}

if ($packageKinds -contains "StandaloneZip") {
    # 普通 ARM64 Preview 不带 marker，继续使用安装态数据目录与系统凭据库。
    $staging = Join-Path $distDir "arm64-standalone-staging"
    New-ZipStaging -Staging $staging -GuiExe $guiExe -CliExe $cliExe
    $zipName = Get-ExpectedAssetName -Version $version -Arch $Arch -FlavorKind "StandaloneZip"
    $zipPath = Join-Path $distDir $zipName
    if (Test-Path -LiteralPath $zipPath) {
        Remove-Item -LiteralPath $zipPath -Force
    }
    Compress-Archive -Path (Join-Path $staging "*") -DestinationPath $zipPath
    $artifacts += $zipPath
}

if ($packageKinds -contains "PortableZip") {
    # marker 决定便携模式；Data/ 由首次确认门控创建，不预置进包。
    $staging = Join-Path $distDir ("{0}-portable-staging" -f $Arch)
    New-ZipStaging -Staging $staging -GuiExe $guiExe -CliExe $cliExe -Portable
    $zipName = Get-ExpectedAssetName -Version $version -Arch $Arch -FlavorKind "PortableZip"
    $zipPath = Join-Path $distDir $zipName
    if (Test-Path -LiteralPath $zipPath) {
        Remove-Item -LiteralPath $zipPath -Force
    }
    Compress-Archive -Path (Join-Path $staging "*") -DestinationPath $zipPath
    $artifacts += $zipPath
}

Write-Host ""
Write-Host "打包完成："
foreach ($artifact in $artifacts) {
    $sizeMb = [math]::Round((Get-Item -LiteralPath $artifact).Length / 1MB, 1)
    Write-Host ("  {0}（{1} MB）" -f $artifact, $sizeMb)
}
