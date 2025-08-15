use crate::{AppState, OLDAPPNAME};
use gtk::{gdk::prelude::DisplayExt, prelude::*};
use gtk4 as gtk;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileDir {
  File,
  Directory,
}

impl FileDir {
  pub fn what(path: impl AsRef<std::path::Path>) -> Result<FileDir, Box<dyn std::error::Error>> {
    let path = path.as_ref();
    if !path.exists() {
      return Err("Invalid path".into());
    }

    if path.is_file() {
      Ok(FileDir::File)
    } else if path.is_dir() {
      Ok(FileDir::Directory)
    } else {
      Err("".into())
    }
  }
}

pub struct Global {}
impl Global {
  pub fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    let millis = duration.subsec_millis();

    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
  }

  pub fn calculate_speed(data_gb: f64, duration: std::time::Duration) -> f64 {
    // Convert Gigabytes (GB) to Megabytes (MB)
    let data_mb = data_gb * 1024.0; // 1 GB = 1024 MB

    // Convert Duration to seconds (as a floating-point number)
    let duration_secs = duration.as_secs_f64();

    // Calculate speed in MB/s
    data_mb / duration_secs
  }

  pub fn check_for_update(aps: Arc<RwLock<AppState>>) -> Result<bool, String> {
    let current_version = aps.read().consts.version.clone();
    let url = aps.read().consts.download_url.clone();

    let client = reqwest::blocking::Client::new();
    let response = client.get(&url).header("User-Agent", "Rust-Reqwest").send().map_err(|e| format!("{}{}", ("Failed to send request: "), e))?;

    if !response.status().is_success() {
      return Err(format!("{}{:?}", ("Failed to fetch latest release: "), response.status()));
    }

    let json: serde_json::Value = response.json().map_err(|e| format!("{}{}", ("Failed to parse JSON: "), e))?;
    let tag_name = json["tag_name"].as_str().unwrap_or_else(|| {
      println!("{}", ("No tag_name found in JSON."));
      ""
    });

    if tag_name.is_empty() {
      return Ok(false);
    }

    // println!("{}{}", ("Latest release: "), tag_name);

    let update_available = match Self::compare_versions(&current_version, tag_name) {
      core::cmp::Ordering::Less => {
        println!("{}{}", ("A new version is available: "), tag_name);
        true
      }
      core::cmp::Ordering::Equal => {
        println!("{}", ("You are using the latest version."));
        false
      }
      core::cmp::Ordering::Greater => {
        println!("{}", ("You are using a newer version than the latest release."));
        false
      }
    };

    Ok(update_available)
  }

  fn compare_versions(current: &str, latest: &str) -> core::cmp::Ordering {
    let separator = (".").chars().next().unwrap(); // Convert to `char`

    let mut current_parts: Vec<u32> = current.split(separator).map(|s| s.parse().unwrap_or(0)).collect();
    let mut latest_parts: Vec<u32> = latest.split(separator).map(|s| s.parse().unwrap_or(0)).collect();

    // Normalize length by padding with zeros
    while current_parts.len() < latest_parts.len() {
      current_parts.push(0);
    }
    while latest_parts.len() < current_parts.len() {
      latest_parts.push(0);
    }

    current_parts.cmp(&latest_parts)
  }

  pub fn download_latest_version(aps: Arc<RwLock<AppState>>) -> Result<String, String> {
    let asset_name = aps.read().consts.file_name.clone();
    // println!("{}{}", ("file name = "), asset_name);
    let url = aps.read().consts.download_url.clone();
    // println!("{}{}", ("download url = "), url);

    // Create a reqwest client
    let client = reqwest::blocking::Client::new();

    // Send a GET request to the GitHub API to fetch the latest release
    let response: reqwest::blocking::Response = client.get(&url).header("User-Agent", "Rust-Reqwest").send().map_err(|e| format!("{}{}", ("Failed to send request: "), e))?;

    // Ensure the request was successful
    if !response.status().is_success() {
      return Err(format!("{}{:?}", ("Failed to fetch latest release: "), response.status()));
    }

    // Parse the JSON response
    let json: serde_json::Value = response.json().map_err(|e| format!("{}{}", ("Failed to parse JSON: "), e))?;

    // println!("{}{}", ("JSON Response: "), json);
    // println!("{}{}", ("Looking for asset: "), asset_name);

    // Find the asset download URL
    let assets = &json["assets"];
    // let download_url = assets.as_array().and_then(|arr| arr.iter().find(|&asset| asset[("name")].as_str() == Some(&asset_name))).and_then(|asset| asset[("browser_download_url")].as_str()).ok_or(("Asset not found"))?;

    let download_url = assets.as_array().and_then(|arr| arr.iter().find(|asset| asset["name"].as_str().map(|name| name.to_lowercase() == asset_name.to_lowercase()).unwrap_or(false))).and_then(|asset| asset["browser_download_url"].as_str()).ok_or("Asset not found")?;

    // Download the asset
    let mut response = client.get(download_url).header("User-Agent", "Rust-Reqwest").send().map_err(|e| format!("{}{}", ("Failed to download asset: "), e))?;

    Self::prepare_update_file().unwrap();

    // Write the file to disk
    let mut file = std::fs::File::create(&asset_name).map_err(|e| format!("{}{}", ("Failed to create file: "), e))?;
    std::io::copy(&mut response, &mut file).map_err(|e| format!("{}{}", ("Failed to write file: "), e))?;

    Ok(format!("{}{}{}", ("Downloaded "), &asset_name, (" successfully!")))
  }

  fn prepare_update_file() -> Result<(), String> {
    let cur_path = std::env::current_exe().unwrap();
    let old_path = cur_path.with_file_name(OLDAPPNAME);
    if let Err(e) = std::fs::rename(&cur_path, &old_path) {
      return Err(format!("Failed to rename file, error: {}", e));
    };

    Ok(())
  }

  /// deletes given PathBuf whather it's file or directory
  pub fn del_path(path: std::path::PathBuf) -> Result<(), String> {
    if path.is_file() {
      if let Err(e) = std::fs::remove_file(path.clone()) {
        return Err(format!("File can't be deleted\n{}", e));
      }
    } else if path.is_dir() {
      if let Err(e) = std::fs::remove_dir_all(path.clone()) {
        return Err(format!("Directory can't be deleted\n{}", e));
      }
    }

    Ok(())
  }
}

// pub struct Tar {}
// impl Tar {
//   pub fn tar(path: std::path::PathBuf) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
//     // Create output tar path
//     let tar_path = Self::construct_tar_path(&path)?;

//     let file = std::fs::File::create(&tar_path)?;
//     let mut builder = tar::Builder::new(file);

//     let base_path = Self::get_base_path(&path)?;

//     // If the input is a single file, store just that file at the archive root
//     if path.is_file() {
//       let file_name = path.file_name().ok_or_else(|| format!("invalid file name for '{}'", path.display()))?;
//       // append the file so it appears at the archive root with just its filename
//       builder.append_path_with_name(&path, file_name)?;
//     } else {
//       // Append all files/dirs under base_path to the archive root (preserves empty dirs).
//       // Passing "." as the archive-path root places contents at the root of the tar.
//       builder.append_dir_all(".", &base_path)?;
//     }

//     builder.finish()?;
//     Ok(tar_path)
//   }

//   pub fn untar(path: std::path::PathBuf) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
//     if !path.is_file() {
//       return Err(format!("'{}' is not a valid tar file", path.display()).into());
//     }

//     let target_dir = Self::construct_output_dir(&path)?;
//     std::fs::create_dir_all(&target_dir)?;

//     let mut f = std::fs::File::open(&path)?;
//     let mut archive = tar::Archive::new(&mut f);
//     archive.unpack(&target_dir)?;

//     std::fs::remove_file(&path)?;
//     Ok(target_dir)
//   }

//   fn construct_tar_path(path: &std::path::PathBuf) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
//     let mut p = path.clone();
//     // if path is a file, replace extension, otherwise add .tar
//     if p.is_file() {
//       p.set_extension("tar");
//     } else {
//       // turn "folder" -> "folder.tar"
//       let file_name = p.file_name().ok_or("invalid path")?;
//       let mut out = p.clone();
//       out.set_file_name(format!("{}.tar", file_name.to_string_lossy()));
//       p = out;
//     }
//     Ok(p)
//   }

//   fn get_base_path(path: &std::path::PathBuf) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> { if path.is_file() { path.parent().map(|p| p.to_path_buf()).ok_or("no parent".into()) } else { Ok(path.clone()) } }

//   fn construct_output_dir(path: &std::path::PathBuf) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
//     // e.g. "archive.tar" -> "archive"
//     let mut out = path.clone();
//     if let Some(stem) = path.file_stem() {
//       out.set_file_name(stem);
//     } else {
//       out.push(".untarred");
//     }
//     Ok(out)
//   }
// }

pub struct GTKhelper {}
impl GTKhelper {
  // pub fn img_from_bytes( bytes: &[u8]) -> Result<gtk::Image, gtk::glib::Error> {
  //   let loader = gtk::gdk_pixbuf::PixbufLoader::new();
  //   loader.write(bytes)?;
  //   loader.close()?;
  //   let pixbuf = loader.pixbuf().ok_or_else(|| gtk::glib::Error::new(gtk::gdk_pixbuf::PixbufError::Failed, "Failed to get pixbuf"))?;
  //   Ok(gtk::Image::from_pixbuf(Some(&pixbuf)))
  // }

  pub fn monitor_info(window: &gtk::ApplicationWindow) -> Result<gtk::gdk::Monitor, Box<dyn std::error::Error>> {
    let display = gtk::prelude::WidgetExt::display(window);
    // println!("Display name: {}", display.name());

    // If you need the monitor (screen) information
    if let Some(monitor) = display.monitor_at_surface(&window.surface().unwrap()) {
      Ok(monitor)
      // println!("Monitor geometry: {:?}", monitor.geometry());
      // println!("Monitor scale factor: {}", monitor.scale_factor());
      // println!("Monitor refresh rate: {}", monitor.refresh_rate());
    } else {
      Err("Failed to get monitor info".into())
    }
  }

  #[cfg(target_os = "windows")]
  pub fn get_window_dimensions(hwnd: winapi::shared::windef::HWND) -> Option<(i32, i32)> {
    let mut rect = winapi::shared::windef::RECT { left: 0, top: 0, right: 0, bottom: 0 };

    unsafe { if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 { Some((rect.right - rect.left, rect.bottom - rect.top)) } else { None } }
  }

  #[cfg(target_os = "windows")]
  fn get_hwnd(window: &gtk::ApplicationWindow) -> Option<*mut winapi::shared::windef::HWND__> {
    // Get the GDK surface

    use gtk::{glib::object::{Cast, ObjectExt}, prelude::NativeExt};
    let surface = window.surface()?;

    // Check if this is a Win32 surface (Windows platform)
    if !surface.is::<gdk4_win32::Win32Surface>() {
      return None;
    }

    // Downcast to Win32Surface
    let win32_surface = surface.downcast::<gdk4_win32::Win32Surface>().ok()?;

    // Conversion from gdk4_win32::HWND to *mut HWND__
    let hwnd_isize = win32_surface.handle().0;
    Some(hwnd_isize as *mut _)
  }

  #[cfg(target_os = "windows")]
  pub fn centre_to_screen(window: &gtk::ApplicationWindow) -> Result<(), Box<dyn std::error::Error>> {
    use gtk::gdk::prelude::MonitorExt;

    let monitor = Self::monitor_info(window)?;
    let monitor_x = monitor.geometry().x();
    let monitor_y = monitor.geometry().y();
    let monitor_w = monitor.geometry().width();
    let monitor_h = monitor.geometry().height();
    let scale = monitor.scale();

    if let Some(hwnd) = Self::get_hwnd(window) {
      unsafe {
        if let Some(win_dim) = Self::get_window_dimensions(hwnd) {
          let new_x = monitor_x + ((monitor_w - win_dim.0) as f64 / 2.0 * scale) as i32;
          let new_y = monitor_y + ((monitor_h - win_dim.1) as f64 / 2.0 * scale) as i32;

          winapi::um::winuser::SetWindowPos(hwnd, winapi::um::winuser::HWND_TOP, new_x, new_y, 0, 0, winapi::um::winuser::SWP_NOSIZE);
        }
      };
    }

    Ok(())
  }

  pub fn message_box(window: &gtk::ApplicationWindow, message: impl AsRef<str>, detail: impl AsRef<str>, buttons: Option<Vec<&str>>) {
    let buttons = buttons.unwrap_or_else(|| vec!["Ok"]);
    let alert = gtk::AlertDialog::builder().modal(true).message(message.as_ref()).detail(detail.as_ref()).buttons(buttons).default_button(1).cancel_button(0).build();
    alert.choose(Some(window), None::<&gtk::gio::Cancellable>, move |res| if res == Ok(1) {});
  }

  pub fn drag_n_drop(entry: &gtk::Entry) {
    let entry_c = entry.clone();

    // Create a DropTarget for files
    let drop_target = gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY);

    drop_target.connect_drop(move |_target, value, _, _| {
      if let Ok(file_list) = value.get::<gtk::gdk::FileList>() {
        if let Some(file) = file_list.files().first() {
          if let Some(path) = file.path() {
            entry_c.set_text(path.to_string_lossy().as_ref());
            return true;
          }
        }
      }
      false
    });

    // Attach drop target to the entry
    entry.add_controller(drop_target);
  }
}
