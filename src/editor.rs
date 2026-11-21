use std::{cell::RefCell, fs, path::Path, rc::Rc};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    Win32::Foundation::{HWND, RECT},
    core::{Interface, PCWSTR, PWSTR},
};

include!(concat!(env!("OUT_DIR"), "/editor_assets.rs"));

pub const EVENT: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 41;

pub fn decode_image(data: &str) -> std::io::Result<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CRYPT_STRING_BASE64, CryptStringToBinaryW};
    if data.len() > 28 * 1024 * 1024 || !data.is_ascii() {
        return Err(std::io::Error::other("图片编码无效或超过 20 MiB"));
    }
    let text = wide(data);
    let mut size = 0;
    unsafe {
        if CryptStringToBinaryW(
            text.as_ptr(),
            data.len() as u32,
            CRYPT_STRING_BASE64,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::other("图片编码无效"));
        }
        if u64::from(size) > crate::assets::MAX_IMAGE_BYTES {
            return Err(std::io::Error::other("图片超过 20 MiB"));
        }
        let mut bytes = vec![0; size as usize];
        if CryptStringToBinaryW(
            text.as_ptr(),
            data.len() as u32,
            CRYPT_STRING_BASE64,
            bytes.as_mut_ptr(),
            &mut size,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::other("无法解码图片"));
        }
        bytes.truncate(size as usize);
        Ok(bytes)
    }
}

#[derive(Default)]
struct State {
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    events: Vec<serde_json::Value>,
    open: Option<serde_json::Value>,
    ready: bool,
    bounds: RECT,
    visible: bool,
    directory: Option<String>,
    closed: bool,
}

pub struct Editor {
    state: Rc<RefCell<State>>,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn notify(state: &Rc<RefCell<State>>, hwnd: HWND, event: serde_json::Value) {
    if state.borrow().closed {
        return;
    }
    state.borrow_mut().events.push(event);
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd.0, EVENT, 0, 0);
    }
}

fn send(webview: &ICoreWebView2, value: &serde_json::Value) -> windows::core::Result<()> {
    unsafe { webview.PostWebMessageAsJson(PCWSTR(wide(&value.to_string()).as_ptr())) }
}

fn extract(root: &Path) -> std::io::Result<()> {
    for (name, bytes) in FILES {
        let path = root.join(name);
        if fs::read(&path).ok().as_deref() != Some(*bytes) {
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, bytes)?;
        }
    }
    Ok(())
}

impl Editor {
    pub fn new(
        hwnd: windows_sys::Win32::Foundation::HWND,
        base: &Path,
    ) -> std::result::Result<Self, String> {
        let root = base.join("editor-runtime").join(env!("CARGO_PKG_VERSION"));
        extract(&root).map_err(|error| error.to_string())?;
        let profile = base.join("webview2-profile");
        fs::create_dir_all(&profile).map_err(|error| error.to_string())?;
        let state = Rc::new(RefCell::new(State::default()));
        let result = Self {
            state: state.clone(),
        };
        let parent = HWND(hwnd);
        let folder = root.to_string_lossy().into_owned();
        let profile = wide(&profile.to_string_lossy());
        let environment_handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |status, environment| {
                if state.borrow().closed {
                    return Ok(());
                }
                let environment = match status
                    .and_then(|()| environment.ok_or_else(windows::core::Error::from_thread))
                {
                    Ok(environment) => environment,
                    Err(error) => {
                        notify(
                            &state,
                            parent,
                            serde_json::json!({"type":"error", "message":format!("无法启动 WebView2，请安装 Microsoft Edge WebView2 Evergreen Runtime。{error}")}),
                        );
                        return Ok(());
                    }
                };
                let inner = state.clone();
                let folder = folder.clone();
                let controller_environment = environment.clone();
                let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                    move |status, controller| {
                        if inner.borrow().closed {
                            if let Some(controller) = controller {
                                unsafe {
                                    let _ = controller.Close();
                                }
                            }
                            return Ok(());
                        }
                        let setup = (|| -> windows::core::Result<()> {
                            status?;
                            let controller =
                                controller.ok_or_else(windows::core::Error::from_thread)?;
                            let webview = unsafe { controller.CoreWebView2()? };
                            unsafe {
                                let settings = webview.Settings()?;
                                settings.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
                                settings.SetIsStatusBarEnabled(false)?;
                                settings.SetAreDefaultScriptDialogsEnabled(false)?;
                                settings.SetAreHostObjectsAllowed(false)?;
                                webview
                                    .cast::<ICoreWebView2_3>()?
                                    .SetVirtualHostNameToFolderMapping(
                                        PCWSTR(wide("editor.esticker.local").as_ptr()),
                                        PCWSTR(wide(&folder).as_ptr()),
                                        COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS,
                                    )?;
                                if let Some(directory) = &inner.borrow().directory {
                                    webview
                                        .cast::<ICoreWebView2_3>()?
                                        .SetVirtualHostNameToFolderMapping(
                                            PCWSTR(wide("notes.esticker.local").as_ptr()),
                                            PCWSTR(wide(directory).as_ptr()),
                                            COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
                                        )?;
                                }
                                let queue = inner.clone();
                                let mut token = 0;
                                webview.add_WebMessageReceived(
                                    &WebMessageReceivedEventHandler::create(Box::new(
                                        move |sender, args| {
                                            if let Some(args) = args {
                                                let mut source = PWSTR::null();
                                                args.Source(&mut source)?;
                                                if CoTaskMemPWSTR::from(source).to_string()
                                                    != "https://editor.esticker.local/index.html"
                                                {
                                                    return Ok(());
                                                }
                                                let mut text = PWSTR::null();
                                                args.WebMessageAsJson(&mut text)?;
                                                let text = CoTaskMemPWSTR::from(text).to_string();
                                                if text.len() > 29 * 1024 * 1024 {
                                                    return Ok(());
                                                }
                                                if let Ok(event) =
                                                    serde_json::from_str::<serde_json::Value>(&text)
                                                {
                                                    if event["type"] == "ready" {
                                                        queue.borrow_mut().ready = true;
                                                        let open = queue.borrow().open.clone();
                                                        if let (Some(sender), Some(open)) =
                                                            (sender, open)
                                                        {
                                                            send(&sender, &open)?;
                                                        }
                                                    } else {
                                                        notify(&queue, parent, event);
                                                    }
                                                }
                                            }
                                            Ok(())
                                        },
                                    )),
                                    &mut token,
                                )?;
                                webview.add_NavigationStarting(
                                    &NavigationStartingEventHandler::create(Box::new(
                                        move |_, args| {
                                            if let Some(args) = args {
                                                let mut uri = PWSTR::null();
                                                args.Uri(&mut uri)?;
                                                if CoTaskMemPWSTR::from(uri).to_string()
                                                    != "https://editor.esticker.local/index.html"
                                                {
                                                    args.SetCancel(true)?;
                                                }
                                            }
                                            Ok(())
                                        },
                                    )),
                                    &mut token,
                                )?;
                                webview.add_NewWindowRequested(
                                    &NewWindowRequestedEventHandler::create(Box::new(
                                        move |_, args| {
                                            if let Some(args) = args {
                                                args.SetHandled(true)?;
                                            }
                                            Ok(())
                                        },
                                    )),
                                    &mut token,
                                )?;
                                let failures = inner.clone();
                                webview.add_ProcessFailed(&ProcessFailedEventHandler::create(Box::new(move |_, _| {
                                    notify(&failures, parent, serde_json::json!({"type":"error", "message":"正文渲染进程异常退出，已同步的内容仍在本地。请保留窗口并备份数据。"}));
                                    Ok(())
                                })), &mut token)?;
                                webview.add_PermissionRequested(
                                    &PermissionRequestedEventHandler::create(Box::new(
                                        move |_, args| {
                                            if let Some(args) = args {
                                                args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                                            }
                                            Ok(())
                                        },
                                    )),
                                    &mut token,
                                )?;
                                webview.AddWebResourceRequestedFilter(
                                    PCWSTR(wide("*").as_ptr()),
                                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                                )?;
                                let resource_environment = controller_environment.clone();
                                webview.add_WebResourceRequested(
                                    &WebResourceRequestedEventHandler::create(Box::new(
                                        move |_, args| {
                                            if let Some(args) = args {
                                                let request = args.Request()?;
                                                let mut uri = PWSTR::null();
                                                request.Uri(&mut uri)?;
                                                let uri = CoTaskMemPWSTR::from(uri).to_string();
                                                let local = uri
                                                    .starts_with("https://editor.esticker.local/");
                                                let mut context =
                                                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
                                                args.ResourceContext(&mut context)?;
                                                let image = uri
                                                    .starts_with("https://notes.esticker.local/")
                                                    && context
                                                        == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE;
                                                if !local && !image {
                                                    let response = resource_environment
                                                        .CreateWebResourceResponse(
                                                            None,
                                                            403,
                                                            PCWSTR(wide("Offline editor").as_ptr()),
                                                            PCWSTR::null(),
                                                        )?;
                                                    args.SetResponse(&response)?;
                                                }
                                            }
                                            Ok(())
                                        },
                                    )),
                                    &mut token,
                                )?;
                                controller.SetBounds(inner.borrow().bounds)?;
                                controller.SetIsVisible(inner.borrow().visible)?;
                                webview.Navigate(PCWSTR(
                                    wide("https://editor.esticker.local/index.html").as_ptr(),
                                ))?;
                            }
                            let mut state = inner.borrow_mut();
                            state.controller = Some(controller);
                            state.webview = Some(webview);
                            Ok(())
                        })();
                        if let Err(error) = setup {
                            notify(
                                &inner,
                                parent,
                                serde_json::json!({"type":"error", "message":error.to_string()}),
                            );
                        }
                        Ok(())
                    },
                ));
                if let Err(error) =
                    unsafe { environment.CreateCoreWebView2Controller(parent, &handler) }
                {
                    notify(
                        &state,
                        parent,
                        serde_json::json!({"type":"error", "message":error.to_string()}),
                    );
                }
                Ok(())
            },
        ));
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(profile.as_ptr()),
                None,
                &environment_handler,
            )
            .map_err(|error| {
                format!(
                    "无法启动 WebView2，请安装 Microsoft Edge WebView2 Evergreen Runtime。{error}"
                )
            })?;
        }
        Ok(result)
    }

    pub fn open(&self, data: serde_json::Value, directory: &Path) -> windows::core::Result<()> {
        let mut state = self.state.borrow_mut();
        state.open = Some(data.clone());
        state.directory = Some(directory.to_string_lossy().into_owned());
        if let Some(webview) = &state.webview {
            unsafe {
                webview
                    .cast::<ICoreWebView2_3>()?
                    .SetVirtualHostNameToFolderMapping(
                        PCWSTR(wide("notes.esticker.local").as_ptr()),
                        PCWSTR(wide(&directory.to_string_lossy()).as_ptr()),
                        COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
                    )?;
            }
            if state.ready {
                send(webview, &data)?;
            }
        }
        Ok(())
    }

    pub fn post(&self, data: serde_json::Value) -> windows::core::Result<()> {
        if let Some(webview) = &self.state.borrow().webview {
            send(webview, &data)?;
        }
        Ok(())
    }

    pub fn export_pdf(&self, path: &Path, session: u64) -> windows::core::Result<()> {
        let webview = self
            .state
            .borrow()
            .webview
            .clone()
            .ok_or_else(windows::core::Error::from_thread)?;
        let state = Rc::downgrade(&self.state);
        let destination = path.to_string_lossy().into_owned();
        let handler = PrintToPdfCompletedHandler::create(Box::new(move |status, successful| {
            if let Some(state) = state.upgrade()
                && !state.borrow().closed
                && let Some(webview) = state.borrow().webview.clone()
            {
                let response = match status {
                    Ok(()) if successful => {
                        serde_json::json!({"type":"pdf-result", "session":session, "path":destination})
                    }
                    Ok(()) => {
                        serde_json::json!({"type":"pdf-result", "session":session, "error":"无法写入 PDF，请检查目标文件是否被占用及目录权限"})
                    }
                    Err(error) => {
                        serde_json::json!({"type":"pdf-result", "session":session, "error":error.to_string()})
                    }
                };
                let _ = send(&webview, &response);
            }
            Ok(())
        }));
        unsafe {
            webview.cast::<ICoreWebView2_7>()?.PrintToPdf(
                PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
                None,
                &handler,
            )
        }
    }

    pub fn layout(&self, bounds: windows_sys::Win32::Foundation::RECT, visible: bool) {
        let mut state = self.state.borrow_mut();
        state.bounds = RECT {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        state.visible = visible;
        if let Some(controller) = &state.controller {
            unsafe {
                let _ = controller.SetBounds(state.bounds);
                let _ = controller.SetIsVisible(visible);
            }
        }
    }

    pub fn drain(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.state.borrow_mut().events)
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        let controller = {
            let mut state = self.state.borrow_mut();
            state.closed = true;
            state.webview = None;
            state.controller.take()
        };
        if let Some(controller) = controller {
            unsafe {
                let _ = controller.Close();
            }
        }
    }
}
