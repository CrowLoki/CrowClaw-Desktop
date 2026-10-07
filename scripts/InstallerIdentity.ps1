function Get-CrowClawNormalizedNsisHash {
    param([Parameter(Mandatory)][byte[]]$Content)
    # Tauri changes this marker while bundling, then restores the build-tree
    # executable. Compare in memory only; never patch installed/signed files.
    # https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs
    $qaEncoding = [Text.Encoding]::Latin1
    $qaText = $qaEncoding.GetString($Content)
    $qaPackagedMarker = '__TAURI_BUNDLE_TYPE_VAR_NSS'
    $qaOriginalMarker = '__TAURI_BUNDLE_TYPE_VAR_UNK'
    $qaOffset = $qaText.IndexOf($qaPackagedMarker, [StringComparison]::Ordinal)
    if ($qaOffset -lt 0 -or $qaOffset -ne $qaText.LastIndexOf($qaPackagedMarker, [StringComparison]::Ordinal) -or
        $qaText.Contains($qaOriginalMarker)) {
        throw 'Installed executable does not contain exactly one unambiguous NSIS bundle marker.'
    }
    $qaNormalized = $qaEncoding.GetBytes($qaText.Replace($qaPackagedMarker, $qaOriginalMarker))
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($qaNormalized))
}
