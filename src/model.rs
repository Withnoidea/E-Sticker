use chrono::{DateTime, Local, Utc};
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
            x: self.x.clamp(0, WALL_W - NOTE_W),
            y: self.y.clamp(0, WALL_H - NOTE_H),
        }
    }
}

#[derive(Clone, Default)]
pub struct Draft {
    pub title: String,
    pub body: String,
    pub paper: Paper,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Note {
    pub id: u64,
    pub title: String,
    pub body: String,
    pub paper: Paper,
    pub at: Point,
    pub created: i64,
    pub edited: i64,
}

impl Note {
    pub fn draft(&self) -> Draft {
        Draft {
            title: self.title.clone(),
            body: self.body.clone(),
            paper: self.paper,
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
                if file.version != 1 {
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
                version: 1,
                notes: self.notes.clone(),
            },
        )
        .map_err(io::Error::other)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temp, path)
    }

    pub fn place(&mut self, draft: Draft, at: Point) {
        let id = self.notes.iter().map(|note| note.id).max().unwrap_or(0) + 1;
        let time = Utc::now().timestamp();
        self.notes.push(Note {
            id,
            title: draft.title,
            body: draft.body,
            paper: draft.paper,
            at: at.on_wall(),
            created: time,
            edited: time,
        });
    }

    pub fn hit(&self, point: Point) -> Option<&Note> {
        self.notes.iter().rev().find(|note| {
            (note.at.x..note.at.x + NOTE_W).contains(&point.x)
                && (note.at.y..note.at.y + NOTE_H).contains(&point.y)
        })
    }

    pub fn move_to(&mut self, id: u64, at: Point) {
        if let Some(index) = self.notes.iter().position(|note| note.id == id) {
            let mut note = self.notes.remove(index);
            note.at = at.on_wall();
            self.notes.push(note);
        }
    }

    pub fn rows(&self, query: &str, sort: Sort) -> Vec<&Note> {
        let query = query.trim().to_lowercase();
        let mut notes: Vec<_> = self
            .notes
            .iter()
            .filter(|note| {
                query.is_empty()
                    || note.title.to_lowercase().contains(&query)
                    || note.body.to_lowercase().contains(&query)
            })
            .collect();
        notes.sort_by(|left, right| {
            match sort {
                Sort::Edited => right.edited.cmp(&left.edited),
                Sort::Created => right.created.cmp(&left.created),
                Sort::Title => left.title.to_lowercase().cmp(&right.title.to_lowercase()),
            }
            .then(right.id.cmp(&left.id))
        });
        notes
    }
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

pub fn path() -> io::Result<PathBuf> {
    if let Some(dir) =
        std::env::var_os("ESTICKER_DATA_DIR").or_else(|| std::env::var_os("PAPERWALL_DATA_DIR"))
    {
        return Ok(PathBuf::from(dir).join("wall.json"));
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("找不到本地应用数据目录"))?;
    let current = base.join("E-Sticker").join("wall.json");
    let legacy = base.join("Paperwall").join("wall.json");
    if !current.try_exists()? && legacy.try_exists()? {
        return Ok(legacy);
    }
    Ok(current)
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

pub fn excerpt(note: &Note, query: &str) -> String {
    let body = note.body.split_whitespace().collect::<Vec<_>>().join(" ");
    let needle = query.trim().to_lowercase();
    let start = if needle.is_empty() {
        0
    } else {
        body.to_lowercase()
            .find(&needle)
            .map(|offset| {
                body.to_lowercase()[..offset]
                    .chars()
                    .count()
                    .saturating_sub(12)
            })
            .unwrap_or(0)
    };
    let preview: String = body.chars().skip(start).take(85).collect();
    format!(
        "{}{preview}{}",
        if start > 0 { "…" } else { "" },
        if body.chars().count() > start + 85 {
            "…"
        } else {
            ""
        }
    )
}
