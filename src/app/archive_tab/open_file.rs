use std::io;
use std::path::Path;
use std::process::Command;

pub fn open_in_viewer(path: &Path) -> io::Result<()> {
    run_if_exists(viewer_command(path), path)
}

/// Opens the folder that holds the PDF in Finder, with the file selected.
pub fn reveal_in_finder(path: &Path) -> io::Result<()> {
    run_if_exists(reveal_command(path), path)
}

/// `open <path>`. A `Command` is only a description of what to run: nothing starts
/// until `spawn`, so building it apart lets a test check it without opening anything.
fn viewer_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg(path);
    command
}

/// `open -R <path>`: `-R` ("reveal") shows the file in Finder instead of opening it.
fn reveal_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg("-R").arg(path);
    command
}

/// Starts `command` if the file is still there; it may have been moved since it was stored.
fn run_if_exists(mut command: Command, path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the file was moved or deleted",
        ));
    }
    // `spawn` starts `open` and does not wait for it, so the window stays responsive.
    // `?` hands an error back to the caller; the `Child` handle is not needed.
    command.spawn()?;
    Ok(())
}
