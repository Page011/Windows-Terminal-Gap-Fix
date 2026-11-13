//! Removes the line at the bottom of a maximized/fullscreen Windows Terminal when the
//! taskbar auto-hides (microsoft/terminal#12959). Windows won't let another process resize
//! a maximized window, so the fix runs inside Terminal: this loader puts
//! wtgapfix_hook.dll (next to this exe) into each Terminal window once, then stays running
//! to catch new windows, with a tray icon (click it, then Exit) to stop it.
//! Delete once Terminal ships microsoft/terminal#20209.
#![windows_subsystem = "windows"]

use std::{mem::zeroed, os::windows::ffi::OsStrExt, ptr::null, ptr::null_mut, sync::OnceLock};

use windows_sys::{
    Win32::{
        Foundation::{
            BOOL, ERROR_ALREADY_EXISTS, FARPROC, GetLastError, HMODULE, HWND, LPARAM, LRESULT,
            POINT, WPARAM,
        },
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW},
            Threading::CreateMutexW,
        },
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook},
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFY_ICON_MESSAGE,
                NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyMenu, DestroyWindow, DispatchMessageW, EVENT_OBJECT_SHOW, EnumWindows,
                GetClassNameW, GetCursorPos, GetMessageW, GetPropW, GetWindowThreadProcessId,
                HICON, HOOKPROC, MF_STRING, OBJID_WINDOW, PostQuitMessage, RegisterClassW,
                RegisterWindowMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, SetForegroundWindow,
                SetWindowsHookExW, TPM_NONOTIFY, TPM_RETURNCMD, TrackPopupMenu,
                UnhookWindowsHookEx, WH_CALLWNDPROC, WINEVENT_OUTOFCONTEXT,
                WINEVENT_SKIPOWNPROCESS, WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
                WNDCLASSW,
            },
        },
    },
    s, w,
};

/// (wtgapfix_hook.dll handle, its `hook` export), set once in main.
static LOADED: OnceLock<(usize, HOOKPROC)> = OnceLock::new();
/// Explorer sends this when the taskbar (re)starts; the tray icon must be added again.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
/// Tray icon handle, made once, kept for the life of the process.
static TRAY_ICON: OnceLock<usize> = OnceLock::new();

const WM_TRAY: u32 = WM_APP + 1;
const ID_EXIT: i32 = 1;

/// Tray icon, 16x16: ">_" on a dark square. ' ' transparent, '#' dark, 'w' light.
// ponytail: 16 px only, soft above 100 % display scaling; embed an .ico if that matters.
const ICON: [&[u8; 16]; 16] = [
    b" ############## ",
    b"################",
    b"################",
    b"################",
    b"##ww############",
    b"###ww###########",
    b"####ww##########",
    b"#####ww#########",
    b"####ww##########",
    b"###ww###########",
    b"##ww####wwwwww##",
    b"########wwwwww##",
    b"################",
    b"################",
    b"################",
    b" ############## ",
];

fn main() {
    unsafe {
        // One copy per session. Handle held until exit on purpose.
        CreateMutexW(null(), 0, w!("Local\\wtgapfix"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let dll: Vec<u16> = exe
            .with_file_name("wtgapfix_hook.dll")
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect();
        let dll = LoadLibraryW(dll.as_ptr());
        let hook = std::mem::transmute::<FARPROC, HOOKPROC>(GetProcAddress(dll, s!("hook")));
        if dll.is_null() || hook.is_none() {
            eprintln!("wtgapfix_hook.dll missing next to wtgapfix.exe");
            std::process::exit(1);
        }
        let _ = LOADED.set((dll as usize, hook));
        let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));

        // Hidden top-level window (never shown): owns the tray icon, gets TaskbarCreated.
        let inst = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            lpszClassName: w!("wtgapfix"),
            ..zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            w!("wtgapfix"),
            w!("wtgapfix"),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            inst,
            null(),
        );
        tray(hwnd, NIM_ADD);

        EnumWindows(Some(on_window), 0);
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            null_mut(),
            Some(on_show),
            0,
            0,
            flags,
        );
        let mut msg = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TRAY if matches!(lp as u32, WM_LBUTTONUP | WM_RBUTTONUP) => {
                let menu = CreatePopupMenu();
                AppendMenuW(menu, MF_STRING, ID_EXIT as usize, w!("Exit wtgapfix"));
                let mut pt: POINT = zeroed();
                GetCursorPos(&mut pt);
                // Foreground first, or the menu stays open when you click elsewhere.
                SetForegroundWindow(hwnd);
                let flags = TPM_RETURNCMD | TPM_NONOTIFY;
                let cmd = TrackPopupMenu(menu, flags, pt.x, pt.y, 0, hwnd, null());
                DestroyMenu(menu);
                if cmd == ID_EXIT {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_DESTROY => {
                tray(hwnd, NIM_DELETE);
                PostQuitMessage(0);
                0
            }
            _ if Some(&msg) == TASKBAR_CREATED.get() => {
                tray(hwnd, NIM_ADD);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Add (NIM_ADD) or remove (NIM_DELETE) the tray icon.
unsafe fn tray(hwnd: HWND, op: NOTIFY_ICON_MESSAGE) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = *TRAY_ICON.get_or_init(|| make_icon() as usize) as HICON;
        let tip = "wtgapfix: Terminal taskbar gap fix".encode_utf16();
        nid.szTip.iter_mut().zip(tip).for_each(|(d, s)| *d = s);
        Shell_NotifyIconW(op, &nid);
    }
}

/// ICON as a 32-bit icon: BGRA colour bits plus a 1-bit mask (1 = transparent).
fn make_icon() -> HICON {
    let mut and = [0u8; 16 * 2];
    let mut xor = [0u8; 16 * 16 * 4];
    for (y, row) in ICON.iter().enumerate() {
        for (x, &c) in row.iter().enumerate() {
            let px = match c {
                b'#' => [0x0C, 0x0C, 0x0C, 0xFF],
                b'w' => [0xF2, 0xF2, 0xF2, 0xFF],
                _ => {
                    and[y * 2 + x / 8] |= 0x80 >> (x % 8);
                    [0; 4]
                }
            };
            xor[(y * 16 + x) * 4..][..4].copy_from_slice(&px);
        }
    }
    unsafe {
        let inst = GetModuleHandleW(null());
        CreateIcon(inst, 16, 16, 1, 32, and.as_ptr(), xor.as_ptr())
    }
}

unsafe extern "system" fn on_window(hwnd: HWND, _: LPARAM) -> BOOL {
    unsafe { inject(hwnd) };
    1
}

unsafe extern "system" fn on_show(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    obj: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if obj == OBJID_WINDOW {
        unsafe { inject(hwnd) };
    }
}

/// Hook the window's thread, poke it so the hook runs there (the DLL subclasses the window
/// and pins itself), then unhook.
unsafe fn inject(hwnd: HWND) {
    unsafe {
        let mut buf = [0u16; 64];
        let n = GetClassNameW(hwnd, buf.as_mut_ptr(), 64).max(0) as usize;
        if String::from_utf16_lossy(&buf[..n]) != "CASCADIA_HOSTING_WINDOW_CLASS"
            || !GetPropW(hwnd, w!("wtgapfix")).is_null()
        {
            return;
        }
        let Some(&(dll, proc)) = LOADED.get() else {
            return;
        };
        let tid = GetWindowThreadProcessId(hwnd, null_mut());
        let hook = SetWindowsHookExW(WH_CALLWNDPROC, proc, dll as HMODULE, tid);
        if hook.is_null() {
            eprintln!("{hwnd:?}: SetWindowsHookExW failed");
            return;
        }
        SendMessageTimeoutW(hwnd, WM_NULL, 0, 0, SMTO_ABORTIFHUNG, 2000, null_mut());
        UnhookWindowsHookEx(hook);
        if GetPropW(hwnd, w!("wtgapfix")).is_null() {
            eprintln!("{hwnd:?}: fix did not load (elevated Terminal?)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::{
        Graphics::Gdi::{
            BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
            GdiFlush, SelectObject,
        },
        UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx},
    };

    /// Draw the real tray icon on magenta and read the pixels back.
    #[test]
    fn icon_draws_shape_with_transparent_corners() {
        unsafe {
            let icon = make_icon();
            assert!(!icon.is_null(), "CreateIcon failed");
            let mut bi: BITMAPINFO = zeroed();
            bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = 16;
            bi.bmiHeader.biHeight = -16; // top-down, so row 0 = ICON[0]
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            let mut bits = null_mut();
            let dc = CreateCompatibleDC(null_mut());
            let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            assert!(!bmp.is_null(), "CreateDIBSection failed");
            SelectObject(dc, bmp);
            std::slice::from_raw_parts_mut(bits as *mut u32, 256).fill(0xFF00FF);
            DrawIconEx(dc, 0, 0, icon, 16, 16, 0, null_mut(), DI_NORMAL);
            GdiFlush();
            let px = std::slice::from_raw_parts(bits as *const u32, 256);
            let at = |x: usize, y: usize| px[y * 16 + x] & 0xFF_FFFF;
            assert_eq!(at(0, 0), 0xFF00FF, "top-left corner transparent");
            assert_eq!(at(15, 15), 0xFF00FF, "bottom-right corner transparent");
            assert_eq!(at(1, 0), 0x0C0C0C, "dark square");
            assert_eq!(at(2, 4), 0xF2F2F2, "chevron top");
            assert_eq!(at(13, 11), 0xF2F2F2, "underscore end");
            assert_eq!(at(14, 11), 0x0C0C0C, "after underscore");
        }
    }
}
