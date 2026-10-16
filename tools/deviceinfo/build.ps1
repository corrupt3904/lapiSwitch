param([Parameter(Mandatory=$true)][string]$NdkRoot)
$ErrorActionPreference='Stop'
$taskCompiler=Join-Path $NdkRoot 'toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android30-clang.cmd'
if(!(Test-Path -LiteralPath $taskCompiler -PathType Leaf)){throw 'NDK r27d ARM64 compiler not found.'}
$taskRepo=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$taskOutput=Join-Path $taskRepo 'assets\lapiswitch_deviceinfo'
$taskTemporary=[IO.Path]::GetTempFileName()
try {
    & $taskCompiler -O2 -Wall -Wextra -Werror -Wformat=2 -fstack-protector-strong (Join-Path $PSScriptRoot 'lapiswitch_deviceinfo.c') -ldl -o $taskTemporary
    if($LASTEXITCODE -ne 0){throw "Helper compiler failed: $LASTEXITCODE"}
    Copy-Item -LiteralPath $taskTemporary -Destination $taskOutput -Force
    Get-FileHash -LiteralPath $taskOutput -Algorithm SHA256
} finally {
    Remove-Item -LiteralPath $taskTemporary -Force -ErrorAction SilentlyContinue
}
