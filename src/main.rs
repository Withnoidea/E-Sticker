#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod assets;
#[cfg(windows)]
mod chrome;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod editor;
mod markdown;
#[cfg(windows)]
mod model;
#[cfg(windows)]
mod preview;
#[cfg(windows)]
mod theme;
#[cfg(windows)]
mod ui;

fn main() {
    #[cfg(windows)]
    ui::run();
    #[cfg(not(windows))]
    eprintln!("E-Sticker requires Windows.");
}
