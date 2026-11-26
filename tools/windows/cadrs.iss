; The Windows installer (issue #200): cadrs-v<version>-x86_64-windows-setup.exe, made by Inno Setup
; from the release's Windows zip (.github/workflows/release.yml, job windows-installer):
;
;   iscc /DAppVersion=<version> /DSourceDir=<the zip's folder> /DOutputDir=<dist> tools\windows\cadrs.iss
;
; - For the current user, without admin rights: into %LOCALAPPDATA%\Programs\cadrs, where
;   `cadrs --install` puts it too. The built-in updater swaps cadrs.exe and libraries\ in place
;   from the zip (crates/cadrs_app/src/update.rs), so the install folder has to be the user's.
; - A Start Menu entry (cadrs.lnk, the one `cadrs --install` makes), a desktop shortcut if asked
;   for, and an uninstaller in Settings > Apps.
; - .cadrs files open in cadrs; cadrs is offered under Open with for the models it imports (STEP,
;   IGES, BREP, STL, GLB) without taking them over.
; - Uninstalling removes the install folder, the updater's leftovers in it included. The documents
;   are in %APPDATA%\cadrs and stay.

#ifndef AppVersion
  #error Pass the version: /DAppVersion=0.2.9
#endif
#ifndef SourceDir
  #error Pass the unpacked zip's folder: /DSourceDir=...
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
; Never change: Windows knows the install by it (upgrades, the uninstaller).
AppId={{73591305-32CD-40D3-AFFE-2EDF48E2EADB}
AppName=cadrs
AppVersion={#AppVersion}
AppVerName=cadrs {#AppVersion}
AppPublisher=cadrs
AppPublisherURL=https://cadrs.net
AppSupportURL=https://github.com/rvdende/cadrs/issues
AppUpdatesURL=https://github.com/rvdende/cadrs/releases
DefaultDirName={localappdata}\Programs\cadrs
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
CloseApplications=yes
SetupIconFile=..\..\assets\icons\cadrs.ico
UninstallDisplayIcon={app}\cadrs.exe
UninstallDisplayName=cadrs
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=cadrs-v{#AppVersion}-x86_64-windows-setup

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\cadrs"; Filename: "{app}\cadrs.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\cadrs"; Filename: "{app}\cadrs.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
; .cadrs: cadrs's own documents.
Root: HKA; Subkey: "Software\Classes\.cadrs"; ValueType: string; ValueName: ""; ValueData: "cadrs.Document"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\cadrs.Document"; ValueType: string; ValueName: ""; ValueData: "cadrs document"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\cadrs.Document\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\cadrs.exe"",0"
Root: HKA; Subkey: "Software\Classes\cadrs.Document\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\cadrs.exe"" ""%1"""
; The models cadrs imports: under Open with, not their default program.
Root: HKA; Subkey: "Software\Classes\cadrs.Model"; ValueType: string; ValueName: ""; ValueData: "Model to import into cadrs"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\cadrs.Model\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\cadrs.exe"",0"
Root: HKA; Subkey: "Software\Classes\cadrs.Model\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\cadrs.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\cadrs.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "cadrs"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\cadrs.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\cadrs.exe"" ""%1"""
#define Model(Ext) \
  'Root: HKA; Subkey: "Software\Classes\.' + Ext + '\OpenWithProgids"; ValueType: string; ValueName: "cadrs.Model"; ValueData: ""; Flags: uninsdeletevalue' + NewLine + \
  'Root: HKA; Subkey: "Software\Classes\Applications\cadrs.exe\SupportedTypes"; ValueType: string; ValueName: ".' + Ext + '"; ValueData: ""'
#emit Model("step")
#emit Model("stp")
#emit Model("p21")
#emit Model("iges")
#emit Model("igs")
#emit Model("brep")
#emit Model("brp")
#emit Model("stl")
#emit Model("glb")
Root: HKA; Subkey: "Software\Classes\.cadrs\OpenWithProgids"; ValueType: string; ValueName: "cadrs.Document"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\cadrs.exe\SupportedTypes"; ValueType: string; ValueName: ".cadrs"; ValueData: ""

[Run]
Filename: "{app}\cadrs.exe"; Description: "{cm:LaunchProgram,cadrs}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What the updater added or left beside what was installed (crates/cadrs_app/src/update.rs).
Type: filesandordirs; Name: "{app}"
