//! Everything that moves or renames files inside the archive folder, or the folder itself.
//! - `path_change`: moves one PDF to a new path (a new name, a new folder, or both).
//! - `rename_pdf`: gives one PDF a new name.
//! - `move_archive`: moves the whole archive folder to a new place.
//!
//! Each line below makes a file in the folder `archive_folder_manipulations/` a child
//! module; from outside it is reached as `crate::archive_folder_manipulations::<name>`.
//! `pub`: other parts of the app may use it.

pub mod move_archive;
pub mod move_file_within_archive;
pub mod utils;
