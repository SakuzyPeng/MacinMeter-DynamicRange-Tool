; The MacinMeter DR GUI installer for Windows x64.
;
; It installs where the 0.3.x Tauri installer did, under the same product name,
; uninstall key and executable name, so running it over a Tauri install
; replaces that install in place: the Start menu entry, Apps & features and an
; existing desktop shortcut all go on pointing at the same path. Like Tauri's
; default it installs for the current user only and asks for no elevation.
;
; scripts/stage-release.py compiles it once per font build with:
;   VERSION      the crate version, e.g. 0.3.1
;   PAYLOAD      the release macinmeter-desktop.exe
;   LICENSES     a directory whose files are installed under licenses\
;   ICON         the application .ico
;   FONT_BUILD   bundled-fonts or system-fonts
;   COPYRIGHT    the copyright line of LICENSE
;   OUTFILE      the installer to write
; It uses only plug-ins and headers that ship with NSIS 3.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!ifndef VERSION | PAYLOAD | LICENSES | ICON | FONT_BUILD | COPYRIGHT | OUTFILE
  !error "define VERSION, PAYLOAD, LICENSES, ICON, FONT_BUILD, COPYRIGHT and OUTFILE with -D"
!endif

; The Tauri GUI's names. Tauri derived MANUFACTURER from the second part of
; the bundle identifier com.macinmeter.dr.gui.
!define PRODUCTNAME "MacinMeter DR GUI"
!define MANUFACTURER "macinmeter"
!define MAINBINARYNAME "macinmeter-gui.exe"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUPRODUCTKEY "Software\${MANUFACTURER}\${PRODUCTNAME}"

Name "${PRODUCTNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PRODUCTNAME}"
; Where Tauri's installer recorded the directory the user chose.
InstallDirRegKey HKCU "${MANUPRODUCTKEY}" ""
BrandingText "${PRODUCTNAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=0 "ProductName" "${PRODUCTNAME}"
VIAddVersionKey /LANG=0 "FileDescription" "${PRODUCTNAME} installer"
VIAddVersionKey /LANG=0 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey /LANG=0 "FontBuild" "${FONT_BUILD}"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${MAINBINARYNAME}"
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(createDesktop)"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; The first language is the fallback when the user's is neither.
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

LangString createDesktop ${LANG_ENGLISH} "Create a desktop shortcut"
LangString createDesktop ${LANG_SIMPCHINESE} "创建桌面快捷方式"
LangString requires64Bit ${LANG_ENGLISH} "${PRODUCTNAME} requires 64-bit Windows."
LangString requires64Bit ${LANG_SIMPCHINESE} "${PRODUCTNAME} 需要 64 位 Windows。"
LangString appRunning ${LANG_ENGLISH} "${PRODUCTNAME} is running. Close it, then choose Retry."
LangString appRunning ${LANG_SIMPCHINESE} "${PRODUCTNAME} 正在运行。请先关闭它，再点“重试”。"

; A running executable cannot be opened for writing, so this finds a running
; copy (this one or the Tauri GUI it replaces) before any file is touched.
!macro WAIT_UNTIL_CLOSED UN
  Function ${UN}WaitUntilClosed
    retry:
    ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}"
      ClearErrors
      FileOpen $0 "$INSTDIR\${MAINBINARYNAME}" a
      ${If} ${Errors}
        MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(appRunning)" /SD IDCANCEL IDRETRY retry
        Abort
      ${EndIf}
      FileClose $0
    ${EndIf}
  FunctionEnd
!macroend
!insertmacro WAIT_UNTIL_CLOSED ""
!insertmacro WAIT_UNTIL_CLOSED "un."

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "$(requires64Bit)" /SD IDOK
    Abort
  ${EndIf}
  SetShellVarContext current
FunctionEnd

Function un.onInit
  SetShellVarContext current
FunctionEnd

Section "Install"
  Call WaitUntilClosed
  SetOutPath "$INSTDIR"
  ; A Tauri install records its executable's name; drop it if it ever differed.
  ReadRegStr $0 HKCU "${UNINSTKEY}" "MainBinaryName"
  ${If} $0 != ""
  ${AndIf} $0 != "${MAINBINARYNAME}"
    Delete "$INSTDIR\$0"
  ${EndIf}
  File "/oname=${MAINBINARYNAME}" "${PAYLOAD}"
  RMDir /r "$INSTDIR\licenses"
  SetOutPath "$INSTDIR\licenses"
  File "${LICENSES}\*"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${MANUPRODUCTKEY}" "" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}$\""
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  WriteRegStr HKCU "${UNINSTKEY}" "FontBuild" "${FONT_BUILD}"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "$0"

  ; Recreated rather than kept: Tauri stamped its shortcuts with an
  ; AppUserModelID this application does not set, which would show the
  ; running window apart from its pinned taskbar icon.
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}"
  ${If} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
    Call CreateDesktopShortcut
  ${EndIf}
SectionEnd

Function CreateDesktopShortcut
  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}"
FunctionEnd

; Settings live outside the install directory and are left in place, as
; Tauri's uninstaller left them unless asked.
Section "Uninstall"
  Call un.WaitUntilClosed
  Delete "$INSTDIR\${MAINBINARYNAME}"
  RMDir /r "$INSTDIR\licenses"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "${MANUPRODUCTKEY}"
  DeleteRegKey /ifempty HKCU "Software\${MANUFACTURER}"
SectionEnd
