; Tauri NSIS installer hooks (bundle > windows > nsis > installerHooks). Per-user install, no admin.
;
; The uninstall page already carries Tauri's checkbox $DeleteAppDataCheckbox (unchecked by default);
; windows/nsis/{English,Russian}.nsh relabel it «Also delete Lowk settings, downloaded models and logs».
; Tauri itself only wipes %APPDATA%\dev.lowk.app, which Lowk never uses, so this hook removes the real
; data dir (%APPDATA%\Lowk: settings, models, engines, logs) and the Credential Manager entries.
;
; NEVER touch the user's vault (Documents\Lowk): it holds the user's notes.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\Lowk"
    RmDir /r "$LOCALAPPDATA\Lowk"
    ; Credential Manager targets are «<provider>.<service>» (keyring crate), service = bundle id.
    nsExec::Exec 'cmdkey /delete:openrouter.${BUNDLEID}'
    Pop $0
    nsExec::Exec 'cmdkey /delete:stt-cloud.${BUNDLEID}'
    Pop $0
  ${EndIf}
!macroend
