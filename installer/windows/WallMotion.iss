; WallMotion Windows installer (Inno Setup 6).
;
; Per-user install, no admin needed:
;   - install folder picker (default %LOCALAPPDATA%\WallMotion)
;   - optional one-click media tools (yt-dlp + ffmpeg, downloaded from
;     the official upstreams into {app}\tools, where the app finds them;
;     same URLs as the in-app auto-install, so both paths stay in sync)
;   - already-present tools are NOT re-downloaded: {app}\tools first,
;     then user/system PATH (the app finds both, so a second copy
;     would only waste bandwidth)
;   - optional user-PATH entry, desktop icon, autostart (same Run value
;     the in-app toggle manages: "WallMotion" -> exe --minimized)
;
; Build locally (needs Inno Setup 6):
;   iscc installer\windows\WallMotion.iss /DAppVersion=1.1.4
; In CI the tag pipeline passes /DAppVersion automatically and expects
;   wallmotion-rs\target\release\wallmotion-settings.exe to exist
; (see .github/workflows/release.yml, build-installer job).
;
; Code signing (SmartScreen): unsigned builds work but SmartScreen warns
; until the binary gains reputation. Free OSS signing via SignPath, see
; docs/SIGNING.md - then sign the exe AND the setup produced here.

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif

#define AppName "WallMotion"
#define AppExe "WallMotion-Settings.exe"
#define YtdlpUrl "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
#define FfmpegUrl "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"

[Setup]
AppId={{B4A7C2E1-8F3D-4A5B-9C6D-1E2F3A4B5C6D}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Jurek1357
AppPublisherURL=https://github.com/Jurek1357/WallMotion-PC
AppSupportURL=https://github.com/Jurek1357/WallMotion-PC/issues
AppUpdatesURL=https://github.com/Jurek1357/WallMotion-PC/releases
DefaultDirName={localappdata}\{#AppName}
PrivilegesRequired=lowest
OutputDir={#SourcePath}\..\..\dist-installer
OutputBaseFilename={#AppName}-Setup-{#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
MinVersion=10.0
CloseApplications=yes
RestartApplications=no
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
VersionInfoVersion={#AppVersion}
DisableProgramGroupPage=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Components]
Name: "app"; Description: "WallMotion app (required)"; Types: full compact custom; Flags: fixed
Name: "tools"; Description: "Media tools (yt-dlp + ffmpeg)"; Types: full
Name: "tools\ytdlp"; Description: "yt-dlp (YouTube downloads, ~15 MB download)"; Types: full
Name: "tools\ffmpeg"; Description: "ffmpeg (1080p merges + thumbnails, ~80 MB download)"; Types: full

[Tasks]
Name: "desktopicon"; Description: "Desktop shortcut"; Flags: unchecked
Name: "autostart"; Description: "Start with Windows (hidden, resumes wallpaper)"; Flags: unchecked
Name: "pathtools"; Description: "Add tools folder to user PATH"; Flags: unchecked

[Files]
Source: "..\..\wallmotion-rs\target\release\wallmotion-settings.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion; Components: app
; In-process playback library (load-time linked: the exe won't start without it).
Source: "..\..\wallmotion-rs\target\release\libmpv-2.dll"; DestDir: "{app}"; Flags: ignoreversion; Components: app

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Same value the in-app autostart toggle manages, so both stay in sync.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "WallMotion"; ValueType: string; ValueData: """{app}\{#AppExe}"" --minimized"; Tasks: autostart; Flags: uninsdeletevalue
; Right-click -> "Set as wallpaper" for media files (double-click keeps
; the user's own defaults in Photos/Media Player). The app forwards
; --set to the running instance, so no second window pops up.
; uninsdeletekey on the verb removes the whole subtree on uninstall.
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.mp4\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.mp4\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.mkv\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.mkv\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.webm\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.webm\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.avi\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.avi\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.mov\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.mov\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.gif\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.gif\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.jpg\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.jpg\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.jpeg\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.jpeg\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.png\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.png\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.bmp\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.bmp\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.webp\shell\WallMotion"; ValueType: string; ValueData: "Set as wallpaper (WallMotion)"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.webp\shell\WallMotion\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" --set ""%1"""

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Only our own leftovers; downloads/ holds the user's videos and stays.
Type: files; Name: "{app}\{#AppExe}.old.exe"
Type: files; Name: "{app}\{#AppExe}.update"
Type: files; Name: "{app}\libmpv-2.dll.download"

[Code]
{ Download + install the checked media tools with PowerShell/tar.exe
  (both ship Windows 10+): no Inno plugins needed. Already-present
  tools are skipped (app tools dir, then PATH) so reinstalls and
  system-wide copies never download twice. Failures only warn —
  the app auto-installs missing tools on first launch anyway. }
procedure FetchTools();
var
  ResultCode: Integer;
  Ps, ToolsDir, WantYt, WantFf: String;
begin
  if not (WizardIsComponentSelected('tools\ytdlp') or WizardIsComponentSelected('tools\ffmpeg')) then
    Exit;
  ToolsDir := ExpandConstant('{app}\tools');
  ForceDirectories(ToolsDir);
  if WizardIsComponentSelected('tools\ytdlp') then
    WantYt := '$toolsYt=1;' else WantYt := '';
  if WizardIsComponentSelected('tools\ffmpeg') then
    WantFf := '$toolsFf=1;' else WantFf := '';
  Ps :=
    '$ErrorActionPreference=''Stop'';'
    + '$ProgressPreference=''SilentlyContinue'';'
    + '$tools=''' + ToolsDir + ''';'
    + '$ytUrl=''{#YtdlpUrl}'';'
    + '$ffUrl=''{#FfmpegUrl}'';'
    + WantYt + WantFf
    + 'function Have-Cmd($n) {'
    + '  foreach ($x in $n) {'
    + '    $c=Get-Command $x -ErrorAction SilentlyContinue | Select-Object -First 1;'
    + '    if ($c) { return $true }'
    + '  }'
    + '  return $false'
    + '};'
    + 'New-Item -ItemType Directory -Force -Path $tools | Out-Null;'
    + 'if ($toolsYt) {'
    + '  $dest=Join-Path $tools ''yt-dlp.exe'';'
    + '  $have=(Test-Path -LiteralPath $dest) -or (Have-Cmd @(''yt-dlp.exe'',''yt-dlp''));'
    + '  if (-not $have) { Invoke-WebRequest -Uri $ytUrl -OutFile $dest; }'
    + '}'
    + 'if ($toolsFf) {'
    + '  $dest=Join-Path $tools ''ffmpeg.exe'';'
    + '  $have=(Test-Path -LiteralPath $dest) -or (Have-Cmd @(''ffmpeg.exe'',''ffmpeg''));'
    + '  if (-not $have) {'
    + '  $zip=Join-Path $env:TEMP ''wallmotion-ffmpeg.zip'';'
    + '  $tmp=Join-Path $env:TEMP ''wallmotion-ffmpeg-x'';'
    + '  Invoke-WebRequest -Uri $ffUrl -OutFile $zip;'
    + '  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue;'
    + '  New-Item -ItemType Directory -Force -Path $tmp | Out-Null;'
    + '  & tar.exe -xf $zip -C $tmp;'
    + '  if ($LASTEXITCODE -ne 0) { throw ''unpacking ffmpeg failed''; }'
    + '  $ff=Get-ChildItem -Path $tmp -Recurse -Filter ''ffmpeg.exe'' | Select-Object -First 1;'
    + '  if (-not $ff) { throw ''ffmpeg.exe not found in archive''; }'
    + '  Move-Item -LiteralPath $ff.FullName -Destination $dest -Force;'
    + '  Remove-Item -Recurse -Force $tmp, $zip -ErrorAction SilentlyContinue;'
    + '  }'
    + '}';
  WizardForm.StatusLabel.Caption := 'Downloading media tools (yt-dlp, ffmpeg)...';
  if not Exec('powershell.exe', '-NoProfile -ExecutionPolicy Bypass -Command ' + Ps,
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode) or (ResultCode <> 0) then
    MsgBox('Media tools could not be downloaded (network?). WallMotion will offer them on first launch instead.',
      mbInformation, MB_OK);
end;

{ Append the tools dir to the user PATH once (pathtools task). Takes
  effect for newly started programs; no admin needed (HKCU). }
procedure AddToolsToPath();
var
  Path, ToolsDir: String;
begin
  ToolsDir := ExpandConstant('{app}\tools');
  if RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
  begin
    if Pos(';' + Uppercase(ToolsDir) + ';', ';' + Uppercase(Path) + ';') > 0 then
      Exit;
    Path := Path + ';' + ToolsDir;
  end
  else
    Path := ToolsDir;
  RegWriteStringValue(HKCU, 'Environment', 'Path', Path);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    FetchTools();
    if WizardIsTaskSelected('pathtools') then
      AddToolsToPath();
  end;
end;
