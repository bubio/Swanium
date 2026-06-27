//! Process-wide logging initialisation.

use tracing_subscriber::EnvFilter;

/// Default log directive when `RUST_LOG` is unset.
const DEFAULT_FILTER: &str = "info";

/// Install the global tracing subscriber.
///
/// Honours the `RUST_LOG` environment variable, falling back to
/// [`DEFAULT_FILTER`] (`info`). Calling this more than once is harmless: the
/// second call fails to install a subscriber and is ignored, so a frontend and
/// a test can both call it without panicking.
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    // `try_init` returns Err if a global subscriber is already set; that is an
    // expected, benign outcome here (e.g. a second call from a test).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
