# wtgapfix

Removes the thin line at the bottom of a maximized or fullscreen Windows Terminal when the
taskbar auto-hides ([microsoft/terminal#12959](https://github.com/microsoft/terminal/issues/12959)).

Terminal shrinks its client area 2 px on the taskbar edge. Windows won't let another process
resize a maximized window, so the fix runs inside Terminal:

- `wtgapfix.exe` (loader, tray icon): puts `wtgapfix_hook.dll` into each Terminal window once
  (SetWindowsHookEx on the window's thread), then waits for new windows.
- `wtgapfix_hook.dll`: subclasses the window; after Terminal's WM_NCCALCSIZE it moves the client
  bottom back to the monitor bottom, and sets `NonRudeHWND` (except in F11) so the taskbar can
  still pop up. Same end state as the upstream fix
  [#20209](https://github.com/microsoft/terminal/pull/20209).

## Install

Needs Rust (MSVC toolchain). In cmd:

```
cargo build --release
mkdir "%LOCALAPPDATA%\wtgapfix"
copy /Y target\release\wtgapfix.exe "%LOCALAPPDATA%\wtgapfix\"
copy /Y target\release\wtgapfix_hook.dll "%LOCALAPPDATA%\wtgapfix\"
reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v wtgapfix /t REG_SZ /d "%LOCALAPPDATA%\wtgapfix\wtgapfix.exe" /f
start "" "%LOCALAPPDATA%\wtgapfix\wtgapfix.exe"
```

A `>_` icon appears in the tray (Windows 11 puts new icons under the `^` overflow; to keep it
visible: Settings > Personalization > Taskbar > Other system tray icons > wtgapfix On).
Click it, then **Exit wtgapfix** to stop it. Windows it already fixed stay fixed until Terminal
restarts; new windows are not fixed.

## Uninstall

Tray icon > Exit, then
`reg delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v wtgapfix /f`, close every
Terminal window, then delete `%LOCALAPPDATA%\wtgapfix` (the DLL is locked until Terminal exits).

## Limits

- Bottom taskbar only.
- A bug in the DLL crashes Terminal (all tabs). It never panics by design; still, it runs in Terminal.
- Elevated Terminal: not fixed unless wtgapfix also runs elevated.
- Updating: Terminal keeps the DLL it loaded locked, so `copy` over it fails until every Terminal
  window is closed. Same for `cargo run` during development: the next debug build fails
  "Access is denied" on `target\debug\wtgapfix_hook.dll`.
- Delete once Terminal ships #20209.

## Verified 2026-11-13

- Fake Terminal window (same class, same 2 px inset), injected then unhooked (DLL stays pinned):
  maximized client bottom 1078 -> 1080; normal and restored windows unchanged; F11 client =
  monitor, NonRudeHWND removed; process stays responsive and exits cleanly.
- Real Windows Terminal, maximized, 1920x1080, auto-hide taskbar at the bottom: line gone (checked
  by eye, not measured).
- Tray: icon added, added again after Explorer's TaskbarCreated, removed on exit
  (Shell_NotifyIconGetRect); window never visible; exit code 0. Icon pixels checked by
  `cargo test`. Clicking the menu is a manual check.
