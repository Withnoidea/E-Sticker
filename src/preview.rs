#![allow(unsafe_op_in_unsafe_fn)]

use crate::{
    assets,
    markdown::{self, Block, Kind, Span},
    theme::*,
    ui::{fill, rect, text},
};
use std::{
    collections::BTreeMap,
    mem::zeroed,
    path::{Path, PathBuf},
    ptr::null_mut,
    time::SystemTime,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Gdi::*, GdiPlus::*},
};

struct Image {
    handle: *mut GpImage,
    width: u32,
    height: u32,
    modified: Option<SystemTime>,
    length: u64,
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.handle);
        }
    }
}

pub struct Preview {
    fonts: [HFONT; 32],
    headings: [HFONT; 6],
    images: BTreeMap<PathBuf, Image>,
    code: BTreeMap<(String, String), Vec<Span>>,
    token: usize,
    pub links: Vec<(RECT, String)>,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

pub fn safe_link(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    !value.chars().any(char::is_control)
        && ["https://", "http://", "mailto:"]
            .iter()
            .any(|prefix| lowered.starts_with(prefix))
}

impl Preview {
    pub unsafe fn new() -> Self {
        let fonts = std::array::from_fn(|index| {
            CreateFontW(
                if index & 8 != 0 { -14 } else { -15 },
                0,
                0,
                0,
                if index & 1 != 0 { 700 } else { 400 },
                (index & 2 != 0) as u32,
                (index & 16 != 0) as u32,
                (index & 4 != 0) as u32,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide(if index & 8 != 0 {
                    "Consolas"
                } else {
                    "Microsoft YaHei UI"
                })
                .as_ptr(),
            )
        });
        let headings = [27, 24, 21, 19, 17, 16].map(|height| {
            CreateFontW(
                -height,
                0,
                0,
                0,
                700,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide("Microsoft YaHei UI").as_ptr(),
            )
        });
        let mut token = 0;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            SuppressExternalCodecs: 1,
            ..zeroed()
        };
        if GdiplusStartup(&mut token, &input, null_mut()) != 0 {
            token = 0;
        }
        Self {
            fonts,
            headings,
            images: BTreeMap::new(),
            code: BTreeMap::new(),
            token,
            links: Vec::new(),
        }
    }

    pub unsafe fn render(
        &mut self,
        dc: HDC,
        source: &str,
        directory: &Path,
        bounds: RECT,
        draw: bool,
    ) -> i32 {
        if draw {
            self.links.clear();
        }
        if source.trim().is_empty() {
            if draw {
                text(
                    dc,
                    self.fonts[0],
                    "还没有正文，切换到编辑模式即可开始。",
                    rect(bounds.left, bounds.top, bounds.right, bounds.top + 30),
                    MUTED,
                    DT_WORDBREAK,
                );
            }
            return 30;
        }
        let mut ypos = bounds.top;
        let mut blocks = markdown::parse(source);
        for block in &mut blocks {
            let Some(language) = block.language.as_deref() else {
                continue;
            };
            if block.kind != Kind::Code {
                continue;
            }
            let key = (language.to_ascii_lowercase(), block.text.clone());
            if !self.code.contains_key(&key)
                && let Some(spans) = markdown::highlight_code(&block.text, language)
            {
                if self.code.len() >= 8 {
                    self.code.clear();
                }
                self.code.insert(key.clone(), spans);
            }
            if let Some(spans) = self.code.get(&key) {
                block.spans.clone_from(spans);
            }
        }
        for block in blocks {
            let area = rect(bounds.left, ypos, bounds.right, bounds.bottom);
            if block.kind == Kind::Rule {
                if draw {
                    fill(
                        dc,
                        rect(bounds.left, ypos + 8, bounds.right, ypos + 9),
                        BORDER,
                    );
                }
                ypos += 20;
            } else if block.kind == Kind::Table {
                ypos += self.table(dc, &block, area, draw) + 10;
            } else if block.kind == Kind::Image {
                ypos += self.image(dc, &block, directory, area, draw) + 10;
            } else {
                let indent = match block.kind {
                    Kind::Quote => 14,
                    Kind::Code => 10,
                    Kind::List => 8,
                    _ => 0,
                };
                let left = bounds.left + indent;
                let right = bounds.right - if block.kind == Kind::Code { 10 } else { 0 };
                let height =
                    self.rich_text(dc, &block, rect(left, ypos, right, bounds.bottom), false);
                if draw {
                    if block.kind == Kind::Code {
                        fill(
                            dc,
                            rect(bounds.left, ypos - 2, bounds.right, ypos + height + 3),
                            SOFT,
                        );
                    }
                    if block.kind == Kind::Quote {
                        fill(
                            dc,
                            rect(bounds.left, ypos, bounds.left + 3, ypos + height),
                            ACCENT,
                        );
                    }
                    self.rich_text(dc, &block, rect(left, ypos, right, bounds.bottom), true);
                }
                ypos += height
                    + if matches!(block.kind, Kind::Heading(_)) {
                        12
                    } else {
                        8
                    };
            }
        }
        (ypos - bounds.top).max(28)
    }

    unsafe fn rich_text(&mut self, dc: HDC, block: &Block, bounds: RECT, draw: bool) -> i32 {
        let heading = if let Kind::Heading(level) = block.kind {
            Some(self.headings[(level as usize).saturating_sub(1).min(5)])
        } else {
            None
        };
        let previous = SelectObject(dc, heading.unwrap_or(self.fonts[0]));
        let mut metrics: TEXTMETRICW = zeroed();
        GetTextMetricsW(dc, &mut metrics);
        let line_height = metrics.tmHeight + 4;
        let mut xpos = bounds.left;
        let mut ypos = bounds.top;
        for span in &block.spans {
            let mask = span.style
                | if span.link.is_some() { 16 } else { 0 }
                | if block.kind == Kind::Code { 8 } else { 0 };
            let face = heading.unwrap_or(self.fonts[mask as usize]);
            SelectObject(dc, face);
            for (line_index, line) in span.text.split('\n').enumerate() {
                if line_index > 0 {
                    xpos = bounds.left;
                    ypos += line_height;
                }
                let units: Vec<_> = line.trim_end_matches('\r').encode_utf16().collect();
                let mut index = 0;
                while index < units.len() {
                    let available = (bounds.right - xpos).max(1);
                    let mut end = (index + 512).min(units.len());
                    if end < units.len() && (0xd800..=0xdbff).contains(&units[end - 1]) {
                        end += 1;
                    }
                    let remaining = &units[index..end];
                    let mut fit = 0i32;
                    let mut size: SIZE = zeroed();
                    GetTextExtentExPointW(
                        dc,
                        remaining.as_ptr(),
                        remaining.len() as i32,
                        available,
                        &mut fit,
                        null_mut(),
                        &mut size,
                    );
                    if fit == 0 && xpos > bounds.left {
                        xpos = bounds.left;
                        ypos += line_height;
                        continue;
                    }
                    let mut count = (fit as usize).min(remaining.len());
                    if count > 0
                        && count < remaining.len()
                        && (0xd800..=0xdbff).contains(&remaining[count - 1])
                    {
                        count -= 1;
                    }
                    if count == 0 {
                        count = if (0xd800..=0xdbff).contains(&remaining[0]) && remaining.len() > 1
                        {
                            2
                        } else {
                            1
                        };
                    }
                    if count < remaining.len()
                        && let Some(space) = remaining[..count].iter().rposition(|unit| *unit == 32)
                        && space > 0
                    {
                        count = space + 1;
                    }
                    GetTextExtentPoint32W(dc, remaining.as_ptr(), count as i32, &mut size);
                    let area = rect(xpos, ypos, xpos + size.cx, ypos + line_height);
                    if draw {
                        if span.style & 8 != 0 {
                            fill(dc, area, SOFT);
                        }
                        text(
                            dc,
                            face,
                            &String::from_utf16_lossy(&remaining[..count]),
                            area,
                            if let Some((red, green, blue)) = span.rgb {
                                crate::ui::rgb(
                                    (u32::from(red) << 16)
                                        | (u32::from(green) << 8)
                                        | u32::from(blue),
                                )
                            } else if span.link.is_some() || block.kind == Kind::Quote {
                                ACCENT
                            } else {
                                INK
                            },
                            DT_SINGLELINE,
                        );
                        if let Some(link) = &span.link {
                            self.links.push((area, link.clone()));
                        }
                    }
                    xpos += size.cx;
                    index += count;
                    if index < units.len() {
                        xpos = bounds.left;
                        ypos += line_height;
                    }
                }
            }
        }
        SelectObject(dc, previous);
        ypos - bounds.top + line_height
    }

    unsafe fn table(&self, dc: HDC, block: &Block, bounds: RECT, draw: bool) -> i32 {
        let rows: Vec<Vec<_>> = block
            .text
            .lines()
            .map(|line| line.trim_end_matches('\t').split('\t').collect())
            .collect();
        let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let mut ypos = bounds.top;
        for (row_index, row) in rows.iter().enumerate() {
            let face = self.fonts[if row_index == 0 { 1 } else { 0 }];
            let mut height = 22;
            for (column, value) in row.iter().enumerate() {
                let left =
                    bounds.left + (bounds.right - bounds.left) * column as i32 / columns as i32;
                let right = bounds.left
                    + (bounds.right - bounds.left) * (column + 1) as i32 / columns as i32;
                height = height.max(text(
                    dc,
                    face,
                    value,
                    rect(left + 6, 0, (right - 6).max(left + 7), 1),
                    INK,
                    DT_WORDBREAK | DT_CALCRECT,
                ));
            }
            height += 12;
            if draw {
                fill(
                    dc,
                    rect(bounds.left, ypos, bounds.right, ypos + height),
                    if row_index == 0 { SOFT } else { WHITE },
                );
                for column in 0..columns {
                    let left =
                        bounds.left + (bounds.right - bounds.left) * column as i32 / columns as i32;
                    let right = bounds.left
                        + (bounds.right - bounds.left) * (column + 1) as i32 / columns as i32;
                    fill(dc, rect(left, ypos, left + 1, ypos + height), BORDER);
                    text(
                        dc,
                        face,
                        row.get(column).copied().unwrap_or(""),
                        rect(
                            left + 6,
                            ypos + 6,
                            (right - 6).max(left + 7),
                            ypos + height - 5,
                        ),
                        INK,
                        DT_WORDBREAK,
                    );
                }
                fill(
                    dc,
                    rect(bounds.right - 1, ypos, bounds.right, ypos + height),
                    BORDER,
                );
                fill(dc, rect(bounds.left, ypos, bounds.right, ypos + 1), BORDER);
                fill(
                    dc,
                    rect(bounds.left, ypos + height - 1, bounds.right, ypos + height),
                    BORDER,
                );
            }
            ypos += height;
        }
        ypos - bounds.top
    }

    unsafe fn load_image(&mut self, path: &Path) -> Option<(*mut GpImage, u32, u32)> {
        if self.token == 0 {
            return None;
        }
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() || metadata.len() > assets::MAX_IMAGE_BYTES {
            return None;
        }
        let modified = metadata.modified().ok();
        if let Some(image) = self.images.get(path)
            && image.modified == modified
            && image.length == metadata.len()
        {
            return Some((image.handle, image.width, image.height));
        }
        if self.images.len() >= 32 {
            self.images.clear();
        }
        let mut handle = null_mut();
        if GdipLoadImageFromFile(wide(&path.to_string_lossy()).as_ptr(), &mut handle) != 0
            || handle.is_null()
        {
            return None;
        }
        let mut width = 0;
        let mut height = 0;
        GdipGetImageWidth(handle, &mut width);
        GdipGetImageHeight(handle, &mut height);
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 64_000_000 {
            GdipDisposeImage(handle);
            return None;
        }
        self.images.insert(
            path.to_path_buf(),
            Image {
                handle,
                width,
                height,
                modified,
                length: metadata.len(),
            },
        );
        Some((handle, width, height))
    }

    unsafe fn image(
        &mut self,
        dc: HDC,
        block: &Block,
        directory: &Path,
        bounds: RECT,
        draw: bool,
    ) -> i32 {
        let destination = block.destination.as_deref().unwrap_or("");
        if let Some(relative) = assets::relative_image(destination)
            && let Some(path) = assets::confined_path(directory, &relative).ok()
            && let Some((image, width, height)) = self.load_image(&path)
        {
            let scale = (f64::from((bounds.right - bounds.left).max(1)) / f64::from(width))
                .min(400.0 / f64::from(height))
                .min(1.0);
            let width = (f64::from(width) * scale).max(1.0) as i32;
            let height = (f64::from(height) * scale).max(1.0) as i32;
            if draw {
                let mut graphics = null_mut();
                if GdipCreateFromHDC(dc, &mut graphics) == 0 {
                    let mut clip: RECT = zeroed();
                    GetClipBox(dc, &mut clip);
                    GdipSetClipRectI(
                        graphics,
                        clip.left,
                        clip.top,
                        clip.right - clip.left,
                        clip.bottom - clip.top,
                        CombineModeReplace,
                    );
                    GdipSetInterpolationMode(graphics, InterpolationModeHighQualityBicubic);
                    GdipDrawImageRectI(graphics, image, bounds.left, bounds.top, width, height);
                    GdipDeleteGraphics(graphics);
                }
                text(
                    dc,
                    self.fonts[0],
                    &block.text,
                    rect(
                        bounds.left,
                        bounds.top + height + 4,
                        bounds.right,
                        bounds.top + height + 27,
                    ),
                    SECONDARY,
                    DT_SINGLELINE | DT_END_ELLIPSIS,
                );
            }
            return height + 30;
        }
        let label = format!(
            "图片：{}\n{}",
            block.text,
            if destination.contains("://") {
                format!("远程图片不会自动联网下载：{destination}")
            } else {
                format!("无法读取本地图片：{destination}")
            }
        );
        let height = text(
            dc,
            self.fonts[0],
            &label,
            rect(bounds.left, 0, bounds.right, 1),
            SECONDARY,
            DT_WORDBREAK | DT_CALCRECT,
        )
        .max(44);
        if draw {
            text(
                dc,
                self.fonts[0],
                &label,
                rect(bounds.left, bounds.top, bounds.right, bounds.top + height),
                SECONDARY,
                DT_WORDBREAK,
            );
        }
        height
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        unsafe {
            self.images.clear();
            for face in self.fonts.into_iter().chain(self.headings) {
                DeleteObject(face);
            }
            if self.token != 0 {
                GdiplusShutdown(self.token);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::safe_link;

    #[test]
    fn links_reject_executable_and_script_schemes() {
        for value in [
            "https://example.com",
            "HTTP://example.com",
            "mailto:hello@example.com",
        ] {
            assert!(safe_link(value));
        }
        for value in [
            "javascript:alert(1)",
            "file:///C:/Windows/notepad.exe",
            "C:\\test.exe",
            "https://example.com\r\nrun",
        ] {
            assert!(!safe_link(value));
        }
    }
}
