param(
    [string]$JavaHome = $env:JAVA_HOME,
    [string]$DefaultUrl = '',
    [string]$FcmApiKey = '',
    [string]$FcmAppId = '',
    [string]$FcmSenderId = '',
    [string]$FcmProjectId = '',
    [ValidateSet('debug', 'release')][string]$Mode = 'debug',
    [string]$AndroidHome = $env:ANDROID_HOME,
    [string]$FlutterRoot = $env:FLUTTER_ROOT
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (!$JavaHome) { $JavaHome = Join-Path $root '.tools/jdk-17' }
if (!$AndroidHome) { $AndroidHome = Join-Path $root '.tools/android-sdk' }
if (!$FlutterRoot) { $FlutterRoot = Join-Path $root '.tools/flutter' }
if (!$JavaHome -or !(Test-Path "$JavaHome/bin/java.exe")) {
    throw 'Pass -JavaHome with a JDK 17 or newer directory.'
}
if ($DefaultUrl) {
    $uri = [uri]$DefaultUrl
    if (!$uri.IsAbsoluteUri -or $uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -ne '/') {
        throw 'DefaultUrl must be an HTTPS origin without credentials, query or path.'
    }
}
$fcmValues = @($FcmApiKey, $FcmAppId, $FcmSenderId, $FcmProjectId)
if ($fcmValues.Where({ $_ }).Count -notin @(0, 4)) {
    throw 'Provide all four Firebase client values, or leave all four empty.'
}
$env:JAVA_HOME = $JavaHome
$env:ANDROID_HOME = $AndroidHome
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
$env:GRADLE_USER_HOME = Join-Path $root '.tools/gradle'
$env:CI = 'true'
$env:FLUTTER_SUPPRESS_ANALYTICS = 'true'
$env:DASH__SUPPRESS_ANALYTICS = 'true'
$env:FLUTTER_STORAGE_BASE_URL = 'https://storage.flutter-io.cn'
$env:PUB_HOSTED_URL = 'https://pub.flutter-io.cn'
$flutter = Join-Path $FlutterRoot 'bin/flutter.bat'
$dartDefines = @("--dart-define=GPU_POCKET_DEFAULT_URL=$DefaultUrl")
if ($fcmValues.Where({ $_ }).Count -eq 4) {
    $dartDefines += "--dart-define=GPU_POCKET_FCM_API_KEY=$FcmApiKey"
    $dartDefines += "--dart-define=GPU_POCKET_FCM_APP_ID=$FcmAppId"
    $dartDefines += "--dart-define=GPU_POCKET_FCM_SENDER_ID=$FcmSenderId"
    $dartDefines += "--dart-define=GPU_POCKET_FCM_PROJECT_ID=$FcmProjectId"
}
Push-Location (Join-Path $root 'mobile')
try {
    & $flutter build apk "--$Mode" --target-platform android-arm64 @dartDefines
    if ($LASTEXITCODE -ne 0) { throw "Android build failed: $LASTEXITCODE" }
    Get-Item "build/app/outputs/flutter-apk/app-$Mode.apk"
} finally {
    Pop-Location
}
