#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crate::{gtk::settings_win::AppSettings, logic::global::Global};
mod gtk;
mod logic;

const APPNAME: &str = "ShinCrypt";
const OLDAPPNAME: &str = "old_GHD_app";
static SIZE_1MB: usize = 1024 * 1024;

#[derive(Clone, Default)]
pub struct AppState {
  pub settings: AppSettings,
  pub consts: AppConsts,
}

#[derive(Clone)]
pub struct AppConsts {
  pub app_name: String,
  pub file_name: String,
  pub version: String,
  pub author: String,
  pub repo_owner: String,
  pub github_repo: String,
  pub download_url: String,
  pub patreon_url: String,

  pub upad: u32,
  pub margin: i32,
}

impl Default for AppConsts {
  fn default() -> Self {
    let app_name = String::from(APPNAME);
    let file_name = if cfg!(target_os = "windows") { format!("{}.exe", app_name) } else { app_name.clone() };
    let version = String::from(env!("CARGO_PKG_VERSION"));
    let author = String::from("Game Hacking Dojo");
    let repo_owner = String::from("GameHackingDojo");
    let github_repo = format!("https://github.com/{}/{}", repo_owner, app_name);
    let download_url = format!("https://api.github.com/repos/{}/{}/releases/latest", repo_owner, app_name);
    let patreon_url = format!("https://www.patreon.com/c/{}", repo_owner);

    return Self {
      app_name: String::from(APPNAME),
      upad: 10,
      margin: 20,
      file_name,
      version,
      author,
      repo_owner,
      github_repo,
      download_url,
      patreon_url,
    };
  }
}

// const ICON_BYTES: &[u8] = if cfg!(target_os = "windows") { include_bytes!("../resources/icon.ico") } else { include_bytes!("../resources/icon.png") };

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let old_app = std::env::current_exe().unwrap().parent().unwrap().join(OLDAPPNAME);
  if old_app.exists() {
    Global::del_path(old_app).unwrap()
  }

  gtk::gtk_ui::gtk_ui();
  Ok(())
}
