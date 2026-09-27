; Zaklon installer hooks.
;
; Keep a copy of this installer next to the household's data, so "Copy to
; USB" can put it on a stick for a friend who has no internet: they install
; Zaklon from the stick, then import the packs from it.

!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$INSTDIR\data\library\installer"
  CopyFiles /SILENT "$EXEPATH" "$INSTDIR\data\library\installer\Zaklon-setup.exe"
!macroend
