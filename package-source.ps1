param([string]$Destination)
$ErrorActionPreference = 'Stop'
if (!$Destination) {
  $version = (Get-Content (Join-Path $PSScriptRoot 'package.json') -Raw | ConvertFrom-Json).version
  $Destination = Join-Path $PSScriptRoot "../output/NFTAI-Toolbox-$version-source.zip"
}
$root = [IO.Path]::GetFullPath($PSScriptRoot)
$target = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $target) { throw "Archive already exists: $target" }
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$files = & git -c core.quotepath=false -C $root ls-files --cached --others --exclude-standard
if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate source files.' }
$archive = [IO.Compression.ZipFile]::Open($target, [IO.Compression.ZipArchiveMode]::Create)
$count = 0
try {
  foreach ($relative in ($files | Sort-Object -Unique)) {
    if ($relative -match '(^|/)(node_modules|target|dist|\.git)(/|$)' -or $relative -match '(^|/)\.env$') { continue }
    $absolute = [IO.Path]::GetFullPath((Join-Path $root $relative))
    if (!$absolute.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid source path.' }
    if (!(Test-Path -LiteralPath $absolute -PathType Leaf)) { continue }
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $absolute, $relative.Replace('\','/'), [IO.Compression.CompressionLevel]::Optimal) | Out-Null
    $count++
  }
} finally { $archive.Dispose() }
Write-Output "Archived $count source files: $target"
Get-FileHash -LiteralPath $target -Algorithm SHA256
