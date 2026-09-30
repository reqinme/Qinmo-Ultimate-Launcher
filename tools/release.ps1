<#
    本文件必须保存为**带 BOM 的 UTF-8**。

    Windows PowerShell 5.1 读 .ps1 时，若没有 BOM 会按当前 ANSI 代码页解析，
    中文注释会变成乱码并**直接导致语法错误**（"字符串缺少终止符"之类）。
    这个坑已经踩过一次：文件是 write 工具生成的（无 BOM），一跑就解析失败。
    改动本文件后请确认前三字节是 239,187,191。
#>
<#
.SYNOPSIS
    出一个可发布的版本，并生成/更新发布清单。

.DESCRIPTION
    这是 P7 发布工程的入口。它做四件事：

      1. 按指定版本号发布单文件 exe
      2. 算出 SHA-256 与字节数
      3. 把它并入发布清单（保留历史版本，方便仍在旧版的用户升级）
      4. **自检**：起一个本地 HTTP 服务托管清单，再用一个**更低版本**的 exe
         去执行 `update --manifest`，确认客户端自己的解析器接受这份清单

    第 4 步是这个脚本存在的主要理由。清单是人手拼出来的 JSON，
    而"字段名拼错"这类错误在真正发布之前不会被任何单元测试发现——
    等用户点「检查更新」时才炸。用客户端自己的解析器验一遍，是成本最低的兜底。

.PARAMETER Version
    版本号，例如 0.3.0。会写进程序集版本，也是清单里的主键。

.PARAMETER OutputDirectory
    产物与清单的输出目录，默认 artifacts/release。

.PARAMETER Notes
    更新说明（可选），会写进清单的 notes 字段。

.PARAMETER MinimumVersion
    低于此版本不能直接升级（可选）。

.PARAMETER BaselineVersion
    自检用的"旧版本"号，默认 0.0.1。必须低于 Version。

.EXAMPLE
    pwsh -File tools/release.ps1 -Version 0.3.0 -Notes "下载引擎加固"
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Version,

    [string] $OutputDirectory = 'artifacts/release',

    [string] $Notes = '',

    [string] $MinimumVersion = '',

    [string] $BaselineVersion = '0.0.1',

    [int] $Port = 8921
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$project = Join-Path $repoRoot 'src/QinmoUltimateLauncher/QinmoUltimateLauncher.csproj'
$exeName = 'QinmoUltimateLauncher.exe'

if (-not (Test-Path $project)) {
    throw "找不到工程文件：$project"
}

function Write-Step([string] $Text) {
    Write-Host "==> $Text" -ForegroundColor Cyan
}

function Invoke-Dotnet([string[]] $Arguments) {
    & dotnet @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet 失败（退出码 $LASTEXITCODE）：dotnet $($Arguments -join ' ')"
    }
}

# ---------- 1. 发布 ----------
$versionDir = Join-Path $repoRoot (Join-Path $OutputDirectory $Version)
$baselineDir = Join-Path $repoRoot (Join-Path $OutputDirectory "_baseline-$BaselineVersion")

Write-Step "发布 $Version"
if (Test-Path $versionDir) { Remove-Item $versionDir -Recurse -Force }
Invoke-Dotnet @('publish', $project, '-c', 'Release', '-o', $versionDir,
                ("-p:Version=$Version"), '-v', 'q', '--nologo')

$exePath = Join-Path $versionDir $exeName
if (-not (Test-Path $exePath)) { throw "发布产物里没有 $exeName" }

# 发布目录里只保留需要分发的东西
Get-ChildItem $versionDir -File | Where-Object { $_.Name -ne $exeName } | ForEach-Object {
    Write-Host "    剔除不分发的文件：$($_.Name)" -ForegroundColor DarkGray
    Remove-Item $_.FullName -Force
}

$size = (Get-Item $exePath).Length
$sha = (Get-FileHash $exePath -Algorithm SHA256).Hash.ToLowerInvariant()
$fileVersion = (Get-Item $exePath).VersionInfo.FileVersion

Write-Host "    $exeName  $size B  sha256=$sha"
Write-Host "    文件版本 $fileVersion"

# ---------- 2. 并入清单 ----------
$manifestPath = Join-Path $repoRoot (Join-Path $OutputDirectory 'manifest.json')

$releases = @()
if (Test-Path $manifestPath) {
    $existing = Get-Content $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($existing.PSObject.Properties.Name -contains 'releases') {
        $releases = @($existing.releases)
    }
}

# 同一个版本号重发时替换而不是追加
$releases = @($releases | Where-Object { $_.version -ne $Version })

$entry = [ordered]@{
    version     = $Version
    downloadUrl = "https://github.com/reqinme/Qinmo-Ultimate-Launcher/releases/download/v$Version/$exeName"
    sha256      = $sha
    sizeBytes   = $size
}
if ($Notes) { $entry.notes = $Notes }
if ($MinimumVersion) { $entry.minimumVersion = $MinimumVersion }

$releases = @($entry) + $releases

$manifest = [ordered]@{
    formatVersion = 1
    releases      = $releases
}

New-Item -ItemType Directory -Path (Split-Path -Parent $manifestPath) -Force | Out-Null
$manifest | ConvertTo-Json -Depth 6 | Set-Content -Path $manifestPath -Encoding UTF8
Write-Host "    清单已更新：$manifestPath（共 $($releases.Count) 个版本）"

# ---------- 3. 自检：让客户端自己的解析器读这份清单 ----------
Write-Step "自检：用一个更低版本的 exe 去读这份清单"

if (Test-Path $baselineDir) { Remove-Item $baselineDir -Recurse -Force }
Invoke-Dotnet @('publish', $project, '-c', 'Release', '-o', $baselineDir,
                ("-p:Version=$BaselineVersion"), '-v', 'q', '--nologo')

$baselineExe = Join-Path $baselineDir $exeName
if (-not (Test-Path $baselineExe)) { throw "基线版本发布失败" }

# 起一个只服务清单的极小 HTTP 服务（客户端只接受 http/https，file:// 会被拒）
$serveDir = Split-Path -Parent $manifestPath
$job = Start-Job -ScriptBlock {
    param($dir, $port)
    $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, $port)
    $listener.Start()
    try {
        while ($true) {
            if (-not $listener.Pending()) { Start-Sleep -Milliseconds 100; continue }
            $client = $listener.AcceptTcpClient()
            $stream = $client.GetStream()
            $buffer = New-Object byte[] 8192
            $read = $stream.Read($buffer, 0, 8192)
            $request = [Text.Encoding]::ASCII.GetString($buffer, 0, $read)
            $name = (($request -split "`r`n")[0] -split ' ')[1].TrimStart('/')
            $path = Join-Path $dir $name
            if (Test-Path $path -PathType Leaf) {
                $bytes = [IO.File]::ReadAllBytes($path)
                $header = [Text.Encoding]::ASCII.GetBytes(
                    "HTTP/1.1 200 OK`r`nContent-Type: application/json`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
                $stream.Write($header, 0, $header.Length)
                $stream.Write($bytes, 0, $bytes.Length)
            } else {
                $header = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 404 Not Found`r`nContent-Length: 0`r`nConnection: close`r`n`r`n")
                $stream.Write($header, 0, $header.Length)
            }
            $stream.Flush()
            $client.Close()
        }
    } finally { $listener.Stop() }
} -ArgumentList $serveDir, $Port

Start-Sleep -Seconds 2

$outFile = Join-Path ([IO.Path]::GetTempPath()) ("qul-release-check-{0}.txt" -f ([guid]::NewGuid().ToString('N')))
$errFile = $outFile + '.err'
$manifestUrl = "http://127.0.0.1:$Port/manifest.json"

try {
    $proc = Start-Process -FilePath $baselineExe `
        -ArgumentList @('update', '--manifest', $manifestUrl) `
        -PassThru -NoNewWindow -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    [void] $proc.WaitForExit(120000)

    $stdout = @(Get-Content $outFile -Encoding Default -ErrorAction SilentlyContinue)
    $text = $stdout -join "`n"

    if ($text -notmatch [regex]::Escape($Version)) {
        Write-Host "    客户端输出：" -ForegroundColor Yellow
        $stdout | Select-Object -First 20 | ForEach-Object { Write-Host "      $_" }
        throw "自检失败：客户端没有从清单里认出 $Version"
    }

    # 只把**清单本身**的问题算作失败。
    #
    # 这一段的目的是"清单能不能被客户端解析"，不是"附件能不能下载"——
    # 脚本里的迷你 HTTP 服务只够发清单（几百字节），发不动几百 KB 的 exe，
    # 所以认出新版本之后必然跟一个网络错误。把它当失败会让自检永远红着，
    # 而一个永远红的检查等于没有检查。
    if ($text -match 'QUL-UPD-\d{4}') {
        Write-Host "    客户端输出：" -ForegroundColor Yellow
        $stdout | Select-Object -First 20 | ForEach-Object { Write-Host "      $_" }
        throw "自检失败：客户端拒绝了解析这份清单（格式问题）"
    }

    Write-Host "    OK —— 客户端认出了 $Version（清单格式正确）" -ForegroundColor Green
    if ($text -match 'QUL-NET-\d{4}') {
        Write-Host "    （下载步骤报了网络错误，属预期：迷你 HTTP 服务发不动附件）" -ForegroundColor DarkGray
    }
} finally {
    Stop-Job $job -ErrorAction SilentlyContinue
    Remove-Job $job -Force -ErrorAction SilentlyContinue
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
    if (Test-Path $baselineDir) { Remove-Item $baselineDir -Recurse -Force }
}

# ---------- 4. 接下来要做什么 ----------
Write-Step '本地已就绪。发布到 GitHub Releases：'
Write-Host @"
    gh release create v$Version "$exePath" --title "v$Version" --notes "$Notes"
    gh release upload v$Version "$manifestPath" --clobber

  清单的固定地址（客户端要填的就是它）：
    https://github.com/reqinme/Qinmo-Ultimate-Launcher/releases/latest/download/manifest.json

  注意：清单里的 downloadUrl 必须与真实附件地址一致，
  否则客户端会下载 404 —— 自检只验证"清单能被解析、版本能被认出"，
  验证不了远端附件是否真的在那个地址上。
"@ -ForegroundColor Gray
