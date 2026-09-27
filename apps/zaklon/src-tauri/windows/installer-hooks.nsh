; Zaklon installer hooks.
;
; 1. Keep a copy of this installer next to the household's data, so "Copy to
;    USB" can put it on a stick for a friend who has no internet: they install
;    Zaklon from the stick, then import the packs from it.
; 2. Bring the phone app along (when the build put windows/zaklon.apk next to
;    this file), so phones can install it from the hub without internet.

; This file's folder, taken while it is being included (inside the macro
; __FILEDIR__ would be the folder of the generated installer script).
!define ZAKLON_HOOKS_DIR "${__FILEDIR__}"

!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$INSTDIR\data\library\installer"
  CopyFiles /SILENT "$EXEPATH" "$INSTDIR\data\library\installer\Zaklon-setup.exe"
  !if /FileExists "${ZAKLON_HOOKS_DIR}\zaklon.apk"
    CreateDirectory "$INSTDIR\data\library\apk"
    SetOutPath "$INSTDIR\data\library\apk"
    File "${ZAKLON_HOOKS_DIR}\zaklon.apk"
    SetOutPath "$INSTDIR"
  !endif
!macroend
