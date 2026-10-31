$ErrorActionPreference='Stop'
$project=$PSScriptRoot
$runtime=Join-Path $project 'src-tauri\runtime'
New-Item -ItemType Directory -Path (Join-Path $runtime 'node_modules') -Force | Out-Null
Copy-Item -LiteralPath (Get-Command node.exe).Source -Destination (Join-Path $runtime 'node.exe') -Force
foreach($package in @('playwright','playwright-core')){
    Copy-Item -LiteralPath (Join-Path $project "node_modules\$package") -Destination (Join-Path $runtime 'node_modules') -Recurse -Force
}
Copy-Item -LiteralPath (Join-Path $project 'frontend\firefox-job-collectors.cjs') -Destination $runtime -Force
& (Join-Path $runtime 'node.exe') --version
if($LASTEXITCODE -ne 0){throw 'Bundled Node verification failed'}
& (Join-Path $runtime 'node.exe') (Join-Path $runtime 'firefox-job-collectors.cjs') --self-test
if($LASTEXITCODE -ne 0){throw 'Bundled collector verification failed'}

$dist=Join-Path $project 'dist'
New-Item -ItemType Directory -Path $dist -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $project 'frontend') -File | Where-Object { $_.Extension -in @('.html','.css','.js') -and $_.Name -notmatch '\.before-|\.bak$' } | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $dist -Force }

Copy-Item -LiteralPath (Join-Path $project 'licenses\NODE-LICENSE.txt') -Destination $runtime -Force
