//! Runs inside WindowsTerminal.exe (loaded by wtgapfix.exe). With an auto-hide taskbar,
//! Terminal shrinks its client area 2 px on the taskbar edge when maximized or fullscreen
//! (microsoft/terminal#12959). This undoes that in WM_NCCALCSIZE, same end state as the
//! upstream fix microsoft/terminal#20209.
//!
//! Never panic in here

use std::{mem::zeroed, ptr::null_mut};

use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW,
        },
        UI::{
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CWPSTRUCT, CallNextHookEx, GWL_STYLE, GetClassNameW, GetPropW, GetWindowLongW,
                NCCALCSIZE_PARAMS, RemovePropW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SWP_NOZORDER, SetPropW, SetWindowPos, WM_NCCALCSIZE, WS_MAXIMIZE,
                WS_POPUP,
            },
        },
    },
    w,
};

/// Terminal shrinks by 2 px
const MAX_GAP: i32 = 8;

/// WH_CALLWNDPROC hook, runs on Terminal's UI thread: subclass its main window, once.
///
/// # Safety
/// Only Windows calls this, as a WH_CALLWNDPROC hook (`lp` = CWPSTRUCT when `code >= 0`).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 {
            let hwnd = (*(lp as *const CWPSTRUCT)).hwnd;
            if is_terminal(hwnd) && GetPropW(hwnd, w!("wtgapfix")).is_null() {
                // Stay loaded after the loader removes the hook: the subclass lives here.
                let mut me = null_mut();
                let flags = GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS;
                GetModuleHandleExW(flags, hook as *const u16, &mut me);
                if SetWindowSubclass(hwnd, Some(subclass), 1, 0) != 0 {
                    SetPropW(hwnd, w!("wtgapfix"), 1 as _);
                    let swp =
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
                    SetWindowPos(hwnd, null_mut(), 0, 0, 0, 0, swp);
                }
            }
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        let ret = DefSubclassProc(hwnd, msg, wp, lp);
        if msg == WM_NCCALCSIZE && wp != 0 {
            let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            let fullscreen = style & WS_POPUP != 0;
            // Client now covers the monitor, so the shell would call a maximized Terminal
            // "fullscreen" and keep the taskbar from popping up. NonRudeHWND says it is not.
            // Real fullscreen (F11) keeps the taskbar down, like every fullscreen app.
            if fullscreen {
                RemovePropW(hwnd, w!("NonRudeHWND"));
            } else {
                SetPropW(hwnd, w!("NonRudeHWND"), 1 as _);
            }
            if fullscreen || style & WS_MAXIMIZE != 0 {
                let mut mi: MONITORINFO = zeroed();
                mi.cbSize = size_of::<MONITORINFO>() as u32;
                GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
                let rc = &mut (*(lp as *mut NCCALCSIZE_PARAMS)).rgrc[0];
                if (1..=MAX_GAP).contains(&(mi.rcMonitor.bottom - rc.bottom)) {
                    rc.bottom = mi.rcMonitor.bottom;
                }
            }
        }
        ret
    }
}

unsafe fn is_terminal(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    n > 0
        && buf[..n as usize]
            == *"CASCADIA_HOSTING_WINDOW_CLASS"
                .encode_utf16()
                .collect::<Vec<_>>()
}
