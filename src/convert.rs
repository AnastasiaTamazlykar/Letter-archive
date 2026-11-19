//! Files a letter on a background thread, so the window keeps responding while Vision
//! reads the letters and the LLM sums them up (several seconds). Two ways in:
//! - images: they first become one PDF in the input folder;
//! - a PDF the user picked.
//!
//! Both then go through `analyze_place_pdf`, which analyzes the PDF, copies it to the
//! output folder and records it.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use eframe::egui;

use crate::{
    store_analyze_pdf::analyze_place_pdf,
    language,
    paths::{DB_FILE, INPUT_DIR, OUTPUT_DIR},
    pdf_crud::PdfCrud,
    pdf_name, pipeline,
};

/// A conversion running in the background; the window asks it for news every frame.
pub struct Conversion {
    /// The receiving end of a channel: the thread sends its one result into it when done:
    /// where the filed PDF is, or what went wrong. The error is text, because
    /// `Box<dyn Error>` may not cross threads.
    outcome: Receiver<Result<PathBuf, String>>,
    /// What the thread is doing, shown while it runs.
    progress: String,
    /// The result, once it has arrived.
    result: Option<Result<PathBuf, String>>,
}

impl Conversion {
    /// Starts making one PDF from `images`, one page per image, in the order given,
    /// and filing it. `ctx` lets the thread wake the window up when it is done.
    pub fn start_images(images: Vec<PathBuf>, ctx: egui::Context) -> Self {
        let progress = format!("Creating and analyzing a PDF of {} image(s)…", images.len());
        // `move` hands `images` over to the closure, which then owns it.
        Self::run(progress, ctx, move || create_and_file(&images))
    }

    /// Starts filing a PDF the user picked. The picked file itself stays where it is.
    pub fn start_pdf(pdf: PathBuf, ctx: egui::Context) -> Self {
        let progress = format!("Analyzing {}…", pdf.display());
        Self::run(progress, ctx, move || {
            analyze_place_pdf(&pdf, Path::new(OUTPUT_DIR), &mut open_archive()?)
        })
    }

    /// Runs `job` on a new thread and sends its result back to the window.
    /// `impl FnOnce() -> … + Send + 'static` accepts any closure that can be called once,
    /// may move to another thread (`Send`), and borrows nothing that could end before
    /// the thread does (`'static`). The compiler checks all three.
    fn run(
        progress: String,
        ctx: egui::Context,
        job: impl FnOnce() -> Result<PathBuf, Box<dyn Error>> + Send + 'static,
    ) -> Self {
        let (sender, outcome) = mpsc::channel();
        thread::spawn(move || {
            let result = job().map_err(|error| error.to_string());
            // Sending fails only if the window was closed; then nobody needs the result.
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        Conversion {
            outcome,
            progress,
            result: None,
        }
    }

    /// Checks, without waiting, whether the thread has sent its result.
    pub fn receive(&mut self) {
        if self.result.is_some() {
            return;
        }
        match self.outcome.try_recv() {
            Ok(result) => self.result = Some(result),
            Err(TryRecvError::Empty) => {} // still working
            // The thread ended without sending: it crashed (a "panic" in Rust).
            Err(TryRecvError::Disconnected) => {
                self.result = Some(Err("the conversion stopped unexpectedly".into()));
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        self.result.is_some()
    }

    /// True once the letter is filed. `matches!` checks the value against a pattern;
    /// `_` stands for any path.
    pub fn succeeded(&self) -> bool {
        matches!(self.result, Some(Ok(_)))
    }

    /// For example "Creating and analyzing a PDF of 3 image(s)…".
    pub fn progress(&self) -> &str {
        &self.progress
    }

    /// For example "Filed as data/pdfs/output/Brief.pdf", or what went wrong.
    pub fn summary(&self) -> String {
        match &self.result {
            Some(Ok(pdf)) => format!("Filed as {}", pdf.display()),
            Some(Err(error)) => format!("Failed: {error}"),
            None => String::new(),
        }
    }
}

/// Runs on the background thread: makes the PDF in the input folder, then files it.
/// The input PDF is kept, even if filing fails.
fn create_and_file(images: &[PathBuf]) -> Result<PathBuf, Box<dyn Error>> {
    let input = new_input_path()?;
    pipeline::images_to_pdf(images, &input, language::LETTERS.code)?;
    analyze_place_pdf(&input, Path::new(OUTPUT_DIR), &mut open_archive()?)
}

/// The thread opens its own database connection: the window's one can't be shared
/// with another thread. SQLite is fine with two connections to one file.
fn open_archive() -> Result<PdfCrud, Box<dyn Error>> {
    PdfCrud::open(Path::new(DB_FILE))
}

/// A free path in the input folder named after the time, e.g. "2026-11-18_14-30-05.pdf".
/// Creates the folder if it is missing.
fn new_input_path() -> Result<PathBuf, Box<dyn Error>> {
    let dir = Path::new(INPUT_DIR);
    fs::create_dir_all(dir)?;
    let now = jiff::ZonedDateTime::now().strftime("%Y-%m-%d_%H-%M-%S").to_string();
    Ok(pdf_name::free_pdf_path(dir, &now))
}

// Small file, so the tests stay here. They show how the thread and the window talk:
// the thread sends one result into the channel, and the window picks it up
// with `receive()`, which never waits.
#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use super::*;

    /// A conversion with no thread behind it: the test itself plays the thread by
    /// sending into (or dropping) the other end of the channel.
    fn by_hand(outcome: Receiver<Result<PathBuf, String>>) -> Conversion {
        Conversion {
            outcome,
            progress: String::new(),
            result: None,
        }
    }

    /// A real thread: the photo is missing, so the pipeline fails at once (no Vision),
    /// and the error reaches the window naming the photo.
    #[test]
    fn test_start_reports_a_failure_from_the_thread() {
        let missing = PathBuf::from("/no/such/photo.HEIC");
        let mut conversion = Conversion::start_images(vec![missing], egui::Context::default());

        // Ask now and then, the way the window does once per frame; give up after 5 s.
        for _ in 0..500 {
            conversion.receive();
            if conversion.is_finished() {
                break;
            }
            sleep(Duration::from_millis(10));
        }

        let summary = conversion.summary();
        assert!(summary.starts_with("Failed: "), "got: {summary}");
        assert!(summary.contains("/no/such/photo.HEIC"), "got: {summary}");
    }

    /// `receive()` returns at once while nothing has arrived, and picks up the result
    /// as soon as it is there.
    #[test]
    fn test_receive_does_not_wait() {
        let (sender, outcome) = mpsc::channel();
        let mut conversion = by_hand(outcome);

        conversion.receive();
        assert!(!conversion.is_finished(), "nothing was sent yet");

        sender.send(Ok(PathBuf::from("data/pdfs/output/Brief.pdf"))).unwrap();
        conversion.receive();
        assert!(conversion.is_finished());
        assert!(conversion.succeeded());
        assert_eq!(conversion.summary(), "Filed as data/pdfs/output/Brief.pdf");
    }

    /// A thread that crashes drops its end of the channel without sending anything.
    /// The window notices that the channel is closed and reports it.
    #[test]
    fn test_a_thread_that_dies_is_reported() {
        let (sender, outcome) = mpsc::channel();
        let mut conversion = by_hand(outcome);

        drop(sender); // what a crashed thread leaves behind
        conversion.receive();

        assert!(conversion.is_finished());
        assert!(conversion.summary().contains("stopped unexpectedly"));
    }
}
