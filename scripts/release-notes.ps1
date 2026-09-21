# 从 CHANGELOG 提取指定版本段并组装 Release notes（发布资产外的文本产物）。
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-notes.ps1 `
#         -Version 0.14.0 -OutFile <路径> [-Arm64] [-Android:$false]
#
# 固定声明文本与 .agents/skills/release/SKILL.md 正文逐字一致
# （契约测试 scripts/release-notes.tests.ps1 锁定漂移），组装顺序按技能
# 规定：CHANGELOG 完整内容 → 便携安全提示 → ARM64 声明（-Arm64 时）→
# Android 声明（-Android:$true 时，默认含 APK 资产）。
#
# 断言即防线：版本段提取经 CRLF 免疫（逐行读，2026-09-19 v0.14.0 事故
# 根因为 sed 行尾锚在 CRLF 上失配吞入全部历史），产出受
# 无版本标题行 / 无链接区 / 行数上限 / 无 CR / 无 BOM 五重终检。
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string] $Version,

    [Parameter(Mandatory = $true)]
    [string] $OutFile,

    [string] $Changelog,

    # 本次 Release 是否包含 WoA ARM64 资产（含则追加 ARM64 Preview 声明）
    [bool] $Arm64 = $false,

    # 本次 Release 是否包含 Android APK 资产（默认 true：APK 由发布 tag CI 自动注入）
    [bool] $Android = $true
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# 默认值放主体求值：PS5.1 下 $PSScriptRoot 在 param 默认值表达式中为空
if (-not $Changelog) {
    $Changelog = Join-Path (Split-Path -Parent $PSScriptRoot) "CHANGELOG.md"
}

# 三段固定声明：release 技能正文原文（含 > 引用前缀），不得缩写改写
$NoticePortable = (@'
> ⚠️ **便携版安全提示**：便携版会将用于解密凭据的主密钥保存在 `Data/portable.key`。虽然配置中的凭据仍以 AES-GCM 密文存储，但密钥与密文位于同一便携目录，因此整个 `Data/` 目录的保密级别等同明文凭据。请勿将其上传网盘、提交版本库或交给他人；若存储介质遗失或目录泄露，请立即轮换其中使用的全部 API Key。
'@).Trim()

$NoticeArm64 = (@'
> 🧪 **ARM64 预览版**：ARM64 构建已通过交叉编译与产物架构检查，但尚未完成真实 Windows on ARM 设备的完整运行验收。该资产仅供预览和反馈，不应视为稳定支持。
'@).Trim()

$NoticeAndroid = (@'
> 🧪 **Android 预览版**：Android 端已在模拟器完成冒烟验收，但尚未完成真实设备的完整运行验收。该资产仅供预览和反馈，不应视为稳定支持。
'@).Trim()

if (-not (Test-Path -LiteralPath $Changelog -PathType Leaf)) {
    throw ("CHANGELOG 不存在：{0}" -f $Changelog)
}

# 逐行读天然免疫 CRLF/LF 行尾差异（Get-Content 剥离行终止符）
$lines = Get-Content -LiteralPath $Changelog -Encoding UTF8

# 版本头须为 Keep a Changelog 形态：## [x.y.z] - YYYY-MM-DD
$headerExact = '^{0} - \d{{4}}-\d{{2}}-\d{{2}}$' -f [regex]::Escape("## [$Version]")
$headerLoose = '^{0}' -f [regex]::Escape("## [$Version]")

$start = -1
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match $headerExact) { $start = $i; break }
}
if ($start -lt 0) {
    if ($lines | Where-Object { $_ -match $headerLoose }) {
        throw ("版本 {0} 的标题行格式不符（须为 ## [{0}] - YYYY-MM-DD）" -f $Version)
    }
    throw ("CHANGELOG 中未找到版本 {0} 的段落" -f $Version)
}

# 提取到下一个版本头（或变更链接标记）为止，不含终止行
$section = [System.Collections.Generic.List[string]]::new()
for ($i = $start + 1; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^## \[' -or $lines[$i] -match '^<!--') { break }
    $section.Add($lines[$i])
}

# 去首尾空行
while ($section.Count -gt 0 -and $section[0] -eq "") { $section.RemoveAt(0) }
while ($section.Count -gt 0 -and $section[$section.Count - 1] -eq "") { $section.RemoveAt($section.Count - 1) }

if ($section.Count -eq 0) {
    throw ("版本 {0} 的段落为空" -f $Version)
}
if (-not ($section | Where-Object { $_ -match '^### ' })) {
    throw ("版本 {0} 的段落缺少 ### 分类标题（Keep a Changelog 完整内容）" -f $Version)
}
if ($section | Where-Object { $_ -match '^## \[' }) {
    throw ("版本 {0} 的提取段混入版本标题行（解析边界失效）" -f $Version)
}
if ($section | Where-Object { $_ -match '^\[\d+\.\d+\.\d+\]:' }) {
    throw ("版本 {0} 的提取段混入变更链接定义（解析边界失效）" -f $Version)
}
if ($section.Count -gt 200) {
    throw ("版本 {0} 的段落达 {1} 行，超出 200 行上限（疑似解析边界失效，人工核查 CHANGELOG）" -f $Version, $section.Count)
}

# 组装：版本内容 → 便携（每个 Release 必含）→ ARM64（-Arm64）→ Android（-Android）
$blocks = [System.Collections.Generic.List[string]]::new()
$blocks.Add(($section -join "`n"))
$blocks.Add($NoticePortable)
if ($Arm64) { $blocks.Add($NoticeArm64) }
if ($Android) { $blocks.Add($NoticeAndroid) }
$content = ($blocks -join "`n`n") + "`n"

# 终检：产出供 gh release --notes-file 直用
if ($content -match "`r") { throw "产出含 CR（必须统一 LF）" }
if (-not $content.EndsWith("`n") -or $content.EndsWith("`n`n")) { throw "结尾必须恰一个换行" }
$firstLine = ($content -split "`n")[0]
if ($firstLine -eq "" -or $firstLine -match '^#') { throw "首行必须为版本总结（非空且非标题）" }

$outDir = Split-Path -Parent $OutFile
if ($outDir -and -not (Test-Path -LiteralPath $outDir)) {
    New-Item -ItemType Directory -Path $outDir | Out-Null
}
# UTF-8 无 BOM 写出（PS5.1 的 Set-Content -Encoding UTF8 带 BOM，gh 会把 BOM 带进 notes 正文）
[System.IO.File]::WriteAllText($OutFile, $content, [System.Text.UTF8Encoding]::new($false))

Write-Host ("Release notes 已生成：{0}（{1} 行；声明段：便携{2}{3}）" -f `
    $OutFile, ($content -split "`n").Count, `
    $(if ($Arm64) { " + ARM64" } else { "" }), `
    $(if ($Android) { " + Android" } else { "" }))
Write-Host "发布前义务：全文逐行过目；上传后线上回读与本地全量 diff（suian-release 完成检查）。"
