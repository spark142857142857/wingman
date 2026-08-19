; The PATH implementation is embedded in the installer and extracted only into
; NSIS's private plugin directory. These short UTF-16LE commands load the fixed
; script without depending on PowerShell's script execution policy.
!define WINGMAN_PATH_SCRIPT_SOURCE "${__FILEDIR__}\wingman-path.ps1"
!define WINGMAN_PATH_INSTALL_LOADER "JABzAD0AWwBJAE8ALgBGAGkAbABlAF0AOgA6AFIAZQBhAGQAQQBsAGwAVABlAHgAdAAoACQAZQBuAHYAOgBXAEkATgBHAE0AQQBOAF8AUABBAFQASABfAFMAQwBSAEkAUABUACkAOwAmACgAWwBTAGMAcgBpAHAAdABCAGwAbwBjAGsAXQA6ADoAQwByAGUAYQB0AGUAKAAkAHMAKQApACAALQBNAG8AZABlACAASQBuAHMAdABhAGwAbAA="
!define WINGMAN_PATH_UNINSTALL_LOADER "JABzAD0AWwBJAE8ALgBGAGkAbABlAF0AOgA6AFIAZQBhAGQAQQBsAGwAVABlAHgAdAAoACQAZQBuAHYAOgBXAEkATgBHAE0AQQBOAF8AUABBAFQASABfAFMAQwBSAEkAUABUACkAOwAmACgAWwBTAGMAcgBpAHAAdABCAGwAbwBjAGsAXQA6ADoAQwByAGUAYQB0AGUAKAAkAHMAKQApACAALQBNAG8AZABlACAAVQBuAGkAbgBzAHQAYQBsAGwA"

!macro WINGMAN_STAGE_PATH_SCRIPT
  InitPluginsDir
  File /oname=$PLUGINSDIR\wingman-path.ps1 "${WINGMAN_PATH_SCRIPT_SOURCE}"
  System::Call 'kernel32::SetEnvironmentVariableW(w "WINGMAN_PATH_SCRIPT", w "$PLUGINSDIR\wingman-path.ps1")'
!macroend

!macro WINGMAN_CLEAR_PATH_SCRIPT
  System::Call 'kernel32::SetEnvironmentVariableW(w "WINGMAN_PATH_SCRIPT", p 0)'
  Delete "$PLUGINSDIR\wingman-path.ps1"
!macroend

!macro WINGMAN_NOTIFY_ENVIRONMENT
  System::Call 'user32::SendMessageTimeoutW(p 0xffff, i 0x001A, p 0, w "Environment", i 0x0002, i 5000, *p .r2)'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro WINGMAN_STAGE_PATH_SCRIPT
  nsExec::ExecToStack /TIMEOUT=30000 '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -EncodedCommand ${WINGMAN_PATH_INSTALL_LOADER}'
  Pop $0
  Pop $1
  !insertmacro WINGMAN_CLEAR_PATH_SCRIPT
  ${If} $0 != 0
    DetailPrint "Wingman PATH registration failed: $1"
    Abort "Wingman could not register its current-user command path."
  ${EndIf}
  !insertmacro WINGMAN_NOTIFY_ENVIRONMENT
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro WINGMAN_STAGE_PATH_SCRIPT
  nsExec::ExecToStack /TIMEOUT=30000 '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -EncodedCommand ${WINGMAN_PATH_UNINSTALL_LOADER}'
  Pop $0
  Pop $1
  !insertmacro WINGMAN_CLEAR_PATH_SCRIPT
  ${If} $0 != 0
    DetailPrint "Wingman PATH removal failed: $1"
    Abort "Wingman could not remove its current-user command path."
  ${EndIf}
  !insertmacro WINGMAN_NOTIFY_ENVIRONMENT
!macroend
