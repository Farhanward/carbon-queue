param(
  [string]$PrivateKeyPath = "$env:USERPROFILE\.carbonflow-signing\carbonflow-dev.key",
  [string]$PrivateKeyPassword = "carbonflow-dev"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

if (-not (Test-Path -LiteralPath $PrivateKeyPath)) {
  throw "Signing private key not found at $PrivateKeyPath. Run: npx tauri signer generate --ci --password '<password>' --write-keys '<path>'"
}

$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -LiteralPath $PrivateKeyPath -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $PrivateKeyPassword

Push-Location $root
try {
  npm run bundle:windows
} finally {
  Pop-Location
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
