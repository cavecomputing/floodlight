//! Puts text on the clipboard through the OS's own tool, so nothing links against a display
//! server: pbcopy on macOS, clip on Windows, wl-copy then xclip then xsel on Linux.

use std::io::Write;
use std::process::{Command, Stdio};

/// The first tool that takes the text wins. One that is installed but can't (wl-copy under X11,
/// say) is passed over for the next; its complaint is the error when none works.
pub fn copy(text: &str) -> Result<(), String> {
    let tools: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    let mut failed = None;
    for (program, args) in tools {
        let Ok(mut child) = Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let written = match child.stdin.take() {
            Some(mut stdin) => stdin.write_all(text.as_bytes()),
            None => Ok(()),
        };
        failed = Some(match written.and_then(|()| child.wait()) {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => format!("{program} exited with {status}"),
            Err(error) => format!("{program}: {error}"),
        });
    }
    Err(failed
        .unwrap_or_else(|| "no clipboard tool found (wl-clipboard, xclip or xsel)".to_string()))
}
