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
