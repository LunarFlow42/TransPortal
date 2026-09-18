; Tauri patches the bundle marker and signs the executable before makensis.
; Seal it here so every byte of the final executable is covered by SHA-256.
!define TP_INTEGRITY_SCRIPT "${__FILEDIR__}\..\..\scripts\integrity\seal.mjs"

!macro NSIS_HOOK_PREINSTALL
  ; Tauri includes this file before defining MAINBINARYSRCPATH and VERSION.
  ; Expand the compile-time command when the install section is generated.
  !system 'node "${TP_INTEGRITY_SCRIPT}" "${MAINBINARYSRCPATH}" "${VERSION}"' = 0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  File /oname=integrity.json "${MAINBINARYSRCPATH}.integrity.json"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\integrity.json"
!macroend
