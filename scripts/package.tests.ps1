# package.ps1 的契约测试：版本解析 / 期望资产名 / PE Machine 读取。
# 运行：powershell -NoProfile -ExecutionPolicy Bypass -File scripts/package.tests.ps1
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$packageScript = Join-Path $PSScriptRoot "package.ps1"
$repoRoot = Split-Path -Parent $PSScriptRoot

# 载入被测脚本的函数定义（dot-source 会执行主流程，改为抽取函数体）
$definitions = [System.Collections.Generic.List[string]]::new()
$inFunction = $false
foreach ($line in Get-Content -LiteralPath $packageScript) {
    if ($line -match '^function\s+([A-Za-z-]+)\s*\{') {
        $inFunction = $true
    }
    if ($inFunction) {
        $definitions.Add($line)
        if ($line -eq "}") {
            $inFunction = $false
        }
    }
}
Invoke-Expression ($definitions -join "`n")

$failures = [System.Collections.Generic.List[string]]::new()
function Assert-That {
    param(
        [Parameter(Mandatory)] [bool] $Condition,
        [Parameter(Mandatory)] [string] $Message
    )
    if (-not $Condition) {
        $script:failures.Add($Message)
        Write-Host "  失败：$Message" -ForegroundColor Red
    }
    else {
        Write-Host "  通过：$Message" -ForegroundColor Green
    }
}

Write-Host "== Get-WorkspaceVersion =="
# 契约：版本从 workspace Cargo.toml 的 [workspace.package] 解析
$version = Get-WorkspaceVersion -Root $repoRoot
Assert-That ($version -match '^\d+\.\d+\.\d+$') "仓库版本解析为三段语义化版本（实际：$version）"
Assert-That ((Get-Content -LiteralPath (Join-Path $repoRoot "Cargo.toml") -Raw) -match [regex]::Escape("version = `"$version`"")) "版本值与 Cargo.toml 一致"

Write-Host "== Get-ExpectedAssetName =="
# 契约：四类资产命名与 core::update::expected_asset_name 同构（预研 §五）
Assert-That ((Get-ExpectedAssetName -Version "0.7.0" -Arch "x64" -FlavorKind "SetupExe") -eq "QuotaTray_0.7.0_x64-setup.exe") "x64 安装包命名"
Assert-That ((Get-ExpectedAssetName -Version "0.7.0" -Arch "x64" -FlavorKind "PortableZip") -eq "QuotaTray_0.7.0_x64-portable.zip") "x64 便携包命名"
Assert-That ((Get-ExpectedAssetName -Version "0.7.0" -Arch "arm64" -FlavorKind "StandaloneZip") -eq "QuotaTray_0.7.0_arm64-preview.zip") "ARM64 普通预览包命名"
Assert-That ((Get-ExpectedAssetName -Version "0.7.0" -Arch "arm64" -FlavorKind "PortableZip") -eq "QuotaTray_0.7.0_arm64-preview-portable.zip") "ARM64 便携包带 -preview 段"

Write-Host "== Get-PackageKinds =="
Assert-That (((Get-PackageKinds -Arch "x64" -Flavor "all") -join ",") -eq "SetupExe,PortableZip") "x64 all = setup + portable"
Assert-That (((Get-PackageKinds -Arch "arm64" -Flavor "all") -join ",") -eq "StandaloneZip,PortableZip") "ARM64 all = 普通预览 zip + 便携预览 zip"
$invalidPairRejected = $false
try { Get-PackageKinds -Arch "arm64" -Flavor "setup" | Out-Null } catch { $invalidPairRejected = $true }
Assert-That $invalidPairRejected "ARM64 拒绝生成未验收的 setup"

Write-Host "== Get-PortableReadme =="
# 契约：固定安全提示为 AGENTS.md 原文——含字面 ** 与反引号（曾因
# PowerShell 双引号 here-string 转义吞掉反引号而失守）；整行锚定与
# README.md 逐字一致（读文件必须显式 UTF8：README 无 BOM，PS5.1 在
# 非 UTF-8 ACP 机器上会按本地码页解码导致断言崩溃）
$readme = Get-PortableReadme
Assert-That ($readme.Contains("**便携版安全提示**")) "说明含粗体标记原文"
Assert-That ($readme.Contains('`Data/portable.key`')) "说明含反引号路径原文"
$zhExpected = [regex]::Match((Get-Content -LiteralPath (Join-Path $repoRoot "README.md") -Raw -Encoding UTF8), '> ⚠️ \*\*便携版安全提示\*\*：[^\r\n]*')
$zhActual = [regex]::Match($readme, '⚠️ \*\*便携版安全提示\*\*：[^\r\n]*')
Assert-That ($zhExpected.Success -and $zhActual.Success -and ($zhActual.Value -eq $zhExpected.Value.Substring(2))) "中文固定提示与 README.md 逐字一致"

Write-Host "== Get-PortableReadmeEn =="
# 契约：英文说明的固定安全提示与 README.en.md 逐字一致（同样含字面
# ** 与反引号），供便携包双语收录
$readmeEn = Get-PortableReadmeEn
Assert-That ($readmeEn.Contains("**Portable security notice**")) "英文说明含粗体标记原文"
Assert-That ($readmeEn.Contains('`Data/portable.key`')) "英文说明含反引号路径原文"
$enExpected = [regex]::Match((Get-Content -LiteralPath (Join-Path $repoRoot "README.en.md") -Raw -Encoding UTF8), '> ⚠️ \*\*Portable security notice\*\*:[^\r\n]*')
$enActual = [regex]::Match($readmeEn, '⚠️ \*\*Portable security notice\*\*:[^\r\n]*')
Assert-That ($enExpected.Success -and $enActual.Success -and ($enActual.Value -eq $enExpected.Value.Substring(2))) "英文固定提示与 README.en.md 逐字一致"

Write-Host "== Get-PEMachine =="
# 契约：最小 PE 字节流读出 Machine 字段（0x8664=x64 / 0xAA64=ARM64）
function New-MinimalPE {
    param([Parameter(Mandatory)] [uint16] $Machine)
    $bytes = [System.Collections.Generic.List[byte]]::new()
    # DOS header 占位 + e_lfanew 指向 0x40
    for ($i = 0; $i -lt 0x40; $i++) { $bytes.Add(0) }
    $bytes[0] = 0x4D; $bytes[1] = 0x5A
    $lfanewBytes = [System.BitConverter]::GetBytes([int]0x40)
    for ($i = 0; $i -lt 4; $i++) { $bytes[0x3C + $i] = $lfanewBytes[$i] }
    # PE\0\0 签名 + Machine（小端）
    $bytes.AddRange([byte[]]@(0x50, 0x45, 0x00, 0x00))
    $machineBytes = [System.BitConverter]::GetBytes($Machine)
    $bytes.AddRange($machineBytes)
    return $bytes.ToArray()
}

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("quotatray-package-test-{0}" -f [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $sandbox | Out-Null
try {
    $x64Pe = Join-Path $sandbox "x64.exe"
    [System.IO.File]::WriteAllBytes($x64Pe, (New-MinimalPE -Machine 0x8664))
    Assert-That ((Get-PEMachine -Path $x64Pe) -eq 0x8664) "最小 x64 PE 读出 Machine=0x8664"

    $armPe = Join-Path $sandbox "arm.exe"
    [System.IO.File]::WriteAllBytes($armPe, (New-MinimalPE -Machine 0xAA64))
    Assert-That ((Get-PEMachine -Path $armPe) -eq 0xAA64) "最小 ARM64 PE 读出 Machine=0xAA64"

    # 契约：架构断言对不匹配的 PE 抛错
    $threw = $false
    try {
        Assert-PEArch -Path $armPe -Arch "x64"
    }
    catch {
        $threw = $true
    }
    Assert-That $threw "Assert-PEArch 对架构不符的 PE 抛错"
    Assert-PEArch -Path $x64Pe -Arch "x64"
    Assert-That $true "Assert-PEArch 对匹配的 PE 放行"

    Write-Host "== New-ZipStaging =="
    $standalone = Join-Path $sandbox "standalone"
    New-ZipStaging -Staging $standalone -GuiExe $armPe -CliExe $armPe
    Assert-That ((Test-Path -LiteralPath (Join-Path $standalone "QuotaTray.exe")) -and (Test-Path -LiteralPath (Join-Path $standalone "quota.exe"))) "普通 ARM64 zip 含 GUI 与 CLI"
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $standalone "portable.marker"))) "普通 ARM64 zip 不含 portable.marker"

    $portable = Join-Path $sandbox "portable"
    New-ZipStaging -Staging $portable -GuiExe $armPe -CliExe $armPe -Portable
    Assert-That (Test-Path -LiteralPath (Join-Path $portable "portable.marker")) "便携 ARM64 zip 含 portable.marker"
    Assert-That ((Test-Path -LiteralPath (Join-Path $portable "便携版说明.txt")) -and (Test-Path -LiteralPath (Join-Path $portable "PORTABLE-README.txt"))) "便携 zip 含中英双说明"
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $portable "Data"))) "便携 zip 不预置 Data 目录"
}
finally {
    Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host ("契约测试失败 {0} 项" -f $failures.Count) -ForegroundColor Red
    exit 1
}
Write-Host "全部契约测试通过" -ForegroundColor Green
exit 0
