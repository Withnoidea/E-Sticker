#![allow(unsafe_op_in_unsafe_fn)]

use crate::{
    theme::*,
    ui::{fill, inside, mouse, rect, rgb, rounded, text},
};
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};

pub const HEIGHT: i32 = 44;
pub const MINIMIZE: usize = 111;
pub const MAXIMIZE: usize = 112;
pub const CLOSE: usize = 113;
const BUTTON_WIDTH: i32 = 46;

pub fn button_area(width: i32, id: usize) -> RECT {
    let right = width - 5 - (CLOSE - id) as i32 * BUTTON_WIDTH;
    rect(right - BUTTON_WIDTH, 5, right, HEIGHT - 1)
}

pub unsafe fn hit_test(hwnd: HWND, lp: LPARAM) -> LRESULT {
    let screen = mouse(lp);
    let mut point = POINT {
        x: screen.x,
        y: screen.y,
    };
    ScreenToClient(hwnd, &mut point);
    let mut bounds = zeroed::<RECT>();
    GetClientRect(hwnd, &mut bounds);
    if IsZoomed(hwnd) == 0 {
        let padding = GetSystemMetrics(SM_CXPADDEDBORDER);
        let horizontal = (GetSystemMetrics(SM_CXSIZEFRAME) + padding).clamp(4, 10);
        let vertical = (GetSystemMetrics(SM_CYSIZEFRAME) + padding).clamp(4, 10);
        let left = point.x < horizontal;
        let right = point.x >= bounds.right - horizontal;
        let top = point.y < vertical;
        let bottom = point.y >= bounds.bottom - vertical;
        match (left, right, top, bottom) {
            (true, _, true, _) => return HTTOPLEFT as LRESULT,
            (_, true, true, _) => return HTTOPRIGHT as LRESULT,
            (true, _, _, true) => return HTBOTTOMLEFT as LRESULT,
            (_, true, _, true) => return HTBOTTOMRIGHT as LRESULT,
            (true, _, _, _) => return HTLEFT as LRESULT,
            (_, true, _, _) => return HTRIGHT as LRESULT,
            (_, _, true, _) => return HTTOP as LRESULT,
            (_, _, _, true) => return HTBOTTOM as LRESULT,
            _ => {}
        }
    }
    if point.y < HEIGHT
        && !(MINIMIZE..=CLOSE).any(|id| {
            inside(
                button_area(bounds.right, id),
                crate::model::Point {
                    x: point.x,
                    y: point.y,
                },
            )
        })
    {
        HTCAPTION as LRESULT
    } else {
        HTCLIENT as LRESULT
    }
}

pub unsafe fn client_frame(hwnd: HWND, lp: LPARAM) {
    if IsZoomed(hwnd) == 0 {
        return;
    }
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..zeroed()
    };
    if GetMonitorInfoW(monitor, &mut info) != 0 {
        let proposed = &mut *(lp as *mut RECT);
        let work = info.rcWork;
        proposed.left = proposed.left.max(work.left);
        proposed.top = proposed.top.max(work.top);
        proposed.right = proposed.right.min(work.right);
        proposed.bottom = proposed.bottom.min(work.bottom);
    }
}

pub unsafe fn command(hwnd: HWND, id: usize) {
    let action = match id {
        MINIMIZE => SC_MINIMIZE,
        MAXIMIZE if IsZoomed(hwnd) != 0 => SC_RESTORE,
        MAXIMIZE => SC_MAXIMIZE,
        CLOSE => SC_CLOSE,
        _ => return,
    };
    PostMessageW(hwnd, WM_SYSCOMMAND, action as WPARAM, 0);
}

pub unsafe fn paint(dc: HDC, width: i32, brand_font: HFONT, meta_font: HFONT) {
    fill(dc, rect(0, 0, width, HEIGHT), SHELL);
    fill(dc, rect(0, HEIGHT - 1, width, HEIGHT), BORDER);
    rounded(dc, rect(19, 10, 43, 34), BUTTON_RADIUS, ACCENT, ACCENT);
    let points = [
        POINT { x: 25, y: 15 },
        POINT { x: 37, y: 15 },
        POINT { x: 37, y: 23 },
        POINT { x: 32, y: 29 },
        POINT { x: 25, y: 29 },
    ];
    let brush = CreateSolidBrush(rgb(WHITE));
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
    Polygon(dc, points.as_ptr(), points.len() as i32);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
    fill(dc, rect(28, 19, 34, 20), ACCENT);
    fill(dc, rect(28, 22, 32, 23), ACCENT);
    text(
        dc,
        brand_font,
        BRAND,
        rect(53, 0, 130, HEIGHT),
        INK,
        DT_SINGLELINE | DT_VCENTER,
    );
    text(
        dc,
        meta_font,
        BRAND_ZH,
        rect(132, 1, 229, HEIGHT),
        SECONDARY,
        DT_SINGLELINE | DT_VCENTER,
    );
}

pub unsafe fn glyph(dc: HDC, area: RECT, id: usize, maximized: bool, color: u32) {
    let x = (area.left + area.right) / 2;
    let y = (area.top + area.bottom) / 2;
    let pen = CreatePen(PS_SOLID, 1, rgb(color));
    let old_pen = SelectObject(dc, pen);
    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    let line = |x1, y1, x2, y2| {
        MoveToEx(dc, x1, y1, null_mut());
        LineTo(dc, x2, y2);
    };
    match id {
        MINIMIZE => line(x - 5, y, x + 6, y),
        MAXIMIZE if maximized => {
            line(x - 2, y - 6, x + 6, y - 6);
            line(x + 5, y - 6, x + 5, y + 3);
            Rectangle(dc, x - 5, y - 3, x + 3, y + 5);
        }
        MAXIMIZE => {
            Rectangle(dc, x - 5, y - 5, x + 6, y + 6);
        }
        CLOSE => {
            line(x - 5, y - 5, x + 6, y + 6);
            line(x + 5, y - 5, x - 6, y + 6);
        }
        _ => {}
    }
    SelectObject(dc, old_brush);
    SelectObject(dc, old_pen);
    DeleteObject(pen);
}
