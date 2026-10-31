; EmuHub's Windows installer (Inno Setup 6). CI passes the version, built exe and output folder.
#define AppName "EmuHub"
#define AppExeName "emuhub.exe"
#define AppUrl "https://github.com/veneito/EmuHub"
#define AppVersion GetEnv("EMUHUB_VERSION")
#define SourceExe GetEnv("EMUHUB_EXE")
#define OutputDir GetEnv("EMUHUB_DIST")

[Setup]
AppId={{99A86B27-D3DC-4094-A6ED-7143CD78B431}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=veneito
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
; Per-user install (%LOCALAPPDATA%\Programs\EmuHub): no admin prompt, and EmuHub can update itself.
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DisableDirPage=no
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\{#AppExeName}
OutputDir={#OutputDir}
OutputBaseFilename=EmuHub-Setup
SetupIconFile=..\..\assets\emuhub.ico
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
; An update runs this silently while EmuHub is quitting; close it if it's still open.
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExeName}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch EmuHub"; Flags: postinstall nowait skipifsilent
; After an in-app update (always silent) start the new version again.
Filename: "{app}\{#AppExeName}"; Flags: nowait; Check: WizardSilent
