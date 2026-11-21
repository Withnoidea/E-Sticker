use chrono::{DateTime, Local, Utc};
#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const NOTE_W: i32 = 174;
pub const NOTE_H: i32 = 142;
pub const WALL_W: i32 = 1560;
pub const WALL_H: i32 = 1150;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
pub enum Paper {
    #[default]
    Yellow,
    Pink,
    Mint,
    Blue,
    Lilac,
}

impl Paper {
    pub const ALL: [Self; 5] = [
        Self::Yellow,
        Self::Pink,
        Self::Mint,
        Self::Blue,
        Self::Lilac,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Yellow => "杏黄",
            Self::Pink => "桃粉",
            Self::Mint => "薄荷",
            Self::Blue => "雾蓝",
            Self::Lilac => "藤紫",
        }
    }
    pub fn colors(self) -> (u32, u32) {
        match self {
            Self::Yellow => (0xffe9a1, 0xead17e),
            Self::Pink => (0xf8d5cf, 0xe7b8b0),
            Self::Mint => (0xd3e7c4, 0xb9d4a7),
            Self::Blue => (0xd1e5ef, 0xb1cddf),
            Self::Lilac => (0xe5ddf3, 0xcbbce2),
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn on_wall(self) -> Self {
        Self {
            x: self.x.clamp(0, i32::MAX / 2),
            y: self.y.clamp(0, i32::MAX / 2),
        }
    }
}

#[derive(Clone, Default)]
pub struct Draft {
    pub title: String,
    pub body: String,
    pub paper: Paper,
    pub tags: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Note {
    pub id: u64,
    pub title: String,
    pub body: String,
    pub paper: Paper,
    pub at: Point,
    pub created: i64,
    pub edited: i64,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub archived: bool,
    pub deleted: bool,
    pub width: i32,
    pub height: i32,
}

impl Default for Note {
    fn default() -> Self {
        Self {
            id: 0,
            title: String::new(),
            body: String::new(),
            paper: Paper::default(),
            at: Point::default(),
            created: 0,
            edited: 0,
            tags: Vec::new(),
            pinned: false,
            archived: false,
            deleted: false,
            width: NOTE_W,
            height: NOTE_H,
        }
    }
}

impl Note {
    pub fn draft(&self) -> Draft {
        Draft {
            title: self.title.clone(),
            body: self.body.clone(),
            paper: self.paper,
            tags: self.tags.clone(),
        }
    }
}

#[derive(Clone, Default)]
pub struct Wall {
    pub notes: Vec<Note>,
}

#[derive(Deserialize, Serialize)]
struct File {
    version: u32,
    notes: Vec<Note>,
}

#[derive(Clone, Copy, Default)]
pub enum Sort {
    #[default]
    Edited,
    Created,
    Title,
}

impl Wall {
    pub fn open(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let mut file: File = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if !(1..=2).contains(&file.version) {
                    return Err(io::Error::other("不支持此数据版本"));
                }
                let mut ids = std::collections::HashSet::new();
                if file
                    .notes
                    .iter()
                    .any(|note| note.id == u64::MAX || !ids.insert(note.id))
                {
                    return Err(io::Error::other("便签编号无效或重复"));
                }
                for note in &mut file.notes {
                    note.at = note.at.on_wall();
                    note.width = note.width.clamp(136, 280);
                    note.height = note.height.clamp(112, 240);
                }
                Ok(Self { notes: file.notes })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| io::Error::other("找不到存储目录"))?,
        )?;
        let temp = path.with_extension("tmp");
        let file = fs::File::create(&temp)?;
        serde_json::to_writer(
            &file,
            &File {
                version: 2,
                notes: self.notes.clone(),
            },
        )
        .map_err(io::Error::other)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temp, path)
    }

    pub fn place(&mut self, draft: Draft, at: Point) {
        let id = self
            .notes
            .iter()
            .map(|note| note.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX)
            .unwrap_or_else(|| {
                (1..u64::MAX)
                    .find(|id| !self.notes.iter().any(|note| note.id == *id))
                    .unwrap()
            });
        let time = Utc::now().timestamp();
        self.notes.push(Note {
            id,
            title: draft.title,
            body: draft.body,
            paper: draft.paper,
            at: at.on_wall(),
            created: time,
            edited: time,
            tags: draft.tags,
            pinned: false,
            archived: false,
            deleted: false,
            width: NOTE_W,
            height: NOTE_H,
        });
    }

    pub fn hit(&self, point: Point) -> Option<&Note> {
        self.notes.iter().rev().find(|note| {
            !note.archived
                && !note.deleted
                && (note.at.x..note.at.x + note.width.clamp(136, 280)).contains(&point.x)
                && (note.at.y..note.at.y + note.height.clamp(112, 240)).contains(&point.y)
        })
    }

    pub fn move_to(&mut self, id: u64, at: Point) {
        if let Some(index) = self.notes.iter().position(|note| note.id == id) {
            let mut note = self.notes.remove(index);
            note.at = at.on_wall();
            self.notes.push(note);
        }
    }

    pub fn set_pinned(&mut self, id: u64, value: bool) {
        if let Some(note) = self.notes.iter_mut().find(|note| note.id == id) {
            note.pinned = value;
            note.edited = Utc::now().timestamp();
        }
    }

    pub fn set_archived(&mut self, id: u64, value: bool) {
        if let Some(note) = self.notes.iter_mut().find(|note| note.id == id) {
            note.archived = value;
            note.edited = Utc::now().timestamp();
        }
    }

    pub fn set_deleted(&mut self, id: u64, value: bool) {
        if let Some(note) = self.notes.iter_mut().find(|note| note.id == id) {
            note.deleted = value;
            note.edited = Utc::now().timestamp();
        }
    }

    pub fn rows_in(&self, query: &str, sort: Sort, bucket: u8) -> Vec<&Note> {
        self.rows_in_filtered(query, "", sort, bucket)
    }

    pub fn rows_in_filtered(&self, query: &str, tag: &str, sort: Sort, bucket: u8) -> Vec<&Note> {
        let query = query.trim().to_lowercase();
        let tag = tag.trim().to_lowercase();
        let mut notes: Vec<_> = self
            .notes
            .iter()
            .filter(|note| {
                let visible = match bucket {
                    1 => note.archived && !note.deleted,
                    2 => note.deleted,
                    _ => !note.archived && !note.deleted,
                };
                visible
                    && (tag.is_empty() || note.tags.iter().any(|item| item.to_lowercase() == tag))
                    && (query.is_empty()
                        || note.title.to_lowercase().contains(&query)
                        || note.body.to_lowercase().contains(&query)
                        || note
                            .tags
                            .iter()
                            .any(|tag| tag.to_lowercase().contains(&query)))
            })
            .collect();
        notes.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then(match sort {
                    Sort::Edited => right.edited.cmp(&left.edited),
                    Sort::Created => right.created.cmp(&left.created),
                    Sort::Title => left.title.to_lowercase().cmp(&right.title.to_lowercase()),
                })
                .then(right.id.cmp(&left.id))
        });
        notes
    }
}

pub fn parse_tags(source: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in source.split([',', '\u{ff0c}', ';', '\u{ff1b}', '\n', '\r']) {
        let tag = raw.trim().trim_start_matches('#').trim();
        if tag.is_empty() || tag.chars().count() > 24 || tag.chars().any(char::is_control) {
            continue;
        }
        if seen.insert(tag.to_lowercase()) {
            tags.push(tag.to_owned());
            if tags.len() == 12 {
                break;
            }
        }
    }
    tags
}

pub fn tags_text(tags: &[String]) -> String {
    tags.join(", ")
}

pub fn lock(path: &Path) -> io::Result<fs::File> {
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("找不到存储目录"))?,
    )?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    file.try_lock().map_err(|error| {
        io::Error::other(format!(
            "无法锁定便签数据，请确认没有另一个 E-Sticker 窗口。\n{error}"
        ))
    })?;
    Ok(file)
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub background: usize,
    pub scroll_wall: bool,
    pub data_dir: Option<PathBuf>,
    pub markdown_shortcuts: bool,
    #[serde(default = "default_true")]
    pub paper_texture: bool,
    pub default_paper: usize,
    pub editor: EditorPreferences,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct EditorPreferences {
    pub mode: String,
    pub font: String,
    pub font_size: u32,
    pub line_height: f64,
    pub content_width: u32,
    pub theme: String,
    pub code_theme: String,
    pub outline: bool,
    pub counter: bool,
    pub autosave_ms: u32,
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self {
            mode: "ir".into(),
            font: "sans".into(),
            font_size: 16,
            line_height: 1.8,
            content_width: 800,
            theme: "classic".into(),
            code_theme: "github".into(),
            outline: true,
            counter: true,
            autosave_ms: 650,
        }
    }
}

impl EditorPreferences {
    pub fn normalize(&mut self) {
        if !["ir", "wysiwyg", "sv"].contains(&self.mode.as_str()) {
            self.mode = "ir".into();
        }
        if !["sans", "serif", "mono"].contains(&self.font.as_str()) {
            self.font = "sans".into();
        }
        if !["classic", "dark"].contains(&self.theme.as_str()) {
            self.theme = "classic".into();
        }
        if !["github", "monokai", "atom-one-dark"].contains(&self.code_theme.as_str()) {
            self.code_theme = "github".into();
        }
        self.font_size = self.font_size.clamp(12, 28);
        self.content_width = self.content_width.clamp(480, 1600);
        self.line_height = if self.line_height.is_finite() {
            self.line_height.clamp(1.2, 2.4)
        } else {
            1.8
        };
        self.autosave_ms = self.autosave_ms.clamp(300, 5000);
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            background: 0,
            scroll_wall: false,
            data_dir: None,
            markdown_shortcuts: true,
            paper_texture: true,
            default_paper: 0,
            editor: EditorPreferences::default(),
        }
    }
}

impl Preferences {
    pub fn read(path: &Path) -> io::Result<Self> {
        match fs::File::open(path) {
            Ok(file) => {
                let mut preferences: Self =
                    serde_json::from_reader(file).map_err(io::Error::other)?;
                preferences.editor.normalize();
                Ok(preferences)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let temp = path.with_extension("tmp");
        let file = fs::File::create(&temp)?;
        serde_json::to_writer(&file, self).map_err(io::Error::other)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temp, path)
    }
}

pub fn app_dir() -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("ESTICKER_DATA_DIR") {
        return std::path::absolute(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("找不到本地应用数据目录"))?;
    Ok(base.join("E-Sticker"))
}

pub struct AssetMigration {
    created: Vec<PathBuf>,
}

impl AssetMigration {
    pub fn copy(wall: &Wall, source: &Path, destination: &Path) -> io::Result<Self> {
        let mut migration = Self {
            created: Vec::new(),
        };
        let mut copied = std::collections::HashSet::new();
        let destination = fs::canonicalize(destination)?;
        for note in &wall.notes {
            for block in crate::markdown::parse(&note.body) {
                let Some(reference) = block.destination else {
                    continue;
                };
                if reference.contains("://")
                    || reference.starts_with("\\\\")
                    || reference.starts_with("//")
                {
                    continue;
                }
                let Some(relative) = crate::assets::relative_image(&reference) else {
                    if Path::new(&reference).is_absolute() {
                        continue;
                    }
                    return Err(io::Error::other("正文图片使用不安全的路径，未迁移"));
                };
                if !copied.insert(relative.clone()) {
                    continue;
                }
                let original = match crate::assets::confined_path(source, &relative) {
                    Ok(path) => path,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                let metadata = match fs::metadata(&original) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                if !metadata.is_file() {
                    continue;
                }
                if metadata.len() > crate::assets::MAX_IMAGE_BYTES {
                    return Err(io::Error::other("正文图片超过 20 MiB，未迁移"));
                }
                let target = destination.join(&relative);
                let parent = target
                    .parent()
                    .ok_or_else(|| io::Error::other("图片路径无效"))?;
                fs::create_dir_all(parent)?;
                if !fs::canonicalize(parent)?.starts_with(&destination) {
                    return Err(io::Error::other("目标图片目录越出存储目录，未复制"));
                }
                if target.try_exists()? {
                    let existing = crate::assets::confined_path(&destination, &relative)?;
                    if fs::metadata(&existing)?.len() == metadata.len()
                        && fs::read(&original)? == fs::read(&existing)?
                    {
                        continue;
                    }
                    return Err(io::Error::other(format!(
                        "目标目录已有同名图片，未覆盖：{}",
                        relative.display()
                    )));
                }
                let mut input = fs::File::open(&original)?;
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)?;
                migration.created.push(target);
                io::copy(&mut input, &mut output)?;
                output.sync_all()?;
            }
        }
        Ok(migration)
    }

    pub fn finish(mut self) {
        self.created.clear();
    }
}

impl Drop for AssetMigration {
    fn drop(&mut self) {
        for path in &self.created {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn data_path(
    base: &Path,
    preferences: &Preferences,
    legacy: Option<&Path>,
) -> io::Result<PathBuf> {
    if let Some(directory) = &preferences.data_dir {
        if !directory.is_absolute() {
            return Err(io::Error::other("设置中的便签目录不是绝对路径"));
        }
        return Ok(directory.join("wall.json"));
    }
    let current = base.join("wall.json");
    if !current.try_exists()?
        && let Some(legacy) = legacy
    {
        let previous = legacy.join("wall.json");
        if previous.try_exists()? {
            return Ok(previous);
        }
    }
    Ok(current)
}

pub fn excerpt(source: &str, query: &str, limit: usize) -> String {
    let normalized = source.split_whitespace().collect::<Vec<_>>().join(" ");
    let characters: Vec<_> = normalized.chars().collect();
    let query = query.trim().to_lowercase();
    let lowered = normalized.to_lowercase();
    let matched = if query.is_empty() {
        None
    } else {
        lowered.find(&query)
    };
    let start = matched
        .map_or(0, |index| {
            lowered[..index].chars().count().saturating_sub(limit / 3)
        })
        .min(characters.len());
    let end = (start + limit).min(characters.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        characters[start..end].iter().collect::<String>(),
        if end < characters.len() { "…" } else { "" }
    )
}

pub fn date(time: i64) -> String {
    DateTime::from_timestamp(time, 0)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}
