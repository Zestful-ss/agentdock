; AgentDock Windows migration hook.
;
; The pre-rename product used the same executable name and the default
; current-user install layout. When productName changes, Tauri's new NSIS
; registry key would otherwise point at a second install directory. Recover
; the old directory before files are copied; the post-install hook removes only
; the old NSIS keys after the new key has been written.

!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $0 HKCU "Software\agentskills\AgentDock" ""
  ${If} $0 == ""
    ReadRegStr $0 HKCU "Software\agentskills\skills-manager" ""
  ${EndIf}
  ${If} $0 == ""
    ReadRegStr $0 HKLM "Software\agentskills\AgentDock" ""
  ${EndIf}
  ${If} $0 == ""
    ReadRegStr $0 HKLM "Software\agentskills\skills-manager" ""
  ${EndIf}

  ; Old MSI installations use a product-specific uninstall key. This also
  ; covers the common case where an older release was installed per-machine.
  ${If} $0 == ""
    ReadRegStr $0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\skills-manager" "InstallLocation"
  ${EndIf}

  ${If} $0 != ""
    StrCpy $INSTDIR $0
    SetOutPath $INSTDIR
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Do not remove arbitrary GUID-based MSI entries here; the pinned WiX
  ; UpgradeCode owns the MSI migration. These are the old Tauri NSIS keys.
  DeleteRegKey HKCU "Software\agentskills\skills-manager"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\skills-manager"
  DeleteRegKey HKLM "Software\agentskills\skills-manager"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\skills-manager"

  ; The new product keeps the old Start Menu folder during the transition;
  ; remove only the pre-rename shortcut names after AgentDock shortcuts exist.
  Delete "$SMPROGRAMS\skills-manager.lnk"
  Delete "$SMPROGRAMS\skills-manager\skills-manager.lnk"
  Delete "$DESKTOP\skills-manager.lnk"
!macroend
