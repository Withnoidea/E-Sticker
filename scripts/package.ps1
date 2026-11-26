param(
    [string]$OutputDirectory = 'dist'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $root
try {
    if (-not (Test-Path -LiteralPath 'node_modules/vditor/dist/index.min.js')) {
        & npm ci --ignore-scripts --no-audit --no-fund
        if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    }
    $checks = @(
        @('fmt', '--all', '--', '--check'),
        @('test', '--locked'),
        @('clippy', '--all-targets', '--locked', '--', '-D', 'warnings'),
        @('build', '--release', '--locked')
    )
    & node --test scripts/markdown_test.cjs
    if ($LASTEXITCODE -ne 0) { throw 'Markdown list checks failed' }
    foreach ($commandArguments in $checks) {
        & cargo @commandArguments
        if ($LASTEXITCODE -ne 0) { throw "cargo $commandArguments failed" }
    }
    $manifest = Get-Content -LiteralPath 'Cargo.toml' -Raw
    $version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if (-not $version) { throw 'Package version not found' }
    $name = "E-Sticker-v$version-windows-x64"
    $outputRoot = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
        $OutputDirectory
    } else {
        Join-Path $root $OutputDirectory
    }
    $folder = Join-Path $outputRoot $name
    $archive = "$folder.zip"
    New-Item -ItemType Directory -Path $folder -Force | Out-Null
    Copy-Item -LiteralPath 'target/release/e-sticker.exe' -Destination (Join-Path $folder 'E-Sticker.exe') -Force
    foreach ($file in @('README.md', 'CHANGELOG.md', 'TESTING.md', 'CONTEXT.md', 'THIRD_PARTY_NOTICES.md')) {
        Copy-Item -LiteralPath $file -Destination (Join-Path $folder $file) -Force
    }
    Copy-Item -LiteralPath 'node_modules/vditor/LICENSE' -Destination (Join-Path $folder 'Vditor-LICENSE.txt') -Force
    Copy-Item -LiteralPath 'node_modules/diff-match-patch/LICENSE' -Destination (Join-Path $folder 'diff-match-patch-LICENSE.txt') -Force
    Copy-Item -LiteralPath 'docs' -Destination $folder -Recurse -Force
    Get-ChildItem -LiteralPath $folder -File -Recurse | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object FullName | ForEach-Object {
        $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $relative = $_.FullName.Substring($folder.Length + 1).Replace('\', '/')
        "$hash *$relative"
    } | Set-Content -LiteralPath (Join-Path $folder 'SHA256SUMS.txt') -Encoding ASCII
    Compress-Archive -LiteralPath $folder -DestinationPath $archive -Force
    $zipHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$zipHash *$name.zip" | Set-Content -LiteralPath "$archive.sha256" -Encoding ASCII
    Get-Item -LiteralPath $archive | Select-Object FullName, Length
}
finally {
    Pop-Location
}
