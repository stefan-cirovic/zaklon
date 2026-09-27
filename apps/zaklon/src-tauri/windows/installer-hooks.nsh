; Zaklon installer hooks.
;
; 1. Keep a copy of this installer next to the household's data, so "Copy to
;    USB" can put it on a stick for a friend who has no internet: they install
;    Zaklon from the stick, then import the packs from it.
; 2. Bring the phone app along (when the build put windows/zaklon.apk next to
;    this file), so phones can install it from the hub without internet.
; 3. Start with Windows unless the person turned it off. An upgrade that
;    uninstalls the old version first removes the entry; without this, the
;    hub would not come back after the next reboot until the app was opened
;    by hand. The names match src/autostart.rs.
; 4. "Delete the application data" in the uninstaller also deletes the
;    household's data folder, and the "Start with Windows" choice.

; This file's folder, taken while it is being included (inside the macro
; __FILEDIR__ would be the folder of the generated installer script).
!define ZAKLON_HOOKS_DIR "${__FILEDIR__}"

!macro NSIS_HOOK_POSTINSTALL
  ; Copy under a temporary name first: a full disk or a cancel must never
  ; leave a half installer that "Copy to USB" would hand on.
  CreateDirectory "$INSTDIR\data\library\installer"
  Delete "$INSTDIR\data\library\installer\Zaklon-setup.exe.part"
  ClearErrors
  CopyFiles /SILENT "$EXEPATH" "$INSTDIR\data\library\installer\Zaklon-setup.exe.part"
  ${If} ${Errors}
    Delete "$INSTDIR\data\library\installer\Zaklon-setup.exe.part"
  ${Else}
    Delete "$INSTDIR\data\library\installer\Zaklon-setup.exe"
    Rename "$INSTDIR\data\library\installer\Zaklon-setup.exe.part" "$INSTDIR\data\library\installer\Zaklon-setup.exe"
  ${EndIf}
  ClearErrors

  !if /FileExists "${ZAKLON_HOOKS_DIR}\zaklon.apk"
    CreateDirectory "$INSTDIR\data\library\apk"
    SetOutPath "$INSTDIR\data\library\apk"
    File "${ZAKLON_HOOKS_DIR}\zaklon.apk"
    SetOutPath "$INSTDIR"
  !endif

  Push $0
  ReadRegDWORD $0 HKCU "Software\Zaklon" "AutostartOff"
  ${If} $0 <> 1
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --minimized'
  ${EndIf}
  Pop $0
  ClearErrors
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Not during an update, and only a data folder Zaklon made itself: never a
  ; "data" folder that happens to be in a folder the person picked by hand.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\data\household\hub.json"
      RMDir /r "$INSTDIR\data"
      RMDir "$INSTDIR"
    ${EndIf}
    DeleteRegValue HKCU "Software\Zaklon" "AutostartOff"
    DeleteRegKey /ifempty HKCU "Software\Zaklon"
  ${EndIf}
!macroend
