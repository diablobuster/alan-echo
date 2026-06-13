# Generates SHA256SUMS.txt for all installers in the bundle output and uploads
# it to the given release tag. Usage: .\scripts\release-checksums.ps1 v1.2.2
param([Parameter(Mandatory)][string]$Tag)
$bundles = Get-ChildItem "src-tauri\target\release\bundle" -Recurse -Include *.exe,*.dmg
$lines = $bundles | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_ -Algorithm SHA256).Hash.ToLower(), $_.Name }
Set-Content -Path SHA256SUMS.txt -Value $lines -Encoding ascii
gh release upload $Tag SHA256SUMS.txt --repo diablobuster/alan-echo-releases --clobber
Write-Host "Uploaded SHA256SUMS.txt for $Tag"
