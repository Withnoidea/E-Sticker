use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "e-sticker-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn note(id: u64, title: &str) -> Note {
    Note {
        id,
        title: title.into(),
        at: Point { x: 20, y: 30 },
        ..Note::default()
    }
}

#[test]
fn wall_round_trip_overwrites_atomically_and_keeps_every_field() {
    let directory = Directory::new();
    let path = directory.file("wall.json");
    let mut wall = Wall::default();
    wall.place(
        Draft {
            title: "中文便签 📝".into(),
            body: "# 正文\n\n**内容**".into(),
            paper: Paper::Blue,
            tags: vec!["travel".into(), "review".into()],
        },
        Point { x: 123, y: 456 },
    );
    wall.save(&path).unwrap();
    wall.notes[0].pinned = true;
    wall.notes[0].archived = true;
    wall.notes[0].deleted = true;
    wall.notes[0].tags = vec!["标签".into()];
    wall.save(&path).unwrap();
    let restored = Wall::open(&path).unwrap();
    assert_eq!(
        serde_json::to_value(&restored.notes).unwrap(),
        serde_json::to_value(&wall.notes).unwrap()
    );
    assert!(!path.with_extension("tmp").exists());
}

#[test]
fn failed_save_preserves_existing_file() {
    let directory = Directory::new();
    let path = directory.file("wall.json");
    let mut wall = Wall {
        notes: vec![note(1, "original")],
    };
    wall.save(&path).unwrap();
    let original = fs::read(&path).unwrap();
    fs::create_dir(path.with_extension("tmp")).unwrap();
    wall.notes[0].title = "must not replace".into();
    assert!(wall.save(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn version_one_notes_receive_compatible_defaults() {
    let directory = Directory::new();
    let path = directory.file("wall.json");
    fs::write(
        &path,
        r#"{"version":1,"notes":[{"id":7,"title":"旧便签","body":"原正文"}]}"#,
    )
    .unwrap();
    let wall = Wall::open(&path).unwrap();
    assert_eq!(wall.notes[0].title, "旧便签");
    assert_eq!(wall.notes[0].body, "原正文");
    assert_eq!(wall.notes[0].width, NOTE_W);
    assert!(!wall.notes[0].deleted);
}

#[test]
fn corrupt_or_future_data_is_not_silently_replaced() {
    let directory = Directory::new();
    let path = directory.file("wall.json");
    for contents in [
        "{broken",
        r#"{"version":3,"notes":[]}"#,
        r#"{"version":2,"notes":[{"id":1},{"id":1}]}"#,
        r#"{"version":2,"notes":[{"id":18446744073709551615}]}"#,
    ] {
        fs::write(&path, contents).unwrap();
        assert!(Wall::open(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }
}

#[test]
fn imported_geometry_is_bounded() {
    let directory = Directory::new();
    let path = directory.file("wall.json");
    fs::write(
        &path,
        r#"{"version":2,"notes":[{"id":1,"at":{"x":-999,"y":2147483647},"width":0,"height":999}]}"#,
    )
    .unwrap();
    let wall = Wall::open(&path).unwrap();
    assert_eq!(wall.notes[0].at.x, 0);
    assert_eq!(wall.notes[0].at.y, i32::MAX / 2);
    assert_eq!(wall.notes[0].width, 136);
    assert_eq!(wall.notes[0].height, 240);
}

#[test]
fn id_allocation_does_not_overflow_imported_ids() {
    let mut wall = Wall {
        notes: vec![note(u64::MAX - 1, "imported")],
    };
    wall.place(Draft::default(), Point::default());
    assert_eq!(wall.notes[1].id, 1);
}

#[test]
fn hit_testing_respects_layering_archive_and_trash() {
    let mut wall = Wall {
        notes: vec![note(1, "bottom"), note(2, "top")],
    };
    let point = Point { x: 21, y: 31 };
    assert_eq!(wall.hit(point).unwrap().id, 2);
    wall.set_archived(2, true);
    assert_eq!(wall.hit(point).unwrap().id, 1);
    wall.set_deleted(1, true);
    assert!(wall.hit(point).is_none());
    wall.set_deleted(1, false);
    wall.move_to(1, Point { x: 100, y: 100 });
    assert!(wall.hit(point).is_none());
    assert_eq!(wall.hit(Point { x: 101, y: 101 }).unwrap().id, 1);
}

#[test]
fn search_sort_and_buckets_preserve_wall_order_and_positions() {
    let mut wall = Wall {
        notes: vec![note(1, "Alpha"), note(2, "Beta"), note(3, "Gamma")],
    };
    wall.notes[0].body = "Only BODY contains 复盘".into();
    wall.notes[1].tags = vec!["标签".into()];
    wall.notes[2].pinned = true;
    let original = serde_json::to_value(&wall.notes).unwrap();
    assert_eq!(wall.rows_in("body", Sort::Title, 0)[0].id, 1);
    assert_eq!(wall.rows_in("复盘", Sort::Edited, 0)[0].id, 1);
    assert_eq!(wall.rows_in("标签", Sort::Created, 0)[0].id, 2);
    assert_eq!(
        wall.rows_in("", Sort::Title, 0)
            .iter()
            .map(|note| note.id)
            .collect::<Vec<_>>(),
        vec![3, 1, 2]
    );
    assert_eq!(serde_json::to_value(&wall.notes).unwrap(), original);
    wall.set_archived(1, true);
    wall.set_deleted(2, true);
    assert_eq!(wall.rows_in("", Sort::Edited, 0)[0].id, 3);
    assert_eq!(wall.rows_in("", Sort::Edited, 1)[0].id, 1);
    assert_eq!(wall.rows_in("", Sort::Edited, 2)[0].id, 2);
}

#[test]
fn tags_parse_separators_deduplicate_case_insensitively_and_enforce_limits() {
    let parsed = parse_tags(" #Work, work；项目\n#review，Long Tag; #work ");
    assert_eq!(parsed, vec!["Work", "项目", "review", "Long Tag"]);
    assert_eq!(parse_tags(&tags_text(&parsed)), parsed);
    assert_eq!(parse_tags("Ärger, ärger"), vec!["Ärger"]);
    assert_eq!(parse_tags(&"🙂".repeat(24)), vec!["🙂".repeat(24)]);
    assert!(parse_tags(&"🙂".repeat(25)).is_empty());

    let too_long = "x".repeat(25);
    let many = (0..13)
        .map(|index| format!("tag{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(parse_tags(&many).last().unwrap(), "tag11");
    assert_eq!(parse_tags(&format!("{too_long},{many}")).len(), 12);
    assert_eq!(
        parse_tags(&format!("{too_long},{many}")).first().unwrap(),
        "tag0"
    );
    assert!(parse_tags("\u{0001},, ; ").is_empty());
}

#[test]
fn tag_filter_is_case_insensitive_and_does_not_reorder_wall_data() {
    let mut wall = Wall {
        notes: vec![note(1, "Alpha"), note(2, "Beta"), note(3, "Gamma")],
    };
    wall.notes[0].tags = vec!["Work".into()];
    wall.notes[1].tags = vec!["personal".into(), "work".into()];
    wall.notes[2].tags = vec!["Personal".into()];
    let original = serde_json::to_value(&wall.notes).unwrap();

    assert_eq!(
        wall.rows_in_filtered("", "WORK", Sort::Title, 0)
            .iter()
            .map(|note| note.id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        wall.rows_in_filtered("beta", "work", Sort::Edited, 0)[0].id,
        2
    );
    assert_eq!(serde_json::to_value(&wall.notes).unwrap(), original);
    wall.set_archived(2, true);
    assert_eq!(wall.rows_in_filtered("", "work", Sort::Edited, 0)[0].id, 1);
    assert_eq!(wall.rows_in_filtered("", "WORK", Sort::Edited, 1)[0].id, 2);
}

#[test]
fn restoring_archived_trash_keeps_original_bucket() {
    let mut wall = Wall {
        notes: vec![note(1, "archive")],
    };
    wall.set_archived(1, true);
    wall.set_deleted(1, true);
    assert_eq!(wall.rows_in("", Sort::Edited, 2).len(), 1);
    wall.set_deleted(1, false);
    assert!(wall.rows_in("", Sort::Edited, 0).is_empty());
    assert_eq!(wall.rows_in("", Sort::Edited, 1).len(), 1);
}

#[test]
fn preferences_defaults_match_empty_and_missing_files() {
    let directory = Directory::new();
    let path = directory.file("settings.json");
    let missing = Preferences::read(&path).unwrap();
    fs::write(&path, "{}").unwrap();
    let empty = Preferences::read(&path).unwrap();
    assert_eq!(
        serde_json::to_value(&missing).unwrap(),
        serde_json::to_value(&empty).unwrap()
    );
    assert!(missing.paper_texture && missing.markdown_shortcuts);
    assert!(!missing.scroll_wall);
    assert_eq!(missing.editor, EditorPreferences::default());
}

#[test]
fn legacy_settings_gain_editor_defaults_without_losing_wall_preferences() {
    let directory = Directory::new();
    let path = directory.file("settings.json");
    fs::write(
        &path,
        r#"{"background":3,"scroll_wall":true,"default_paper":4,"markdown_shortcuts":false}"#,
    )
    .unwrap();
    let preferences = Preferences::read(&path).unwrap();
    assert_eq!(preferences.background, 3);
    assert_eq!(preferences.default_paper, 4);
    assert!(preferences.scroll_wall);
    assert!(!preferences.markdown_shortcuts);
    assert_eq!(preferences.editor, EditorPreferences::default());
}

#[test]
fn editor_settings_are_normalized_when_loading() {
    let directory = Directory::new();
    let path = directory.file("settings.json");
    fs::write(&path, r#"{"editor":{"mode":"unknown","font":"injected","font_size":999,"line_height":0,"content_width":0,"theme":"unknown","code_theme":"unknown","autosave_ms":1}}"#).unwrap();
    let settings = Preferences::read(&path).unwrap().editor;
    assert_eq!(settings.mode, "ir");
    assert_eq!(settings.font, "sans");
    assert_eq!(settings.font_size, 28);
    assert_eq!(settings.line_height, 1.2);
    assert_eq!(settings.content_width, 480);
    assert_eq!(settings.theme, "classic");
    assert_eq!(settings.code_theme, "github");
    assert_eq!(settings.autosave_ms, 300);
}

#[test]
fn all_preferences_survive_repeated_save_and_reload() {
    let directory = Directory::new();
    let path = directory.file("settings.json");
    let mut preferences = Preferences {
        background: 3,
        scroll_wall: true,
        data_dir: Some(directory.file("notes")),
        markdown_shortcuts: false,
        paper_texture: false,
        default_paper: 4,
        editor: EditorPreferences::default(),
    };
    preferences.save(&path).unwrap();
    preferences.background = 2;
    preferences.save(&path).unwrap();
    assert_eq!(
        serde_json::to_value(Preferences::read(&path).unwrap()).unwrap(),
        serde_json::to_value(&preferences).unwrap()
    );
}

#[test]
fn legacy_path_only_applies_without_new_or_configured_data() {
    let directory = Directory::new();
    let legacy = directory.file("Paperwall");
    let base = directory.file("E-Sticker");
    fs::create_dir_all(&legacy).unwrap();
    fs::create_dir_all(&base).unwrap();
    let mut preferences = Preferences::default();
    assert_eq!(
        data_path(&base, &preferences, Some(&legacy)).unwrap(),
        base.join("wall.json")
    );
    fs::write(legacy.join("wall.json"), "legacy").unwrap();
    assert_eq!(
        data_path(&base, &preferences, Some(&legacy)).unwrap(),
        legacy.join("wall.json")
    );
    fs::write(base.join("wall.json"), "new").unwrap();
    assert_eq!(
        data_path(&base, &preferences, Some(&legacy)).unwrap(),
        base.join("wall.json")
    );
    preferences.data_dir = Some(directory.file("custom"));
    assert_eq!(
        data_path(&base, &preferences, Some(&legacy)).unwrap(),
        directory.file("custom/wall.json")
    );
    preferences.data_dir = Some(PathBuf::from("relative"));
    assert!(data_path(&base, &preferences, Some(&legacy)).is_err());
}

#[test]
fn simultaneous_writers_are_rejected_and_lock_is_released() {
    let directory = Directory::new();
    let path = directory.file("wall.json");
    let first = lock(&path).unwrap();
    assert!(lock(&path).is_err());
    drop(first);
    assert!(lock(&path).is_ok());
}

#[test]
fn excerpts_find_body_matches_without_splitting_unicode() {
    let source = format!(
        "{}\n复盘🙂关键内容\n{}",
        "前文".repeat(100),
        "后文".repeat(100)
    );
    let snippet = excerpt(&source, "关键", 40);
    assert!(snippet.contains("关键内容"));
    assert!(snippet.starts_with('…') && snippet.ends_with('…'));
    assert!(snippet.chars().count() <= 42);
    assert_eq!(excerpt("a\r\nb   c", "", 20), "a b c");
}

#[test]
fn relative_images_migrate_without_overwriting_or_leaking_on_failure() {
    let directory = Directory::new();
    let source = directory.file("source");
    let target = directory.file("target");
    fs::create_dir_all(source.join("images")).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(source.join("images/图片.png"), b"sample image").unwrap();
    let mut wall = Wall {
        notes: vec![note(1, "image")],
    };
    wall.notes[0].body = "![图](images/图片.png)".into();
    let pending = AssetMigration::copy(&wall, &source, &target).unwrap();
    assert_eq!(
        fs::read(target.join("images/图片.png")).unwrap(),
        b"sample image"
    );
    drop(pending);
    assert!(!target.join("images/图片.png").exists());
    AssetMigration::copy(&wall, &source, &target)
        .unwrap()
        .finish();
    assert!(target.join("images/图片.png").exists());
    fs::write(target.join("images/图片.png"), b"do not overwrite").unwrap();
    assert!(AssetMigration::copy(&wall, &source, &target).is_err());
    assert_eq!(
        fs::read(target.join("images/图片.png")).unwrap(),
        b"do not overwrite"
    );
    wall.notes[0].body = "![图](../outside.png)".into();
    assert!(AssetMigration::copy(&wall, &source, &target).is_err());
}

#[test]
fn imported_images_get_unique_portable_references_and_can_rollback() {
    use crate::assets::{confined_path, import_image, relative_image};
    let directory = Directory::new();
    let source = directory.file("图片.png");
    let root = directory.file("wall");
    fs::create_dir(&root).unwrap();
    let bytes = b"\x89PNG\r\n\x1a\n test";
    fs::write(&source, bytes).unwrap();
    let retained = import_image(&root, &source).unwrap();
    let pending = import_image(&root, &source).unwrap();
    assert_ne!(retained.reference(), pending.reference());
    let retained_path = root.join(retained.reference());
    let pending_path = root.join(pending.reference());
    retained.finish();
    drop(pending);
    assert_eq!(fs::read(retained_path).unwrap(), bytes);
    assert!(!pending_path.exists());
    fs::write(&source, b"invalid").unwrap();
    assert!(import_image(&root, &source).is_err());
    assert!(confined_path(&root, Path::new("../图片.png")).is_err());
    for blocked in [
        "../图片.png",
        "C:图片.png",
        "C:/图片.png",
        "//server/share/a.png",
        "https://example.com/a.png",
        "assets/foo.png:stream",
    ] {
        assert!(relative_image(blocked).is_none(), "Accepted {blocked}");
    }
    assert_eq!(
        relative_image("./images/old%20photo.png").unwrap(),
        Path::new("./images/old photo.png")
    );
}
