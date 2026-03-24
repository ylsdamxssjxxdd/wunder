!macro customInstall
  SetShellVarContext current
  Delete "$LOCALAPPDATA\wunder-desktop-electron-win7-updater\installer.exe"
  Delete "$LOCALAPPDATA\wunder-desktop-electron-updater\installer.exe"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron-win7-updater"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron-updater"
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron-win7-updater"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron-updater"'

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
  SetShellVarContext current

  # Stop running app processes first to reduce locked-file residue.
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "Wunder Desktop.exe"'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wunder-desktop.exe"'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wunder-desktop-bridge.exe"'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "wunder-desktop-updater.exe"'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "Update.exe"'
  Sleep 1200

  MessageBox MB_ICONQUESTION|MB_YESNO|MB_DEFBUTTON2 "是否清空缓存并删除本地数据？" IDNO skip_data_cleanup
  Delete "$LOCALAPPDATA\wunder-desktop-electron-win7-updater\installer.exe"
  Delete "$LOCALAPPDATA\wunder-desktop-electron-updater\installer.exe"
  RMDir /r "$APPDATA\wunder-desktop-electron-win7"
  RMDir /r "$APPDATA\wunder-desktop-electron"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron-win7"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron-win7-updater"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron-updater"
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$APPDATA\wunder-desktop-electron-win7"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$APPDATA\wunder-desktop-electron"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron-win7"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron-win7-updater"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron-updater"'
skip_data_cleanup:

  Delete "$LOCALAPPDATA\wunder-desktop-electron-win7-updater\installer.exe"
  Delete "$LOCALAPPDATA\wunder-desktop-electron-updater\installer.exe"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron-win7-updater"
  RMDir /r "$LOCALAPPDATA\wunder-desktop-electron-updater"
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron-win7-updater"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\wunder-desktop-electron-updater"'

  RMDir /r "$INSTDIR"
  RMDir /r "$LOCALAPPDATA\Programs\Wunder Desktop"
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$INSTDIR"'
  nsExec::ExecToLog '"$SYSDIR\cmd.exe" /C rd /s /q "$LOCALAPPDATA\Programs\Wunder Desktop"'
!macroend

!macro customUnInstallSection
  # Keep macro for compatibility.
!macroend
