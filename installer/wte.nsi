Unicode true
RequestExecutionLevel user
AllowSkipFiles off
ManifestDPIAware true
SetCompressor /SOLID lzma
Name "Windows Theme Engine"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\WTE"
Icon "${SOURCE}/assets/wte.ico"
UninstallIcon "${SOURCE}/assets/wte.ico"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Windows Theme Engine"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Windows Theme Engine Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Windows Theme Engine contributors"

!include "MUI2.nsh"
!include "x64.nsh"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\WindowsThemeEngine"
!define MUI_ICON "${SOURCE}/assets/wte.ico"
!define MUI_UNICON "${SOURCE}/assets/wte.ico"
!define MUI_WELCOMEPAGE_TEXT "Install Windows Theme Engine for your Windows account. Setup will add a shortcut to your Start menu."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\wte.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Windows Theme Engine"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
    ${IfNot} ${RunningX64}
        MessageBox MB_OK|MB_ICONSTOP "This build requires 64-bit Windows." /SD IDOK
        Abort
    ${EndIf}
FunctionEnd

Section "Install"
    SetShellVarContext current
    SetRegView 64
    SetOutPath "$INSTDIR"
    File "/oname=wte.exe" "${WTE_EXE}"
    File "/oname=ExoVoid-LICENSE.txt" "${SOURCE}/assets/exovoid/LICENSE"
    File "/oname=DM-Sans-OFL.txt" "${SOURCE}/assets/fonts/OFL.txt"
    File "/oname=Wallpaper-CREDITS.md" "${SOURCE}/assets/wallpapers/CREDITS.md"
    File "${SOURCE}/installer/THIRD-PARTY-NOTICES.txt"
    WriteUninstaller "$INSTDIR\uninstall.exe"
    CreateShortcut "$SMPROGRAMS\Windows Theme Engine.lnk" "$INSTDIR\wte.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Windows Theme Engine"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\wte.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
    SetShellVarContext current
    SetRegView 64
    IfFileExists "$INSTDIR\wte.exe" 0 appRemoved
    ClearErrors
    Delete "$INSTDIR\wte.exe"
    IfErrors 0 +3
        MessageBox MB_OK|MB_ICONSTOP "Close Windows Theme Engine and run the uninstaller again." /SD IDOK
        Abort
    appRemoved:
    Delete "$INSTDIR\ExoVoid-LICENSE.txt"
    Delete "$INSTDIR\DM-Sans-OFL.txt"
    Delete "$INSTDIR\Wallpaper-CREDITS.md"
    Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
    Delete "$SMPROGRAMS\Windows Theme Engine.lnk"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
SectionEnd
