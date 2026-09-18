//! The `clipboard-serve` helper subcommand.

use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use zeroize::Zeroizing;

/// Printed on stdout once the helper owns the selection.
pub const READY: &str = "ready";
/// Exit code when the compositor offers no data-control protocol.
pub const EXIT_UNAVAILABLE: i32 = 2;
/// Exit code when stdin was empty.
pub const EXIT_EMPTY: i32 = 3;

pub const PASSWORD_HINT_MIME: &str = "x-kde-passwordManagerHint";

/// Offered MIME types as `(mime, is_hint)`. The text types carry the value; the hint carries
/// `secret`.
pub fn offers(secret: bool) -> Vec<(&'static str, bool)> {
    let mut offers = vec![
        ("text/plain;charset=utf-8", false),
        ("text/plain", false),
        ("UTF8_STRING", false),
    ];
    if secret {
        offers.push((PASSWORD_HINT_MIME, true));
    }
    offers
}

/// Reads the whole value; `None` if it is empty.
pub fn read_value(mut input: impl Read) -> std::io::Result<Option<Zeroizing<Vec<u8>>>> {
    let mut buf = Zeroizing::new(Vec::new());
    input.read_to_end(&mut buf)?;
    Ok((!buf.is_empty()).then_some(buf))
}

/// How long the helper waits before exiting on its own, in case the parent died without
/// killing it. Only secret copies are time-limited: a non-secret value stays on the clipboard
/// until another client replaces it, like any normal copy.
pub fn watchdog_delay(timeout: u64, secret: bool) -> Option<Duration> {
    secret.then(|| Duration::from_secs(timeout.saturating_add(5)))
}

/// Serves stdin on the clipboard until ownership is lost. A secret copy also exits on its own
/// `timeout + 5` s after start in case the parent died without killing it.
pub fn main(timeout: u64, secret: bool) -> ExitCode {
    let value = match read_value(std::io::stdin().lock()) {
        Ok(Some(v)) => v,
        Ok(None) => return exit(EXIT_EMPTY),
        Err(e) => {
            tracing::error!("reading stdin: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(delay) = watchdog_delay(timeout, secret) {
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            std::process::exit(0);
        });
    }

    use wl_clipboard_rs::copy::{self, MimeSource, MimeType, Options, ServeRequests, Source};
    let sources = offers(secret)
        .into_iter()
        .map(|(mime, is_hint)| MimeSource {
            source: Source::Bytes(if is_hint {
                b"secret".to_vec().into_boxed_slice()
            } else {
                value.to_vec().into_boxed_slice()
            }),
            mime_type: MimeType::Specific(mime.to_owned()),
        })
        .collect();
    drop(value);

    let mut options = Options::new();
    options
        .foreground(true)
        .serve_requests(ServeRequests::Unlimited)
        .omit_additional_text_mime_types(true);
    let prepared = match options.prepare_copy_multi(sources) {
        Ok(p) => p,
        Err(copy::Error::MissingProtocol { name, .. }) => {
            tracing::error!("compositor lacks {name}");
            return exit(EXIT_UNAVAILABLE);
        }
        Err(e) => {
            tracing::error!("clipboard: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut stdout = std::io::stdout().lock();
    if writeln!(stdout, "{READY}")
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    drop(stdout);

    match prepared.serve() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("clipboard: {e}");
            ExitCode::FAILURE
        }
    }
}

fn exit(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_offers_include_hint() {
        let offers = offers(true);
        assert!(offers.contains(&("text/plain;charset=utf-8", false)));
        assert!(offers.contains(&("text/plain", false)));
        assert!(offers.contains(&("UTF8_STRING", false)));
        assert!(offers.contains(&(PASSWORD_HINT_MIME, true)));
    }

    #[test]
    fn plain_offers_have_no_hint() {
        assert!(offers(false).iter().all(|(_, hint)| !hint));
    }

    #[test]
    fn empty_input_is_none() {
        assert!(read_value(&b""[..]).unwrap().is_none());
        assert_eq!(read_value(&b"abc"[..]).unwrap().unwrap().as_slice(), b"abc");
    }

    #[test]
    fn only_secret_copies_self_destruct() {
        assert_eq!(watchdog_delay(90, true), Some(Duration::from_secs(95)));
        // A username or website must survive: no timeout was requested for it.
        assert_eq!(watchdog_delay(90, false), None);
        assert_eq!(watchdog_delay(0, false), None);
    }

    #[test]
    fn empty_stdin_exit_code() {
        assert_eq!(EXIT_EMPTY, 3);
        assert_eq!(EXIT_UNAVAILABLE, 2);
    }
}
