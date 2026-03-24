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
  # Try to stop running app process first; otherwise locked files may leave residue.
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "Wunder Desktop.exe"'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wunder-desktop.exe"'
  Sleep 1200
  MessageBox MB_ICONQUESTION|MB_YESNO|MB_DEFBUTTON2 "是否清理应用临时缓存（仅删除 WUNDER_TEMPD 目录）？" IDNO cache_cleanup_done
  SetShellVarContext current
  # Only remove the temp cache folder. Do not touch workspaces because users may store their own files there.
  RMDir /r "$APPDATA\wunder-desktop-electron\WUNDER_TEMPD"
  RMDir /r "$APPDATA\wunder-desktop-electron-win7\WUNDER_TEMPD"
  # Trim verified app-data roots only when they become empty after temp cleanup.
  RMDir "$APPDATA\wunder-desktop-electron"
  RMDir "$APPDATA\wunder-desktop-electron-win7"
cache_cleanup_done:
!macroend

!macro customUnInstallSection
  # Keep macro for compatibility; cache cleanup option is now prompted explicitly in customUnInstall.
!macroend
