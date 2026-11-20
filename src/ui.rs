#![allow(unsafe_op_in_unsafe_fn)]

use crate::model::{self, Draft, NOTE_H, NOTE_W, Paper, Point, Sort, WALL_H, WALL_W, Wall};
use std::{
    cell::RefCell,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, EM_GETLINECOUNT, EM_LIMITTEXT, EM_SETCUEBANNER, EM_SETRECTNP, EM_SETSEL,
    ODS_FOCUS, ODS_SELECTED, WM_MOUSELEAVE,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const INK: u32 = 0x302b25;
const MUTED: u32 = 0x675c51;
const SHELL: u32 = 0xf8f5ef;
const WHITE: u32 = 0xfffdf8;
const WALL: u32 = 0xece6db;
const BORDER: u32 = 0xded5c7;
const ACCENT: u32 = 0x3f5848;
const PAPER_TEXT: u32 = DT_WORDBREAK | DT_EDITCONTROL;
const TOP: i32 = 132;
const EDGE: i32 = 22;
const ROW: i32 = 81;
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
const BODY_TOP: i32 = TOP + 222;

thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}
fn inside(area: RECT, point: Point) -> bool {
    (area.left..area.right).contains(&point.x) && (area.top..area.bottom).contains(&point.y)
}
fn mouse(value: LPARAM) -> Point {
    Point {
        x: value as i16 as i32,
        y: (value >> 16) as i16 as i32,
    }
}
fn rgb(hex: u32) -> u32 {
    ((hex & 255) << 16) | (hex & 0xff00) | (hex >> 16)
}

#[derive(Clone)]
struct Editing {
    id: Option<u64>,
    paper: Paper,
    preview: bool,
    scroll: i32,
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
    wall: Wall,
    controls: [HWND; 10],
    fonts: [HFONT; 5],
    query: String,
    sort: Sort,
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

unsafe fn fill(dc: HDC, area: RECT, color: u32) {
    let brush = CreateSolidBrush(rgb(color));
    FillRect(dc, &area, brush);
    DeleteObject(brush);
}

unsafe fn rounded(dc: HDC, area: RECT, radius: i32, background: u32, border: u32) {
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
        radius,
        radius,
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

unsafe fn sheet(dc: HDC, area: RECT, paper: Paper, flying: bool, backdrop: u32) {
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

unsafe fn text(dc: HDC, font: HFONT, label: &str, mut area: RECT, color: u32, flags: u32) -> i32 {
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
    MessageBoxW(hwnd, wide(body).as_ptr(), wide("E-Sticker").as_ptr(), flags)
}

unsafe fn control_text(handle: HWND) -> String {
    let mut value = vec![0; GetWindowTextLengthW(handle) as usize + 1];
    let count = GetWindowTextW(handle, value.as_mut_ptr(), value.len() as i32);
    String::from_utf16_lossy(&value[..count as usize])
}

unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    let id = item.CtlID as usize;
    let primary = matches!(id, NEW | SAVE);
    let pressed = item.itemState & ODS_SELECTED != 0;
    let focused = item.itemState & ODS_FOCUS != 0;
    let base = if primary { ACCENT } else { WHITE };
    let background = if pressed { mix(base, INK, 13) } else { base };
    let edge = if focused {
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
        if matches!(id, NEW | VIEW) {
            SHELL
        } else {
            WHITE
        },
    );
    rounded(item.hDC, item.rcItem, 12, background, edge);
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

impl App {
    unsafe fn paper(
        &self,
        dc: HDC,
        at: Point,
        title: &str,
        paper: Paper,
        flying: bool,
        body: bool,
    ) {
        let left = EDGE + at.x - self.offset.x;
        let top = TOP + at.y - self.offset.y;
        let right = left + NOTE_W;
        let bottom = top + NOTE_H;
        let area = rect(left, top, right, bottom);
        sheet(dc, area, paper, flying, WALL);
        text(dc, self.fonts[2], title, paper_text(area), INK, PAPER_TEXT);
        if body {
            text(
                dc,
                self.fonts[3],
                "正文  ↗",
                rect(left + 12, bottom - 17, right - 11, bottom - 2),
                INK,
                DT_RIGHT | DT_SINGLELINE,
            );
        }
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
        fill(dc, wall, WALL);
        let brush = CreateSolidBrush(rgb(0xdad0c1));
        for ypos in ((wall.top + 9 - self.offset.y.rem_euclid(17))..wall.bottom).step_by(17) {
            for xpos in ((wall.left + 9 - self.offset.x.rem_euclid(17))..wall.right).step_by(17) {
                FillRect(dc, &rect(xpos, ypos, xpos + 1, ypos + 1), brush);
            }
        }
        DeleteObject(brush);
        let moving = self.drag.as_ref().filter(|drag| drag.moved);
        for note in self
            .wall
            .notes
            .iter()
            .filter(|note| !moving.is_some_and(|drag| drag.id == note.id))
        {
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
                    !note.body.trim().is_empty(),
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
                !note.body.trim().is_empty(),
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
                false,
            );
        } else if self.wall.notes.is_empty() && self.placement.is_none() {
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
        let area = self.canvas();
        fill(dc, area, BORDER);
        let inner = rect(area.left + 1, area.top + 1, area.right - 1, area.bottom - 1);
        fill(dc, inner, WHITE);
        let saved = SaveDC(dc);
        IntersectClipRect(dc, inner.left, inner.top, inner.right, inner.bottom);
        let rows = self.wall.rows(&self.query, self.sort);
        if rows.is_empty() {
            text(
                dc,
                self.fonts[4],
                if self.query.trim().is_empty() {
                    "还没有便签"
                } else {
                    "没找到匹配的便签"
                },
                rect(inner.left, inner.top + 55, inner.right, inner.top + 95),
                INK,
                DT_CENTER | DT_SINGLELINE,
            );
        }
        for (index, note) in rows
            .iter()
            .enumerate()
            .skip((self.list_scroll / ROW) as usize)
        {
            let top = TOP + index as i32 * ROW - self.list_scroll;
            if top >= inner.bottom {
                break;
            }
            let (color, _) = note.paper.colors();
            fill(
                dc,
                rect(inner.left + 15, top + 16, inner.left + 21, top + 62),
                color,
            );
            text(
                dc,
                self.fonts[4],
                &note.title,
                rect(inner.left + 33, top + 13, inner.right - 150, top + 39),
                INK,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            text(
                dc,
                self.fonts[3],
                &model::date(note.edited),
                rect(inner.right - 144, top + 18, inner.right - 16, top + 39),
                MUTED,
                DT_RIGHT | DT_SINGLELINE,
            );
            let excerpt = model::excerpt(note, &self.query);
            text(
                dc,
                self.fonts[0],
                if excerpt.is_empty() {
                    "只有墙面短句"
                } else {
                    &excerpt
                },
                rect(inner.left + 33, top + 46, inner.right - 16, top + 69),
                MUTED,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            fill(
                dc,
                rect(inner.left + 15, top + ROW - 1, inner.right - 15, top + ROW),
                BORDER,
            );
        }
        RestoreDC(dc, saved);
    }

    unsafe fn markdown(&self, dc: HDC, left: i32, top: i32, right: i32, draw: bool) -> i32 {
        let source = self.value(BODY);
        if source.trim().is_empty() {
            if draw {
                text(
                    dc,
                    self.fonts[0],
                    "还没有正文。选择「编辑正文」补写。",
                    rect(left, top, right, top + 28),
                    MUTED,
                    DT_SINGLELINE,
                );
            }
            return 28;
        }
        let mut ypos = top;
        let mut code = false;
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                code = !code;
                ypos += 8;
                continue;
            }
            if trimmed.is_empty() {
                ypos += 13;
                continue;
            }
            let (label, face, color, indent) = if code {
                (line.to_string(), self.fonts[0], ACCENT, 12)
            } else if let Some(heading) = trimmed
                .strip_prefix("### ")
                .or_else(|| trimmed.strip_prefix("## "))
                .or_else(|| trimmed.strip_prefix("# "))
            {
                (heading.to_string(), self.fonts[4], INK, 0)
            } else if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                (format!("•  {item}"), self.fonts[0], INK, 10)
            } else if let Some(quote) = trimmed.strip_prefix("> ") {
                (format!("│  {quote}"), self.fonts[0], ACCENT, 10)
            } else {
                (trimmed.to_string(), self.fonts[0], INK, 0)
            };
            let label = if code {
                label
            } else {
                label.replace("**", "").replace("__", "").replace('`', "")
            };
            let area = rect(left + indent, ypos, right, ypos + 1);
            let height = text(dc, face, &label, area, color, DT_WORDBREAK | DT_CALCRECT);
            if draw {
                text(
                    dc,
                    face,
                    &label,
                    rect(area.left, ypos, right, ypos + height + 1),
                    color,
                    DT_WORDBREAK,
                );
            }
            ypos += height.max(16) + 6;
        }
        ypos - top
    }

    unsafe fn draw_editor(&self, dc: HDC, edit: &Editing) {
        let area = self.editor_area();
        let left = area.left + 24;
        let right = area.right - 24;
        rounded(dc, area, 14, WHITE, BORDER);
        let sheet_area = editor_paper(area);
        sheet(dc, sheet_area, edit.paper, false, WHITE);
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
        if let Some(note) = edit
            .id
            .and_then(|id| self.wall.notes.iter().find(|note| note.id == id))
        {
            text(
                dc,
                self.fonts[0],
                "详细内容  ·  Markdown",
                rect(left, BODY_TOP - 33, right, BODY_TOP - 5),
                INK,
                DT_SINGLELINE,
            );
            if edit.preview {
                let preview = rect(left, BODY_TOP, right, area.bottom - 66);
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
                    left + 12,
                    preview.top + 9 - edit.scroll,
                    right - 12,
                    true,
                );
                RestoreDC(dc, state);
            }
            let dates = format!(
                "创建 {}   ·   最近编辑 {}",
                model::date(note.created),
                model::date(note.edited)
            );
            text(
                dc,
                self.fonts[3],
                &dates,
                rect(left, area.bottom - 62, right, area.bottom - 44),
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

    unsafe fn paint(&self, dc: HDC) {
        let area = self.client();
        fill(dc, area, SHELL);
        let count = self.wall.notes.len();
        fill(dc, rect(EDGE, 25, EDGE + 25, 48), ACCENT);
        fill(dc, rect(EDGE + 5, 30, EDGE + 20, 32), 0xd6e4cb);
        fill(dc, rect(EDGE + 5, 36, EDGE + 16, 38), 0xd6e4cb);
        text(
            dc,
            self.fonts[4],
            "E-Sticker",
            rect(EDGE + 34, 24, 160, 53),
            INK,
            DT_SINGLELINE,
        );
        fill(dc, rect(EDGE, 67, area.right - EDGE, 68), BORDER);
        if self.editing.is_none() && self.placement.is_none() {
            let search = self.search_area();
            rounded(
                dc,
                search,
                14,
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
        let title = if let Some(edit) = &self.editing {
            if edit.id.is_some() {
                "便签详情"
            } else {
                "写一张便签"
            }
        } else if self.placement.is_some() {
            "找个位置贴上"
        } else if !self.query.trim().is_empty() {
            "搜索结果"
        } else if self.view_list() {
            "文章列表"
        } else {
            "我的便签墙"
        };
        text(
            dc,
            self.fonts[1],
            title,
            rect(EDGE, 80, area.right - EDGE, 120),
            INK,
            DT_SINGLELINE,
        );
        if self.placement.is_some() {
            text(
                dc,
                self.fonts[0],
                "单击贴住  ·  Esc 返回输入",
                rect(EDGE + 220, 94, area.right - EDGE, 118),
                ACCENT,
                DT_SINGLELINE,
            );
        }
        if let Some(edit) = &self.editing {
            self.draw_editor(dc, edit);
        } else if self.view_list() {
            self.draw_list(dc);
        } else {
            self.draw_wall(dc);
        }
        let hint = if self.editing.is_some() {
            "Enter 换行  ·  Ctrl+Enter 确认  ·  Esc 取消"
        } else if self.placement.is_some() {
            "移动纸张预览位置  ·  可以贴在另一张纸上"
        } else if self.view_list() {
            "点击打开详情  ·  列表排序不影响墙面"
        } else {
            "单击看详情  ·  拖动移开  ·  滚轮纵向／Shift+滚轮横向"
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
            if self.view_list() && !self.query.trim().is_empty() {
                format!("{} 张匹配", self.wall.rows(&self.query, self.sort).len())
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
        WM_GETMINMAXINFO => {
            let info = &mut *(lp as *mut MINMAXINFO);
            info.ptMinTrackSize.x = 610;
            info.ptMinTrackSize.y = 530;
        }
        WM_SIZE => with_app(|app| app.resize()),
        WM_ERASEBKGND => return 1,
        WM_DRAWITEM => {
            let item = &*(lp as *const DRAWITEMSTRUCT);
            if matches!(
                item.CtlID as usize,
                NEW | VIEW | SAVE | CANCEL | PREVIEW | DELETE
            ) {
                draw_button(item);
                return 1;
            }
        }
        WM_CTLCOLOREDIT => {
            let id = GetDlgCtrlID(lp as HWND) as usize;
            if matches!(id, TITLE | SEARCH) {
                let mut background = WHITE;
                if id == TITLE {
                    with_app(|app| {
                        if let Some(edit) = &app.editing {
                            background = edit.paper.colors().0;
                        }
                    });
                }
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
            with_app(|app| {
                let area = app.client();
                if area.right <= 0 || area.bottom <= 0 {
                    return;
                }
                let buffer = CreateCompatibleDC(dc);
                let bitmap = CreateCompatibleBitmap(dc, area.right, area.bottom);
                if buffer.is_null() || bitmap.is_null() {
                    app.paint(dc);
                } else {
                    let previous = SelectObject(buffer, bitmap);
                    app.paint(buffer);
                    BitBlt(dc, 0, 0, area.right, area.bottom, buffer, 0, 0, SRCCOPY);
                    SelectObject(buffer, previous);
                }
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                if !buffer.is_null() {
                    DeleteDC(buffer);
                }
            });
            EndPaint(hwnd, &paint);
        }
        WM_COMMAND => with_app(|app| app.command(wp & 0xffff, (wp >> 16) as u32 & 0xffff)),
        WM_LBUTTONDOWN => with_app(|app| app.down(mouse(lp))),
        WM_LBUTTONUP => with_app(|app| app.up(mouse(lp))),
        WM_MOUSEMOVE => with_app(|app| app.motion(mouse(lp))),
        WM_MOUSELEAVE => with_app(|app| {
            app.cursor_tracking = false;
            if app.hover.take().is_some() {
                app.redraw();
            }
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
        WM_DESTROY => PostQuitMessage(0),
        _ => return DefWindowProcW(hwnd, event, wp, lp),
    }
    0
}

pub fn run() {
    unsafe {
        let wall = model::path().and_then(|path| {
            let lock = model::lock(&path)?;
            Wall::open(&path).map(|wall| (path, wall, lock))
        });
        let (path, wall, _lock) = match wall {
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
            wide("E-Sticker · 便签墙").as_ptr(),
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
        let app = App::new(hwnd, path, wall);
        APP.with(|slot| *slot.borrow_mut() = Some(app));
        ShowWindow(hwnd, SW_SHOW);
        let mut event = zeroed::<MSG>();
        while GetMessageW(&mut event, null_mut(), 0, 0) > 0 {
            let mut handled = false;
            if event.message == WM_KEYDOWN {
                with_app(|app| {
                    let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
                    let combo_open = SendMessageW(app.control(SORT), CB_GETDROPPEDSTATE, 0, 0) != 0;
                    match event.wParam as u16 {
                        VK_ESCAPE if !combo_open => {
                            app.cancel();
                            handled = true;
                        }
                        VK_RETURN if app.editing.is_some() && ctrl => {
                            app.save_edit();
                            handled = true;
                        }
                        0x4e if ctrl && app.editing.is_none() && app.placement.is_none() => {
                            app.command(NEW, BN_CLICKED);
                            handled = true;
                        }
                        0x46 if ctrl && app.editing.is_none() && app.placement.is_none() => {
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
    }
}

impl App {
    unsafe fn new(hwnd: HWND, path: PathBuf, wall: Wall) -> Self {
        let definitions = [
            (NEW, "BUTTON", "＋ 新建便签", BS_OWNERDRAW as u32),
            (VIEW, "BUTTON", "文章列表", BS_OWNERDRAW as u32),
            (SEARCH, "EDIT", "", ES_AUTOHSCROLL as u32),
            (SORT, "COMBOBOX", "", CBS_DROPDOWNLIST as u32 | WS_VSCROLL),
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
                (ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL) as u32 | WS_VSCROLL | WS_BORDER,
            ),
            (SAVE, "BUTTON", "去贴上 →", BS_OWNERDRAW as u32),
            (CANCEL, "BUTTON", "取消", BS_OWNERDRAW as u32),
            (PREVIEW, "BUTTON", "预览正文", BS_OWNERDRAW as u32),
            (DELETE, "BUTTON", "删除便签", BS_OWNERDRAW as u32),
        ];
        let fonts = [
            font("Microsoft YaHei UI", 15, 400),
            font("Microsoft YaHei UI", 30, 700),
            font("Microsoft YaHei UI", 17, 400),
            font("Microsoft YaHei UI", 12, 400),
            font("Microsoft YaHei UI", 19, 700),
        ];
        let controls = definitions.map(|(id, class, label, style)| {
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
            handle
        });
        let app = Self {
            hwnd,
            path,
            wall,
            controls,
            fonts,
            query: String::new(),
            sort: Sort::Edited,
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
        };
        for label in ["最近编辑", "创建时间", "按短句排序"] {
            SendMessageW(
                app.control(SORT),
                CB_ADDSTRING,
                0,
                wide(label).as_ptr() as LPARAM,
            );
        }
        SendMessageW(app.control(SORT), CB_SETCURSEL, 0, 0);
        SendMessageW(app.control(TITLE), EM_LIMITTEXT, 48, 0);
        SendMessageW(app.control(BODY), EM_LIMITTEXT, 100_000, 0);
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
        self.controls[id - NEW]
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
        self.placement.is_none() && (self.list || !self.query.trim().is_empty())
    }

    unsafe fn canvas(&self) -> RECT {
        let area = self.client();
        rect(EDGE, TOP, area.right - EDGE, area.bottom - 43)
    }

    unsafe fn editor_area(&self) -> RECT {
        let area = self.canvas();
        let width = (area.right - area.left).min(752);
        let left = (area.right + area.left - width) / 2;
        rect(left, area.top, left + width, area.bottom)
    }

    unsafe fn search_area(&self) -> RECT {
        let right = self.client().right;
        rect(right - 460, 21, right - 245, 57)
    }

    unsafe fn layout(&self) {
        let width = self.client().right;
        let editing = self.editing.as_ref();
        let active = editing.is_none() && self.placement.is_none();
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
        show(NEW, active, width - 131, 22, 109, 34);
        show(VIEW, active, width - 235, 22, 96, 34);
        let search = self.search_area();
        show(
            SEARCH,
            active,
            search.left + 29,
            search.top + 8,
            search.right - search.left - 40,
            22,
        );
        show(SORT, active && self.view_list(), width - 149, 90, 127, 190);
        let paper = paper_text(editor_paper(field));
        show(
            TITLE,
            editing.is_some(),
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
        show(
            BODY,
            detail && editing.is_some_and(|edit| !edit.preview),
            inner,
            BODY_TOP,
            end - inner,
            (field.bottom - BODY_TOP - 66).max(42),
        );
        show(SAVE, editing.is_some(), end - 109, button_y, 109, 30);
        show(CANCEL, editing.is_some(), end - 201, button_y, 82, 30);
        show(PREVIEW, detail, end - 109, BODY_TOP - 36, 109, 27);
        show(DELETE, detail, inner, button_y, 100, 30);
        self.label(
            VIEW,
            if !self.query.is_empty() {
                "清空搜索"
            } else if self.list {
                "回到墙面"
            } else {
                "文章列表"
            },
        );
    }

    unsafe fn redraw(&self) {
        InvalidateRect(self.hwnd, null(), 0);
    }

    unsafe fn begin_edit(&mut self, id: Option<u64>, draft: Draft) {
        self.placement = None;
        self.editing = Some(Editing {
            id,
            paper: draft.paper,
            preview: id.is_some() && !draft.body.is_empty(),
            scroll: 0,
        });
        self.status.clear();
        self.label(
            TITLE,
            &draft.title.replace("\r\n", "\n").replace('\n', "\r\n"),
        );
        self.label(
            BODY,
            &draft.body.replace("\r\n", "\n").replace('\n', "\r\n"),
        );
        self.label(
            SAVE,
            if id.is_some() {
                "保存修改"
            } else {
                "去贴上 →"
            },
        );
        self.label(
            PREVIEW,
            if self.editing.as_ref().is_some_and(|edit| edit.preview) {
                "编辑正文"
            } else {
                "预览正文"
            },
        );
        self.layout();
        SetFocus(self.control(TITLE));
        let end = GetWindowTextLengthW(self.control(TITLE)) as usize;
        SendMessageW(self.control(TITLE), EM_SETSEL, end, end as LPARAM);
        self.redraw();
    }

    unsafe fn leave_edit(&mut self) {
        self.editing = None;
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

    unsafe fn save_edit(&mut self) {
        let Some(edit) = self.editing.clone() else {
            return;
        };
        let title = self.value(TITLE).replace("\r\n", "\n");
        if title.trim().is_empty() {
            SetFocus(self.control(TITLE));
            self.status = "先写一句，再贴上墙".into();
            self.redraw();
            return;
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
            return;
        }
        let body = if edit.id.is_some() {
            self.value(BODY).replace("\r\n", "\n")
        } else {
            String::new()
        };
        let draft = Draft {
            title,
            body,
            paper: edit.paper,
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
                });
            if !changed
                || self.commit(
                    |wall| {
                        if let Some(note) = wall.notes.iter_mut().find(|note| note.id == id) {
                            note.title = draft.title;
                            note.body = draft.body;
                            note.paper = draft.paper;
                            note.edited = chrono::Utc::now().timestamp();
                        }
                    },
                    "已保存修改",
                )
            {
                self.leave_edit();
            }
        } else {
            self.placement = Some(draft);
            self.hover = None;
            self.status.clear();
            self.leave_edit();
        }
    }

    unsafe fn cancel(&mut self) {
        if let Some(draft) = self.placement.take() {
            self.hover = None;
            self.begin_edit(None, draft);
        } else if self.editing.is_some() {
            self.leave_edit();
        } else if self.drag.take().is_some() {
            ReleaseCapture();
            self.redraw();
        } else if !self.query.is_empty() {
            self.label(SEARCH, "");
            self.query.clear();
            self.list_scroll = 0;
            self.layout();
            self.redraw();
        }
    }

    unsafe fn command(&mut self, id: usize, notification: u32) {
        match id {
            NEW if notification == BN_CLICKED => {
                self.status.clear();
                self.begin_edit(None, Draft::default());
            }
            VIEW if notification == BN_CLICKED => {
                if !self.query.is_empty() {
                    self.label(SEARCH, "");
                    self.query.clear();
                } else {
                    self.list = !self.list;
                }
                self.list_scroll = 0;
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
            SEARCH if matches!(notification, EN_SETFOCUS | EN_KILLFOCUS) => self.redraw(),
            TITLE if notification == EN_CHANGE => {
                self.status.clear();
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
            SAVE if notification == BN_CLICKED => self.save_edit(),
            CANCEL if notification == BN_CLICKED => self.cancel(),
            PREVIEW if notification == BN_CLICKED => {
                if let Some(edit) = &mut self.editing {
                    edit.preview = !edit.preview;
                    edit.scroll = 0;
                    let preview = edit.preview;
                    self.label(
                        PREVIEW,
                        if preview {
                            "编辑正文"
                        } else {
                            "预览正文"
                        },
                    );
                    self.layout();
                    if !preview {
                        SetFocus(self.control(BODY));
                    }
                    self.redraw();
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
        self.offset.x = self
            .offset
            .x
            .clamp(0, (WALL_W - area.right + area.left).max(0));
        self.offset.y = self
            .offset
            .y
            .clamp(0, (WALL_H - area.bottom + area.top).max(0));
        if self.view_list() {
            self.list_scroll = self.list_scroll.clamp(
                0,
                (self.wall.rows(&self.query, self.sort).len() as i32 * ROW - area.bottom
                    + area.top)
                    .max(0),
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
        Point {
            x: point.x - NOTE_W / 2,
            y: point.y - 16,
        }
        .on_wall()
    }

    unsafe fn wall_area(&self) -> RECT {
        let mut area = self.canvas();
        area.right = area.right.min(EDGE + WALL_W - self.offset.x);
        area.bottom = area.bottom.min(TOP + WALL_H - self.offset.y);
        area
    }

    unsafe fn clamped_mouse(&self, point: Point) -> Point {
        let area = self.wall_area();
        Point {
            x: point.x.clamp(area.left, (area.right - 1).max(area.left)),
            y: point.y.clamp(area.top, (area.bottom - 1).max(area.top)),
        }
    }

    unsafe fn down(&mut self, point: Point) {
        if let Some(edit) = &self.editing {
            let area = self.editor_area();
            for (index, paper) in Paper::ALL.into_iter().enumerate() {
                if inside(swatch(area, index), point) && edit.paper != paper {
                    self.editing.as_mut().unwrap().paper = paper;
                    InvalidateRect(self.control(TITLE), null(), 1);
                    self.redraw();
                    return;
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
            let index = ((point.y - TOP + self.list_scroll) / ROW) as usize;
            if let Some(note) = self.wall.rows(&self.query, self.sort).get(index) {
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
        if let Some(drag) = &mut self.drag {
            let delta = Point {
                x: cursor.x - drag.start.x,
                y: cursor.y - drag.start.y,
            };
            if drag.moved || delta.x.abs() > 5 || delta.y.abs() > 5 {
                drag.moved = true;
                drag.at = Point {
                    x: drag.original.x + delta.x,
                    y: drag.original.y + delta.y,
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
        if let Some(edit) = &self.editing {
            if edit.preview {
                let max =
                    self.preview_height() + 9 - (self.editor_area().bottom - BODY_TOP - 66).max(42);
                self.editing.as_mut().unwrap().scroll = (edit.scroll + delta).clamp(0, max.max(0));
            }
        } else if self.view_list() {
            self.list_scroll += delta;
        } else if horizontal {
            self.offset.x += delta;
        } else {
            self.offset.y += delta;
        }
        self.clamp_scroll();
        self.redraw();
    }

    unsafe fn preview_height(&self) -> i32 {
        let area = self.editor_area();
        let dc = GetDC(self.hwnd);
        let height = self.markdown(dc, area.left + 36, 0, area.right - 36, false);
        if !dc.is_null() {
            ReleaseDC(self.hwnd, dc);
        }
        height
    }
}
