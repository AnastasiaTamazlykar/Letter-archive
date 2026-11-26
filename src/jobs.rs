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

use crate::{file_system_actions::utils::free_pdf_path, process_document::analyze_place_pdf, constants::{DB_FILE, INPUT_DIR}, document_crud::PdfCrud, images_to_pdf, constants};

pub struct Job {
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

impl Job {
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
        Job {
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
        Job {
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

/// The filing jobs, running in the background at the same time or already done, oldest
/// first. A job that worked leaves the list at once; a failed one stays (to show its
/// error) until the user clears it. It knows nothing about the window's tabs: `check`
/// only reports that a letter was filed, and the window decides what to show.
#[derive(Default)]
pub struct JobQueue {
    // `pub(crate)` for now, so the window's tests can look inside; private again once
    // those tests move next to the jobs.
    pub(crate) list: Vec<Job>,
}

impl JobQueue {
    /// Adds a job that has just been started.
    pub fn add(&mut self, job: Job) {
        self.list.push(job);
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// True while at least one job has not finished.
    pub fn is_filing(&self) -> bool {
        self.list.iter().any(|job| !job.is_finished())
    }

    /// Draws the list of jobs; removes the finished ones if "Clear finished" was clicked.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        if job_queue(ui, &self.list) {
            // `retain` keeps only the jobs for which the closure says true.
            self.list.retain(|job| !job.is_finished());
        }
    }

    pub fn are_freshly_finished_jobs(&mut self) -> bool {
        let mut are_freshly_finished_jobs = false;
        for job in &mut self.list {
            let was_finished = job.is_finished();
            job.receive();
            // Only a job that finished in this frame counts; old ones were handled before.
            are_freshly_finished_jobs |= !was_finished && job.succeeded();
        }
        // A job that worked has nothing more to say: the letter is in the table. A failed
        // one stays in the queue, with its error, until the user clears it.
        self.list.retain(|job| !job.succeeded());
        are_freshly_finished_jobs
    }
}

/// Draws one line per job: a spinner and its progress while it runs, then its result.
/// Draws nothing when there are no jobs. Returns whether "Clear finished" was clicked;
/// the caller removes the finished jobs, because only it owns the list.
pub(crate) fn job_queue(ui: &mut egui::Ui, jobs: &[Job]) -> bool {
    if jobs.is_empty() {
        return false;
    }
    let running = jobs.iter().filter(|job| !job.is_finished()).count();
    // A job that worked has already left the list, so a finished one here has failed.
    let failed = jobs.len() - running;
    let mut clear = false;
    ui.horizontal(|ui| {
        ui.strong(format!("Jobs: {running} running, {failed} failed"));
        // Nothing to clear while every job is still running.
        clear = ui
            .add_enabled(failed > 0, egui::Button::new("Clear failed"))
            .clicked();
    });
    for job in jobs {
        ui.horizontal(|ui| {
            if job.is_finished() {
                ui.label(format!("{}: {}", job.label(), job.summary()));
            } else {
                ui.spinner();
                ui.label(job.progress());
            }
        });
    }
    clear
}

fn create_and_file(images: &[PathBuf], output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let input = new_input_path()?;
    images_to_pdf::images_to_pdf(images, &input, constants::LETTERS.code)?;
    analyze_place_pdf(&input, output_dir, &mut open_archive()?)
}

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
    Ok(free_pdf_path(dir, &format!("{now}_{number}")))
}

// Small file, so the tests stay here. They show how the thread and the window talk:
// the thread sends one result into the channel, and the window picks it up
// with `receive()`, which never waits.
#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use super::*;

    fn by_hand(outcome: Receiver<Result<PathBuf, String>>) -> Job {
        Job {
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
        let output = PathBuf::from(crate::constants::OUTPUT_DIR);
        let mut conversion =
            Job::start_images(vec![missing], output, egui::Context::default());

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
        let output = PathBuf::from(crate::constants::OUTPUT_DIR);

        let conversion = Job::start_images(photos, output, egui::Context::default());

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
