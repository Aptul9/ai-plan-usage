; Per-user installer for the Flutter build. Built by scripts/build-flutter.ps1
; and by CI, which pass AppVersion from pubspec.yaml.

#ifndef AppVersion
  #error AppVersion is required: ISCC /DAppVersion=x.y.z ai-plan-usage.iss
#endif
#ifndef ReleaseDir
  #define ReleaseDir "..\build\windows\x64\runner\Release"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\..\artifacts"
#endif

#define AppExe "ai-plan-usage-flutter.exe"
#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"
#define RunValue "ai-plan-usage-flutter"

[Setup]
AppId={{A3846194-215C-4957-9B31-D7FC6BB42598}
AppName=ai-plan-usage
AppVersion={#AppVersion}
AppVerName=ai-plan-usage {#AppVersion}
AppPublisher=Aptul9
AppPublisherURL=https://github.com/Aptul9/ai-plan-usage
AppSupportURL=https://github.com/Aptul9/ai-plan-usage/issues
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DefaultDirName={autopf}\ai-plan-usage
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir={#OutputDir}
OutputBaseFilename=ai-plan-usage-{#AppVersion}-windows-x64-setup
SetupIconFile=..\windows\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=ai-plan-usage
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=no

[Files]
Source: "{#ReleaseDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
Type: filesandordirs; Name: "{app}\data"

[Icons]
Name: "{autoprograms}\ai-plan-usage"; Filename: "{app}\{#AppExe}"

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch ai-plan-usage"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "StopApp"
Filename: "{sys}\reg.exe"; Parameters: "delete ""HKCU\{#RunKey}"" /v {#RunValue} /f"; Flags: runhidden; RunOnceId: "DropAutostart"

[Code]
// The tray app has no window the Restart Manager can close, so stop it
// before files are replaced.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
  Result := '';
end;
