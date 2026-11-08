//! Makes one PDF from several images on a background thread, so the window keeps
//! responding while Vision reads the letters (a few seconds per image).

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use eframe::egui;

use crate::{language, pipeline};

/// A conversion running in the background; the window asks it for news every frame.
pub struct Conversion {
    /// The receiving end of a channel: the thread sends its one result into it when done.
    /// The error is text, because the pipeline's `Box<dyn Error>` may not cross threads.
    outcome: Receiver<Result<(), String>>,
    pdf: PathBuf,
    count: usize,
    /// The result, once it has arrived.
    result: Option<Result<(), String>>,
}

impl Conversion {
    /// Starts making `pdf` from `images`, one page per image, in the order given.
    /// `ctx` lets the thread wake the window up when it is done.
    pub fn start(images: Vec<PathBuf>, pdf: PathBuf, ctx: egui::Context) -> Self {
        let (sender, outcome) = mpsc::channel();
        let count = images.len();
        let target = pdf.clone();

        // `move` hands `images`, `target`, `sender` and `ctx` over to the new thread.
        // The compiler checks they are safe to use from another thread (the `Send` trait).
        thread::spawn(move || {
            let result = pipeline::images_to_pdf(&images, &target, language::LETTERS.code)
                .map_err(|error| error.to_string());
            // Sending fails only if the window was closed; then nobody needs the result.
            let _ = sender.send(result);
            ctx.request_repaint();
        });

        Conversion {
            outcome,
            pdf,
            count,
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

    /// For example "Making a PDF of 3 image(s)…".
    pub fn progress(&self) -> String {
        format!("Making a PDF of {} image(s)…", self.count)
    }

    /// For example "Saved /Users/…/letter.pdf", or what went wrong.
    pub fn summary(&self) -> String {
        match &self.result {
            Some(Ok(())) => format!("Saved {}", self.pdf.display()),
            Some(Err(error)) => format!("Failed: {error}"),
            None => String::new(),
        }
    }
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
    fn by_hand(outcome: Receiver<Result<(), String>>) -> Conversion {
        let pdf = PathBuf::from("/letters/brief.pdf");
        Conversion {
            outcome,
            pdf,
            count: 2,
            result: None,
        }
    }

    /// A real thread: the photo is missing, so the pipeline fails at once (no Vision),
    /// and the error reaches the window naming the photo.
    #[test]
    fn test_start_reports_a_failure_from_the_thread() {
        let missing = PathBuf::from("/no/such/photo.HEIC");
        let pdf = std::env::temp_dir().join("paper_archive_test_never.pdf");
        let mut conversion = Conversion::start(vec![missing], pdf, egui::Context::default());

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

        sender.send(Ok(())).unwrap();
        conversion.receive();
        assert!(conversion.is_finished());
        assert_eq!(conversion.summary(), "Saved /letters/brief.pdf");
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
