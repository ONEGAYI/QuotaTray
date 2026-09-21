# release-notes.ps1 的契约测试：版本段提取 / 固定声明逐字 / 组装顺序 / CRLF 免疫。
# 每条断言注明其对应的发布格式条款（.agents/skills/release/SKILL.md）。
# 运行：powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-notes.tests.ps1
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$notesScript = Join-Path $PSScriptRoot "release-notes.ps1"
$repoRoot = Split-Path -Parent $PSScriptRoot
$skillFile = Join-Path $repoRoot ".agents/skills/release/SKILL.md"

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

# 沙箱 CHANGELOG 模板：目标版本 9.9.8 夹在 9.9.9（更新）与 9.9.7（更旧）之间，
# 链接区在文件尾部——覆盖"提取段必须严格止于下一个版本头"的边界。
$sampleLines = @(
    "# 更新日志",
    "",
    "## [9.9.9] - 2026-01-02",
    "",
    "本版本为测试版本更新，不应出现在输出。",
    "",
    "### 新功能",
    "",
    "- 更新版本的条目 A",
    "",
    "## [9.9.8] - 2026-01-01",
    "",
    "本版本为测试版本目标，总结行应为输出首行。",
    "",
    "### 新功能",
    "",
    "- 目标版本的条目 X",
    "",
    "### Bug 修复",
    "",
    "- 目标版本的条目 Y",
    "",
    "## [9.9.7] - 2025-12-31",
    "",
    "本版本为测试版本更旧，不应出现在输出。",
    "",
    "### 新功能",
    "",
    "- 更旧版本的条目 Z",
    "",
    "<!-- 变更链接 -->",
    "[9.9.9]: https://github.com/ONEGAYI/QuotaTray/compare/v9.9.8...v9.9.9",
    "[9.9.8]: https://github.com/ONEGAYI/QuotaTray/compare/v9.9.7...v9.9.8"
)

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) (
    "quotatray-release-notes-test-{0}" -f [System.Guid]::NewGuid().ToString("N")
)
New-Item -ItemType Directory -Path $sandbox | Out-Null

try {
    # LF 与 CRLF 两份同内容 CHANGELOG：CRLF 是 0.14.0 notes 污染事故的根因形态，
    # 两份产出必须逐字节全等（Get-Content 逐行读免疫行尾差异）。
    $lfChangelog = Join-Path $sandbox "CHANGELOG-lf.md"
    [System.IO.File]::WriteAllText($lfChangelog, ($sampleLines -join "`n") + "`n")
    $crlfChangelog = Join-Path $sandbox "CHANGELOG-crlf.md"
    [System.IO.File]::WriteAllText($crlfChangelog, ($sampleLines -join "`r`n") + "`r`n")

    Write-Host "== 版本段提取 =="
    # 契约：notes 只含目标版本的 CHANGELOG 完整内容——版本标题行与旧版本条目
    # 零泄漏（0.14.0 事故：sed 范围吞入整个历史，正是本条回归锁）
    $lfOut = Join-Path $sandbox "notes-lf.md"
    & $notesScript -Version "9.9.8" -Changelog $lfChangelog -OutFile $lfOut | Out-Null
    $lfBody = [System.IO.File]::ReadAllText($lfOut)
    Assert-That (@($lfBody -split "`n" | Where-Object { $_ -match '^## \[' }).Count -eq 0) "输出不含任何 ## [ 版本标题行"
    Assert-That ($lfBody -notmatch '9\.9\.9|9\.9\.7|条目 A|条目 Z') "新旧版本条目零泄漏"
    Assert-That ($lfBody -notmatch 'compare/v') "变更链接区不进输出"
    Assert-That ($lfBody -match '(?m)^本版本为测试版本目标') "输出含目标版本总结段"
    # 契约：notes 直接以版本总结开头（v0.13.2 起惯例），不带版本标题行
    $firstLine = ($lfBody -split "`n")[0]
    Assert-That ($firstLine -eq "本版本为测试版本目标，总结行应为输出首行。") "首行为版本总结（非标题行）"
    Assert-That ($lfBody -match '(?m)^### 新功能') "输出含分类标题（完整 CHANGELOG 内容）"

    Write-Host "== CRLF 免疫 =="
    # 契约：提取不受 CHANGELOG 行尾形态影响（事故根因回归锁）
    $crlfOut = Join-Path $sandbox "notes-crlf.md"
    & $notesScript -Version "9.9.8" -Changelog $crlfChangelog -OutFile $crlfOut | Out-Null
    $crlfBody = [System.IO.File]::ReadAllText($crlfOut)
    Assert-That ($crlfBody -eq $lfBody) "CRLF 输入产出与 LF 逐字节全等"

    Write-Host "== 输出形态 =="
    # 契约：产出供 gh --notes-file 直用——无 BOM、无 CR、结尾恰一个换行
    $bytes = [System.IO.File]::ReadAllBytes($lfOut)
    Assert-That (-not ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)) "输出无 UTF-8 BOM"
    Assert-That ($lfBody -notmatch "`r") "输出无 CR（统一 LF）"
    Assert-That ($lfBody.EndsWith("`n") -and -not $lfBody.EndsWith("`n`n")) "结尾恰一个换行"

    Write-Host "== 固定声明逐字一致 =="
    # 契约：三段固定文本与 release 技能正文逐字一致（技能红线：不得缩写、
    # 改写、凭记忆复述）——脚本常量是复述位，本组断言锁死漂移。
    # SKILL.md 无 BOM，PS5.1 下必须显式 UTF8 读取（package.tests.ps1 同款坑）
    $skillText = Get-Content -LiteralPath $skillFile -Raw -Encoding UTF8
    $portableExpected = [regex]::Match($skillText, '(?m)^> ⚠️ \*\*便携版安全提示\*\*：.*$').Value
    $arm64Expected = [regex]::Match($skillText, '(?m)^> 🧪 \*\*ARM64 预览版\*\*：.*$').Value
    $androidExpected = [regex]::Match($skillText, '(?m)^> 🧪 \*\*Android 预览版\*\*：.*$').Value
    Assert-That ($portableExpected.Length -gt 0) "SKILL.md 含便携提示基准行"
    Assert-That ($arm64Expected.Length -gt 0) "SKILL.md 含 ARM64 声明基准行"
    Assert-That ($androidExpected.Length -gt 0) "SKILL.md 含 Android 声明基准行"
    Assert-That (($lfBody -split "`n") -contains $portableExpected) "便携提示与 SKILL.md 逐字一致"
    Assert-That (($lfBody -split "`n") -contains $androidExpected) "Android 声明与 SKILL.md 逐字一致（默认含 APK 资产）"

    Write-Host "== 组装顺序 =="
    # 契约：notes 顺序为 CHANGELOG 内容 → 便携提示 → ARM64（含 WoA 资产时）
    # → Android（含 APK 资产时）；便携提示每个 Release 无条件必含
    $lines = $lfBody -split "`n"
    $portableIdx = [array]::IndexOf($lines, $portableExpected)
    $androidIdx = [array]::IndexOf($lines, $androidExpected)
    Assert-That ($portableIdx -ge 0 -and $androidIdx -gt $portableIdx) "默认尾序：便携 → Android"
    $arm64Out = Join-Path $sandbox "notes-arm64.md"
    & $notesScript -Version "9.9.8" -Changelog $lfChangelog -OutFile $arm64Out -Arm64:$true | Out-Null
    $arm64Body = [System.IO.File]::ReadAllText($arm64Out)
    $armLines = $arm64Body -split "`n"
    $aPortable = [array]::IndexOf($armLines, $portableExpected)
    $aArm64 = [array]::IndexOf($armLines, $arm64Expected)
    $aAndroid = [array]::IndexOf($armLines, $androidExpected)
    Assert-That (($armLines -contains $arm64Expected)) "-Arm64 时含 ARM64 声明且与 SKILL.md 逐字一致"
    Assert-That ($aPortable -lt $aArm64 -and $aArm64 -lt $aAndroid) "-Arm64 尾序：便携 → ARM64 → Android"
    $noAndroidOut = Join-Path $sandbox "notes-no-android.md"
    & $notesScript -Version "9.9.8" -Changelog $lfChangelog -OutFile $noAndroidOut -Android:$false | Out-Null
    $noAndroidBody = [System.IO.File]::ReadAllText($noAndroidOut)
    Assert-That (($noAndroidBody -split "`n" -notcontains $androidExpected)) "-Android:`$false 时无 Android 声明"
    Assert-That (($noAndroidBody -split "`n") -contains $portableExpected) "无 APK 资产时便携提示仍在（每个 Release 必含）"

    Write-Host "== 默认路径冒烟 =="
    # 契约：不传 -Changelog 时默认取仓库根 CHANGELOG.md——$PSScriptRoot 在
    # PS5.1 的 param 默认值表达式中为空（曾致 Split-Path 崩溃），默认值
    # 必须在脚本主体求值。用 workspace 当前版本对真实 CHANGELOG 冒烟。
    $cargoToml = Get-Content -LiteralPath (Join-Path $repoRoot "Cargo.toml") -Raw -Encoding UTF8
    $currentVersion = [regex]::Match($cargoToml, '(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
    $defaultOut = Join-Path $sandbox "notes-default.md"
    & $notesScript -Version $currentVersion -OutFile $defaultOut | Out-Null
    $defaultBody = [System.IO.File]::ReadAllText($defaultOut)
    Assert-That ($defaultBody.Length -gt 0) "默认 CHANGELOG 路径（workspace 版本 $currentVersion）成功生成非空 notes"

    Write-Host "== 拒绝路径 =="
    # 契约：版本段未找到必须确定性失败，不得静默产出空/全文 notes
    $missingRejected = $false
    try { & $notesScript -Version "9.9.0" -Changelog $lfChangelog -OutFile (Join-Path $sandbox "dead.md") | Out-Null }
    catch { $missingRejected = $true }
    Assert-That $missingRejected "版本段未找到时拒绝"
    # 契约：版本头须为 Keep a Changelog 形态（## [x.y.z] - 日期），缺日期即坏档
    $badHeader = Join-Path $sandbox "CHANGELOG-bad-header.md"
    [System.IO.File]::WriteAllText($badHeader, "## [9.9.8]`n`n总结。`n`n### 新功能`n`n- 条目`n")
    $badHeaderRejected = $false
    try { & $notesScript -Version "9.9.8" -Changelog $badHeader -OutFile (Join-Path $sandbox "dead2.md") | Out-Null }
    catch { $badHeaderRejected = $true }
    Assert-That $badHeaderRejected "版本头缺日期时拒绝"
    # 契约：版本段须含 ### 分类标题（Keep a Changelog 完整内容），空段拒绝
    $noSection = Join-Path $sandbox "CHANGELOG-no-section.md"
    [System.IO.File]::WriteAllText($noSection, "## [9.9.8] - 2026-01-01`n`n只有总结没有分类。`n`n## [9.9.7] - 2025-12-31`n`n### 新功能`n`n- 旧`n")
    $noSectionRejected = $false
    try { & $notesScript -Version "9.9.8" -Changelog $noSection -OutFile (Join-Path $sandbox "dead3.md") | Out-Null }
    catch { $noSectionRejected = $true }
    Assert-That $noSectionRejected "段内无 ### 分类时拒绝"
    # 契约：产出行数上限 200——版本段异常膨胀（如解析边界失效吞入历史）
    # 在此兜底拦截
    $huge = [System.Collections.Generic.List[string]]::new()
    $huge.Add("## [9.9.8] - 2026-01-01")
    $huge.Add("")
    $huge.Add("总结。")
    $huge.Add("")
    $huge.Add("### 新功能")
    for ($i = 0; $i -lt 210; $i++) { $huge.Add("- 条目 $i") }
    $hugeChangelog = Join-Path $sandbox "CHANGELOG-huge.md"
    [System.IO.File]::WriteAllText($hugeChangelog, ($huge -join "`n") + "`n")
    $hugeRejected = $false
    try { & $notesScript -Version "9.9.8" -Changelog $hugeChangelog -OutFile (Join-Path $sandbox "dead4.md") | Out-Null }
    catch { $hugeRejected = $true }
    Assert-That $hugeRejected "超出 200 行上限时拒绝"
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
