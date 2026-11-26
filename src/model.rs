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
pub const DEFAULT_WALL: u64 = 0;

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
    pub wall_id: u64,
    pub wall_at: Point,
    pub wall_order: u64,
    pub home_tilt: i32,
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
            wall_id: DEFAULT_WALL,
            wall_at: Point::default(),
            wall_order: 0,
            home_tilt: 0,
        }
    }
}

impl Note {
    pub fn position(&self, wall_id: u64) -> Point {
        if wall_id == DEFAULT_WALL {
            self.at
        } else {
            self.wall_at
        }
    }

    pub fn contains(&self, point: Point, wall_id: u64) -> bool {
        let at = self.position(wall_id);
        let width = self.width.clamp(136, 280) as f64;
        let height = self.height.clamp(112, 240) as f64;
        let angle = if wall_id == DEFAULT_WALL {
            self.home_tilt as f64
        } else {
            0.0
        }
        .to_radians();
        let delta_x = (point.x - at.x) as f64 - width / 2.0;
        let delta_y = (point.y - at.y) as f64 - height / 2.0;
        let local_x = delta_x * angle.cos() + delta_y * angle.sin() + width / 2.0;
        let local_y = -delta_x * angle.sin() + delta_y * angle.cos() + height / 2.0;
        (0.0..width).contains(&local_x) && (0.0..height).contains(&local_y)
    }

    pub fn draft(&self) -> Draft {
        Draft {
            title: self.title.clone(),
            body: self.body.clone(),
            paper: self.paper,
            tags: self.tags.clone(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CategoryWall {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Default)]
pub struct Wall {
    pub notes: Vec<Note>,
    pub walls: Vec<CategoryWall>,
    legacy_version: u32,
}

#[derive(Deserialize, Serialize)]
struct File {
    version: u32,
    notes: Vec<Note>,
    #[serde(default)]
    walls: Vec<CategoryWall>,
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
                if !(1..=3).contains(&file.version) {
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
                let mut wall_ids = std::collections::HashSet::new();
                let mut wall_names = std::collections::HashSet::new();
                if file.version < 3 {
                    file.walls.clear();
                }
                for wall in &mut file.walls {
                    wall.name = wall.name.trim().to_owned();
                    if wall.id == DEFAULT_WALL
                        || wall.id == u64::MAX
                        || !wall_ids.insert(wall.id)
                        || !valid_wall_name(&wall.name)
                        || !wall_names.insert(wall.name.to_lowercase())
                    {
                        return Err(io::Error::other("分类墙编号或名称无效、重复"));
                    }
                }
                for note in &mut file.notes {
                    note.at = note.at.on_wall();
                    note.width = note.width.clamp(136, 280);
                    note.height = note.height.clamp(112, 240);
                    if file.version < 3 {
                        note.wall_id = DEFAULT_WALL;
                        note.wall_at = note.at;
                        note.wall_order = note.id;
                        note.home_tilt = 0;
                    }
                    note.wall_at = note.wall_at.on_wall();
                    note.home_tilt = note.home_tilt.clamp(-5, 5);
                    if note.wall_id != DEFAULT_WALL && !wall_ids.contains(&note.wall_id) {
                        return Err(io::Error::other("便签引用了不存在的分类墙"));
                    }
                }
                Ok(Self {
                    notes: file.notes,
                    walls: file.walls,
                    legacy_version: file.version,
                })
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
        if (1..=2).contains(&self.legacy_version) && path.exists() {
            use std::io::Write;
            let backup = path.with_extension(format!("v{}.bak", self.legacy_version));
            let bytes = fs::read(path)?;
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&backup)
            {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
                        drop(file);
                        let _ = fs::remove_file(&backup);
                        return Err(error);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists && backup.is_file() => {}
                Err(error) => return Err(error),
            }
        }
        let temp = path.with_extension("tmp");
        let file = fs::File::create(&temp)?;
        serde_json::to_writer(
            &file,
            &File {
                version: 3,
                notes: self.notes.clone(),
                walls: self.walls.clone(),
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
            wall_id: DEFAULT_WALL,
            wall_at: at.on_wall(),
            wall_order: id,
            home_tilt: (scatter(id) % 9) as i32 - 4,
        });
    }

    pub fn place_in(&mut self, draft: Draft, at: Point, wall_id: u64) -> io::Result<u64> {
        self.check_wall(wall_id)?;
        let order = self.next_wall_order(wall_id);
        self.place(draft, at);
        let note = self.notes.last_mut().unwrap();
        note.wall_id = wall_id;
        note.wall_order = order;
        if wall_id != DEFAULT_WALL {
            note.at = collage_position(note.id);
        }
        Ok(note.id)
    }

    pub fn canvas_notes(&self, wall_id: u64) -> Vec<&Note> {
        let mut notes: Vec<_> = self
            .notes
            .iter()
            .filter(|note| {
                !note.deleted
                    && !note.archived
                    && (wall_id == DEFAULT_WALL || note.wall_id == wall_id)
            })
            .collect();
        if wall_id != DEFAULT_WALL {
            notes.sort_by_key(|note| (note.wall_order, note.id));
        }
        notes
    }

    pub fn hit_in(&self, point: Point, wall_id: u64) -> Option<&Note> {
        if wall_id == DEFAULT_WALL {
            self.hit(point)
        } else {
            self.canvas_notes(wall_id)
                .into_iter()
                .rev()
                .find(|note| note.contains(point, wall_id))
        }
    }

    pub fn move_in(&mut self, id: u64, at: Point, wall_id: u64) {
        if wall_id == DEFAULT_WALL {
            self.move_to(id, at);
            return;
        }
        if !self
            .notes
            .iter()
            .any(|note| note.id == id && note.wall_id == wall_id)
        {
            return;
        }
        let order = self.next_wall_order(wall_id);
        if let Some(note) = self
            .notes
            .iter_mut()
            .find(|note| note.id == id && note.wall_id == wall_id)
        {
            note.wall_at = at.on_wall();
            note.wall_order = order;
        }
    }

    fn check_wall(&self, wall_id: u64) -> io::Result<()> {
        if wall_id == DEFAULT_WALL || self.walls.iter().any(|wall| wall.id == wall_id) {
            Ok(())
        } else {
            Err(io::Error::other("找不到这面分类墙"))
        }
    }

    fn next_wall_order(&mut self, wall_id: u64) -> u64 {
        let order = self
            .notes
            .iter()
            .filter(|note| note.wall_id == wall_id)
            .map(|note| note.wall_order)
            .max()
            .unwrap_or(0);
        if let Some(next) = order.checked_add(1) {
            return next;
        }
        let mut indices: Vec<_> = self
            .notes
            .iter()
            .enumerate()
            .filter(|(_, note)| note.wall_id == wall_id)
            .map(|(index, note)| (note.wall_order, note.id, index))
            .collect();
        indices.sort_unstable();
        for (index, (_, _, position)) in indices.iter().enumerate() {
            self.notes[*position].wall_order = index as u64 + 1;
        }
        indices.len() as u64 + 1
    }

    pub fn wall_name(&self, wall_id: u64) -> &str {
        self.walls
            .iter()
            .find(|wall| wall.id == wall_id)
            .map_or("未分类", |wall| wall.name.as_str())
    }

    fn check_name(&self, name: &str, except: u64) -> io::Result<String> {
        let name = name.trim();
        if !valid_wall_name(name) {
            return Err(io::Error::other(
                "墙面名称需要 1–32 个字符，不能包含换行或控制字符",
            ));
        }
        if self
            .walls
            .iter()
            .any(|wall| wall.id != except && wall.name.to_lowercase() == name.to_lowercase())
        {
            return Err(io::Error::other("已有同名分类墙，请换一个名称"));
        }
        Ok(name.to_owned())
    }

    pub fn create_wall(&mut self, name: &str) -> io::Result<u64> {
        let name = self.check_name(name, DEFAULT_WALL)?;
        let id = self
            .walls
            .iter()
            .map(|wall| wall.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX)
            .unwrap_or_else(|| {
                (1..u64::MAX)
                    .find(|id| !self.walls.iter().any(|wall| wall.id == *id))
                    .unwrap()
            });
        self.walls.push(CategoryWall { id, name });
        Ok(id)
    }

    pub fn rename_wall(&mut self, wall_id: u64, name: &str) -> io::Result<()> {
        let name = self.check_name(name, wall_id)?;
        let wall = self
            .walls
            .iter_mut()
            .find(|wall| wall.id == wall_id)
            .ok_or_else(|| io::Error::other("找不到这面分类墙"))?;
        wall.name = name;
        Ok(())
    }

    pub fn delete_wall(&mut self, wall_id: u64) -> io::Result<usize> {
        if wall_id == DEFAULT_WALL {
            return Err(io::Error::other("首页不能删除"));
        }
        self.check_wall(wall_id)?;
        let mut count = 0;
        let time = Utc::now().timestamp();
        for note in self.notes.iter_mut().filter(|note| note.wall_id == wall_id) {
            if !note.deleted {
                count += 1;
            }
            note.deleted = true;
            note.wall_id = DEFAULT_WALL;
            note.edited = time;
        }
        self.walls.retain(|wall| wall.id != wall_id);
        Ok(count)
    }

    pub fn assign_wall(&mut self, id: u64, wall_id: u64) -> io::Result<()> {
        self.check_wall(wall_id)?;
        let current = self
            .notes
            .iter()
            .find(|note| note.id == id && !note.deleted)
            .ok_or_else(|| io::Error::other("请先从回收站恢复文章"))?
            .wall_id;
        if current == wall_id {
            return Ok(());
        }
        let count = self
            .notes
            .iter()
            .filter(|note| note.wall_id == wall_id && !note.deleted)
            .count() as i32;
        let order = self.next_wall_order(wall_id);
        let note = self
            .notes
            .iter_mut()
            .find(|note| note.id == id && !note.deleted)
            .ok_or_else(|| io::Error::other("请先从回收站恢复文章"))?;
        if note.wall_id != wall_id {
            note.wall_id = wall_id;
            note.wall_at = Point {
                x: 32 + count.rem_euclid(7) * 28,
                y: 32 + count.rem_euclid(7) * 24,
            };
            note.wall_order = order;
            note.edited = Utc::now().timestamp();
        }
        Ok(())
    }

    pub fn rows_in_category(
        &self,
        query: &str,
        tag: &str,
        sort: Sort,
        bucket: u8,
        category: Option<u64>,
    ) -> Vec<&Note> {
        self.rows_in_filtered(query, tag, sort, bucket)
            .into_iter()
            .filter(|note| category.is_none_or(|wall_id| note.wall_id == wall_id))
            .collect()
    }

    pub fn hit(&self, point: Point) -> Option<&Note> {
        self.notes
            .iter()
            .rev()
            .find(|note| !note.archived && !note.deleted && note.contains(point, DEFAULT_WALL))
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
            let restoring = note.deleted && !value;
            note.deleted = value;
            if restoring {
                note.wall_id = DEFAULT_WALL;
                note.archived = false;
            }
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

fn valid_wall_name(name: &str) -> bool {
    !name.is_empty() && name.chars().count() <= 32 && !name.chars().any(char::is_control)
}

fn scatter(seed: u64) -> u64 {
    let mixed = (seed ^ 0x9e3779b97f4a7c15).wrapping_mul(0xbf58476d1ce4e5b9);
    (mixed ^ (mixed >> 30)).wrapping_mul(0x94d049bb133111eb)
}

fn collage_position(id: u64) -> Point {
    let seed = scatter(id);
    Point {
        x: 26 + (seed % 760) as i32,
        y: 26 + (scatter(seed) % 390) as i32,
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
