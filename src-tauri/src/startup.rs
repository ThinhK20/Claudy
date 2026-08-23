//! What the app shows on launch.
//!
//! Split out from `lib.rs` so the rule is a pure function: `setup()` needs a
//! live `AppHandle` and cannot be unit-tested, this can.

/// Passed to the executable by the OS login entry. Registered on the autostart
/// plugin builder in `lib.rs`, which bakes it into the command it writes.
pub const AUTOSTART_FLAG: &str = "--autostart";

/// The main window opens on launch unless the user asked otherwise: either a
/// standing "start minimized" preference, or this launch coming from the OS
/// login entry, which is always silent — nobody wants a window in their face
/// every time they sign in.
pub fn should_show_main<I>(args: I, start_minimized: bool) -> bool
where
    I: IntoIterator<Item = String>,
{
    let launched_at_login = args.into_iter().any(|a| a == AUTOSTART_FLAG);
    !launched_at_login && !start_minimized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> Vec<String> {
        std::iter::once("claudy.exe".to_string())
            .chain(extra.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn plain_launch_opens_the_window() {
        assert!(should_show_main(args(&[]), false));
    }

    #[test]
    fn start_minimized_hides_a_plain_launch() {
        assert!(!should_show_main(args(&[]), true));
    }

    #[test]
    fn login_launch_stays_hidden_even_when_start_minimized_is_off() {
        assert!(!should_show_main(args(&[AUTOSTART_FLAG]), false));
    }

    #[test]
    fn login_launch_stays_hidden_when_start_minimized_is_on() {
        assert!(!should_show_main(args(&[AUTOSTART_FLAG]), true));
    }

    #[test]
    fn an_unrelated_argument_does_not_suppress_the_window() {
        assert!(should_show_main(args(&["--autostart-later"]), false));
    }
}
