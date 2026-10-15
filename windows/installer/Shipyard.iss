#ifndef AppName
  #define AppName "Shipyard"
#endif
#ifndef AppId
  #define AppId "Shipyard.Shipyard"
#endif
#ifndef ExeName
  #define ExeName "Shipyard"
#endif
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Publisher
  #define Publisher "Vaache"
#endif
#ifndef UrlScheme
  #define UrlScheme "shipyard"
#endif
#ifndef RepoUrl
  #define RepoUrl "https://github.com/Vaache/shipyard"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\x64\app"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif
#ifndef OutputBase
  #define OutputBase AppName + "-windows-" + Arch + "-setup"
#endif

[Setup]
AppId={#AppId}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#RepoUrl}
AppSupportURL={#RepoUrl}/issues
AppUpdatesURL={#RepoUrl}/releases
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBase}
SetupIconFile=..\src\Shipyard\Assets\AppIcon.ico
UninstallDisplayIcon={app}\{#ExeName}.exe
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoCompany={#Publisher}
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[InstallDelete]
Type: filesandordirs; Name: "{app}\Web"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#ExeName}.exe"; AppUserModelID: "{#AppId}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#ExeName}.exe"; Tasks: desktopicon; AppUserModelID: "{#AppId}"

[Registry]
Root: HKA; Subkey: "Software\Classes\{#UrlScheme}"; ValueType: string; ValueName: ""; ValueData: "URL:{#AppName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#UrlScheme}"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\{#UrlScheme}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}.exe"",0"
Root: HKA; Subkey: "Software\Classes\{#UrlScheme}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}.exe"" ""%1"""

[Run]
Filename: "{app}\{#ExeName}.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#ExeName}.exe"; Flags: nowait; Check: ShouldRelaunch

[UninstallRun]
Filename: "{cmd}"; Parameters: "/c taskkill /im ""{#ExeName}.exe"" /f"; Flags: runhidden; RunOnceId: "StopApp"

[Code]
function ShouldRelaunch(): Boolean;
begin
  Result := WizardSilent() and (ExpandConstant('{param:RELAUNCH|0}') = '1');
end;
