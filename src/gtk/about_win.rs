use crate::{AppState, gtk::gtk_ui::MarginAll, logic::global::{GTKhelper, Global}};
use gtk::prelude::*;
use gtk4 as gtk;
use parking_lot::RwLock;
use std::sync::Arc;

/// Show the settings dialog as a child of `parent_win`.
///
/// `parent_win` must already be realized (shown) by your main UI.
pub fn about_win(window: &gtk::ApplicationWindow, aps: Arc<RwLock<AppState>>) {
  // Clone for closures
  let consts = aps.read().consts.clone();

  // Build a new ApplicationWindow, but attach it to the same Application as the parent
  let settings_win = gtk::ApplicationWindow::builder().transient_for(window).modal(true).resizable(false).title("About").default_width(300).default_height(100).build();

  // Layout grid with margins
  let grid = gtk::Grid::new();
  grid.set_row_spacing(consts.upad);
  grid.set_column_spacing(consts.upad);
  grid.set_margin_all(consts.margin);
  settings_win.set_child(Some(&grid));

  // let aps_c = aps.clone();

  let author = aps.read().consts.author.clone();
  let version = aps.read().consts.version.clone();
  let text = format!("Made by the\n{}\nThank you for your support\n\n{}", author, version);

  let about = gtk::Label::new(Some(&text));
  about.set_justify(gtk::Justification::Center);
  about.set_halign(gtk::Align::Center);
  about.set_hexpand(true);

  grid.attach(&about, 0, 0, 2, 1);

  let window_c = window.clone();
  let aps_c = aps.clone();

  let update_btn = gtk::Button::with_label("Update");
  update_btn.connect_clicked(move |_| {
    match Global::check_for_update(aps_c.clone()) {
      Ok(v) => {
        if v {
          match Global::download_latest_version(aps_c.clone()) {
            Ok(_) => (),
            Err(e) => GTKhelper::message_box(&window_c, "Error", format!("{}", e), None),
          }
        } else {
          GTKhelper::message_box(&window_c, "No updates", "No updates are currently available\nThis is the latest version\n\n", None);
        }
      }
      Err(e) => println!("{}{}", ("error"), e),
    };
  });

  // update_btn.add_css_class(("custom_button"));

  grid.attach(&update_btn, 0, 1, 1, 1);

  // let window_c = window.clone();
  // let aps_c = aps.clone();
  let url = aps.read().consts.patreon_url.clone();

  let url_c = url.clone();

  let support_btn = gtk::Button::with_label("Support");
  support_btn.connect_clicked(move |_| webbrowser::open(&url_c).unwrap());

  // let controller = gtk::EventControllerLegacy::new();
  // let url_c = url.clone();
  // controller.connect_event(move |ctrl, event| {
  //   // Only handle button-press events
  //   if event.event_type() == gtk::gdk::EventType::ButtonPress {
  //     // Downcast and inspect which mouse button
  //     let be: gtk::gdk::ButtonEvent = event.clone().downcast().unwrap();
  //     match be.button() {
  //       1 => {
  //         // Left-click: open URL in default browser
  //         if let Err(e) = gtk::gio::AppInfo::launch_default_for_uri(&url, None::<&gtk::gdk::AppLaunchContext>) {
  //           eprintln!("{}{}", ("Failed to open URL: "), e);
  //         }
  //       }
  //       3 => {
  //         // Right-click: copy URL to clipboard
  //         let display = support_btn.display();
  //         let clipboard = DisplayExt::clipboard(&display);
  //         clipboard.set_text(&url);

  //         // Show "Link copied" desktop notification
  //         let notificaion = gtk::gio::Notification::new(("Link copied"));
  //         notificaion.set_body(Some(&format!("Copied {} to clipboard", url)));
  //       }
  //       _ => {}
  //     }
  //   }
  //   // Return Inhibit(false) so GTK can continue normal handling
  //   // gtk::glib::Inhibit(false)
  //   gtk::glib::SignalHandlerId(1)
  // });

  // Attach the controller to our button
  // support_btn.add_controller(controller);

  grid.attach(&support_btn, 1, 1, 1, 1);

  settings_win.present();
}
