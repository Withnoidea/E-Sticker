#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod model;
#[cfg(windows)]
mod ui;

fn main() {
    #[cfg(windows)]
    ui::run();
    #[cfg(not(windows))]
    eprintln!("E-Sticker requires Windows.");
}
