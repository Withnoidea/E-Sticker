#![allow(unsafe_op_in_unsafe_fn)]

use crate::model::{
    self, Draft, NOTE_H, NOTE_W, Paper, Point, Preferences, Sort, WALL_H, WALL_W, Wall,
};
use crate::{assets, chrome, clipboard, theme::*};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs, io,
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, EM_GETLINECOUNT, EM_GETSEL, EM_LIMITTEXT, EM_REPLACESEL, EM_SCROLLCARET,
    EM_SETCUEBANNER, EM_SETRECTNP, EM_SETSEL, ODS_COMBOBOXEDIT, ODS_FOCUS, ODS_SELECTED,
    ODT_BUTTON, WM_MOUSELEAVE,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER,
            OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
            OPENFILENAMEW,
        },
        Input::KeyboardAndMouse::*,
        Shell::{
            DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, HDROP,
            RemoveWindowSubclass, SetWindowSubclass, ShellExecuteW,
        },
        WindowsAndMessaging::*,
    },
};

const PAPER_TEXT: u32 = DT_WORDBREAK | DT_EDITCONTROL;
const TOP: i32 = 154;
const EDGE: i32 = 22;
const ROW: i32 = 88;
const NEW: usize = 101;
const VIEW: usize = 102;
const SEARCH: usize = 103;
const SORT: usize = 104;
const TITLE: usize = 105;
const BODY: usize = 106;
const SAVE: usize = 107;
const CANCEL: usize = 108;
const PREVIEW: usize = 109;
const DELETE: usize = 110;
const SETTINGS: usize = 114;
const BACKGROUND: usize = 115;
const WALL_SCROLL: usize = 116;
const DIRECTORY: usize = 117;
const MARKDOWN_SHORTCUTS: usize = 140;
const TRASH: usize = 143;
const PAPER_TEXTURE: usize = 144;
const DEFAULT_PAPER: usize = 145;
const PIN: usize = 141;
const ARCHIVE: usize = 142;
const SETTINGS_SAVE: usize = 118;
const SETTINGS_BACK: usize = 119;
const ARTICLE: usize = 120;
const OUTLINE: usize = 146;
const FOCUS: usize = 147;
const IMPORT_IMAGE: usize = 148;
const TAGS: usize = 149;
const TAG_FILTER: usize = 150;
const EDITOR_SAVE: usize = 151;
const EDITOR_CLOSE: usize = 152;
const EDITOR_TIMEOUT: usize = 57;
const BODY_LIMIT: usize = 100_000;
const AUTOSAVE_TIMER: usize = 56;
const TOOLBAR: [(usize, &str, &str); 19] = [
    (121, "H1", "# "),
    (122, "H2", "## "),
    (123, "H3", "### "),
    (132, "H4", "#### "),
    (133, "H5", "##### "),
    (134, "H6", "###### "),
    (124, "粗体", "**"),
    (125, "斜体", "*"),
    (135, "删除线", "~~"),
    (126, "列表", "- "),
    (136, "任务", "- [ ] "),
    (127, "序号", "1. "),
    (128, "引用", "> "),
    (129, "代码", "`"),
    (137, "代码块", "```"),
    (
        138,
        "表格",
        "| 列1 | 列2 |\n| --- | --- |\n| 内容 | 内容 |\n",
    ),
    (130, "链接", "[]()"),
    (139, "图片", "![alt](path)"),
    (131, "分割线", "\n---\n"),
];
const BACKGROUNDS: [(&str, u32); 4] = [
    ("米色点阵", WALL),
    ("暖米白", SHELL),
    ("浅鼠尾草绿", SOFT),
    ("晨雾蓝", 0xe9eff2),
];
const BODY_TOP: i32 = TOP + 252;
const SORT_LABELS: [&str; 3] = ["最近编辑", "创建时间", "按短句排序"];
const MOTION_TIMER: usize = 55;
const HOVER_MS: f64 = 150.0;

thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }
thread_local! { static TITLE_BACKGROUND: Cell<u32> = const { Cell::new(0xffe9a1) }; }
thread_local! { static LAST_FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) }; }

struct Frame {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    width: i32,
    height: i32,
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
pub(crate) fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}
pub(crate) fn inside(area: RECT, point: Point) -> bool {
    (area.left..area.right).contains(&point.x) && (area.top..area.bottom).contains(&point.y)
}
pub(crate) fn mouse(value: LPARAM) -> Point {
    Point {
        x: value as i16 as i32,
        y: (value >> 16) as i16 as i32,
    }
}
pub(crate) fn rgb(hex: u32) -> u32 {
    ((hex & 255) << 16) | (hex & 0xff00) | (hex >> 16)
}

#[derive(Clone)]
struct Editing {
    id: Option<u64>,
    paper: Paper,
    article: bool,
    mode: u8,
    scroll: i32,
    focused: bool,
}

impl Editing {
    fn body_top(&self) -> i32 {
        if self.focused { TOP + 124 } else { BODY_TOP }
    }
}

struct Drag {
    id: u64,
    start: Point,
    original: Point,
    at: Point,
    moved: bool,
}

struct App {
    hwnd: HWND,
    path: PathBuf,
    data_lock: fs::File,
    preferences_path: PathBuf,
    preferences: Preferences,
    settings: Option<Preferences>,
    editor: Option<crate::editor::Editor>,
    editor_session: u64,
    editor_loaded: bool,
    editor_replaying: bool,
    pending_editor_action: Option<usize>,
    body_dirty: bool,
    tags_dirty: bool,
    wall: Wall,
    controls: BTreeMap<usize, HWND>,
    fonts: [HFONT; 6],
    preview: RefCell<crate::preview::Preview>,
    query: String,
    available_tags: Vec<String>,
    selected_tag: Option<String>,
    sort: Sort,
    bucket: u8,
    list: bool,
    list_scroll: i32,
    offset: Point,
    placement: Option<Draft>,
    hover: Option<Point>,
    drag: Option<Drag>,
    editing: Option<Editing>,
    status: String,
    wheel_remainder: i32,
    cursor_tracking: bool,
    pointer: Option<Point>,
    row_hover: Hover<u64>,
    button_hover: Hover<usize>,
    transition: Option<(Instant, u64)>,
    animate: bool,
}

struct Hover<T> {
    current: Option<T>,
    previous: Option<T>,
    start: Option<Instant>,
}

impl<T: Copy + Eq> Hover<T> {
    fn new() -> Self {
        Self {
            current: None,
            previous: None,
            start: None,
        }
    }

    fn set(&mut self, next: Option<T>) -> bool {
        if self.current == next {
            return false;
        }
        self.previous = self.current;
        self.current = next;
        self.start = Some(Instant::now());
        true
    }

    fn active(&self) -> bool {
        self.start
            .is_some_and(|start| start.elapsed().as_secs_f64() * 1000.0 < HOVER_MS)
    }

    fn amount(&self, id: T, animate: bool) -> u32 {
        let progress = if animate {
            self.start.map_or(1.0, |start| {
                (start.elapsed().as_secs_f64() * 1000.0 / HOVER_MS).clamp(0.0, 1.0)
            })
        } else {
            1.0
        };
        if self.current == Some(id) {
            (100.0 * progress) as u32
        } else if self.previous == Some(id) {
            (100.0 * (1.0 - progress)) as u32
        } else {
            0
        }
    }
}

unsafe fn system_motion() -> bool {
    let mut enabled = 0i32;
    SystemParametersInfoW(
        SPI_GETCLIENTAREAANIMATION,
        0,
        (&mut enabled as *mut i32).cast(),
        0,
    ) != 0
        && enabled != 0
}

fn swatch(area: RECT, index: usize) -> RECT {
    let left = area.left + 228 + index as i32 * 52;
    rect(left, TOP + 91, left + 44, TOP + 121)
}

fn editor_paper(area: RECT) -> RECT {
    rect(
        area.left + 26,
        TOP + 36,
        area.left + 26 + NOTE_W,
        TOP + 36 + NOTE_H,
    )
}

fn toolbar_width(label: &str) -> i32 {
    if label.chars().count() > 2 { 54 } else { 43 }
}

fn paper_text(area: RECT) -> RECT {
    rect(
        area.left + 13,
        area.top + 23,
        area.right - 13,
        area.bottom - 18,
    )
}

unsafe fn font(family: &str, height: i32, weight: i32) -> HFONT {
    CreateFontW(
        -height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        0,
        0,
        CLEARTYPE_QUALITY as u32,
        0,
        wide(family).as_ptr(),
    )
}

unsafe fn paper_font(hwnd: HWND) -> HFONT {
    let handwriting = font("KaiTi", 17, 400);
    if !handwriting.is_null() {
        let dc = GetDC(hwnd);
        if !dc.is_null() {
            let previous = SelectObject(dc, handwriting);
            let mut face = [0u16; LF_FACESIZE as usize];
            let length = if previous.is_null() {
                0
            } else {
                let length = GetTextFaceW(dc, face.len() as i32, face.as_mut_ptr());
                SelectObject(dc, previous);
                length
            };
            ReleaseDC(hwnd, dc);
            let family = face.split(|unit| *unit == 0).next().unwrap_or_default();
            if length > 0
                && (String::from_utf16_lossy(family).eq_ignore_ascii_case("KaiTi")
                    || family == [0x6977, 0x4f53])
            {
                return handwriting;
            }
        }
        DeleteObject(handwriting);
    }
    font("Microsoft YaHei UI", 17, 400)
}

pub(crate) unsafe fn fill(dc: HDC, area: RECT, color: u32) {
    let brush = CreateSolidBrush(rgb(color));
    FillRect(dc, &area, brush);
    DeleteObject(brush);
}

pub(crate) unsafe fn rounded(dc: HDC, area: RECT, radius: i32, background: u32, border: u32) {
    let brush = CreateSolidBrush(rgb(background));
    let pen = CreatePen(PS_SOLID, 1, rgb(border));
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, pen);
    RoundRect(
        dc,
        area.left,
        area.top,
        area.right,
        area.bottom,
        radius * 2,
        radius * 2,
    );
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);
}

fn mix(base: u32, tint: u32, strength: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |color, shift| {
        let channel = |value: u32| (value >> shift) & 255u32;
        color | ((channel(base) * (100 - strength) + channel(tint) * strength) / 100) << shift
    })
}

unsafe fn gradient(dc: HDC, area: RECT, top: u32, bottom: u32) {
    let vertex = |x, y, color: u32| TRIVERTEX {
        x,
        y,
        Red: ((color >> 16) as u8 as u16) << 8,
        Green: ((color >> 8) as u8 as u16) << 8,
        Blue: ((color as u8) as u16) << 8,
        Alpha: 0,
    };
    let colors = [
        vertex(area.left, area.top, top),
        vertex(area.right, area.bottom, bottom),
    ];
    let mesh = GRADIENT_RECT {
        UpperLeft: 0,
        LowerRight: 1,
    };
    GradientFill(
        dc,
        colors.as_ptr(),
        2,
        (&mesh as *const GRADIENT_RECT).cast(),
        1,
        GRADIENT_FILL_RECT_V,
    );
}

unsafe fn sheet(dc: HDC, area: RECT, paper: Paper, flying: bool, backdrop: u32, texture: bool) {
    let (color, edge) = paper.colors();
    let drop = if flying { 7 } else { 2 };
    for layer in (0..4).rev() {
        let shadow = rect(
            area.left + 2 - layer,
            area.top + drop,
            area.right + drop + layer,
            area.bottom + drop + layer,
        );
        rounded(
            dc,
            shadow,
            7,
            mix(backdrop, 0x766044, 16 - layer as u32 * 3),
            mix(backdrop, 0x766044, 16 - layer as u32 * 3),
        );
    }
    fill(dc, area, color);
    if !texture {
        return;
    }
    gradient(
        dc,
        rect(area.left, area.top, area.right, area.top + 19),
        mix(color, edge, 41),
        color,
    );
    fill(
        dc,
        rect(area.left + 1, area.top, area.right, area.top + 1),
        mix(color, 0xffffff, 45),
    );
    gradient(
        dc,
        rect(area.left, area.bottom - 13, area.right, area.bottom),
        color,
        mix(color, 0xffffff, 33),
    );
    fill(
        dc,
        rect(area.right - 1, area.top + 1, area.right, area.bottom),
        mix(color, edge, 38),
    );
    fill(
        dc,
        rect(area.left + 3, area.bottom - 1, area.right - 1, area.bottom),
        mix(color, edge, 34),
    );
}

pub(crate) unsafe fn text(
    dc: HDC,
    font: HFONT,
    label: &str,
    mut area: RECT,
    color: u32,
    flags: u32,
) -> i32 {
    let prior = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, rgb(color));
    let value = wide(label);
    let height = DrawTextW(
        dc,
        value.as_ptr(),
        value.len() as i32 - 1,
        &mut area,
        flags | DT_NOPREFIX,
    );
    SelectObject(dc, prior);
    height
}

unsafe fn message(hwnd: HWND, body: &str, flags: u32) -> MESSAGEBOX_RESULT {
    let focused = GetFocus();
    let result = MessageBoxW(
        hwnd,
        wide(body).as_ptr(),
        wide(WINDOW_TITLE).as_ptr(),
        flags,
    );
    if !hwnd.is_null() {
        if !focused.is_null()
            && IsWindow(focused) != 0
            && (focused == hwnd || IsChild(hwnd, focused) != 0)
        {
            SetFocus(focused);
        }
        RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
    }
    result
}

unsafe fn control_text(handle: HWND) -> String {
    let mut value = vec![0; GetWindowTextLengthW(handle) as usize + 1];
    let count = GetWindowTextW(handle, value.as_mut_ptr(), value.len() as i32);
    String::from_utf16_lossy(&value[..count as usize])
}

unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    let id = item.CtlID as usize;
    let primary = matches!(id, NEW | SAVE | SETTINGS_SAVE);
    let pressed = item.itemState & ODS_SELECTED != 0;
    let focused = item.itemState & ODS_FOCUS != 0;
    let hover = APP.with(|slot| {
        slot.try_borrow()
            .ok()
            .and_then(|state| {
                state
                    .as_ref()
                    .map(|app| app.button_hover.amount(id, app.animate))
            })
            .unwrap_or(0)
    });
    if (chrome::MINIMIZE..=chrome::CLOSE).contains(&id) {
        let level = if pressed { 100 } else { hover };
        let close = id == chrome::CLOSE;
        let background = mix(SHELL, if close { 0xc42b1c } else { 0xece7dd }, level);
        fill(item.hDC, item.rcItem, background);
        if focused {
            DrawFocusRect(item.hDC, &item.rcItem);
        }
        chrome::glyph(
            item.hDC,
            item.rcItem,
            id,
            IsZoomed(GetParent(item.hwndItem)) != 0,
            if close && level >= 65 { 0xffffff } else { INK },
        );
        return;
    }
    let toolbar = (121..=139).contains(&id);
    let theme_color = if id == BACKGROUND {
        APP.with(|slot| {
            slot.try_borrow()
                .ok()
                .and_then(|state| {
                    state.as_ref().and_then(|app| {
                        app.settings.as_ref().map(|settings| {
                            BACKGROUNDS[settings.background.min(BACKGROUNDS.len() - 1)].1
                        })
                    })
                })
                .unwrap_or(WHITE)
        })
    } else {
        WHITE
    };
    let base = if primary {
        mix(ACCENT, 0x354a3d, hover)
    } else if id == BACKGROUND {
        mix(theme_color, WHITE, hover / 3)
    } else {
        mix(WHITE, SOFT, hover)
    };
    let background = if pressed { mix(base, INK, 13) } else { base };
    let edge = if toolbar {
        ACCENT
    } else if focused {
        0x9b7646
    } else if primary {
        ACCENT
    } else {
        BORDER
    };
    let color = if primary {
        WHITE
    } else if id == DELETE {
        0x974d40
    } else {
        INK
    };
    fill(
        item.hDC,
        item.rcItem,
        if matches!(id, NEW | VIEW | SETTINGS) {
            SHELL
        } else {
            WHITE
        },
    );
    rounded(item.hDC, item.rcItem, BUTTON_RADIUS, background, edge);
    let face = SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HFONT;
    text(
        item.hDC,
        face,
        &control_text(item.hwndItem),
        item.rcItem,
        color,
        DT_SINGLELINE | DT_CENTER | DT_VCENTER,
    );
}

unsafe fn draw_sort(item: &DRAWITEMSTRUCT) {
    let collapsed = item.itemState & ODS_COMBOBOXEDIT != 0;
    let selected = item.itemState & ODS_SELECTED != 0;
    fill(
        item.hDC,
        item.rcItem,
        if selected && !collapsed { SOFT } else { WHITE },
    );
    let label = if item.CtlID as usize == SORT {
        SORT_LABELS
            .get(item.itemID as usize)
            .copied()
            .unwrap_or("")
            .to_owned()
    } else if collapsed {
        control_text(item.hwndItem)
    } else {
        let length =
            SendMessageW(item.hwndItem, CB_GETLBTEXTLEN, item.itemID as usize, 0).max(0) as usize;
        let mut value = vec![0; length + 1];
        let written = SendMessageW(
            item.hwndItem,
            CB_GETLBTEXT,
            item.itemID as usize,
            value.as_mut_ptr() as LPARAM,
        )
        .max(0) as usize;
        String::from_utf16_lossy(&value[..written.min(length)])
    };
    let face = SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HFONT;
    let mut area = item.rcItem;
    area.left += 9;
    text(
        item.hDC,
        face,
        &label,
        area,
        INK,
        DT_SINGLELINE | DT_VCENTER,
    );
    if item.itemState & ODS_FOCUS != 0 {
        DrawFocusRect(item.hDC, &item.rcItem);
    }
}

unsafe extern "system" fn child_hover(
    hwnd: HWND,
    event: u32,
    wp: WPARAM,
    lp: LPARAM,
    subclass: usize,
    id: usize,
) -> LRESULT {
    match event {
        WM_DROPFILES if id == BODY => {
            let drop_handle = wp as HDROP;
            let paths = dropped_paths(drop_handle);
            DragFinish(drop_handle);
            with_app(|app| app.import_images(&paths));
            return 0;
        }
        WM_PASTE if id == BODY => {
            let mut handled = false;
            with_app(|app| handled = app.paste_image());
            if handled {
                return 0;
            }
        }
        WM_MOUSEMOVE => {
            with_app(|app| app.hover_button(Some(id)));
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
        }
        WM_MOUSELEAVE => with_app(|app| {
            if app.button_hover.current == Some(id) {
                app.hover_button(None);
            }
        }),
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(child_hover), subclass);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, event, wp, lp)
}

unsafe fn dropped_paths(drop_handle: HDROP) -> Vec<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let count = DragQueryFileW(drop_handle, u32::MAX, null_mut(), 0);
    (0..count.min(32))
        .filter_map(|index| {
            let length = DragQueryFileW(drop_handle, index, null_mut(), 0);
            let mut units = vec![0; length as usize + 1];
            let written = DragQueryFileW(drop_handle, index, units.as_mut_ptr(), length + 1);
            (written != 0).then(|| PathBuf::from(OsString::from_wide(&units[..written as usize])))
        })
        .collect()
}

unsafe fn choose_image(hwnd: HWND) -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let filter = wide("图片\0*.png;*.jpg;*.jpeg;*.gif;*.bmp\0所有文件\0*.*\0");
    let title = wide("导入本地图片");
    let mut path = vec![0; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR,
        ..zeroed()
    };
    if GetOpenFileNameW(&mut dialog) == 0 {
        return None;
    }
    let end = path.iter().position(|unit| *unit == 0)?;
    Some(PathBuf::from(OsString::from_wide(&path[..end])))
}

unsafe fn choose_pdf(hwnd: HWND) -> io::Result<Option<PathBuf>> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let filter = wide("PDF 文档\0*.pdf\0");
    let title = wide("导出 PDF");
    let extension = wide("pdf");
    let mut path = vec![0; 32768];
    let filename = wide("E-Sticker.pdf");
    path[..filename.len()].copy_from_slice(&filename);
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: extension.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..zeroed()
    };
    if GetSaveFileNameW(&mut dialog) == 0 {
        let error = CommDlgExtendedError();
        return if error == 0 {
            Ok(None)
        } else {
            Err(io::Error::other(format!("无法打开保存对话框：{error}")))
        };
    }
    let end = path
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(path.len());
    Ok(Some(PathBuf::from(OsString::from_wide(&path[..end]))))
}

impl App {
    unsafe fn paper(
        &self,
        dc: HDC,
        at: Point,
        title: &str,
        paper: Paper,
        flying: bool,
        size: Point,
    ) {
        let left = EDGE + at.x - self.offset.x;
        let top = TOP + at.y - self.offset.y;
        let right = left + size.x.clamp(136, 280);
        let bottom = top + size.y.clamp(112, 240);
        let area = rect(left, top, right, bottom);
        sheet(
            dc,
            area,
            paper,
            flying,
            self.wall_color(),
            self.preferences.paper_texture,
        );
        text(dc, self.fonts[2], title, paper_text(area), INK, PAPER_TEXT);
    }

    unsafe fn draw_wall(&self, dc: HDC) {
        let area = self.canvas();
        fill(dc, area, BORDER);
        let saved = SaveDC(dc);
        let wall = self.wall_area();
        IntersectClipRect(
            dc,
            wall.left + 1,
            wall.top + 1,
            wall.right - 1,
            wall.bottom - 1,
        );
        fill(dc, wall, self.wall_color());
        if self.preferences.background == 0 {
            let brush = CreateSolidBrush(rgb(0xdad0c1));
            for ypos in ((wall.top + 9 - self.offset.y.rem_euclid(17))..wall.bottom).step_by(17) {
                for xpos in ((wall.left + 9 - self.offset.x.rem_euclid(17))..wall.right).step_by(17)
                {
                    FillRect(dc, &rect(xpos, ypos, xpos + 1, ypos + 1), brush);
                }
            }
            DeleteObject(brush);
        }
        let moving = self.drag.as_ref().filter(|drag| drag.moved);
        for note in self.wall.notes.iter().filter(|note| {
            !note.archived && !note.deleted && !moving.is_some_and(|drag| drag.id == note.id)
        }) {
            let xpos = EDGE + note.at.x - self.offset.x;
            let ypos = TOP + note.at.y - self.offset.y;
            if xpos + NOTE_W + 8 >= wall.left
                && xpos < wall.right
                && ypos + NOTE_H + 8 >= wall.top
                && ypos < wall.bottom
            {
                self.paper(
                    dc,
                    note.at,
                    &note.title,
                    note.paper,
                    false,
                    Point {
                        x: note.width,
                        y: note.height,
                    },
                );
            }
        }
        if let Some(drag) = moving
            && let Some(note) = self.wall.notes.iter().find(|note| note.id == drag.id)
        {
            self.paper(
                dc,
                drag.at,
                &note.title,
                note.paper,
                true,
                Point {
                    x: note.width,
                    y: note.height,
                },
            );
        }
        if let Some(draft) = &self.placement
            && let Some(point) = self.hover
            && inside(wall, point)
        {
            self.paper(
                dc,
                self.placement_at(point),
                &draft.title,
                draft.paper,
                true,
                Point {
                    x: NOTE_W,
                    y: NOTE_H,
                },
            );
        } else if !self
            .wall
            .notes
            .iter()
            .any(|note| !note.archived && !note.deleted)
            && self.placement.is_none()
        {
            let middle = (wall.top + wall.bottom) / 2;
            let center = (wall.left + wall.right) / 2;
            fill(
                dc,
                rect(center - 20, middle - 74, center + 24, middle - 33),
                0xd4dacb,
            );
            fill(
                dc,
                rect(center - 23, middle - 77, center + 20, middle - 36),
                0xffe9a1,
            );
            fill(
                dc,
                rect(center - 23, middle - 77, center + 20, middle - 69),
                0xead17e,
            );
            text(
                dc,
                self.fonts[4],
                "这里还没有便签",
                rect(wall.left, middle - 18, wall.right, middle + 9),
                INK,
                DT_CENTER | DT_SINGLELINE,
            );
            text(
                dc,
                self.fonts[0],
                "新建一张，写一句，再亲手贴到墙上。",
                rect(wall.left, middle + 16, wall.right, middle + 44),
                MUTED,
                DT_CENTER | DT_SINGLELINE,
            );
        }
        RestoreDC(dc, saved);
    }

    unsafe fn draw_list(&self, dc: HDC) {
        let inner = self.list_area();
        let saved = SaveDC(dc);
        IntersectClipRect(dc, inner.left, inner.top, inner.right, inner.bottom);
        let rows = self.rows();
        if rows.is_empty() {
            text(
                dc,
                self.fonts[4],
                if self.query.trim().is_empty() && self.selected_tag.is_none() {
                    "还没有便签"
                } else {
                    "没找到匹配的便签"
                },
                rect(inner.left, inner.top + 47, inner.right, inner.top + 78),
                INK,
                DT_CENTER | DT_SINGLELINE,
            );
            text(
                dc,
                self.fonts[0],
                if self.query.trim().is_empty() && self.selected_tag.is_none() {
                    "新建一张便签，它也会出现在这里。"
                } else {
                    "试试其他关键词，或清空筛选。"
                },
                rect(inner.left, inner.top + 84, inner.right, inner.top + 111),
                SECONDARY,
                DT_CENTER | DT_SINGLELINE,
            );
        }
        for (index, note) in rows
            .iter()
            .enumerate()
            .skip((self.list_scroll / ROW) as usize)
        {
            let card = self.row_area(index);
            let top = card.top;
            if top >= inner.bottom {
                break;
            }
            let hover = self.row_hover.amount(note.id, self.animate);
            rounded(
                dc,
                card,
                CARD_RADIUS,
                mix(WHITE, SOFT, hover * 52 / 100),
                mix(BORDER, ACCENT, hover * 18 / 100),
            );
            let (color, _) = note.paper.colors();
            rounded(
                dc,
                rect(card.left + 16, top + 17, card.left + 23, top + 59),
                3,
                color,
                color,
            );
            text(
                dc,
                self.fonts[4],
                &format!(
                    "{}{}",
                    if note.pinned { "置顶 · " } else { "" },
                    note.title.replace(['\r', '\n'], " ")
                ),
                rect(card.left + 36, top + 12, card.right - 157, top + 38),
                INK,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            text(
                dc,
                self.fonts[3],
                &model::date(note.edited),
                rect(card.right - 148, top + 17, card.right - 16, top + 39),
                MUTED,
                DT_RIGHT | DT_SINGLELINE,
            );
            if !note.tags.is_empty() {
                text(
                    dc,
                    self.fonts[3],
                    &note
                        .tags
                        .iter()
                        .map(|tag| format!("#{tag}"))
                        .collect::<Vec<_>>()
                        .join("  "),
                    rect(card.left + 36, top + 39, card.right - 20, top + 55),
                    ACCENT,
                    DT_SINGLELINE | DT_END_ELLIPSIS,
                );
            }
            text(
                dc,
                self.fonts[3],
                &if note.body.trim().is_empty() {
                    "还没有正文，点击补写 Markdown。".into()
                } else {
                    model::excerpt(&note.body, &self.query, 90)
                },
                rect(card.left + 36, top + 56, card.right - 20, top + 75),
                SECONDARY,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
        RestoreDC(dc, saved);
    }

    unsafe fn markdown(&self, dc: HDC, left: i32, top: i32, right: i32, draw: bool) -> i32 {
        self.preview.borrow_mut().render(
            dc,
            &self.value(BODY),
            self.path.parent().unwrap(),
            rect(left, top, right, self.editor_area().bottom - 149),
            draw,
        )
    }

    unsafe fn draw_editor(&self, dc: HDC, edit: &Editing) {
        let area = self.editor_area();
        let left = area.left + 24;
        let right = area.right - 24;
        rounded(dc, area, PANEL_RADIUS, WHITE, BORDER);
        if edit.focused || edit.article {
            text(
                dc,
                self.fonts[4],
                &self.value(TITLE).replace(['\r', '\n'], " "),
                rect(left, TOP + 15, right - 298, TOP + 45),
                INK,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        } else {
            let sheet_area = editor_paper(area);
            sheet(
                dc,
                sheet_area,
                edit.paper,
                false,
                WHITE,
                self.preferences.paper_texture,
            );
            text(
                dc,
                self.fonts[3],
                "墙面短句",
                rect(left + 2, TOP + 13, right, TOP + 35),
                INK,
                DT_SINGLELINE,
            );
            text(
                dc,
                self.fonts[4],
                "直接写在纸上",
                rect(left + 204, TOP + 34, right, TOP + 60),
                INK,
                DT_SINGLELINE,
            );
            let used = self.value(TITLE).encode_utf16().count();
            text(
                dc,
                self.fonts[3],
                &format!("换行和字号贴上后不变  ·  {used}/48"),
                rect(left + 204, TOP + 63, right, TOP + 88),
                MUTED,
                DT_SINGLELINE,
            );
            for (index, paper) in Paper::ALL.into_iter().enumerate() {
                let spot = swatch(area, index);
                let (color, _) = paper.colors();
                rounded(
                    dc,
                    spot,
                    9,
                    color,
                    if edit.paper == paper {
                        ACCENT
                    } else {
                        mix(color, INK, 13)
                    },
                );
                text(
                    dc,
                    self.fonts[3],
                    paper.name(),
                    spot,
                    INK,
                    DT_SINGLELINE | DT_CENTER | DT_VCENTER,
                );
            }
        }
        if let Some(note) = edit
            .id
            .and_then(|id| self.wall.notes.iter().find(|note| note.id == id))
        {
            if edit.article && self.editor.is_none() {
                if !edit.focused {
                    text(
                        dc,
                        self.fonts[0],
                        "详细内容  ·  Markdown",
                        rect(left + 204, TOP + 138, right, TOP + 168),
                        INK,
                        DT_SINGLELINE,
                    );
                }
                if edit.mode != 0 {
                    let preview_left = if edit.mode == 1 {
                        left + (right - left) / 2 + 12
                    } else {
                        left
                    };
                    let preview = rect(preview_left, edit.body_top(), right, area.bottom - 149);
                    fill(dc, preview, WALL);
                    let state = SaveDC(dc);
                    IntersectClipRect(
                        dc,
                        preview.left + 1,
                        preview.top + 1,
                        preview.right - 1,
                        preview.bottom - 1,
                    );
                    self.markdown(
                        dc,
                        preview_left + 12,
                        preview.top + 9 - edit.scroll,
                        right - 12,
                        true,
                    );
                    RestoreDC(dc, state);
                }
            }
            let dates = format!(
                "创建 {}   ·   最近编辑 {}",
                model::date(note.created),
                model::date(note.edited)
            );
            if !edit.article {
                text(
                    dc,
                    self.fonts[3],
                    "标签 · 用逗号分隔",
                    rect(left + 204, TOP + 178, right, TOP + 198),
                    MUTED,
                    DT_SINGLELINE,
                );
            }
            let body = self.value(BODY);
            let characters = body.chars().count();
            let minutes = characters.div_ceil(400);
            let save_label = if self.body_dirty || self.tags_dirty {
                "正在保存…"
            } else if self.status.is_empty() {
                "自动保存已开启"
            } else {
                &self.status
            };
            text(
                dc,
                self.fonts[3],
                &format!(
                    "{characters} 字  ·  预计阅读 {} 分钟  ·  {save_label}",
                    minutes.max(1)
                ),
                rect(left, area.bottom - 133, right, area.bottom - 115),
                MUTED,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            text(
                dc,
                self.fonts[3],
                &dates,
                rect(left, area.bottom - 108, right, area.bottom - 90),
                MUTED,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        } else {
            text(
                dc,
                self.fonts[0],
                "先写一句。想写日记或复盘，可以贴好后再加正文。",
                rect(left + 204, TOP + 136, right, TOP + 185),
                MUTED,
                DT_WORDBREAK,
            );
        }
    }

    unsafe fn draw_settings(&self, dc: HDC, settings: &Preferences) {
        if self.editor.is_some() {
            let area = self.editor_area();
            rounded(dc, area, PANEL_RADIUS, WHITE, BORDER);
            text(
                dc,
                self.fonts[3],
                "本地存储目录 · 更换目录会复制便签及图片，不覆盖已有数据",
                rect(
                    area.left + 24,
                    area.bottom - 112,
                    area.right - 24,
                    area.bottom - 90,
                ),
                SECONDARY,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            return;
        }
        let area = self.editor_area();
        let left = area.left + 24;
        let right = area.right - 24;
        rounded(dc, area, PANEL_RADIUS, WHITE, BORDER);
        rounded(
            dc,
            rect(left - 1, TOP + 360, right + 1, TOP + 393),
            BUTTON_RADIUS,
            WHITE,
            BORDER,
        );
        let entries = [
            ("墙面背景", "选择便签墙的颜色与纹理。", 13),
            ("扩展墙面滚动", "开启后可用滚轮在更大的墙面上移动。", 75),
            (
                "Markdown 快捷键",
                "启用 Ctrl+B / I / K 和 Tab；Ctrl+S 始终可保存。",
                137,
            ),
            ("默认纸色", "新便签使用此颜色，已有便签不受影响。", 199),
            ("纸张质感", "开启纸边渐变，关闭后使用纯色纸面。", 261),
            (
                "本地存储目录",
                "输入绝对目录；保存后会复制便签，不覆盖已有数据。",
                323,
            ),
        ];
        for (name, description, top) in entries {
            text(
                dc,
                self.fonts[0],
                name,
                rect(left, TOP + top, right - 236, TOP + top + 20),
                INK,
                DT_SINGLELINE,
            );
            text(
                dc,
                self.fonts[3],
                description,
                rect(left, TOP + top + 22, right - 166, TOP + top + 42),
                SECONDARY,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
        let preview = rect(right - 224, TOP + 20, right - 166, TOP + 54);
        rounded(
            dc,
            preview,
            BUTTON_RADIUS,
            BACKGROUNDS[settings.background.min(BACKGROUNDS.len() - 1)].1,
            BORDER,
        );
        if settings.background == 0 {
            let brush = CreateSolidBrush(rgb(0xdad0c1));
            for ypos in ((preview.top + 6)..preview.bottom - 4).step_by(10) {
                for xpos in ((preview.left + 6)..preview.right - 4).step_by(10) {
                    FillRect(dc, &rect(xpos, ypos, xpos + 1, ypos + 1), brush);
                }
            }
            DeleteObject(brush);
        }
        for top in [65, 127, 189, 251, 313] {
            fill(dc, rect(left, TOP + top, right, TOP + top + 1), BORDER);
        }
    }

    unsafe fn paint(&self, dc: HDC) {
        let area = self.client();
        fill(dc, area, SHELL);
        let count = self.wall.rows_in("", self.sort, self.bucket).len();
        chrome::paint(dc, area.right, self.fonts[5], self.fonts[3]);
        text(
            dc,
            self.fonts[3],
            TAGLINE,
            rect(EDGE, 58, area.right - EDGE, 88),
            SECONDARY,
            DT_SINGLELINE | DT_VCENTER,
        );
        if self.settings.is_none() && self.editing.is_none() && self.placement.is_none() {
            let search = self.search_area();
            rounded(
                dc,
                search,
                BUTTON_RADIUS,
                WHITE,
                if GetFocus() == self.control(SEARCH) {
                    ACCENT
                } else {
                    BORDER
                },
            );
            let pen = CreatePen(PS_SOLID, 1, rgb(MUTED));
            let previous = SelectObject(dc, pen);
            let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            Ellipse(
                dc,
                search.left + 11,
                search.top + 10,
                search.left + 21,
                search.top + 20,
            );
            MoveToEx(dc, search.left + 19, search.top + 18, null_mut());
            LineTo(dc, search.left + 24, search.top + 23);
            SelectObject(dc, brush);
            SelectObject(dc, previous);
            DeleteObject(pen);
        }
        let title = if self.settings.is_some() {
            "设置"
        } else if let Some(edit) = &self.editing {
            if edit.id.is_some() {
                "便签详情"
            } else {
                "写一张便签"
            }
        } else if self.placement.is_some() {
            "找个位置贴上"
        } else if !self.query.trim().is_empty() || self.selected_tag.is_some() {
            "\u{7b5b}\u{9009}\u{7ed3}\u{679c}"
        } else if self.view_list() {
            match self.bucket {
                1 => "归档便签",
                2 => "回收站",
                _ => "文章列表",
            }
        } else {
            "我的便签墙"
        };
        text(
            dc,
            self.fonts[1],
            title,
            rect(EDGE, 104, area.right - EDGE, 144),
            INK,
            DT_SINGLELINE,
        );
        if self.placement.is_some() {
            text(
                dc,
                self.fonts[0],
                "单击贴住  ·  Esc 返回输入",
                rect(EDGE + 220, 117, area.right - EDGE, 143),
                ACCENT,
                DT_SINGLELINE,
            );
        }
        if let Some(settings) = &self.settings {
            self.draw_settings(dc, settings);
        } else if let Some(edit) = &self.editing {
            self.draw_editor(dc, edit);
        } else if self.view_list() {
            self.draw_list(dc);
        } else {
            self.draw_wall(dc);
        }
        let hint = if self.settings.is_some() {
            "Esc 返回  ·  保存后生效"
        } else if self.editing.is_some() {
            "Enter 换行  ·  Ctrl+Enter 确认  ·  Esc 取消"
        } else if self.placement.is_some() {
            "移动纸张预览位置  ·  可以贴在另一张纸上"
        } else if self.view_list() {
            "点击打开详情  ·  列表排序不影响墙面"
        } else if self.preferences.scroll_wall {
            "单击看详情  ·  拖动移开  ·  滚轮纵向／Shift+滚轮横向"
        } else {
            "单击看详情  ·  拖动移开"
        };
        text(
            dc,
            self.fonts[3],
            hint,
            rect(EDGE, area.bottom - 31, area.right - 270, area.bottom - 7),
            MUTED,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        let status = if self.status.is_empty() {
            if self.view_list() && (!self.query.trim().is_empty() || self.selected_tag.is_some()) {
                format!("{} 张匹配", self.rows().len())
            } else {
                format!("{count} 张便签  ·  本机存储")
            }
        } else {
            self.status.clone()
        };
        text(
            dc,
            self.fonts[3],
            &status,
            rect(
                area.right - 260,
                area.bottom - 31,
                area.right - EDGE,
                area.bottom - 7,
            ),
            ACCENT,
            DT_RIGHT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
    }
}

unsafe fn with_app(action: impl FnOnce(&mut App)) {
    APP.with(|slot| {
        if let Ok(mut state) = slot.try_borrow_mut()
            && let Some(app) = state.as_mut()
        {
            action(app);
        }
    });
}

unsafe extern "system" fn procedure(hwnd: HWND, event: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match event {
        WM_NCCALCSIZE => {
            chrome::client_frame(hwnd, lp);
            return 0;
        }
        WM_NCHITTEST => return chrome::hit_test(hwnd, lp),
        WM_NCACTIVATE => return DefWindowProcW(hwnd, event, wp, -1),
        WM_GETMINMAXINFO => {
            let info = &mut *(lp as *mut MINMAXINFO);
            info.ptMinTrackSize.x = 780;
            info.ptMinTrackSize.y = 700;
        }
        WM_SIZE => {
            if wp != SIZE_MINIMIZED as WPARAM {
                with_app(|app| app.resize());
            }
        }
        WM_ERASEBKGND => return 1,
        WM_DRAWITEM => {
            let item = &*(lp as *const DRAWITEMSTRUCT);
            if matches!(item.CtlID as usize, SORT | TAG_FILTER) {
                draw_sort(item);
                return 1;
            }
            if item.CtlType == ODT_BUTTON {
                draw_button(item);
                return 1;
            }
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            let id = GetDlgCtrlID(lp as HWND) as usize;
            if matches!(id, TITLE | SEARCH | BODY | DIRECTORY | TAGS) {
                let background = if id == TITLE {
                    TITLE_BACKGROUND.with(Cell::get)
                } else if matches!(id, BODY | TAGS) {
                    WALL
                } else {
                    WHITE
                };
                let dc = wp as HDC;
                SetTextColor(dc, rgb(INK));
                SetBkColor(dc, rgb(background));
                SetDCBrushColor(dc, rgb(background));
                return GetStockObject(DC_BRUSH) as LRESULT;
            }
        }
        WM_PAINT => {
            let mut paint = zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut painted = false;
            with_app(|app| {
                let area = app.client();
                if area.right <= 0 || area.bottom <= 0 {
                    return;
                }
                let buffer = CreateCompatibleDC(dc);
                let bitmap = CreateCompatibleBitmap(dc, area.right, area.bottom);
                if buffer.is_null() || bitmap.is_null() {
                    app.paint(dc);
                    if !bitmap.is_null() {
                        DeleteObject(bitmap);
                    }
                    if !buffer.is_null() {
                        DeleteDC(buffer);
                    }
                } else {
                    let previous = SelectObject(buffer, bitmap);
                    app.paint(buffer);
                    BitBlt(dc, 0, 0, area.right, area.bottom, buffer, 0, 0, SRCCOPY);
                    LAST_FRAME.with(|frame| {
                        *frame.borrow_mut() = Some(Frame {
                            dc: buffer,
                            bitmap,
                            previous,
                            width: area.right,
                            height: area.bottom,
                        })
                    });
                }
                painted = true;
            });
            if !painted {
                let mut area = zeroed();
                GetClientRect(hwnd, &mut area);
                fill(dc, area, SHELL);
                LAST_FRAME.with(|frame| {
                    if let Some(frame) = frame.borrow().as_ref() {
                        BitBlt(
                            dc,
                            0,
                            0,
                            area.right.min(frame.width),
                            area.bottom.min(frame.height),
                            frame.dc,
                            0,
                            0,
                            SRCCOPY,
                        );
                    }
                });
            }
            EndPaint(hwnd, &paint);
        }
        WM_COMMAND if wp & 0xffff == IMPORT_IMAGE && (wp >> 16) as u32 & 0xffff == BN_CLICKED => {
            if let Some(path) = choose_image(hwnd) {
                with_app(|app| app.import_images(&[path]));
            }
        }
        WM_COMMAND => with_app(|app| app.command(wp & 0xffff, (wp >> 16) as u32 & 0xffff)),
        WM_LBUTTONDOWN => with_app(|app| app.down(mouse(lp))),
        WM_LBUTTONUP => with_app(|app| app.up(mouse(lp))),
        WM_MOUSEMOVE => with_app(|app| app.motion(mouse(lp))),
        WM_MOUSELEAVE => with_app(|app| {
            app.cursor_tracking = false;
            app.pointer = None;
            if app.row_hover.set(None) {
                app.start_motion();
            }
            if app.hover.take().is_some() {
                app.redraw();
            }
        }),
        WM_TIMER if wp == MOTION_TIMER => with_app(|app| app.tick_motion()),
        WM_TIMER if wp == AUTOSAVE_TIMER => with_app(|app| {
            KillTimer(hwnd, AUTOSAVE_TIMER);
            app.save_body();
        }),
        crate::editor::EVENT => with_app(|app| app.editor_events()),
        WM_TIMER if wp == EDITOR_TIMEOUT => with_app(|app| {
            KillTimer(hwnd, EDITOR_TIMEOUT);
            if app.pending_editor_action.take().is_some() {
                app.status = "编辑器尚未完成同步，未关闭或覆盖正文。请稍后重试。".into();
                app.redraw();
                message(hwnd, &app.status, MB_ICONWARNING);
            }
        }),
        WM_CLOSE => {
            let mut saved = true;
            with_app(|app| {
                if app.defer_editor_action(EDITOR_CLOSE) {
                    saved = false;
                } else {
                    saved = app.save_body();
                }
            });
            if !saved {
                return 0;
            }
            return DefWindowProcW(hwnd, event, wp, lp);
        }
        WM_SETTINGCHANGE => with_app(|app| {
            app.animate = system_motion();
            app.tick_motion();
        }),
        WM_CAPTURECHANGED => with_app(|app| {
            if app.drag.take().is_some() {
                app.redraw();
            }
        }),
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => with_app(|app| {
            let delta = (wp >> 16) as i16 as i32;
            app.wheel_remainder += if event == WM_MOUSEHWHEEL {
                delta
            } else {
                -delta
            };
            let steps = app.wheel_remainder / 120;
            app.wheel_remainder %= 120;
            if steps != 0 {
                app.scroll(steps * 85, event == WM_MOUSEHWHEEL || wp & 4 != 0);
            }
        }),
        WM_DESTROY => {
            with_app(|app| {
                app.save_body();
            });
            PostQuitMessage(0);
        }
        _ => return DefWindowProcW(hwnd, event, wp, lp),
    }
    0
}

pub fn run() {
    unsafe {
        if let Err(error) = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        )
        .ok()
        {
            message(
                null_mut(),
                &format!("无法初始化正文组件：{error}"),
                MB_ICONERROR,
            );
            return;
        }
        let wall = model::app_dir().and_then(|base| {
            let preferences_path = base.join("settings.json");
            let settings_lock = model::lock(&preferences_path)?;
            let preferences = Preferences::read(&preferences_path)?;
            let legacy = std::env::var_os("LOCALAPPDATA")
                .filter(|_| std::env::var_os("ESTICKER_DATA_DIR").is_none())
                .map(|directory| PathBuf::from(directory).join("Paperwall"));
            let path = model::data_path(&base, &preferences, legacy.as_deref())?;
            let lock = model::lock(&path)?;
            Wall::open(&path).map(|wall| {
                (
                    path,
                    wall,
                    lock,
                    settings_lock,
                    preferences_path,
                    preferences,
                )
            })
        });
        let (path, wall, data_lock, _settings_lock, preferences_path, preferences) = match wall {
            Ok(data) => data,
            Err(error) => {
                message(
                    null_mut(),
                    &format!("无法读取本地便签，原文件未改动。\n{error}"),
                    MB_ICONERROR,
                );
                return;
            }
        };
        let instance = GetModuleHandleW(null());
        let class = wide("EStickerWindow");
        let description = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
            ..zeroed()
        };
        if RegisterClassW(&description) == 0 {
            message(null_mut(), "无法注册窗口。", MB_ICONERROR);
            return;
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide(WINDOW_TITLE).as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1040,
            740,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            message(null_mut(), "无法打开窗口。", MB_ICONERROR);
            return;
        }
        let app = App::new(hwnd, path, wall, data_lock, preferences_path, preferences);
        APP.with(|slot| *slot.borrow_mut() = Some(app));
        ShowWindow(hwnd, SW_SHOW);
        let mut event = zeroed::<MSG>();
        while GetMessageW(&mut event, null_mut(), 0, 0) > 0 {
            let mut handled = false;
            if event.message == WM_KEYDOWN {
                with_app(|app| {
                    let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
                    let combo_open = [SORT, TAG_FILTER]
                        .iter()
                        .any(|id| SendMessageW(app.control(*id), CB_GETDROPPEDSTATE, 0, 0) != 0);
                    match event.wParam as u16 {
                        VK_ESCAPE if !combo_open => {
                            app.command(CANCEL, BN_CLICKED);
                            handled = true;
                        }
                        VK_RETURN if app.editing.is_some() && ctrl => {
                            if app.editing.as_ref().is_some_and(|edit| edit.article) {
                                app.command(EDITOR_SAVE, BN_CLICKED);
                            } else {
                                app.save_edit();
                            }
                            handled = true;
                        }
                        0x53 if ctrl
                            && app.editing.as_ref().is_some_and(|edit| edit.id.is_some()) =>
                        {
                            app.command(EDITOR_SAVE, BN_CLICKED);
                            handled = true;
                        }
                        0x56 if ctrl
                            && GetKeyState(VK_MENU as i32) >= 0
                            && GetFocus() == app.control(BODY) =>
                        {
                            handled = app.paste_image();
                        }
                        VK_TAB
                            if app.preferences.markdown_shortcuts
                                && GetFocus() == app.control(BODY) =>
                        {
                            app.insert_markdown("    ");
                            handled = true;
                        }
                        0x42 if ctrl
                            && app.preferences.markdown_shortcuts
                            && GetFocus() == app.control(BODY) =>
                        {
                            app.insert_markdown("**");
                            handled = true;
                        }
                        0x49 if ctrl
                            && app.preferences.markdown_shortcuts
                            && GetFocus() == app.control(BODY) =>
                        {
                            app.insert_markdown("*");
                            handled = true;
                        }
                        0x4b if ctrl
                            && app.preferences.markdown_shortcuts
                            && GetFocus() == app.control(BODY) =>
                        {
                            app.insert_markdown("[]()");
                            handled = true;
                        }
                        0x4e if ctrl
                            && app.settings.is_none()
                            && app.editing.is_none()
                            && app.placement.is_none() =>
                        {
                            app.command(NEW, BN_CLICKED);
                            handled = true;
                        }
                        0x46 if ctrl
                            && app.settings.is_none()
                            && app.editing.is_none()
                            && app.placement.is_none() =>
                        {
                            SetFocus(app.control(SEARCH));
                            handled = true;
                        }
                        _ => {}
                    }
                });
            }
            if !handled && IsDialogMessageW(hwnd, &event) == 0 {
                TranslateMessage(&event);
                DispatchMessageW(&event);
            }
        }
        APP.with(|slot| {
            if let Some(app) = slot.borrow_mut().take() {
                for face in app.fonts {
                    DeleteObject(face);
                }
            }
        });
        LAST_FRAME.with(|frame| *frame.borrow_mut() = None);
        windows::Win32::System::Com::CoUninitialize();
    }
}

impl App {
    unsafe fn new(
        hwnd: HWND,
        path: PathBuf,
        wall: Wall,
        data_lock: fs::File,
        preferences_path: PathBuf,
        preferences: Preferences,
    ) -> Self {
        let mut definitions = vec![
            (NEW, "BUTTON", "＋ 新建便签", BS_OWNERDRAW as u32),
            (VIEW, "BUTTON", "文章列表", BS_OWNERDRAW as u32),
            (SEARCH, "EDIT", "", ES_AUTOHSCROLL as u32),
            (
                TAG_FILTER,
                "COMBOBOX",
                "",
                (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32 | WS_VSCROLL,
            ),
            (
                SORT,
                "COMBOBOX",
                "",
                (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32 | WS_VSCROLL,
            ),
            (
                TITLE,
                "EDIT",
                "",
                (ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL) as u32,
            ),
            (
                BODY,
                "EDIT",
                "",
                (ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL) as u32 | WS_VSCROLL,
            ),
            (TAGS, "EDIT", "", ES_AUTOHSCROLL as u32),
            (SAVE, "BUTTON", "去贴上 →", BS_OWNERDRAW as u32),
            (CANCEL, "BUTTON", "取消", BS_OWNERDRAW as u32),
            (PREVIEW, "BUTTON", "预览正文", BS_OWNERDRAW as u32),
            (DELETE, "BUTTON", "删除便签", BS_OWNERDRAW as u32),
            (chrome::MINIMIZE, "BUTTON", "最小化", BS_OWNERDRAW as u32),
            (chrome::MAXIMIZE, "BUTTON", "最大化", BS_OWNERDRAW as u32),
            (chrome::CLOSE, "BUTTON", "关闭", BS_OWNERDRAW as u32),
            (SETTINGS, "BUTTON", "设置", BS_OWNERDRAW as u32),
            (BACKGROUND, "BUTTON", "", BS_OWNERDRAW as u32),
            (WALL_SCROLL, "BUTTON", "", BS_OWNERDRAW as u32),
            (DIRECTORY, "EDIT", "", ES_AUTOHSCROLL as u32),
            (SETTINGS_SAVE, "BUTTON", "保存设置", BS_OWNERDRAW as u32),
            (SETTINGS_BACK, "BUTTON", "返回", BS_OWNERDRAW as u32),
            (ARTICLE, "BUTTON", "查看正文", BS_OWNERDRAW as u32),
            (OUTLINE, "BUTTON", "文章大纲", BS_OWNERDRAW as u32),
            (FOCUS, "BUTTON", "专注模式", BS_OWNERDRAW as u32),
            (IMPORT_IMAGE, "BUTTON", "导入图片", BS_OWNERDRAW as u32),
            (
                MARKDOWN_SHORTCUTS,
                "BUTTON",
                "Markdown shortcuts",
                BS_OWNERDRAW as u32,
            ),
            (PIN, "BUTTON", "置顶", BS_OWNERDRAW as u32),
            (ARCHIVE, "BUTTON", "归档", BS_OWNERDRAW as u32),
            (TRASH, "BUTTON", "恢复", BS_OWNERDRAW as u32),
            (PAPER_TEXTURE, "BUTTON", "", BS_OWNERDRAW as u32),
            (DEFAULT_PAPER, "BUTTON", "", BS_OWNERDRAW as u32),
        ];
        definitions.extend(
            TOOLBAR
                .iter()
                .map(|(id, label, _)| (*id, "BUTTON", *label, BS_OWNERDRAW as u32)),
        );
        let fonts = [
            font("Microsoft YaHei UI", 15, 400),
            font("Microsoft YaHei UI", 27, 700),
            paper_font(hwnd),
            font("Microsoft YaHei UI", 12, 400),
            font("Microsoft YaHei UI", 19, 700),
            font("Segoe UI", 16, 600),
        ];
        let definition_count = definitions.len();
        let controls: BTreeMap<_, _> = definitions
            .into_iter()
            .map(|(id, class, label, style)| {
                let handle = CreateWindowExW(
                    0,
                    wide(class).as_ptr(),
                    wide(label).as_ptr(),
                    WS_CHILD | WS_TABSTOP | style,
                    0,
                    0,
                    0,
                    0,
                    hwnd,
                    id as HMENU,
                    GetModuleHandleW(null()),
                    null(),
                );
                SendMessageW(
                    handle,
                    WM_SETFONT,
                    fonts[if id == TITLE { 2 } else { 0 }] as WPARAM,
                    0,
                );
                if class == "BUTTON" {
                    SetWindowSubclass(handle, Some(child_hover), 1, id);
                }
                assert!(!handle.is_null(), "Unable to create control {id}");
                (id, handle)
            })
            .collect();
        assert_eq!(
            controls.len(),
            definition_count,
            "Duplicate control identifier"
        );
        let mut app = Self {
            hwnd,
            path,
            data_lock,
            preferences_path,
            preferences,
            settings: None,
            editor: None,
            editor_session: 0,
            editor_loaded: false,
            editor_replaying: false,
            pending_editor_action: None,
            body_dirty: false,
            tags_dirty: false,
            wall,
            controls,
            fonts,
            preview: RefCell::new(crate::preview::Preview::new()),
            query: String::new(),
            available_tags: Vec::new(),
            selected_tag: None,
            sort: Sort::Edited,
            bucket: 0,
            list: false,
            list_scroll: 0,
            offset: Point::default(),
            placement: None,
            hover: None,
            drag: None,
            editing: None,
            status: String::new(),
            wheel_remainder: 0,
            cursor_tracking: false,
            pointer: None,
            row_hover: Hover::new(),
            button_hover: Hover::new(),
            transition: None,
            animate: system_motion(),
        };
        for label in SORT_LABELS {
            SendMessageW(
                app.control(SORT),
                CB_ADDSTRING,
                0,
                wide(label).as_ptr() as LPARAM,
            );
        }
        SendMessageW(app.control(SORT), CB_SETITEMHEIGHT, 0, 29);
        SendMessageW(app.control(SORT), CB_SETITEMHEIGHT, usize::MAX, 24);
        SendMessageW(app.control(SORT), CB_SETCURSEL, 0, 0);
        SendMessageW(app.control(TAG_FILTER), CB_SETITEMHEIGHT, 0, 29);
        SendMessageW(app.control(TAG_FILTER), CB_SETITEMHEIGHT, usize::MAX, 24);
        app.refresh_tag_filter();
        SendMessageW(app.control(TITLE), EM_LIMITTEXT, 48, 0);
        SendMessageW(app.control(TAGS), EM_LIMITTEXT, 1024, 0);
        SendMessageW(
            app.control(TAGS),
            EM_SETCUEBANNER,
            1,
            wide("\u{6807}\u{7b7e}\u{7528}\u{9017}\u{53f7}\u{5206}\u{9694}\u{ff0c}\u{6700}\u{591a} 12 \u{4e2a}").as_ptr() as LPARAM,
        );
        SendMessageW(app.control(BODY), EM_LIMITTEXT, BODY_LIMIT, 0);
        SetWindowSubclass(app.control(BODY), Some(child_hover), 1, BODY);
        DragAcceptFiles(app.control(BODY), 1);
        SendMessageW(app.control(SEARCH), EM_LIMITTEXT, 240, 0);
        SendMessageW(
            app.control(SEARCH),
            EM_SETCUEBANNER,
            0,
            wide("搜索短句或正文…").as_ptr() as LPARAM,
        );
        app.layout();
        app
    }

    fn control(&self, id: usize) -> HWND {
        *self.controls.get(&id).expect("Unknown control identifier")
    }

    unsafe fn label(&self, id: usize, text: &str) {
        SetWindowTextW(self.control(id), wide(text).as_ptr());
    }

    unsafe fn value(&self, id: usize) -> String {
        control_text(self.control(id))
    }

    unsafe fn client(&self) -> RECT {
        let mut area = zeroed();
        GetClientRect(self.hwnd, &mut area);
        area
    }

    fn view_list(&self) -> bool {
        self.placement.is_none()
            && (self.list
                || self.bucket != 0
                || !self.query.trim().is_empty()
                || self.selected_tag.is_some())
    }

    unsafe fn canvas(&self) -> RECT {
        let area = self.client();
        rect(EDGE, TOP, area.right - EDGE, area.bottom - 43)
    }

    unsafe fn list_area(&self) -> RECT {
        let area = self.canvas();
        let width = (area.right - area.left).min(840);
        let left = (area.right + area.left - width) / 2;
        rect(left, area.top, left + width, area.bottom)
    }

    unsafe fn row_area(&self, index: usize) -> RECT {
        let area = self.list_area();
        let top = area.top + index as i32 * ROW - self.list_scroll;
        rect(area.left, top, area.right, top + ROW - 9)
    }

    unsafe fn list_hit(&self, point: Point) -> Option<&model::Note> {
        if !inside(self.list_area(), point) {
            return None;
        }
        let index = ((point.y - TOP + self.list_scroll) / ROW) as usize;
        if !inside(self.row_area(index), point) {
            return None;
        }
        self.rows().get(index).copied()
    }

    fn rows(&self) -> Vec<&model::Note> {
        self.wall.rows_in_filtered(
            &self.query,
            self.selected_tag.as_deref().unwrap_or_default(),
            self.sort,
            self.bucket,
        )
    }

    unsafe fn refresh_tag_filter(&mut self) {
        let mut tags = BTreeMap::new();
        for note in &self.wall.notes {
            for tag in &note.tags {
                tags.entry(tag.to_lowercase())
                    .or_insert_with(|| tag.clone());
            }
        }
        self.available_tags = tags.into_values().collect();
        if self.selected_tag.as_ref().is_some_and(|selected| {
            !self
                .available_tags
                .iter()
                .any(|tag| tag.to_lowercase() == selected.to_lowercase())
        }) {
            self.selected_tag = None;
        }
        let filter = self.control(TAG_FILTER);
        SendMessageW(filter, CB_RESETCONTENT, 0, 0);
        SendMessageW(filter, CB_ADDSTRING, 0, wide("所有标签").as_ptr() as LPARAM);
        for tag in &self.available_tags {
            SendMessageW(filter, CB_ADDSTRING, 0, wide(tag).as_ptr() as LPARAM);
        }
        let selection = self
            .selected_tag
            .as_ref()
            .and_then(|selected| {
                self.available_tags
                    .iter()
                    .position(|tag| tag.to_lowercase() == selected.to_lowercase())
            })
            .map_or(0, |index| index + 1);
        SendMessageW(filter, CB_SETCURSEL, selection, 0);
    }

    unsafe fn hover_button(&mut self, next: Option<usize>) {
        let old = self.button_hover.current;
        if self.button_hover.set(next) {
            if let Some(id) = old {
                InvalidateRect(self.control(id), null(), 0);
            }
            if let Some(id) = next {
                InvalidateRect(self.control(id), null(), 0);
            }
            self.start_motion();
        }
    }

    unsafe fn start_motion(&self) {
        if self.animate {
            SetTimer(self.hwnd, MOTION_TIMER, 16, None);
        }
        self.redraw();
    }

    unsafe fn tick_motion(&mut self) {
        for id in [self.button_hover.previous, self.button_hover.current]
            .into_iter()
            .flatten()
        {
            InvalidateRect(self.control(id), null(), 0);
        }
        if self.transition.is_some_and(|(start, duration)| {
            !self.animate || start.elapsed().as_millis() >= duration.into()
        }) {
            self.transition = None;
        }
        if !self.animate
            || !(self.row_hover.active() || self.button_hover.active() || self.transition.is_some())
        {
            KillTimer(self.hwnd, MOTION_TIMER);
        }
        self.redraw();
    }

    unsafe fn fade(&mut self, duration: u64) {
        if self.animate {
            self.transition = Some((Instant::now(), duration));
            self.start_motion();
        }
    }

    unsafe fn editor_area(&self) -> RECT {
        let area = self.canvas();
        let max_width = if self.editing.as_ref().is_some_and(|edit| edit.article) {
            if self.editing.as_ref().is_some_and(|edit| edit.focused) {
                1200
            } else {
                1000
            }
        } else if self.settings.is_some() {
            940
        } else {
            752
        };
        let width = (area.right - area.left).min(max_width);
        let left = (area.right + area.left - width) / 2;
        let compact =
            self.settings.is_some() || self.editing.as_ref().is_some_and(|edit| !edit.article);
        rect(
            left,
            area.top,
            left + width,
            if self.settings.is_some() {
                area.bottom
            } else if compact {
                area.bottom.min(
                    TOP + if self.editing.as_ref().is_some_and(|edit| edit.id.is_some()) {
                        360
                    } else {
                        292
                    },
                )
            } else {
                area.bottom
            },
        )
    }

    unsafe fn search_area(&self) -> RECT {
        let right = self.client().right;
        rect(right - 460, 56, right - 311, 90)
    }

    unsafe fn layout(&self) {
        let width = self.client().right;
        let editing = self.editing.as_ref();
        let active = self.settings.is_none() && editing.is_none() && self.placement.is_none();
        let field = self.editor_area();
        let inner = field.left + 24;
        let end = field.right - 24;
        let button_y = field.bottom - 43;
        let show = |id, visible, left, top, w, h| {
            ShowWindow(self.control(id), if visible { SW_SHOW } else { SW_HIDE });
            if visible {
                MoveWindow(self.control(id), left, top, w, h, 1);
            }
        };
        show(NEW, active, width - 131, 56, 109, 34);
        show(VIEW, active, width - 235, 56, 96, 34);
        show(SETTINGS, active, width - 302, 56, 58, 34);
        for id in chrome::MINIMIZE..=chrome::CLOSE {
            let area = chrome::button_area(width, id);
            show(
                id,
                true,
                area.left,
                area.top,
                area.right - area.left,
                area.bottom - area.top,
            );
        }
        self.label(
            chrome::MAXIMIZE,
            if IsZoomed(self.hwnd) != 0 {
                "还原"
            } else {
                "最大化"
            },
        );
        InvalidateRect(self.control(chrome::MAXIMIZE), null(), 0);
        let search = self.search_area();
        show(
            SEARCH,
            active,
            search.left + 29,
            search.top + 8,
            search.right - search.left - 40,
            22,
        );
        show(SORT, active && self.view_list(), width - 149, 112, 127, 190);
        show(
            TAG_FILTER,
            active && self.view_list(),
            width - 319,
            112,
            160,
            190,
        );
        let paper = paper_text(editor_paper(field));
        let focused = editing.is_some_and(|edit| edit.focused);
        show(
            TITLE,
            editing.is_some() && !focused && !editing.is_some_and(|edit| edit.article),
            paper.left,
            paper.top,
            paper.right - paper.left,
            paper.bottom - paper.top,
        );
        if editing.is_some() {
            let format = rect(0, 0, paper.right - paper.left, paper.bottom - paper.top);
            SendMessageW(
                self.control(TITLE),
                EM_SETRECTNP,
                0,
                &format as *const RECT as LPARAM,
            );
            InvalidateRect(self.control(TITLE), null(), 1);
        }
        let detail = editing.is_some_and(|edit| edit.id.is_some());
        let article = editing.is_some_and(|edit| edit.article);
        let body_top = editing.map_or(BODY_TOP, Editing::body_top);
        show(
            BODY,
            false,
            inner,
            body_top,
            if editing.is_some_and(|edit| edit.mode == 1) {
                (end - inner) / 2 - 8
            } else {
                end - inner
            },
            (field.bottom - body_top - 149).max(42),
        );
        show(SAVE, editing.is_some(), end - 109, button_y, 109, 30);
        show(CANCEL, editing.is_some(), end - 201, button_y, 82, 30);
        show(PREVIEW, false, end - 109, body_top - 36, 109, 27);
        show(
            TAGS,
            editing.is_some() && !article && !focused,
            inner + 204,
            TOP + 201,
            end - inner - 204,
            25,
        );
        show(OUTLINE, false, end - 188, TOP + 14, 88, 28);
        show(FOCUS, article, end - 88, TOP + 14, 88, 28);
        show(
            IMPORT_IMAGE,
            article && editing.is_some_and(|edit| edit.mode != 2),
            end - 286,
            TOP + 14,
            88,
            28,
        );
        for (index, (id, label, _)) in TOOLBAR.iter().enumerate() {
            let row = index / 10;
            let xpos = inner
                + TOOLBAR[row * 10..index]
                    .iter()
                    .map(|(_, label, _)| toolbar_width(label) + 4)
                    .sum::<i32>();
            show(
                *id,
                false,
                xpos,
                body_top - 66 + row as i32 * 30,
                toolbar_width(label),
                27,
            );
        }
        show(ARTICLE, detail && !article, inner + 204, TOP + 138, 109, 30);
        let action_y = field.bottom - 80;
        show(
            DELETE,
            detail && self.bucket == 2,
            inner + 116,
            action_y,
            109,
            30,
        );
        show(PIN, detail && self.bucket == 0, inner, action_y, 80, 30);
        show(
            ARCHIVE,
            detail && self.bucket != 2,
            inner + 88,
            action_y,
            86,
            30,
        );
        show(
            TRASH,
            detail,
            if self.bucket == 2 { inner } else { inner + 182 },
            action_y,
            109,
            30,
        );
        let settings = self.settings.is_some() && self.editor.is_none();
        show(BACKGROUND, settings, end - 154, TOP + 20, 154, 32);
        show(WALL_SCROLL, settings, end - 154, TOP + 82, 154, 32);
        show(MARKDOWN_SHORTCUTS, settings, end - 154, TOP + 144, 154, 32);
        show(DEFAULT_PAPER, settings, end - 154, TOP + 206, 154, 32);
        show(PAPER_TEXTURE, settings, end - 154, TOP + 268, 154, 32);
        show(
            DIRECTORY,
            self.settings.is_some(),
            inner + 5,
            if self.editor.is_some() {
                field.bottom - 85
            } else {
                TOP + 366
            },
            end - inner - 10,
            22,
        );
        show(
            SETTINGS_SAVE,
            self.settings.is_some(),
            end - 109,
            button_y,
            109,
            30,
        );
        show(
            SETTINGS_BACK,
            self.settings.is_some(),
            end - 201,
            button_y,
            82,
            30,
        );
        if let Some(editor) = &self.editor {
            let bounds = if self.settings.is_some() {
                rect(inner, TOP + 12, end, field.bottom - 125)
            } else {
                rect(inner, TOP + 52, end, (field.bottom - 149).max(TOP + 160))
            };
            editor.layout(bounds, self.settings.is_some() || article);
        }
        self.label(
            VIEW,
            if !self.query.is_empty() || self.selected_tag.is_some() {
                "\u{6e05}\u{7a7a}\u{7b5b}\u{9009}"
            } else {
                match (self.list, self.bucket) {
                    (false, _) => "文章列表",
                    (true, 0) => "归档便签 →",
                    (true, 1) => "回收站 →",
                    _ => "回到墙面",
                }
            },
        );
        self.label(
            PIN,
            if editing
                .and_then(|edit| edit.id)
                .and_then(|id| self.wall.notes.iter().find(|note| note.id == id))
                .is_some_and(|note| note.pinned)
            {
                "取消置顶"
            } else {
                "置顶"
            },
        );
        self.label(
            ARCHIVE,
            if self.bucket == 1 {
                "取消归档"
            } else {
                "归档"
            },
        );
        self.label(
            TRASH,
            if self.bucket == 2 {
                "恢复便签"
            } else {
                "移入回收站"
            },
        );
        self.label(DELETE, "永久删除");
        self.label(
            SAVE,
            if article {
                "保存并返回"
            } else if detail {
                "保存修改"
            } else {
                "去贴上 →"
            },
        );
        self.label(CANCEL, if article { "返回短句" } else { "返回" });
    }

    unsafe fn redraw(&self) {
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn open_article(&mut self) {
        if !self.open_web_editor(false) {
            return;
        }
        let has_body = !self.value(BODY).trim().is_empty();
        let edit = self.editing.as_mut().unwrap();
        edit.article = true;
        edit.mode = 0;
        edit.scroll = 0;
        self.label(
            PREVIEW,
            if has_body {
                "分屏预览"
            } else {
                "编辑模式"
            },
        );
        self.layout();
        self.redraw();
    }

    unsafe fn open_web_editor(&mut self, settings: bool) -> bool {
        if self.editor.is_none() {
            let result = model::app_dir()
                .map_err(|error| error.to_string())
                .and_then(|base| crate::editor::Editor::new(self.hwnd, &base));
            match result {
                Ok(editor) => self.editor = Some(editor),
                Err(error) => {
                    message(self.hwnd, &error, MB_ICONERROR);
                    return false;
                }
            }
        }
        self.editor_session += 1;
        self.editor_loaded = false;
        let data = serde_json::json!({
            "type":"open", "session":self.editor_session,
            "page":if settings { "settings" } else { "editor" },
            "body":self.value(BODY).replace("\r\n", "\n"),
            "preferences":self.settings.as_ref().unwrap_or(&self.preferences),
            "papers":Paper::ALL.iter().map(|paper| paper.name()).collect::<Vec<_>>()
        });
        if let Err(error) = self
            .editor
            .as_ref()
            .unwrap()
            .open(data, self.path.parent().unwrap())
        {
            message(
                self.hwnd,
                &format!("无法打开正文组件：{error}"),
                MB_ICONERROR,
            );
            return false;
        }
        true
    }

    unsafe fn defer_editor_action(&mut self, action: usize) -> bool {
        if self.editor_replaying
            || self.editor.is_none()
            || (self.settings.is_none() && self.editing.as_ref().is_none_or(|edit| !edit.article))
        {
            return false;
        }
        if self.pending_editor_action.is_some() {
            return true;
        }
        self.pending_editor_action = Some(action);
        SetTimer(self.hwnd, EDITOR_TIMEOUT, 10000, None);
        if self.editor_loaded
            && let Err(error) = self
                .editor
                .as_ref()
                .unwrap()
                .post(serde_json::json!({"type":"flush", "session":self.editor_session}))
        {
            self.pending_editor_action = None;
            KillTimer(self.hwnd, EDITOR_TIMEOUT);
            message(
                self.hwnd,
                &format!("尚未同步正文，未离开编辑器：{error}"),
                MB_ICONERROR,
            );
        }
        true
    }

    unsafe fn editor_events(&mut self) {
        let events = self
            .editor
            .as_ref()
            .map(|editor| editor.drain())
            .unwrap_or_default();
        for event in events {
            if event["type"] == "error" {
                self.pending_editor_action = None;
                KillTimer(self.hwnd, EDITOR_TIMEOUT);
                let error = event["message"].as_str().unwrap_or("正文组件加载失败");
                self.status = format!("正文组件出错，已有数据未改动：{error}");
                if !self.editor_loaded {
                    if let Some(edit) = &mut self.editing {
                        edit.article = false;
                    }
                    self.settings = None;
                    self.editor = None;
                    self.layout();
                }
                self.redraw();
                message(self.hwnd, &self.status, MB_ICONERROR);
                continue;
            }
            if event["session"].as_u64() != Some(self.editor_session) {
                continue;
            }
            match event["type"].as_str().unwrap_or("") {
                "loaded" => {
                    self.editor_loaded = true;
                    if self.pending_editor_action.is_some() {
                        let _ = self.editor.as_ref().unwrap().post(
                            serde_json::json!({"type":"flush", "session":self.editor_session}),
                        );
                    }
                }
                "change" if self.editing.as_ref().is_some_and(|edit| edit.article) => {
                    if let Some(body) = event["body"].as_str() {
                        if body.encode_utf16().count() > BODY_LIMIT {
                            continue;
                        }
                        if self.value(BODY).replace("\r\n", "\n") != body {
                            self.label(BODY, &body.replace('\n', "\r\n"));
                            self.body_dirty = true;
                            self.status = "正在保存…".into();
                            SetTimer(
                                self.hwnd,
                                AUTOSAVE_TIMER,
                                self.preferences.editor.autosave_ms,
                                None,
                            );
                            self.redraw();
                        }
                    }
                }
                "preferences" if self.settings.is_some() => {
                    if let Ok(mut preferences) =
                        serde_json::from_value::<Preferences>(event["preferences"].clone())
                    {
                        preferences.editor.normalize();
                        preferences.background = preferences.background.min(BACKGROUNDS.len() - 1);
                        preferences.default_paper =
                            preferences.default_paper.min(Paper::ALL.len() - 1);
                        preferences.data_dir = self.settings.as_ref().unwrap().data_dir.clone();
                        self.settings = Some(preferences);
                    }
                }
                "reset" if self.settings.is_some() => {
                    self.settings.as_mut().unwrap().editor = model::EditorPreferences::default();
                    self.open_web_editor(true);
                }
                "image" if self.editing.as_ref().is_some_and(|edit| edit.article) => {
                    let result = crate::editor::decode_image(event["data"].as_str().unwrap_or(""))
                        .and_then(|bytes| assets::store_image(self.path.parent().unwrap(), &bytes));
                    let response = match result {
                        Ok(image) => {
                            let reference = image.reference().to_owned();
                            image.finish();
                            serde_json::json!({"type":"image-result", "session":self.editor_session, "reference":reference})
                        }
                        Err(error) => {
                            serde_json::json!({"type":"image-result", "session":self.editor_session, "error":error.to_string()})
                        }
                    };
                    let _ = self.editor.as_ref().unwrap().post(response);
                }
                "invalid" => {
                    self.pending_editor_action = None;
                    KillTimer(self.hwnd, EDITOR_TIMEOUT);
                    self.status = event["message"]
                        .as_str()
                        .unwrap_or("正文无效，未保存")
                        .into();
                    self.redraw();
                }
                "action" => {
                    let action = if event["action"] == "back" {
                        CANCEL
                    } else if self.settings.is_some() {
                        SETTINGS_SAVE
                    } else {
                        EDITOR_SAVE
                    };
                    self.command(action, BN_CLICKED);
                }
                "export-pdf" if self.editing.as_ref().is_some_and(|edit| edit.article) => {
                    let result = choose_pdf(self.hwnd);
                    let error = match result {
                        Ok(Some(path)) => self
                            .editor
                            .as_ref()
                            .unwrap()
                            .export_pdf(&path, self.editor_session)
                            .err()
                            .map(|error| error.to_string()),
                        Ok(None) => {
                            let _ = self.editor.as_ref().unwrap().post(serde_json::json!({"type":"pdf-result", "session":self.editor_session}));
                            None
                        }
                        Err(error) => Some(error.to_string()),
                    };
                    if let Some(error) = error {
                        let _ = self.editor.as_ref().unwrap().post(serde_json::json!({"type":"pdf-result", "session":self.editor_session, "error":error}));
                    }
                }
                "link" => {
                    if let Some(url) = event["url"].as_str()
                        && (url.starts_with("https://")
                            || url.starts_with("http://")
                            || url.starts_with("mailto:"))
                        && !url.starts_with("https://notes.esticker.local/")
                        && !url.starts_with("https://editor.esticker.local/")
                    {
                        ShellExecuteW(
                            self.hwnd,
                            wide("open").as_ptr(),
                            wide(url).as_ptr(),
                            null(),
                            null(),
                            SW_SHOWNORMAL,
                        );
                    }
                }
                "flushed" => {
                    if let Some(action) = self.pending_editor_action.take() {
                        KillTimer(self.hwnd, EDITOR_TIMEOUT);
                        self.editor_replaying = true;
                        if action == EDITOR_CLOSE {
                            if self.save_body() {
                                DestroyWindow(self.hwnd);
                            }
                        } else {
                            self.command(action, BN_CLICKED);
                        }
                        self.editor_replaying = false;
                    }
                }
                _ => {}
            }
        }
    }

    unsafe fn toggle_focus(&mut self) {
        let Some(edit) = self.editing.as_mut().filter(|edit| edit.article) else {
            return;
        };
        edit.focused = !edit.focused;
        let focused = edit.focused;
        let mode = edit.mode;
        if mode != 0 {
            let visible =
                (self.editor_area().bottom - self.editing.as_ref().unwrap().body_top() - 149)
                    .max(42);
            let max = (self.preview_height() + 9 - visible).max(0);
            let edit = self.editing.as_mut().unwrap();
            edit.scroll = edit.scroll.min(max);
        }
        self.label(
            FOCUS,
            if focused {
                "退出专注"
            } else {
                "专注模式"
            },
        );
        self.layout();
        if mode != 2 {
            SetFocus(self.control(BODY));
        }
        self.redraw();
    }

    unsafe fn open_outline(&mut self) {
        if self.editing.as_ref().is_none_or(|edit| !edit.article) {
            return;
        }
        let entries = crate::markdown::outline(&self.value(BODY));
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        if entries.is_empty() {
            AppendMenuW(
                menu,
                MF_GRAYED,
                0,
                wide("暂无标题（使用 H1–H6 添加）").as_ptr(),
            );
        }
        for (index, entry) in entries.iter().enumerate() {
            let label = entry
                .label
                .chars()
                .take(72)
                .collect::<String>()
                .replace('&', "&&");
            let label = if label.trim().is_empty() {
                "无标题"
            } else {
                &label
            };
            let indent = "\u{3000}".repeat((entry.level - 1) as usize);
            let caption = format!("{indent}H{}  {label}", entry.level);
            AppendMenuW(menu, MF_STRING, index + 1, wide(&caption).as_ptr());
        }
        let mut anchor = RECT::default();
        GetWindowRect(self.control(OUTLINE), &mut anchor);
        let choice = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            anchor.left,
            anchor.bottom,
            0,
            self.hwnd,
            null(),
        );
        DestroyMenu(menu);
        let Some(entry) = (choice > 0)
            .then(|| entries.get(choice as usize - 1))
            .flatten()
        else {
            return;
        };
        if let Some(edit) = self.editing.as_mut().filter(|edit| edit.mode == 2) {
            edit.mode = 1;
            edit.scroll = 0;
            self.label(PREVIEW, "分屏预览");
            self.layout();
        }
        let body = self.control(BODY);
        SetFocus(body);
        SendMessageW(body, EM_SETSEL, entry.offset, entry.offset as LPARAM);
        SendMessageW(body, EM_SCROLLCARET, 0, 0);
        self.redraw();
    }
    unsafe fn begin_edit(&mut self, id: Option<u64>, draft: Draft) {
        self.placement = None;
        TITLE_BACKGROUND.with(|color| color.set(draft.paper.colors().0));
        self.editing = Some(Editing {
            id,
            paper: draft.paper,
            article: false,
            mode: 0,
            scroll: 0,
            focused: false,
        });
        self.fade(180);
        self.status.clear();
        self.label(
            TITLE,
            &draft.title.replace("\r\n", "\n").replace('\n', "\r\n"),
        );
        self.label(
            BODY,
            &draft.body.replace("\r\n", "\n").replace('\n', "\r\n"),
        );
        self.label(TAGS, &model::tags_text(&draft.tags));
        self.body_dirty = false;
        self.tags_dirty = false;
        KillTimer(self.hwnd, AUTOSAVE_TIMER);
        self.label(
            SAVE,
            if id.is_some() {
                "保存修改"
            } else {
                "去贴上 →"
            },
        );
        self.label(
            ARTICLE,
            if draft.body.trim().is_empty() {
                "添加正文"
            } else {
                "查看正文"
            },
        );
        self.layout();
        SetFocus(self.control(TITLE));
        let end = GetWindowTextLengthW(self.control(TITLE)) as usize;
        SendMessageW(self.control(TITLE), EM_SETSEL, end, end as LPARAM);
        self.redraw();
    }

    unsafe fn leave_edit(&mut self) {
        if !self.save_body() {
            return;
        }
        KillTimer(self.hwnd, AUTOSAVE_TIMER);
        self.editing = None;
        self.fade(200);
        self.layout();
        SetFocus(self.hwnd);
        self.redraw();
    }

    unsafe fn commit(&mut self, change: impl FnOnce(&mut Wall), notice: &str) -> bool {
        let mut next = self.wall.clone();
        change(&mut next);
        match next.save(&self.path) {
            Ok(()) => {
                self.wall = next;
                self.refresh_tag_filter();
                self.status = notice.into();
                self.redraw();
                true
            }
            Err(error) => {
                self.status = "未能保存，原有便签未改动".into();
                self.redraw();
                message(self.hwnd, &format!("写入失败：{error}"), MB_ICONERROR);
                false
            }
        }
    }

    unsafe fn save_body(&mut self) -> bool {
        if !self.body_dirty && !self.tags_dirty {
            return true;
        }
        let Some(id) = self.editing.as_ref().and_then(|edit| edit.id) else {
            return false;
        };
        let body = self.value(BODY).replace("\r\n", "\n");
        let tags = model::parse_tags(&self.value(TAGS));
        let tags_dirty = self.tags_dirty;
        if self.commit(
            |wall| {
                if let Some(note) = wall.notes.iter_mut().find(|note| note.id == id) {
                    note.body = body;
                    if tags_dirty {
                        note.tags = tags;
                    }
                    note.edited = chrono::Utc::now().timestamp();
                }
            },
            "已自动保存",
        ) {
            self.body_dirty = false;
            self.tags_dirty = false;
            true
        } else {
            false
        }
    }

    unsafe fn insert_markdown(&mut self, marker: &str) {
        if self.editing.as_ref().is_none_or(|edit| !edit.article) {
            return;
        }
        if let Some(editor) = &self.editor {
            if let Err(error) = editor.post(
                serde_json::json!({"type":"insert", "session":self.editor_session, "text":marker}),
            ) {
                self.status = format!("未能插入正文：{error}");
            }
            return;
        }
        SetFocus(self.control(BODY));
        let body = self.control(BODY);
        let mut start = 0u32;
        let mut end = 0u32;
        SendMessageW(
            body,
            EM_GETSEL,
            &mut start as *mut u32 as WPARAM,
            &mut end as *mut u32 as LPARAM,
        );
        let length = GetWindowTextLengthW(body).max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        GetWindowTextW(body, buffer.as_mut_ptr(), buffer.len() as i32);
        let selected = String::from_utf16_lossy(
            &buffer[(start as usize).min(length)..(end as usize).min(length)],
        );
        let insertion = crate::markdown::insertion(marker, &selected, start as usize);
        SendMessageW(
            body,
            EM_REPLACESEL,
            1,
            wide(&insertion.text).as_ptr() as LPARAM,
        );
        SendMessageW(
            body,
            EM_SETSEL,
            insertion.selection_start as WPARAM,
            insertion.selection_end as LPARAM,
        );
        self.body_dirty = true;
        self.status = "正在保存…".into();
        SetTimer(self.hwnd, AUTOSAVE_TIMER, 650, None);
        self.redraw();
    }

    unsafe fn import_images(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() || !self.can_insert_image() {
            return;
        }
        self.insert_images(|directory| {
            paths
                .iter()
                .map(|path| assets::import_image(directory, path))
                .collect()
        });
    }

    fn can_insert_image(&self) -> bool {
        self.editing
            .as_ref()
            .is_some_and(|edit| edit.article && edit.mode != 2)
    }

    unsafe fn paste_image(&mut self) -> bool {
        if !self.can_insert_image() {
            return false;
        }
        match clipboard::image(self.hwnd) {
            Ok(None) => false,
            Ok(Some(bytes)) => {
                self.insert_images(|directory| Ok(vec![assets::store_image(directory, &bytes)?]));
                true
            }
            Err(error) => {
                message(self.hwnd, &format!("未能粘贴图片：{error}"), MB_ICONERROR);
                true
            }
        }
    }

    unsafe fn insert_images(
        &mut self,
        load: impl FnOnce(&Path) -> io::Result<Vec<assets::ImportedImage>>,
    ) {
        let result = (|| -> io::Result<()> {
            let directory = self.path.parent().unwrap_or_else(|| Path::new("."));
            let images = load(directory)?;
            let insertion = images
                .iter()
                .map(|image| format!("\r\n![图片]({})\r\n", image.reference()))
                .collect::<String>();
            if let Some(editor) = &self.editor {
                if self.value(BODY).encode_utf16().count() + insertion.encode_utf16().count()
                    > BODY_LIMIT
                {
                    return Err(io::Error::other("正文字数已达上限，未插入图片"));
                }
                editor.post(serde_json::json!({"type":"insert", "session":self.editor_session, "text":insertion})).map_err(io::Error::other)?;
                for image in images {
                    image.finish();
                }
                return Ok(());
            }
            let body = self.control(BODY);
            let mut start = 0u32;
            let mut end = 0u32;
            SendMessageW(
                body,
                EM_GETSEL,
                &mut start as *mut u32 as WPARAM,
                &mut end as *mut u32 as LPARAM,
            );
            let length = GetWindowTextLengthW(body).max(0) as usize;
            if length - (end - start) as usize + insertion.encode_utf16().count() > BODY_LIMIT {
                return Err(io::Error::other("正文字数已达上限，未插入图片"));
            }
            SetFocus(body);
            SendMessageW(body, EM_REPLACESEL, 1, wide(&insertion).as_ptr() as LPARAM);
            for image in images {
                image.finish();
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.body_dirty = true;
                self.status = "正在保存…".into();
                SetTimer(self.hwnd, AUTOSAVE_TIMER, 650, None);
                self.redraw();
            }
            Err(error) => {
                message(self.hwnd, &format!("未能导入图片：{error}"), MB_ICONERROR);
            }
        }
    }

    unsafe fn save_edit(&mut self) {
        self.persist_edit(true);
    }

    unsafe fn persist_edit(&mut self, leave: bool) -> bool {
        let Some(edit) = self.editing.clone() else {
            return false;
        };
        let title = self.value(TITLE).replace("\r\n", "\n");
        if title.trim().is_empty() {
            SetFocus(self.control(TITLE));
            self.status = "先写一句，再贴上墙".into();
            self.redraw();
            return false;
        }
        let dc = GetDC(self.hwnd);
        let bounds = paper_text(editor_paper(self.editor_area()));
        let needed = text(
            dc,
            self.fonts[2],
            &title,
            bounds,
            INK,
            PAPER_TEXT | DT_CALCRECT,
        );
        let prior = SelectObject(dc, self.fonts[2]);
        let mut metrics = zeroed::<TEXTMETRICW>();
        GetTextMetricsW(dc, &mut metrics);
        SelectObject(dc, prior);
        ReleaseDC(self.hwnd, dc);
        let rows = SendMessageW(self.control(TITLE), EM_GETLINECOUNT, 0, 0) as i32;
        if needed.max(rows * metrics.tmHeight) > bounds.bottom - bounds.top {
            self.status = "纸面已写满，长内容可以放进正文".into();
            SetFocus(self.control(TITLE));
            self.redraw();
            return false;
        }
        let body = if edit.id.is_some() {
            self.value(BODY).replace("\r\n", "\n")
        } else {
            String::new()
        };
        let tags = model::parse_tags(&self.value(TAGS));
        let draft = Draft {
            title,
            body,
            paper: edit.paper,
            tags: self
                .wall
                .notes
                .iter()
                .find(|note| Some(note.id) == edit.id)
                .filter(|note| model::tags_text(&note.tags) == self.value(TAGS))
                .map_or(tags, |note| note.tags.clone()),
        };
        if let Some(id) = edit.id {
            let changed = self
                .wall
                .notes
                .iter()
                .find(|note| note.id == id)
                .is_some_and(|note| {
                    note.title != draft.title
                        || note.body != draft.body
                        || note.paper != draft.paper
                        || note.tags != draft.tags
                });
            if !changed
                || self.commit(
                    |wall| {
                        if let Some(note) = wall.notes.iter_mut().find(|note| note.id == id) {
                            note.title = draft.title;
                            note.body = draft.body;
                            note.paper = draft.paper;
                            note.tags = draft.tags;
                            note.edited = chrono::Utc::now().timestamp();
                        }
                    },
                    "已保存修改",
                )
            {
                self.body_dirty = false;
                self.tags_dirty = false;
                KillTimer(self.hwnd, AUTOSAVE_TIMER);
                if leave {
                    self.leave_edit();
                }
                return true;
            }
        } else if leave {
            self.placement = Some(draft);
            self.hover = None;
            self.status.clear();
            self.leave_edit();
            return true;
        }
        false
    }

    unsafe fn cancel(&mut self) {
        if self.settings.take().is_some() {
            self.status.clear();
            self.layout();
            SetFocus(self.control(SETTINGS));
            self.redraw();
        } else if let Some(draft) = self.placement.take() {
            self.hover = None;
            self.begin_edit(None, draft);
        } else if self.editing.as_ref().is_some_and(|edit| edit.focused) {
            self.toggle_focus();
        } else if self.editing.as_ref().is_some_and(|edit| edit.article) {
            if !self.save_body() {
                return;
            }
            if let Some(edit) = &mut self.editing {
                edit.article = false;
                edit.mode = 0;
                edit.scroll = 0;
            }
            self.layout();
            SetFocus(self.control(ARTICLE));
            self.redraw();
        } else if self.editing.is_some() {
            self.leave_edit();
        } else if self.drag.take().is_some() {
            ReleaseCapture();
            self.redraw();
        } else if !self.query.is_empty() || self.selected_tag.is_some() {
            self.label(SEARCH, "");
            self.query.clear();
            self.selected_tag = None;
            SendMessageW(self.control(TAG_FILTER), CB_SETCURSEL, 0, 0);
            self.list_scroll = 0;
            self.layout();
            self.redraw();
        }
    }

    fn wall_color(&self) -> u32 {
        let index = self
            .settings
            .as_ref()
            .map_or(self.preferences.background, |settings| settings.background);
        BACKGROUNDS.get(index).unwrap_or(&BACKGROUNDS[0]).1
    }

    unsafe fn open_settings(&mut self) {
        let mut settings = self.preferences.clone();
        settings.background = settings.background.min(BACKGROUNDS.len() - 1);
        settings.default_paper = settings.default_paper.min(Paper::ALL.len() - 1);
        self.settings = Some(settings);
        if !self.open_web_editor(true) {
            self.settings = None;
            return;
        }
        self.status.clear();
        self.label(DIRECTORY, &self.path.parent().unwrap().to_string_lossy());
        self.settings_labels();

        self.layout();
        SetFocus(self.control(BACKGROUND));
        self.redraw();
    }

    unsafe fn settings_labels(&self) {
        if let Some(settings) = &self.settings {
            self.label(
                BACKGROUND,
                &format!("墙面：{}", BACKGROUNDS[settings.background].0),
            );
            self.label(
                MARKDOWN_SHORTCUTS,
                if settings.markdown_shortcuts {
                    "已开启"
                } else {
                    "已关闭"
                },
            );
            self.label(
                PAPER_TEXTURE,
                if settings.paper_texture {
                    "已开启"
                } else {
                    "已关闭"
                },
            );
            self.label(
                DEFAULT_PAPER,
                Paper::ALL[settings.default_paper.min(Paper::ALL.len() - 1)].name(),
            );
            self.label(
                WALL_SCROLL,
                if settings.scroll_wall {
                    "已开启"
                } else {
                    "已关闭"
                },
            );
        }
    }

    unsafe fn save_settings(&mut self) {
        let Some(mut settings) = self.settings.clone() else {
            return;
        };
        let directory = self.value(DIRECTORY);
        let directory = std::path::Path::new(directory.trim().trim_matches('"'));
        let result = (|| -> io::Result<()> {
            if !directory.is_absolute() {
                return Err(io::Error::other("请输入绝对路径，例如 D:\\Notes"));
            }
            fs::create_dir_all(directory)?;
            let directory = fs::canonicalize(directory)?;
            let current_dir = fs::canonicalize(self.path.parent().unwrap())?;
            if directory == current_dir {
                settings.save(&self.preferences_path)?;
                return Ok(());
            }
            let next = directory.join("wall.json");
            let next_lock = model::lock(&next)?;
            if next.try_exists()? {
                return Err(io::Error::other(
                    "目标目录已有 wall.json，未覆盖。请选择另一个目录。",
                ));
            }
            settings.data_dir = Some(directory);
            let assets =
                model::AssetMigration::copy(&self.wall, &current_dir, next.parent().unwrap())?;
            self.wall.save(&next)?;
            if let Err(error) = settings.save(&self.preferences_path) {
                let _ = fs::remove_file(&next);
                return Err(error);
            }
            self.path = next;
            self.data_lock = next_lock;
            assets.finish();
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.preferences = settings;
                self.settings = None;
                self.status = "设置已保存".into();
                self.clamp_scroll();
                self.layout();
                SetFocus(self.control(SETTINGS));
            }
            Err(error) => {
                message(self.hwnd, &format!("未能保存设置：{error}"), MB_ICONERROR);
            }
        }
        self.redraw();
    }

    unsafe fn command(&mut self, id: usize, notification: u32) {
        if notification == BN_CLICKED
            && matches!(
                id,
                SAVE | CANCEL
                    | SETTINGS_SAVE
                    | SETTINGS_BACK
                    | PIN
                    | ARCHIVE
                    | TRASH
                    | DELETE
                    | EDITOR_SAVE
            )
            && self.defer_editor_action(id)
        {
            return;
        }
        match id {
            EDITOR_SAVE => {
                KillTimer(self.hwnd, AUTOSAVE_TIMER);
                if self.save_body() {
                    self.status = "正文已保存".into();
                    self.redraw();
                }
            }
            chrome::MINIMIZE | chrome::MAXIMIZE | chrome::CLOSE if notification == BN_CLICKED => {
                chrome::command(self.hwnd, id)
            }
            NEW if notification == BN_CLICKED => {
                self.status.clear();
                self.begin_edit(
                    None,
                    Draft {
                        paper: Paper::ALL[self.preferences.default_paper.min(Paper::ALL.len() - 1)],
                        ..Draft::default()
                    },
                );
            }
            VIEW if notification == BN_CLICKED => {
                if !self.query.is_empty() || self.selected_tag.is_some() {
                    self.label(SEARCH, "");
                    self.query.clear();
                    self.selected_tag = None;
                    SendMessageW(self.control(TAG_FILTER), CB_SETCURSEL, 0, 0);
                } else if !self.list {
                    self.list = true;
                    self.bucket = 0;
                } else if self.bucket < 2 {
                    self.bucket += 1;
                } else {
                    self.list = false;
                    self.bucket = 0;
                }
                self.label(
                    VIEW,
                    match (self.list, self.bucket) {
                        (false, _) => "文章列表",
                        (true, 1) => "归档便签",
                        (true, 2) => "回收站",
                        _ => "回到墙面",
                    },
                );
                self.list_scroll = 0;
                self.fade(200);
                self.layout();
                self.redraw();
            }
            SEARCH if notification == EN_CHANGE => {
                self.query = self.value(SEARCH);
                self.list_scroll = 0;
                self.status.clear();
                self.layout();
                self.redraw();
            }
            TAGS if notification == EN_CHANGE
                && GetFocus() == self.control(TAGS)
                && self.editing.as_ref().is_some_and(|edit| edit.id.is_some()) =>
            {
                self.tags_dirty = true;
                self.status = "正在保存…".into();
                SetTimer(self.hwnd, AUTOSAVE_TIMER, 650, None);
                self.redraw();
            }
            SEARCH if matches!(notification, EN_SETFOCUS | EN_KILLFOCUS) => self.redraw(),
            TITLE if notification == EN_CHANGE => {
                self.status.clear();
                self.redraw();
            }
            BODY if notification == EN_CHANGE
                && GetFocus() == self.control(BODY)
                && self
                    .editing
                    .as_ref()
                    .is_some_and(|edit| edit.article && edit.id.is_some()) =>
            {
                self.body_dirty = true;
                self.status = "正在保存…".into();
                SetTimer(self.hwnd, AUTOSAVE_TIMER, 650, None);
                self.redraw();
            }
            SORT if notification == CBN_SELCHANGE => {
                self.sort = match SendMessageW(self.control(SORT), CB_GETCURSEL, 0, 0) {
                    1 => Sort::Created,
                    2 => Sort::Title,
                    _ => Sort::Edited,
                };
                self.list_scroll = 0;
                self.redraw();
            }
            TAG_FILTER if notification == CBN_SELCHANGE => {
                let selection = SendMessageW(self.control(TAG_FILTER), CB_GETCURSEL, 0, 0);
                self.selected_tag = if selection > 0 {
                    self.available_tags.get(selection as usize - 1).cloned()
                } else {
                    None
                };
                self.list_scroll = 0;
                self.layout();
                self.redraw();
            }
            SAVE if notification == BN_CLICKED => self.save_edit(),
            CANCEL if notification == BN_CLICKED => self.cancel(),
            SETTINGS if notification == BN_CLICKED => self.open_settings(),
            SETTINGS_BACK if notification == BN_CLICKED => self.cancel(),
            SETTINGS_SAVE if notification == BN_CLICKED => self.save_settings(),
            BACKGROUND | WALL_SCROLL | MARKDOWN_SHORTCUTS | PAPER_TEXTURE | DEFAULT_PAPER
                if notification == BN_CLICKED =>
            {
                if let Some(settings) = &mut self.settings {
                    if id == BACKGROUND {
                        settings.background = (settings.background + 1) % BACKGROUNDS.len();
                    } else if id == WALL_SCROLL {
                        settings.scroll_wall = !settings.scroll_wall;
                    } else if id == MARKDOWN_SHORTCUTS {
                        settings.markdown_shortcuts = !settings.markdown_shortcuts;
                    } else if id == PAPER_TEXTURE {
                        settings.paper_texture = !settings.paper_texture;
                    } else {
                        settings.default_paper = (settings.default_paper + 1) % Paper::ALL.len();
                    }
                    self.settings_labels();
                    self.layout();
                    self.redraw();
                }
            }
            ARTICLE if notification == BN_CLICKED => self.open_article(),
            FOCUS if notification == BN_CLICKED => self.toggle_focus(),
            OUTLINE if notification == BN_CLICKED => self.open_outline(),
            121..=139 if notification == BN_CLICKED => {
                if let Some((_, _, insertion)) = TOOLBAR.iter().find(|(tool, _, _)| *tool == id) {
                    self.insert_markdown(insertion);
                }
            }
            PREVIEW if notification == BN_CLICKED => {
                if let Some(edit) = &mut self.editing {
                    edit.mode = (edit.mode + 1) % 3;
                    edit.scroll = 0;
                    let mode = edit.mode;
                    self.label(
                        PREVIEW,
                        match mode {
                            0 => "编辑模式",
                            1 => "分屏预览",
                            _ => "阅读模式",
                        },
                    );
                    self.layout();
                    if mode != 2 {
                        SetFocus(self.control(BODY));
                    }
                    self.redraw();
                }
            }
            PIN if notification == BN_CLICKED => {
                if let Some(id) = self.editing.as_ref().and_then(|edit| edit.id) {
                    let value = self
                        .wall
                        .notes
                        .iter()
                        .find(|note| note.id == id)
                        .is_some_and(|note| !note.pinned);
                    if self.persist_edit(false) {
                        self.commit(|wall| wall.set_pinned(id, value), "置顶状态已更新");
                        self.layout();
                    }
                    self.redraw();
                }
            }
            ARCHIVE if notification == BN_CLICKED => {
                if let Some(id) = self.editing.as_ref().and_then(|edit| edit.id) {
                    let value = self
                        .wall
                        .notes
                        .iter()
                        .find(|note| note.id == id)
                        .is_some_and(|note| !note.archived);
                    if self.persist_edit(false)
                        && self.commit(|wall| wall.set_archived(id, value), "归档状态已更新")
                    {
                        self.leave_edit();
                    }
                }
            }
            TRASH if notification == BN_CLICKED => {
                if let Some(id) = self.editing.as_ref().and_then(|edit| edit.id) {
                    let value = self
                        .wall
                        .notes
                        .iter()
                        .find(|note| note.id == id)
                        .is_some_and(|note| !note.deleted);
                    if self.persist_edit(false)
                        && self.commit(|wall| wall.set_deleted(id, value), "回收站状态已更新")
                    {
                        self.leave_edit();
                    }
                }
            }
            DELETE if notification == BN_CLICKED => {
                if let Some(id) = self.editing.as_ref().and_then(|edit| edit.id)
                    && message(
                        self.hwnd,
                        "删除这张便签及其正文？此操作不可撤销。",
                        MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING,
                    ) == IDYES
                    && self.commit(|wall| wall.notes.retain(|note| note.id != id), "便签已删除")
                {
                    self.body_dirty = false;
                    self.leave_edit();
                }
            }
            _ => {}
        }
    }

    unsafe fn resize(&mut self) {
        self.clamp_scroll();
        self.layout();
        self.redraw();
    }

    unsafe fn clamp_scroll(&mut self) {
        let area = self.canvas();
        if self.preferences.scroll_wall {
            let (extent_x, extent_y) =
                self.wall
                    .notes
                    .iter()
                    .fold((WALL_W, WALL_H), |(width, height), note| {
                        (
                            width.max(note.at.x + NOTE_W),
                            height.max(note.at.y + NOTE_H),
                        )
                    });
            self.offset.x = self
                .offset
                .x
                .clamp(0, (extent_x - area.right + area.left).max(0));
            self.offset.y = self
                .offset
                .y
                .clamp(0, (extent_y - area.bottom + area.top).max(0));
        } else {
            self.offset = Point::default();
        }
        if self.view_list() {
            self.list_scroll = self.list_scroll.clamp(
                0,
                (self.rows().len() as i32 * ROW - area.bottom + area.top).max(0),
            );
        }
    }

    unsafe fn wall_point(&self, point: Point) -> Point {
        Point {
            x: point.x - EDGE + self.offset.x,
            y: point.y - TOP + self.offset.y,
        }
    }

    unsafe fn placement_at(&self, point: Point) -> Point {
        let point = self.wall_point(point);
        self.clamp_note(Point {
            x: point.x - NOTE_W / 2,
            y: point.y - 16,
        })
    }

    unsafe fn wall_area(&self) -> RECT {
        self.canvas()
    }

    unsafe fn clamp_note(&self, at: Point) -> Point {
        let area = self.wall_area();
        let max_x = self.offset.x + (area.right - area.left - NOTE_W).max(0);
        let max_y = self.offset.y + (area.bottom - area.top - NOTE_H).max(0);
        Point {
            x: at.x.clamp(0, max_x),
            y: at.y.clamp(0, max_y),
        }
        .on_wall()
    }

    unsafe fn clamped_mouse(&self, point: Point) -> Point {
        let area = self.wall_area();
        Point {
            x: point.x.clamp(area.left, (area.right - 1).max(area.left)),
            y: point.y.clamp(area.top, (area.bottom - 1).max(area.top)),
        }
    }

    unsafe fn down(&mut self, point: Point) {
        if self.settings.is_some() {
            return;
        }
        if let Some(edit) = &self.editing {
            let area = self.editor_area();
            let preview_left = if edit.mode == 1 {
                area.left + 24 + (area.right - area.left - 48) / 2 + 12
            } else {
                area.left + 24
            };
            if edit.article
                && edit.mode != 0
                && inside(
                    rect(
                        preview_left,
                        edit.body_top(),
                        area.right - 24,
                        area.bottom - 149,
                    ),
                    point,
                )
            {
                let destination = self
                    .preview
                    .borrow()
                    .links
                    .iter()
                    .find(|(bounds, _)| inside(*bounds, point))
                    .map(|(_, link)| link.clone());
                if let Some(destination) = destination
                    && crate::preview::safe_link(&destination)
                {
                    if ShellExecuteW(
                        self.hwnd,
                        wide("open").as_ptr(),
                        wide(&destination).as_ptr(),
                        null(),
                        null(),
                        SW_SHOWNORMAL,
                    ) as isize
                        <= 32
                    {
                        self.status = "未能打开链接，请检查默认浏览器设置".into();
                        self.redraw();
                    }
                    return;
                }
            }
            if !edit.focused {
                for (index, paper) in Paper::ALL.into_iter().enumerate() {
                    if inside(swatch(area, index), point) && edit.paper != paper {
                        self.editing.as_mut().unwrap().paper = paper;
                        TITLE_BACKGROUND.with(|color| color.set(paper.colors().0));
                        InvalidateRect(self.control(TITLE), null(), 1);
                        self.redraw();
                        return;
                    }
                }
            }
            return;
        }
        if self.placement.is_none() && inside(self.search_area(), point) {
            SetFocus(self.control(SEARCH));
            return;
        }
        if !inside(self.canvas(), point) {
            return;
        }
        SetFocus(self.hwnd);
        if self.view_list() {
            if let Some(note) = self.list_hit(point) {
                self.begin_edit(Some(note.id), note.draft());
            }
            return;
        }
        if !inside(self.wall_area(), point) {
            return;
        }
        if let Some(draft) = self.placement.clone() {
            let at = self.placement_at(point);
            if self.commit(|wall| wall.place(draft, at), "已贴上墙 · 保存在本机") {
                self.placement = None;
                self.hover = None;
                self.query.clear();
                self.label(SEARCH, "");
                self.list = false;
                self.layout();
                self.redraw();
            }
        } else if let Some(note) = self.wall.hit(self.wall_point(point)) {
            self.drag = Some(Drag {
                id: note.id,
                start: self.wall_point(point),
                original: note.at,
                at: note.at,
                moved: false,
            });
            SetCapture(self.hwnd);
        }
    }

    unsafe fn motion(&mut self, point: Point) {
        self.pointer = Some(point);
        let row = if self.editing.is_none() && self.view_list() {
            self.list_hit(point).map(|note| note.id)
        } else {
            None
        };
        if self.row_hover.set(row) {
            self.start_motion();
        }
        if !self.cursor_tracking && (self.placement.is_some() || row.is_some()) {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            self.cursor_tracking = TrackMouseEvent(&mut tracking) != 0;
        }
        if self.placement.is_some() {
            self.hover = inside(self.wall_area(), point).then_some(point);
            if !self.cursor_tracking {
                let mut event = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: self.hwnd,
                    dwHoverTime: 0,
                };
                self.cursor_tracking = TrackMouseEvent(&mut event) != 0;
            }
            self.redraw();
        }
        let cursor = self.wall_point(self.clamped_mouse(point));
        let wall = self.wall_area();
        let offset = self.offset;
        if let Some(drag) = &mut self.drag {
            let delta = Point {
                x: cursor.x - drag.start.x,
                y: cursor.y - drag.start.y,
            };
            if drag.moved || delta.x.abs() > 5 || delta.y.abs() > 5 {
                drag.moved = true;
                drag.at = Point {
                    x: (drag.original.x + delta.x)
                        .clamp(0, offset.x + (wall.right - wall.left - NOTE_W).max(0)),
                    y: (drag.original.y + delta.y)
                        .clamp(0, offset.y + (wall.bottom - wall.top - NOTE_H).max(0)),
                }
                .on_wall();
                self.redraw();
            }
        }
    }

    unsafe fn up(&mut self, point: Point) {
        if self.drag.is_none() {
            return;
        }
        self.motion(point);
        let drag = self.drag.take().unwrap();
        ReleaseCapture();
        if drag.moved {
            if drag.at != drag.original {
                self.commit(|wall| wall.move_to(drag.id, drag.at), "已移动便签");
            }
        } else if let Some(note) = self.wall.notes.iter().find(|note| note.id == drag.id) {
            self.begin_edit(Some(note.id), note.draft());
        }
        self.redraw();
    }

    unsafe fn scroll(&mut self, delta: i32, horizontal: bool) {
        if self.settings.is_some() {
            return;
        }
        if let Some(edit) = &self.editing {
            if edit.article && edit.mode != 0 {
                let max = self.preview_height() + 9
                    - (self.editor_area().bottom - edit.body_top() - 149).max(42);
                self.editing.as_mut().unwrap().scroll = (edit.scroll + delta).clamp(0, max.max(0));
            }
        } else if self.view_list() {
            self.list_scroll += delta;
        } else if !self.preferences.scroll_wall {
            return;
        } else if horizontal {
            self.offset.x += delta;
        } else {
            self.offset.y += delta;
        }
        self.clamp_scroll();
        if let Some(point) = self.pointer {
            let row = if self.editing.is_none() && self.view_list() {
                self.list_hit(point).map(|note| note.id)
            } else {
                None
            };
            if self.row_hover.set(row) {
                self.start_motion();
            }
        }
        self.redraw();
    }

    unsafe fn preview_height(&self) -> i32 {
        let area = self.editor_area();
        let dc = GetDC(self.hwnd);
        let left = area.left + 24;
        let right = area.right - 24;
        let preview_left = if self.editing.as_ref().is_some_and(|edit| edit.mode == 1) {
            left + (right - left) / 2 + 12
        } else {
            left
        };
        let height = self.markdown(dc, preview_left + 12, 0, right - 12, false);
        if !dc.is_null() {
            ReleaseDC(self.hwnd, dc);
        }
        height
    }
}
