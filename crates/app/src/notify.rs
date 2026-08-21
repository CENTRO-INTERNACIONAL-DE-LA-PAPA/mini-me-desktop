//! A Windows toast when a background run finishes and nobody is watching.
//!
//! # Why this, and why not a crate
//!
//! The roadmap has carried this since P6.4b and calls it *"the clearest remaining 'the web app
//! cannot do this' affordance"* — the plan display (§209) says where a forty-minute run has got to
//! **if you are looking**, and §244's banner speaks only to somebody with the window open. Neither
//! reaches a researcher who switched to Excel.
//!
//! The same roadmap entry names the cost: *"a new dependency, and a new way for the packaged build
//! to fail on the machines §57–§60 fought."* That is the deciding constraint. A WinRT crate would
//! be a real dependency, `cfg`-gated to Windows, **impossible to compile or test on the machine
//! this is written on**, and a fresh build failure mode on a colleague's laptop.
//!
//! So this spawns PowerShell, which the app already does for WSL and git, and which cannot fail at
//! build time at all. It follows the pattern `theory_tools`, `datavoyager_tools` and
//! `autodiscovery_tools` all use: **a pure command builder, unit-tested, so the contract is checked
//! here rather than in a live run.** What remains untestable is one `Command::spawn`, which is as
//! small as this could be made.
//!
//! # What it deliberately does not do
//!
//! No icon, no buttons, no click-to-open. A toast that opens a conversation needs an
//! `AppUserModelID` registered in the Start Menu and a COM activator — real work, and none of it
//! worth anything until a plain toast is confirmed to appear at all. §244's banner is still there
//! when they come back, which is where the errand actually gets completed.

/// The most a toast line may carry.
///
/// Windows truncates a long toast itself, silently and mid-word. Cutting deliberately means the
/// sentence still reads, and the run's own name is what matters in it.
const TOAST_CHARS: usize = 90;

/// Cut on a word boundary, the way the panel does.
fn clip(text: &str, max: usize) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= max {
        return cleaned;
    }
    let clipped: String = cleaned.chars().take(max).collect();
    let cut = clipped.rfind(' ').unwrap_or(clipped.len());
    let kept = if cut > max / 2 {
        &clipped[..cut]
    } else {
        clipped.as_str()
    };
    format!("{}…", kept.trim_end())
}

/// Escape a string for a single-quoted PowerShell literal *and* for the XML it lands inside.
///
/// Both, because the text is a researcher's own run name and reaches two parsers. PowerShell
/// doubles an embedded `'`; the XML needs the five predefined entities. Getting either wrong turns
/// a run called `Bactericera & "spp." <trial>` into a toast that never appears, and a silent
/// no-toast is indistinguishable from the feature being absent.
fn escape(text: &str) -> String {
    let xml = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    // PowerShell's single-quote escape, applied after the XML pass so an `&apos;` is not re-escaped.
    xml.replace('\'', "''")
}

/// The PowerShell argv that raises one toast.
///
/// Pure, so the contract is a test rather than a live run. The script is deliberately blunt: load
/// the WinRT toast types, fill a `ToastText02` template, and show it under PowerShell's own
/// `AppUserModelID`. Using an existing AppId is what makes this work with no installer and no
/// Start Menu entry — the toast is attributed to PowerShell, which is honest and which a real
/// registration would fix later.
pub fn toast_command(title: &str, body: &str) -> Vec<String> {
    let title = escape(&clip(title, TOAST_CHARS));
    let body = escape(&clip(body, TOAST_CHARS));
    // `-Sta` because the WinRT notification APIs want a single-threaded apartment, and
    // `-NoProfile` so a researcher's profile script cannot slow or break it.
    let script = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, \
         ContentType=WindowsRuntime] > $null; \
         $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(\
         [Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
         $n = $t.GetElementsByTagName('text'); \
         $n.Item(0).AppendChild($t.CreateTextNode('{title}')) > $null; \
         $n.Item(1).AppendChild($t.CreateTextNode('{body}')) > $null; \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier(\
         '{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe')\
         .Show([Windows.UI.Notifications.ToastNotification]::new($t))"
    );
    vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        "-Sta".to_string(),
        "-WindowStyle".to_string(),
        "Hidden".to_string(),
        "-Command".to_string(),
        script,
    ]
}

/// Whether a finished run is worth interrupting somebody for.
///
/// **Only when the window is not the one they are looking at.** A toast for something already on
/// screen is noise, and §245 is the record of what a panel that says too much costs. The banner and
/// the jobs row already speak to someone with the window open; this is for the case neither can
/// reach.
pub fn worth_interrupting(window_active: bool) -> bool {
    !window_active
}

/// Raise the toast, or do nothing where there is nothing to raise it with.
///
/// Best-effort by design: a notification that fails must never affect the run it is about. Spawned
/// and forgotten — the child is a PowerShell that exits on its own, and waiting on it would block a
/// UI callback for the sake of a courtesy.
pub fn toast(title: &str, body: &str) {
    let argv = toast_command(title, body);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: without it a console flashes on screen, which is worse than no toast.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        match std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(_) => tracing::info!(%title, "raised a desktop notification"),
            Err(error) => tracing::warn!(%error, "could not raise a desktop notification"),
        }
    }
    #[cfg(not(windows))]
    {
        // Said rather than skipped silently, because this file is written and tested on Linux and
        // "nothing happened" is the same output as a bug.
        let _ = &argv;
        tracing::info!(%title, %body, "a desktop notification would be raised on Windows");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_the_one_windows_needs() {
        let argv = toast_command("Discovery finished", "Bactericera synthetic monitoring");
        assert_eq!(argv[0], "powershell");
        // `-Sta` is not optional: the WinRT notification APIs require a single-threaded apartment.
        assert!(argv.contains(&"-Sta".to_string()));
        // A researcher's profile script must not be able to slow or break a courtesy.
        assert!(argv.contains(&"-NoProfile".to_string()));
        assert!(argv.contains(&"Hidden".to_string()));

        let script = argv.last().expect("the script");
        assert!(script.contains("ToastNotificationManager"));
        assert!(script.contains("ToastText02"));
        assert!(script.contains("Discovery finished"));
        assert!(script.contains("Bactericera synthetic monitoring"));
        // An AppId that already exists, so this works with no installer and no Start Menu entry.
        assert!(script.contains("powershell.exe"));
    }

    /// A run name is the researcher's own text and reaches two parsers on the way to the screen.
    #[test]
    fn a_run_name_with_quotes_and_ampersands_cannot_break_the_script() {
        let argv = toast_command("Done", r#"Bactericera & "spp." <trial> it's"#);
        let script = argv.last().expect("the script");

        // XML entities, so the toast template still parses.
        assert!(script.contains("&amp;"), "{script}");
        assert!(script.contains("&quot;"), "{script}");
        assert!(script.contains("&lt;trial&gt;"), "{script}");
        // And no bare apostrophe survives to end the PowerShell literal early: the XML pass turns
        // it into `&apos;` and the PowerShell pass leaves that alone.
        assert!(!script.contains("it's"), "{script}");
        assert!(script.contains("it&apos;s"), "{script}");
        // Nothing that could start a new statement.
        assert!(!script.contains("';"), "{script}");
    }

    #[test]
    fn a_long_name_is_cut_on_a_word_boundary() {
        let long = "Bactericera synthetic monitoring across every covariate in the trial series \
                    including the held-out partition and its diagnostics";
        let argv = toast_command("Discovery finished", long);
        let script = argv.last().expect("the script");
        assert!(script.contains('…'), "{script}");
        // Cut between words, not through one.
        assert!(!script.contains(" …"), "{script}");
        // Short enough to say in full is said in full.
        assert!(toast_command("a", "short")[6].contains("short"));
        assert!(!toast_command("a", "short")[6].contains('…'));
    }

    /// A toast for something already on screen is noise.
    #[test]
    fn nothing_interrupts_somebody_who_is_already_looking() {
        assert!(!worth_interrupting(true));
        assert!(worth_interrupting(false));
    }

    /// Newlines would end the PowerShell statement; the clip collapses them first.
    #[test]
    fn a_name_with_newlines_stays_one_line() {
        let argv = toast_command("Done", "first line\nsecond line\r\nthird");
        let script = argv.last().expect("the script");
        assert!(!script.contains('\n'), "the body must not carry a line break");
        assert!(script.contains("first line second line third"), "{script}");
    }
}
