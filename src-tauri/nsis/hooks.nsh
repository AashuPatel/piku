; Piku NSIS lifecycle hooks
; On a real uninstall (not an in-place upgrade), remove Piku's per-user Windows
; startup entry while preserving the user's SQLite database and preferences in
; %LOCALAPPDATA%\com.piku.desktop\.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Piku"
  ${EndIf}
!macroend
