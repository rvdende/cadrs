; The Windows installer (#200): cadrs-v<version>-x86_64-windows-setup.exe, built by
; `tools/release/package.sh windows` with NSIS's makensis (cross-built on Linux like the exe):
;
;   makensis -DVERSION=0.2.9 -DVIVERSION=0.2.9.0 -DSRC=<the packaged folder> -DOUT=<setup.exe>
;            tools/windows/cadrs.nsi
;
; The release signs cadrs.exe before it goes in, and then the installer (tools/windows/README.md).
; The uninstaller it writes stays unsigned: it is made on the user's machine, so it carries no
; download mark and SmartScreen never checks it.
;
; It installs for the current user, with no administrator prompt, where `cadrs --install` does
; (%LOCALAPPDATA%\Programs\cadrs): the in-app updater (cadrs_app::update) swaps a new release
; into the install folder in place, so that folder has to be the user's to write. It adds a
; Start Menu shortcut (a desktop one if ticked), the entry in Settings › Apps (with its
; uninstaller) and "Open with" for the files cadrs opens, without taking any type's default.
; `/S` installs silently, `/D=<dir>` (last) picks the folder.

; 64-bit, like cadrs.exe (so it also runs where 32-bit programs can't).
Target amd64-unicode
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!macro Need name
  !ifndef ${name}
    !error "pass -D${name}= (see the top of this file)"
  !endif
!macroend
!insertmacro Need VERSION
!insertmacro Need SRC
!insertmacro Need OUT
!ifndef VIVERSION
  !define VIVERSION "0.0.0.0"
!endif

!define APP "cadrs"
!define EXE "cadrs.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\cadrs"
!define PROGID "cadrs.File"
!define ICON "${__FILEDIR__}\..\..\assets\icons\cadrs.ico"

Name "${APP}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
; A reinstall goes where the last one went.
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} ${VERSION} setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"

!include "MUI2.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Registers `.ext` for "Open with" cadrs, leaving its default app alone.
!macro OpenWith ext
  WriteRegStr HKCU "Software\Classes\.${ext}\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKCU "Software\Classes\Applications\${EXE}\SupportedTypes" ".${ext}" ""
!macroend
!macro UnOpenWith ext
  DeleteRegValue HKCU "Software\Classes\.${ext}\OpenWithProgids" "${PROGID}"
  DeleteRegKey /ifempty HKCU "Software\Classes\.${ext}\OpenWithProgids"
  DeleteRegKey /ifempty HKCU "Software\Classes\.${ext}"
!macroend

Section "${APP}" SecApp
  SectionIn RO
  SetOutPath "$INSTDIR"
  ; A running cadrs keeps its executable locked, but it can be renamed, as the updater does; the
  ; next start removes it (cadrs_app::update::remove_old_executable).
  Delete "$INSTDIR\cadrs.old"
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    Rename "$INSTDIR\${EXE}" "$INSTDIR\cadrs.old"
  ${EndIf}
  ; The built-in libraries as a whole, so none of an older release's stay behind.
  RMDir /r "$INSTDIR\libraries"
  File "${SRC}\${EXE}"
  File "${SRC}\README.md"
  File "${SRC}\LICENSE-APACHE"
  File "${SRC}\LICENSE-MIT"
  File /r "${SRC}\libraries"
  Delete "$INSTDIR\cadrs.old"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; The Start Menu shortcut where `cadrs --install` makes it.
  CreateShortCut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0

  ; Settings › Apps.
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "cadrs"
  WriteRegStr HKCU "${UNINST_KEY}" "URLInfoAbout" "https://cadrs.net"
  WriteRegStr HKCU "${UNINST_KEY}" "HelpLink" "https://github.com/rvdende/cadrs"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  ; "Open with", for the types of tools/linux/cadrs.desktop: `cadrs --open <file>` imports a model, or opens a native document.
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "${APP} file"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${EXE}" --open "%1"'
  WriteRegStr HKCU "Software\Classes\Applications\${EXE}\shell\open\command" "" '"$INSTDIR\${EXE}" --open "%1"'
  WriteRegStr HKCU "Software\Classes\Applications\${EXE}" "FriendlyAppName" "${APP}"
  !insertmacro OpenWith cadrs
  !insertmacro OpenWith step
  !insertmacro OpenWith stp
  !insertmacro OpenWith p21
  !insertmacro OpenWith iges
  !insertmacro OpenWith igs
  !insertmacro OpenWith brep
  !insertmacro OpenWith brp
  !insertmacro OpenWith stl
  !insertmacro OpenWith glb
  ; Tells Explorer the associations changed (SHCNE_ASSOCCHANGED).
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

Section /o "Desktop shortcut" SecDesktop
  CreateShortCut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "The app, its built-in part libraries and a Start Menu shortcut."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "A shortcut to ${APP} on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; Removes only what the installer and the updater put there (never the folder's other files, in
; case the folder chosen was not one of its own). Documents live in the user's data folder and
; stay.
Section "Uninstall"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\cadrs.old"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\LICENSE-MIT"
  RMDir /r "$INSTDIR\libraries"
  RMDir /r "$INSTDIR\libraries.old"
  RMDir /r "$INSTDIR\update.new"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"

  !insertmacro UnOpenWith cadrs
  !insertmacro UnOpenWith step
  !insertmacro UnOpenWith stp
  !insertmacro UnOpenWith p21
  !insertmacro UnOpenWith iges
  !insertmacro UnOpenWith igs
  !insertmacro UnOpenWith brep
  !insertmacro UnOpenWith brp
  !insertmacro UnOpenWith stl
  !insertmacro UnOpenWith glb
  DeleteRegKey HKCU "Software\Classes\${PROGID}"
  DeleteRegKey HKCU "Software\Classes\Applications\${EXE}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
