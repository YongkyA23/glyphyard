; Before removing files, turn off every font Typeshelf activated (registry values + loaded fonts).
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$INSTDIR\typeshelf.exe" --unregister-fonts'
!macroend
