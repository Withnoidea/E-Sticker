use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

pub fn relative_image(reference: &str) -> Option<PathBuf> {
    let decoded = reference.replace("%20", " ");
    if decoded.is_empty() || decoded.contains([':', '\0']) {
        return None;
    }
    let path = PathBuf::from(decoded);
    path.components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        .then_some(path)
}

pub fn confined_path(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let root = fs::canonicalize(root)?;
    let path = fs::canonicalize(root.join(relative))?;
    if !path.starts_with(&root) {
        return Err(io::Error::other("图片路径越出便签存储目录"));
    }
    Ok(path)
}

fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

pub struct ImportedImage {
    path: PathBuf,
    reference: String,
    keep: bool,
}

impl ImportedImage {
    pub fn reference(&self) -> &str {
        &self.reference
    }

    pub fn finish(mut self) {
        self.keep = true;
    }
}

impl Drop for ImportedImage {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn import_image(root: &Path, source: &Path) -> io::Result<ImportedImage> {
    let input = File::open(source)?;
    let metadata = input.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
        return Err(io::Error::other("请选择不超过 20 MiB 的图片文件"));
    }
    let mut bytes = Vec::new();
    input.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes)?;
    store_image(root, &bytes)
}

pub fn store_image(root: &Path, bytes: &[u8]) -> io::Result<ImportedImage> {
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(io::Error::other("图片超过 20 MiB"));
    }
    let extension = image_extension(bytes)
        .ok_or_else(|| io::Error::other("图片格式不受支持（PNG、JPEG、GIF 或 BMP）"))?;
    let relative = Path::new("assets");
    match fs::create_dir(root.join(relative)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let directory = confined_path(root, relative)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for sequence in 0..256 {
        let name = format!("image-{stamp:x}-{sequence:x}.{extension}");
        let path = directory.join(&name);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let image = ImportedImage {
            path,
            reference: format!("assets/{name}"),
            keep: false,
        };
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        written?;
        return Ok(image);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "无法分配图片文件名",
    ))
}
