use crate::App;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
    UI::{Shell::*, WindowsAndMessaging::*},
};

const CALLBACK: u32 = WM_APP + 1;
const OPEN: u32 = WM_APP + 2;
const CLASS: &str = "NCMOverlayControl.Tray";
static WINDOW: AtomicUsize = AtomicUsize::new(0);
static APP: OnceLock<Arc<App>> = OnceLock::new();
static RECREATE: AtomicUsize = AtomicUsize::new(0);
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn open_url(url: &str) -> Result<(), String> {
    let operation = wide("open");
    let url = wide(url);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        Err("无法打开默认浏览器".into())
    } else {
        Ok(())
    }
}
pub fn quit() {
    let window = WINDOW.load(Ordering::Relaxed);
    if window != 0 {
        unsafe {
            PostMessageW(window as HWND, WM_CLOSE, 0, 0);
        }
    }
}

pub struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn single_instance() -> Result<Option<Instance>, String> {
    unsafe {
        let name = wide("Local\\NCMOverlayControl");
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            return Err("无法创建程序实例".into());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            for _ in 0..40 {
                let window = FindWindowW(wide(CLASS).as_ptr(), std::ptr::null());
                if !window.is_null() {
                    PostMessageW(window, OPEN, 0, 0);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            return Ok(None);
        }
        Ok(Some(Instance(handle)))
    }
}

unsafe fn make_icon() -> HICON {
    let mut pixels = [0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let bg = (3..29).contains(&x) && (3..29).contains(&y);
            let note = ((14..17).contains(&x) && (8..23).contains(&y))
                || ((16..24).contains(&x) && (8..11).contains(&y))
                || ((x as i32 - 11).pow(2) + (y as i32 - 23).pow(2) <= 16);
            let i = (y * 32 + x) * 4;
            if bg {
                pixels[i..i + 4].copy_from_slice(&[44, 36, 34, 255]);
            }
            if note {
                pixels[i..i + 4].copy_from_slice(&[167, 201, 72, 255]);
            }
        }
    }
    CreateIcon(
        GetModuleHandleW(std::ptr::null()),
        32,
        32,
        1,
        32,
        [0u8; 128].as_ptr(),
        pixels.as_ptr(),
    )
}
unsafe fn icon_data(window: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: CALLBACK,
        hIcon: icon,
        ..Default::default()
    };
    let tip = wide("NCM Overlay Control");
    data.szTip[..tip.len()].copy_from_slice(&tip);
    data
}
unsafe fn open_settings() {
    if let Some(app) = APP.get() {
        let url = format!(
            "http://127.0.0.1:{}/settings",
            app.settings.lock().unwrap().web_port
        );
        let _ = open_url(&url);
    }
}
unsafe fn create_menu() -> HMENU {
    let menu = CreatePopupMenu();
    if !menu.is_null() {
        AppendMenuW(menu, MF_STRING, 1, wide("关于").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, 2, wide("退出").as_ptr());
    }
    menu
}
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == OPEN {
        open_settings();
        return 0;
    }
    if message == RECREATE.load(Ordering::Relaxed) as u32 {
        let icon = GetWindowLongPtrW(window, GWLP_USERDATA) as HICON;
        Shell_NotifyIconW(NIM_ADD, &icon_data(window, icon));
        return 0;
    }
    match message {
        CALLBACK => {
            match lparam as u32 {
                WM_LBUTTONUP => open_settings(),
                WM_RBUTTONUP => {
                    let menu = create_menu();
                    if !menu.is_null() {
                        let mut point = POINT::default();
                        GetCursorPos(&mut point);
                        SetForegroundWindow(window);
                        let choice = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON,
                            point.x,
                            point.y,
                            0,
                            window,
                            std::ptr::null(),
                        );
                        DestroyMenu(menu);
                        PostMessageW(window, WM_NULL, 0, 0);
                        match choice {
                            1 => {
                                let _ = open_url("https://github.com/XHZ-Steve");
                            }
                            2 => {
                                if let Some(app) = APP.get() {
                                    let app = app.clone();
                                    std::thread::spawn(move || app.shutdown());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            Shell_NotifyIconW(NIM_DELETE, &icon_data(window, std::ptr::null_mut()));
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            WINDOW.store(0, Ordering::Relaxed);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}
pub fn run(app: Arc<App>) -> Result<(), String> {
    APP.set(app).map_err(|_| "托盘已启动".to_string())?;
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let name = wide(CLASS);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            lpszClassName: name.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err("无法创建托盘窗口".into());
        }
        let window = CreateWindowExW(
            0,
            name.as_ptr(),
            name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        if window.is_null() {
            return Err("无法创建托盘窗口".into());
        }
        let icon = make_icon();
        SetWindowLongPtrW(window, GWLP_USERDATA, icon as isize);
        WINDOW.store(window as usize, Ordering::Relaxed);
        RECREATE.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) as usize,
            Ordering::Relaxed,
        );
        if Shell_NotifyIconW(NIM_ADD, &icon_data(window, icon)) == 0 {
            DestroyIcon(icon);
            DestroyWindow(window);
            return Err("无法添加托盘图标".into());
        }
        let mut message = MSG::default();
        if APP
            .get()
            .is_some_and(|app| app.settings.lock().unwrap().show_settings)
        {
            PostMessageW(window, OPEN, 0, 0);
        }
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        DestroyIcon(icon);
    }
    Ok(())
}

pub fn fatal(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an interactive Explorer desktop"]
    fn native_tray_lifecycle() {
        use crate::{Event, Settings, Shared};
        use serde_json::Value;
        use std::sync::atomic::AtomicBool;
        use std::sync::{mpsc, Mutex};
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            if let Ok(Event::Stop(reply)) = rx.recv() {
                let _ = reply.send(());
            }
        });
        let server = Arc::new(tiny_http::Server::http(("127.0.0.1", 0)).unwrap());
        let app = Arc::new(App {
            settings: Mutex::new(Settings {
                show_settings: false,
                ..Settings::default()
            }),
            tx,
            shared: Arc::new(Shared {
                state: Mutex::new(Value::Null),
                listeners: Mutex::new(Vec::new()),
                active: AtomicUsize::new(0),
            }),
            token: Arc::new("test".into()),
            listener: Mutex::new(server),
            stopping: AtomicBool::new(false),
            ncm_busy: AtomicBool::new(false),
            headless: false,
        });
        let cloned = app.clone();
        let thread = std::thread::spawn(move || run(cloned));
        let mut window = std::ptr::null_mut();
        let mut found = false;
        for _ in 0..100 {
            window = WINDOW.load(Ordering::Relaxed) as HWND;
            if !window.is_null() {
                unsafe {
                    let id = NOTIFYICONIDENTIFIER {
                        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
                        hWnd: window,
                        uID: 1,
                        ..Default::default()
                    };
                    let mut rect = RECT::default();
                    if Shell_NotifyIconGetRect(&id, &mut rect) == 0 {
                        found = true;
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        unsafe {
            assert!(found, "Tray icon must be registered with Explorer");
            assert_eq!(
                IsWindowVisible(window),
                0,
                "No application window may be shown"
            );
            let menu = create_menu();
            assert!(!menu.is_null());
            let mut text = [0u16; 16];
            GetMenuStringW(menu, 1, text.as_mut_ptr(), 16, MF_BYCOMMAND);
            assert_eq!(
                String::from_utf16_lossy(&text).trim_end_matches('\0'),
                "关于"
            );
            GetMenuStringW(menu, 2, text.as_mut_ptr(), 16, MF_BYCOMMAND);
            assert_eq!(
                String::from_utf16_lossy(&text).trim_end_matches('\0'),
                "退出"
            );
            DestroyMenu(menu);
        }
        app.shutdown();
        assert!(thread.join().unwrap().is_ok());
        worker.join().unwrap();
        unsafe {
            let id = NOTIFYICONIDENTIFIER {
                cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: window,
                uID: 1,
                ..Default::default()
            };
            assert_ne!(
                Shell_NotifyIconGetRect(&id, &mut RECT::default()),
                0,
                "Exit must remove tray icon"
            );
        }
    }
}
