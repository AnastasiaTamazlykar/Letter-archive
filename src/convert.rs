//! Files a letter on a background thread, so the window keeps responding while Vision
//! reads the letters and the LLM sums them up (several seconds). Every job gets its own
//! thread, so any number of them can run at the same time. Two ways in:
//! - images: they first become one PDF in the input folder;
//! - a PDF the user picked.
//!
//! Both then go through `analyze_place_pdf`, which analyzes the PDF, copies it to the
//! output folder and records it.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

use eframe::egui;

use crate::{
    store_analyze_pdf::analyze_place_pdf,
    language,
    paths::{DB_FILE, INPUT_DIR},
    pdf_crud::PdfCrud,
    pdf_name, pipeline,
};

/// A conversion running in the background; the window asks it for news every frame.
pub struct Conversion {
    /// What the job is about, to tell the jobs in the queue apart: "3 image(s)" or a file name.
    label: String,
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
    /// and filing it in `output_dir`. `ctx` lets the thread wake the window up when done.
    pub fn start_images(images: Vec<PathBuf>, output_dir: PathBuf, ctx: egui::Context) -> Self {
        let label = format!("{} image(s)", images.len());
        let progress = format!("Creating and analyzing a PDF of {label}…");
        // `move` hands `images` and `output_dir` over to the closure, which then owns them.
        Self::run(label, progress, ctx, move || create_and_file(&images, &output_dir))
    }

    /// Starts filing a PDF the user picked in `output_dir`. The picked file itself stays
    /// where it is.
    pub fn start_pdf(pdf: PathBuf, output_dir: PathBuf, ctx: egui::Context) -> Self {
        let label = pdf
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let progress = format!("Analyzing {label}…");
        Self::run(label, progress, ctx, move || {
            analyze_place_pdf(&pdf, &output_dir, &mut open_archive()?)
        })
    }

    /// Runs `job` on a new thread and sends its result back to the window.
    /// `impl FnOnce() -> … + Send + 'static` accepts any closure that can be called once,
    /// may move to another thread (`Send`), and borrows nothing that could end before
    /// the thread does (`'static`). The compiler checks all three.
    fn run(
        label: String,
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
            label,
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

    /// For example "3 image(s)" or "scan.pdf".
    pub fn label(&self) -> &str {
        &self.label
    }

    /// A job that is already done, with no thread behind it: lets the tests of the window
    /// show a success without the LLM. `cfg(test)`: left out of the app itself.
    #[cfg(test)]
    pub fn already_done(label: &str, result: Result<PathBuf, String>) -> Self {
        let (sender, outcome) = mpsc::channel();
        sender.send(result).unwrap(); // waits in the channel until `receive` picks it up
        Conversion {
            label: label.to_string(),
            outcome,
            progress: String::new(),
            result: None,
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

/// Runs on the background thread: makes the PDF in the input folder, then files it in
/// `output_dir`. The input PDF is kept, even if filing fails.
fn create_and_file(images: &[PathBuf], output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let input = new_input_path()?;
    pipeline::images_to_pdf(images, &input, language::LETTERS.code)?;
    analyze_place_pdf(&input, output_dir, &mut open_archive()?)
}

/// The thread opens its own database connection: the window's one can't be shared
/// with another thread. SQLite is fine with two connections to one file.
fn open_archive() -> Result<PdfCrud, Box<dyn Error>> {
    PdfCrud::open(Path::new(DB_FILE))
}

/// Counts the jobs started since the app began; see `new_input_path`.
/// An atomic number can be changed from several threads at once, without a lock.
static JOB_COUNTER: AtomicU32 = AtomicU32::new(1);

/// A free path in the input folder named after the time and the job number,
/// e.g. "2026-11-18_14-30-05_1.pdf". Creates the folder if it is missing.
/// The number keeps jobs started in the same second apart: the file only appears when
/// its PDF is done, so `free_pdf_path` alone could give two jobs the same name.
fn new_input_path() -> Result<PathBuf, Box<dyn Error>> {
    let dir = Path::new(INPUT_DIR);
    fs::create_dir_all(dir)?;
    let now = jiff::ZonedDateTime::now().strftime("%Y-%m-%d_%H-%M-%S").to_string();
    let number = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(pdf_name::free_pdf_path(dir, &format!("{now}_{number}")))
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
            label: String::new(),
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
        let output = PathBuf::from(crate::paths::OUTPUT_DIR);
        let mut conversion =
            Conversion::start_images(vec![missing], output, egui::Context::default());

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
    fn test_input_paths_differ_within_the_same_second() {
        // Jobs started in the same second must not get the same input file name.
        let paths: Vec<PathBuf> = (0..20).map(|_| new_input_path().unwrap()).collect();

        for (index, path) in paths.iter().enumerate() {
            assert!(!paths[index + 1..].contains(path), "twice: {}", path.display());
        }
    }

    #[test]
    fn test_label_names_the_job() {
        // The label tells the jobs in the queue apart: how many images, or the file name.
        let photos = vec![PathBuf::from("/no/such/a.HEIC"), PathBuf::from("/no/such/b.HEIC")];
        let output = PathBuf::from(crate::paths::OUTPUT_DIR);

        let conversion = Conversion::start_images(photos, output, egui::Context::default());

        assert_eq!(conversion.label(), "2 image(s)");
    }

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
