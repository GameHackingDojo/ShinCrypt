#[cfg(target_os = "windows")]
extern crate winres;

fn main() {
  // Read the *target* OS (not the host!) from Cargo’s env vars.
  let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS must be set by Cargo");

  if target_os == "windows" {
    // On Windows, embed the .ico via winres
    let mut res = winres::WindowsResource::new();
    // Use forward slashes—Rust will normalize for Windows at compile time
    res.set_icon("resources/icon.ico");
    res.compile().expect("failed to compile Windows resources")
  } else {
    // On non‑Windows targets, emit a warning but continue building
    println!("cargo:warning=Skipping Windows resources on target OS `{}`", target_os);
  }

  // Always rerun if build.rs changes
  println!("cargo:rerun-if-changed=build.rs");
}
