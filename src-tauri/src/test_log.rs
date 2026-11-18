//! A logger for tests: the lines this thread logs while [`capture`] runs,
//! so a test can check what a log line leaves out (P06, review A N-2).

use std::cell::RefCell;
use std::future::Future;

thread_local! {
    static LINES: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        LINES.with(|lines| {
            if let Some(lines) = lines.borrow_mut().as_mut() {
                lines.push(record.args().to_string());
            }
        });
    }

    fn flush(&self) {}
}

static CAPTURE: Capture = Capture;

/// Run `f` and return every line this thread logged meanwhile, at any level.
pub(crate) async fn capture<F, Fut, T>(f: F) -> Vec<String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    // The first test to ask installs it: test builds set no other logger.
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Trace);
    LINES.with(|lines| *lines.borrow_mut() = Some(Vec::new()));
    f().await;
    LINES.with(|lines| lines.borrow_mut().take().unwrap_or_default())
}
