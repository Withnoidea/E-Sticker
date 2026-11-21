use std::{env, fs, path::Path};

fn collect(root: &Path, directory: &Path, entries: &mut String) {
    if directory.is_file() {
        let name = directory
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let absolute = fs::canonicalize(directory).unwrap();
        entries.push_str(&format!("({name:?}, include_bytes!({absolute:?})),\n"));
        return;
    }
    let mut paths = fs::read_dir(directory)
        .expect("Run npm ci --ignore-scripts to install the pinned Vditor assets")
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect(root, &path, entries);
        } else if !matches!(
            path.extension().and_then(|part| part.to_str()),
            Some("ts" | "map")
        ) {
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let absolute = fs::canonicalize(&path).unwrap();
            entries.push_str(&format!("({name:?}, include_bytes!({absolute:?})),\n"));
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=web");
    println!("cargo:rerun-if-changed=package-lock.json");
    println!("cargo:rerun-if-changed=node_modules/vditor/dist");
    let root = Path::new("node_modules");
    let mut entries = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    collect(root, &root.join("vditor/dist"), &mut entries);
    collect(Path::new("web"), Path::new("web"), &mut entries);
    let license = fs::canonicalize("node_modules/vditor/LICENSE").unwrap();
    entries.push_str(&format!(
        "(\"Vditor-LICENSE.txt\", include_bytes!({license:?})),\n];\n"
    ));
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("editor_assets.rs"),
        entries,
    )
    .unwrap();
}
