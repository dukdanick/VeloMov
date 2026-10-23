; Inno Setup script for the VeloMov installer. Built by packaging/windows/build-cross.sh:
;   ISCC /DAppVersion=0.9.0 /DSourceDir=<staged folder> /DOutputDir=<dist> /DRepoDir=<repo> velomov.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6B0F3B52-8E7A-4C33-9D7E-5A1C0E0F2B11}
AppName=VeloMov
AppVersion={#AppVersion}
AppVerName=VeloMov {#AppVersion}
AppPublisher=VeloMov
AppPublisherURL=https://github.com/velomov/velomov
DefaultDirName={autopf}\VeloMov
DefaultGroupName=VeloMov
DisableProgramGroupPage=yes
LicenseFile={#RepoDir}\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=VeloMov-{#AppVersion}-windows-x64-setup
SetupIconFile={#RepoDir}\packaging\windows\velomov.ico
UninstallDisplayIcon={app}\velomov.exe
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ChangesAssociations=yes
WizardStyle=modern
MinVersion=10.0

[Languages]
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "assoc"; Description: "{cm:AssocFileExtension,VeloMov,.vmov}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\VeloMov"; Filename: "{app}\velomov.exe"
Name: "{group}\{cm:UninstallProgram,VeloMov}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\VeloMov"; Filename: "{app}\velomov.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.vmov"; ValueType: string; ValueName: ""; ValueData: "VeloMov.Project"; Flags: uninsdeletevalue; Tasks: assoc
; projects from 0.8.0 and older
Root: HKA; Subkey: "Software\Classes\.vcut"; ValueType: string; ValueName: ""; ValueData: "VeloMov.Project"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\VeloMov.Project"; ValueType: string; ValueName: ""; ValueData: "VeloMov project"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\VeloMov.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\velomov.exe,0"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\VeloMov.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\velomov.exe"" ""%1"""; Tasks: assoc

[Run]
Filename: "{app}\velomov.exe"; Description: "{cm:LaunchProgram,VeloMov}"; Flags: nowait postinstall skipifsilent
