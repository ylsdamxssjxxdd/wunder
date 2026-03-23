!macro customInstall
  ${if} ${FileExists} "$INSTDIR\resources\wunder-cli.exe"
    CopyFiles /SILENT "$INSTDIR\resources\wunder-cli.exe" "$INSTDIR\wunder-cli.exe"
  ${endif}
  ${if} ${FileExists} "$INSTDIR\resources\README-win7-supplement.txt"
    CopyFiles /SILENT "$INSTDIR\resources\README-win7-supplement.txt" "$INSTDIR\README-win7-supplement.txt"
  ${endif}
  ${if} ${FileExists} "$INSTDIR\resources\icon.ico"
    ${if} ${FileExists} "$newDesktopLink"
      CreateShortCut "$newDesktopLink" "$appExe" "" "$INSTDIR\resources\icon.ico" 0 "" "" "${APP_DESCRIPTION}"
      WinShell::SetLnkAUMI "$newDesktopLink" "${APP_ID}"
    ${endif}
    ${if} ${FileExists} "$newStartMenuLink"
      CreateShortCut "$newStartMenuLink" "$appExe" "" "$INSTDIR\resources\icon.ico" 0 "" "" "${APP_DESCRIPTION}"
      WinShell::SetLnkAUMI "$newStartMenuLink" "${APP_ID}"
    ${endif}
    System::Call 'Shell32::SHChangeNotify(i 0x8000000, i 0, i 0, i 0)'
  ${endif}
!macroend

!macro customUnInstall
  Delete "$INSTDIR\wunder-cli.exe"
  Delete "$INSTDIR\README-win7-supplement.txt"
!macroend

!macro customUnInstallSection
  # Keep a static ASCII label here so NSIS language tables added by electron-builder
  # do not require per-language LangString entries in CI.
  Section /o "un.Delete temp cache (WUNDER_TEMPD only)"
    SetShellVarContext current
    # Only remove the temp cache folder. Do not touch workspaces because users may store their own files there.
    RMDir /r "$APPDATA\wunder-desktop-electron\WUNDER_TEMPD"
    RMDir /r "$APPDATA\wunder-desktop-electron-win7\WUNDER_TEMPD"
    # Trim verified app-data roots only when they become empty after temp cleanup.
    RMDir "$APPDATA\wunder-desktop-electron"
    RMDir "$APPDATA\wunder-desktop-electron-win7"
  SectionEnd
!macroend
