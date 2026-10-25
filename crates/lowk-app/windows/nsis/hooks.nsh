; Tauri NSIS installer hooks (bundle > windows > nsis > installerHooks). Per-user install, no admin.
; Macros mirror Tauri's documented NSIS_HOOK_PREINSTALL / POSTINSTALL / PREUNINSTALL / POSTUNINSTALL.
;
; 1. Before an install AND before an uninstall Lowk and its helper processes are closed (graceful, then forced).
;    Tauri only looks at lowk.exe; a running lowk-stt-worker.exe kept model files locked and a running app rewrote
;    its settings, which is why the old «delete settings» option left data behind.
; 2. The uninstall page carries Tauri's checkbox $DeleteAppDataCheckbox (unchecked by default); the language files
;    relabel it «Also delete Lowk settings, downloaded models and logs». When it is ticked, `lowk.exe
;    --purge-user-data` removes every location the app uses (settings, data root with models/engines/logs, WebView2
;    profile, Credential Manager entries), computed by the app's own path helpers (crates/lowk-platform/src/purge.rs)
;    so the list can never drift. A plain RmDir below is only the fallback if that call failed.
; 3. NEVER touch the user's vault (Documents\Lowk): it holds the user's notes.
;
; Welcome page text (the finish page keeps MUI's localised «Run Lowk» / «Запустить Lowk»).
!define MUI_WELCOMEPAGE_TITLE "$(lowkWelcomeTitle)"
!define MUI_WELCOMEPAGE_TEXT "$(lowkWelcomeText)"

; Closes Lowk and helpers owned by the current user. `taskkill` without /F asks windows to close; after a short wait
; whatever is left is killed with /F /T. nsExec::Exec runs without a console window.
!macro LOWK_STOP_APP
  nsExec::Exec 'cmd /c taskkill /IM lowk.exe /FI "USERNAME eq %USERDOMAIN%\%USERNAME%" >nul 2>&1'
  Pop $0
  Sleep 2500
  nsExec::Exec 'cmd /c taskkill /F /T /IM lowk.exe /FI "USERNAME eq %USERDOMAIN%\%USERNAME%" >nul 2>&1'
  Pop $0
  nsExec::Exec 'cmd /c taskkill /F /T /IM lowk-stt-worker.exe /FI "USERNAME eq %USERDOMAIN%\%USERNAME%" >nul 2>&1'
  Pop $0
  nsExec::Exec 'cmd /c taskkill /F /T /IM lowk-embed-worker.exe /FI "USERNAME eq %USERDOMAIN%\%USERNAME%" >nul 2>&1'
  Pop $0
  Sleep 800
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro LOWK_STOP_APP
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro LOWK_STOP_APP
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; The app is still on disk here, so it can purge itself (headless, no window).
    nsExec::Exec '"$INSTDIR\${MAINBINARYNAME}.exe" --purge-user-data'
    Pop $0
    DetailPrint "Lowk: user data purge exit code $0"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    ; Fallback for the case the purge call above could not run (paths are the defaults only).
    RmDir /r "$APPDATA\Lowk"
    RmDir /r "$LOCALAPPDATA\Lowk"
    RmDir /r "$INSTDIR\data"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
    ; Credential Manager targets are «<provider>.<service>» (keyring crate), service = bundle id.
    nsExec::Exec 'cmdkey /delete:openrouter.${BUNDLEID}'
    Pop $0
    nsExec::Exec 'cmdkey /delete:stt-cloud.${BUNDLEID}'
    Pop $0
    ; Remove the install folder if nothing is left in it.
    RmDir "$INSTDIR"
  ${EndIf}
!macroend
