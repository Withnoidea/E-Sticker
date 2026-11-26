use super::*;
use windows_sys::Win32::UI::Controls::{COMBOBOXINFO, GetComboBoxInfo};

unsafe fn paint_combo(hwnd: HWND, dc: HDC) {
    let mut area = zeroed();
    GetClientRect(hwnd, &mut area);
    let focused = GetFocus() == hwnd;
    let open = SendMessageW(hwnd, CB_GETDROPPEDSTATE, 0, 0) != 0;
    let mut cursor = zeroed();
    GetCursorPos(&mut cursor);
    ScreenToClient(hwnd, &mut cursor);
    let hover = inside(
        area,
        Point {
            x: cursor.x,
            y: cursor.y,
        },
    );
    fill(dc, area, SHELL);
    rounded(
        dc,
        rect(0, 0, area.right, area.bottom),
        7,
        if open { SOFT } else { WHITE },
        if focused || open {
            ACCENT
        } else if hover {
            0xb1bcad
        } else {
            BORDER
        },
    );
    text(
        dc,
        SendMessageW(hwnd, WM_GETFONT, 0, 0) as HFONT,
        &control_text(hwnd),
        rect(12, 0, area.right - 31, area.bottom),
        if IsWindowEnabled(hwnd) != 0 {
            INK
        } else {
            MUTED
        },
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    let center_x = area.right - 17;
    let center_y = area.bottom / 2;
    let direction = if open { -1 } else { 1 };
    let points = [
        POINT {
            x: center_x - 4,
            y: center_y - direction * 2,
        },
        POINT {
            x: center_x,
            y: center_y + direction * 2,
        },
        POINT {
            x: center_x + 4,
            y: center_y - direction * 2,
        },
    ];
    let pen = CreatePen(
        PS_SOLID,
        1,
        rgb(if focused || open { ACCENT } else { SECONDARY }),
    );
    let previous = SelectObject(dc, pen);
    Polyline(dc, points.as_ptr(), points.len() as i32);
    SelectObject(dc, previous);
    DeleteObject(pen);
}

unsafe extern "system" fn combo_proc(
    hwnd: HWND,
    event: u32,
    wp: WPARAM,
    lp: LPARAM,
    subclass: usize,
    _data: usize,
) -> LRESULT {
    match event {
        WM_PAINT => {
            let mut paint = zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            paint_combo(hwnd, dc);
            EndPaint(hwnd, &paint);
            return 0;
        }
        WM_PRINTCLIENT => {
            paint_combo(hwnd, wp as HDC);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_MOUSEMOVE => {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
            InvalidateRect(hwnd, null(), 0);
        }
        WM_MOUSELEAVE => {
            InvalidateRect(hwnd, null(), 0);
        }
        WM_SETFOCUS | WM_KILLFOCUS | CB_SETCURSEL | CB_SHOWDROPDOWN | CB_RESETCONTENT => {
            let result = DefSubclassProc(hwnd, event, wp, lp);
            InvalidateRect(hwnd, null(), 0);
            return result;
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(combo_proc), subclass);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, event, wp, lp)
}

pub(super) unsafe fn style_combo(hwnd: HWND) {
    windows_sys::Win32::UI::Controls::SetWindowTheme(hwnd, wide("").as_ptr(), wide("").as_ptr());
    SetWindowSubclass(hwnd, Some(combo_proc), 2, 0);
    SendMessageW(hwnd, CB_SETITEMHEIGHT, 0, 36);
    SendMessageW(hwnd, CB_SETITEMHEIGHT, usize::MAX, 30);
    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..zeroed()
    };
    if GetComboBoxInfo(hwnd, &mut info) != 0 {
        windows_sys::Win32::UI::Controls::SetWindowTheme(
            info.hwndList,
            wide("").as_ptr(),
            wide("").as_ptr(),
        );
        SetWindowSubclass(info.hwndList, Some(combo_list_proc), 3, 0);
    }
}

unsafe extern "system" fn combo_list_proc(
    hwnd: HWND,
    event: u32,
    wp: WPARAM,
    lp: LPARAM,
    subclass: usize,
    _data: usize,
) -> LRESULT {
    match event {
        WM_NCPAINT => {
            let result = DefSubclassProc(hwnd, event, wp, lp);
            let dc = GetWindowDC(hwnd);
            let mut bounds = zeroed();
            GetWindowRect(hwnd, &mut bounds);
            SetDCBrushColor(dc, rgb(BORDER));
            FrameRect(
                dc,
                &rect(0, 0, bounds.right - bounds.left, bounds.bottom - bounds.top),
                GetStockObject(DC_BRUSH),
            );
            ReleaseDC(hwnd, dc);
            return result;
        }
        WM_SIZE => {
            let result = DefSubclassProc(hwnd, event, wp, lp);
            let mut bounds = zeroed();
            GetWindowRect(hwnd, &mut bounds);
            let region = CreateRoundRectRgn(
                0,
                0,
                bounds.right - bounds.left + 1,
                bounds.bottom - bounds.top + 1,
                12,
                12,
            );
            if SetWindowRgn(hwnd, region, 1) == 0 {
                DeleteObject(region);
            }
            return result;
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(combo_list_proc), subclass);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, event, wp, lp)
}

#[repr(C, align(4))]
struct NoticeTemplate {
    dialog: DLGTEMPLATE,
    menu: u16,
    class: u16,
    title: u16,
}

struct Notice<'a> {
    body: &'a str,
    flags: u32,
    faces: [HFONT; 3],
    height: i32,
}

impl Notice<'_> {
    fn choice(&self) -> bool {
        self.flags & 0xf == MB_YESNO
    }
    fn danger(&self) -> bool {
        self.choice() && self.body.contains("删除")
    }
    fn color(&self) -> u32 {
        if self.danger() || self.flags & 0xf0 == MB_ICONERROR {
            0x9c5044
        } else if self.flags & 0xf0 == MB_ICONWARNING {
            0x9b7646
        } else {
            ACCENT
        }
    }
}

unsafe extern "system" fn notice_proc(hwnd: HWND, event: u32, wp: WPARAM, lp: LPARAM) -> isize {
    if event == WM_INITDIALOG {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, lp);
    }
    let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Notice<'_>;
    if data.is_null() {
        return 0;
    }
    let notice = &mut *data;
    match event {
        WM_INITDIALOG => {
            SetWindowTextW(hwnd, wide(WINDOW_TITLE).as_ptr());
            let width = 530;
            let dc = GetDC(hwnd);
            let body_height = text(
                dc,
                notice.faces[0],
                notice.body,
                rect(0, 0, width - 56, 0),
                INK,
                DT_CALCRECT | DT_WORDBREAK,
            )
            .max(40);
            ReleaseDC(hwnd, dc);
            let mut monitor = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..zeroed()
            };
            GetMonitorInfoW(
                MonitorFromWindow(GetParent(hwnd), MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            );
            notice.height =
                (body_height + 174).min((monitor.rcWork.bottom - monitor.rcWork.top - 90).max(240));
            let mut bounds = rect(0, 0, width, notice.height);
            let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            AdjustWindowRectEx(
                &mut bounds,
                style,
                0,
                GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
            );
            let outer_width = bounds.right - bounds.left;
            let outer_height = bounds.bottom - bounds.top;
            let mut owner = zeroed();
            GetWindowRect(GetParent(hwnd), &mut owner);
            let left = ((owner.left + owner.right - outer_width) / 2).clamp(
                monitor.rcWork.left,
                (monitor.rcWork.right - outer_width).max(monitor.rcWork.left),
            );
            let top = ((owner.top + owner.bottom - outer_height) / 2).clamp(
                monitor.rcWork.top,
                (monitor.rcWork.bottom - outer_height).max(monitor.rcWork.top),
            );
            SetWindowPos(
                hwnd,
                null_mut(),
                left,
                top,
                outer_width,
                outer_height,
                SWP_NOZORDER,
            );
            let title = if notice.choice() {
                "确认这次操作"
            } else if notice.flags & 0xf0 == MB_ICONERROR {
                "操作未完成"
            } else if notice.flags & 0xf0 == MB_ICONWARNING {
                "请留意"
            } else {
                "提示"
            };
            for (id, class, label, style, area, face) in [
                (
                    10,
                    "STATIC",
                    title,
                    0,
                    rect(74, 29, width - 28, 59),
                    notice.faces[1],
                ),
                (
                    11,
                    "EDIT",
                    notice.body,
                    ES_MULTILINE as u32
                        | ES_READONLY as u32
                        | ES_AUTOVSCROLL as u32
                        | if body_height > notice.height - 174 {
                            WS_VSCROLL
                        } else {
                            0
                        },
                    rect(28, 86, width - 28, notice.height - 88),
                    notice.faces[0],
                ),
            ] {
                let control = CreateWindowExW(
                    0,
                    wide(class).as_ptr(),
                    wide(label).as_ptr(),
                    WS_CHILD | WS_VISIBLE | style,
                    area.left,
                    area.top,
                    area.right - area.left,
                    area.bottom - area.top,
                    hwnd,
                    id as HMENU,
                    GetModuleHandleW(null()),
                    null(),
                );
                SendMessageW(control, WM_SETFONT, face as WPARAM, 0);
            }
            let primary = if notice.choice() { IDYES } else { IDOK };
            let action = if !notice.choice() {
                "知道了"
            } else if notice.body.contains("不可撤销") {
                "永久删除"
            } else if notice.body.contains("删除分类墙") {
                "删除墙面"
            } else {
                "确认"
            };
            let mut buttons = vec![(primary, action, width - 140)];
            if notice.choice() {
                buttons.insert(0, (IDNO, "取消", width - 240));
            }
            for (id, label, left) in buttons {
                let control = CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide(label).as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
                    left,
                    notice.height - 58,
                    if id == primary { 112 } else { 88 },
                    34,
                    hwnd,
                    id as HMENU,
                    GetModuleHandleW(null()),
                    null(),
                );
                SendMessageW(control, WM_SETFONT, notice.faces[0] as WPARAM, 0);
            }
            let default = if notice.choice() && notice.flags & MB_DEFBUTTON2 != 0 {
                IDNO
            } else {
                primary
            };
            SendMessageW(hwnd, DM_SETDEFID, default as WPARAM, 0);
            SetFocus(GetDlgItem(hwnd, default));
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            let mut paint = zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut area = zeroed();
            GetClientRect(hwnd, &mut area);
            fill(dc, area, WHITE);
            fill(
                dc,
                rect(0, notice.height - 80, area.right, notice.height),
                SHELL,
            );
            rounded(
                dc,
                rect(28, 26, 60, 58),
                10,
                mix(WHITE, notice.color(), 12),
                mix(WHITE, notice.color(), 12),
            );
            text(
                dc,
                notice.faces[1],
                if notice.flags & 0xf0 == MB_ICONERROR {
                    "×"
                } else {
                    "!"
                },
                rect(28, 25, 60, 58),
                notice.color(),
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
            text(
                dc,
                notice.faces[2],
                if notice.choice() {
                    "请确认后继续"
                } else {
                    "关闭提示后可继续操作"
                },
                rect(28, notice.height - 57, 280, notice.height - 25),
                SECONDARY,
                DT_SINGLELINE | DT_VCENTER,
            );
            EndPaint(hwnd, &paint);
            return 1;
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            let dc = wp as HDC;
            SetTextColor(dc, rgb(INK));
            SetBkColor(dc, rgb(WHITE));
            SetDCBrushColor(dc, rgb(WHITE));
            return GetStockObject(DC_BRUSH) as isize;
        }
        WM_DRAWITEM => {
            let item = &*(lp as *const DRAWITEMSTRUCT);
            let primary = item.CtlID as i32 == if notice.choice() { IDYES } else { IDOK };
            let focused = item.itemState & ODS_FOCUS != 0;
            let pressed = item.itemState & ODS_SELECTED != 0;
            let color = if notice.danger() {
                notice.color()
            } else {
                ACCENT
            };
            fill(item.hDC, item.rcItem, SHELL);
            rounded(
                item.hDC,
                item.rcItem,
                7,
                if primary {
                    if pressed { mix(color, INK, 18) } else { color }
                } else if pressed {
                    SOFT
                } else {
                    WHITE
                },
                if focused || primary { color } else { BORDER },
            );
            if focused {
                rounded(
                    item.hDC,
                    rect(3, 3, item.rcItem.right - 3, item.rcItem.bottom - 3),
                    4,
                    if primary { color } else { WHITE },
                    if primary {
                        mix(color, WHITE, 55)
                    } else {
                        color
                    },
                );
            }
            text(
                item.hDC,
                notice.faces[0],
                &control_text(item.hwndItem),
                item.rcItem,
                if primary { WHITE } else { INK },
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            return 1;
        }
        WM_COMMAND => {
            let id = (wp & 0xffff) as i32;
            if matches!(id, IDOK | IDYES | IDNO | IDCANCEL) {
                EndDialog(
                    hwnd,
                    if id == IDCANCEL {
                        if notice.choice() { IDNO } else { IDOK }
                    } else {
                        id
                    } as isize,
                );
                return 1;
            }
        }
        WM_CLOSE => {
            EndDialog(hwnd, if notice.choice() { IDNO } else { IDOK } as isize);
            return 1;
        }
        _ => {}
    }
    0
}

pub(super) unsafe fn notice(hwnd: HWND, body: &str, flags: u32) -> MESSAGEBOX_RESULT {
    if hwnd.is_null() || flags & 0xf != MB_OK && flags & 0xf != MB_YESNO {
        return MessageBoxW(
            hwnd,
            wide(body).as_ptr(),
            wide(WINDOW_TITLE).as_ptr(),
            flags,
        );
    }
    let mut data = Notice {
        body,
        flags,
        faces: [
            font("Microsoft YaHei UI", 15, 400),
            font("Microsoft YaHei UI", 20, 600),
            font("Microsoft YaHei UI", 12, 400),
        ],
        height: 240,
    };
    let template = NoticeTemplate {
        dialog: DLGTEMPLATE {
            style: WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME as u32,
            dwExtendedStyle: 0,
            cdit: 0,
            x: 0,
            y: 0,
            cx: 280,
            cy: 130,
        },
        menu: 0,
        class: 0,
        title: 0,
    };
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(null()),
        &template.dialog,
        hwnd,
        Some(notice_proc),
        &mut data as *mut Notice<'_> as LPARAM,
    );
    for face in data.faces {
        DeleteObject(face);
    }
    if result == -1 {
        MessageBoxW(
            hwnd,
            wide(body).as_ptr(),
            wide(WINDOW_TITLE).as_ptr(),
            flags,
        )
    } else {
        result as MESSAGEBOX_RESULT
    }
}
