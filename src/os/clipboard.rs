//! Puts text on the clipboard through the OS's own tool, so nothing links against a display
//! server: pbcopy on macOS, clip on Windows, wl-copy then xclip then xsel on Linux.

use std::io::Write;
use std::process::{Command, Stdio};

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
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|error| format!("{program}: {error}"))?;
        }
        return match child.wait() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("{program} exited with {status}")),
            Err(error) => Err(format!("{program}: {error}")),
        };
    }
    Err("no clipboard tool found (wl-clipboard, xclip or xsel)".to_string())
}
