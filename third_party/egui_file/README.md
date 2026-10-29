# File dialog (a.k.a. file picker) for [egui](https://github.com/emilk/egui)

[![Crates.io](https://img.shields.io/crates/v/egui_file)](https://crates.io/crates/egui_file)
[![docs.rs](https://img.shields.io/badge/docs-website-blue)](https://docs.rs/egui_file)

![Screenshot from 2022-08-18 07-41-11](https://a4.pbase.com/o12/09/605909/1/176078343.sWACRhpq.ScreenshotFrom20260123140113.png)

Release notes are [here](https://codeberg.org/Barugon/egui_file/releases).

## Example

````toml
[dependencies]
egui_file = "0.28"
eframe = "0.36"
````

````rust
use eframe::{App, Frame, egui::CentralPanel};
use egui_file::FileDialog;
use std::{
  ffi::OsStr,
  path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Demo {
  opened_file: Option<PathBuf>,
  open_file_dialog: Option<FileDialog>,
}

impl App for Demo {
  fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
    CentralPanel::default().show(ui, |ui| {
      if (ui.button("Open")).clicked() {
        // Show only Rust source files.
        let filter = Box::new({
          let ext = Some(OsStr::new("rs"));
          move |path: &Path| -> bool { path.extension() == ext }
        });

        let mut dialog = FileDialog::open_file().show_files_filter(filter);
        if let Some(mut path) = self.opened_file.clone()
          && path.pop()
        {
          // Use the path of the previously opened file.
          dialog = dialog.initial_path(path);
        }

        dialog.open();
        self.open_file_dialog = Some(dialog);
      }

      if let Some(dialog) = &mut self.open_file_dialog
        && dialog.show(ui).selected()
        && let Some(file) = dialog.path()
      {
        self.opened_file = Some(file.to_path_buf());
      }
    });
  }
}

fn main() {
  let _ = eframe::run_native(
    "Open File Demo",
    eframe::NativeOptions::default(),
    Box::new(|_cc| Ok(Box::new(Demo::default()))),
  );
}
````

---

Originally part of the [dotrix project](https://github.com/lowenware/dotrix/blob/release/v0.5.3/dotrix_egui/src/extras/file_dialog.rs), separated into a stand-alone library and extended.