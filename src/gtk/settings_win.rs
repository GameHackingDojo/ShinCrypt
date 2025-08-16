use crate::{AppState, SIZE_1MB, gtk::{about_win::about_win, gtk_ui::MarginAll}, logic::{encryption::ShinCrypt, global::{GTKhelper, Global}}};
use gtk4::prelude::*;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const SETTINGS_FILE: &str = "settings.ron";

#[derive(Clone, Serialize, Deserialize)]
pub struct AppSettings {
  pub dark_mode: bool,
  pub remove_org: bool,
  pub same_dir: bool,
}

impl Default for AppSettings {
  fn default() -> Self { Self { dark_mode: true, remove_org: false, same_dir: false } }
}

impl AppSettings {
  fn get_settings_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> { Ok(std::env::current_exe().unwrap().parent().unwrap().to_path_buf().join(SETTINGS_FILE)) }

  /// Save settings to a RON file
  pub fn export(&self) -> Result<(), Box<dyn std::error::Error>> {
    let path = Self::get_settings_path().unwrap();
    let ron_string = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(|e| format!("Failed to serialize settings to RON, error: {}", e))?;

    std::fs::write(path.clone(), ron_string).map_err(|e| format!("Failed to write settings to {:?}, error: {}", path, e))?;

    Ok(())
  }

  /// Load settings from a RON file
  pub fn import() -> Result<Self, Box<dyn std::error::Error>> {
    let path = Self::get_settings_path().unwrap();

    if !path.exists() {
      return Err("Failed to import settings files doesn't exist".into());
    }

    let file_content = std::fs::read_to_string(path.clone()).map_err(|e| format!("Failed to read settings from {:?}, error: {}", path.clone(), e))?;

    let settings = ron::from_str(&file_content).map_err(|e| format!("Failed to parse RON from {:?}, error: {}", path, e))?;

    Ok(settings)
  }
}

/// Show the settings dialog as a child of `parent_win`.
pub fn settings_ui(window: &gtk4::ApplicationWindow, aps: Arc<RwLock<AppState>>) {
  let consts = aps.read().consts.clone();

  // Build window
  let settings_win = gtk4::ApplicationWindow::builder().transient_for(window).modal(true).resizable(true).title("Settings").default_width(300).default_height(150).build();

  let grid = gtk4::Grid::new();
  grid.set_row_spacing(consts.upad);
  grid.set_column_spacing(consts.upad);
  grid.set_margin_all(consts.margin);

  // Dark mode checkbox
  {
    let aps_c = aps.clone();
    let dark_mode_cb = gtk4::CheckButton::with_label("Dark mode");
    dark_mode_cb.set_active(aps_c.read().settings.dark_mode);
    dark_mode_cb.connect_toggled(move |cb| {
      aps_c.write().settings.dark_mode = cb.is_active();
      gtk4::Settings::default().expect("Failed to get Settings").set_gtk_application_prefer_dark_theme(cb.is_active());
      aps_c.read().settings.export().unwrap();
    });
    grid.attach(&dark_mode_cb, 0, 0, 2, 1);
  }

  // Remove source file checkbox
  {
    let aps_c = aps.clone();
    let remove_cb = gtk4::CheckButton::with_label("Remove source file");
    remove_cb.set_active(aps_c.read().settings.remove_org);
    remove_cb.connect_toggled(move |cb| {
      aps_c.write().settings.remove_org = cb.is_active();
      aps_c.read().settings.export().unwrap();
    });
    grid.attach(&remove_cb, 0, 1, 2, 1);
  }

  // Smae directory output checkbox
  {
    let aps_c = aps.clone();
    let same_dir_cb = gtk4::CheckButton::with_label("Same directory output");
    same_dir_cb.set_active(aps_c.read().settings.same_dir);
    same_dir_cb.connect_toggled(move |cb| {
      aps_c.write().settings.same_dir = cb.is_active();
      aps_c.read().settings.export().unwrap();
    });
    grid.attach(&same_dir_cb, 0, 2, 2, 1);
  }

  {
    let window_c = window.clone();

    let benchmark_btn = gtk4::Button::with_label("Benchmark 🚝");
    benchmark_btn.set_hexpand(true);
    benchmark_btn.connect_clicked(move |_| {
      match std::thread::Builder::new().stack_size(SIZE_1MB * 4).spawn(|| ShinCrypt::benchmark()).unwrap().join().unwrap() {
        Ok((e_time, d_time)) => GTKhelper::message_box(&window_c, "Done", format!("Encrypted 1GB:\n\nTime: {}\nSpeed: {:.2} MB/s\n\nDecrypted 1GB:\n\nTime: {}\nSpeed: {:.2} MB/s\n", Global::format_duration(e_time), Global::calculate_speed(1.0, e_time), Global::format_duration(d_time), Global::calculate_speed(1.0, d_time)), None),
        Err(e) => GTKhelper::message_box(&window_c, "Error", e, None),
      };
    });
    grid.attach(&benchmark_btn, 0, 3, 1, 1);
  }

  {
    let window_c = window.clone();
    let aps_c = aps.clone();

    let about_btn = gtk4::Button::with_label("About ℹ️");
    about_btn.set_hexpand(true);
    about_btn.connect_clicked(move |_| {
      about_win(&window_c, aps_c.clone());
    });
    grid.attach(&about_btn, 1, 3, 1, 1);
  }

  settings_win.set_child(Some(&grid));
  settings_win.present();
}
