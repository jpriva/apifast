//! Files only their owner can open, on Windows: what mode `0600` is on Unix.
//!
//! A file on Windows takes the access list of its folder: in the user's
//! profile that is the user, the system and the administrators, and anywhere
//! else it may be everyone. Here the list is cut down to one entry, full
//! access for the user apifast runs as.
//!
//! The workspace forbids `unsafe`, which calling the Windows API needs, so
//! this runs `icacls.exe` and `whoami.exe`, both part of Windows, by their
//! full path in the system folder. Each run takes some 50 ms, and closing
//! a file takes two.

use std::io;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

/// Without it, each tool would open a console window for a moment: the
/// interface is a windowed program.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A tool of Windows, from the system folder: never one of the same name
/// found in the `PATH` or next to apifast.
fn system_tool(name: &str) -> io::Result<Command> {
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::other("the SystemRoot variable is not set"))?;
    let mut command = Command::new(PathBuf::from(root).join("System32").join(name));
    command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null());
    Ok(command)
}

/// What a tool said, when it failed. It names files and accounts, never
/// what the files hold.
fn failure(tool: &str, output: &Output) -> io::Error {
    let said = [&output.stderr, &output.stdout]
        .map(|bytes| String::from_utf8_lossy(bytes).trim().to_owned())
        .into_iter()
        .find(|text| !text.is_empty())
        .unwrap_or_default();
    io::Error::other(format!("{tool} failed ({}): {said}", output.status))
}

/// Whether `text` is written as a security identifier, `S-1-5-21-…`.
fn is_sid(text: &str) -> bool {
    text.strip_prefix("S-1-").is_some_and(|rest| {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '-')
    })
}

/// The identifier in what `whoami /user /fo csv /nh` prints:
/// `"domain\name","S-1-5-21-…"`.
fn parse_sid(text: &str) -> Option<&str> {
    text.trim().rsplit('"').find(|field| is_sid(field))
}

/// The security identifier of the user apifast runs as. Asked once: it does
/// not change while the program runs.
fn user_sid() -> io::Result<&'static str> {
    static SID: OnceLock<Result<String, String>> = OnceLock::new();
    SID.get_or_init(|| {
        let output = system_tool("whoami.exe")
            .and_then(|mut whoami| whoami.args(["/user", "/fo", "csv", "/nh"]).output())
            .map_err(|e| format!("whoami: {e}"))?;
        if !output.status.success() {
            return Err(failure("whoami", &output).to_string());
        }
        parse_sid(&String::from_utf8_lossy(&output.stdout))
            .map(str::to_owned)
            .ok_or_else(|| "whoami did not say who the user is".to_owned())
    })
    .as_deref()
    .map_err(|e| io::Error::other(e.clone()))
}

fn icacls(path: &Path, arguments: &[&str]) -> io::Result<()> {
    let output = system_tool("icacls.exe")?
        .arg(path)
        .args(arguments)
        .arg("/q")
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure("icacls", &output))
    }
}

/// Leaves `path` open only to its owner, whatever it was given: one entry,
/// full access for the user, nothing inherited.
///
/// It first goes back to what the folder passes down, which drops every
/// entry of the file's own. That matters for a new file too: where the
/// folder passes nothing down, Windows gives it the entries of the user's
/// token (the system, the administrators, the user), and they are the
/// file's own, not inherited. Then inheritance is cut and the user granted.
///
/// # Errors
///
/// If the tools cannot run, or the file is not there or is where Windows
/// keeps no access lists (a FAT drive, some network shares).
pub(crate) fn close(path: &Path) -> io::Result<()> {
    let grant = format!("*{}:F", user_sid()?);
    icacls(path, &["/reset"])?;
    icacls(path, &["/inheritance:r", "/grant:r", &grant])
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::keys::owner_only;
    use crate::{KeyBackend as _, KeyFile};

    /// The file's access list as `icacls` prints it, one entry per line,
    /// each as `account:(rights)`; inherited entries have `(I)`.
    fn entries(path: &Path) -> String {
        let output = system_tool("icacls.exe")
            .unwrap()
            .arg(path)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn assert_only_its_owner(path: &Path) {
        let list = entries(path);
        assert_eq!(list.matches(":(").count(), 1, "one entry: {list}");
        assert!(list.contains(":(F)"), "full access, not inherited: {list}");
    }

    #[test]
    fn the_identifier_is_read_from_what_whoami_prints() {
        assert_eq!(
            parse_sid("\"desk\\ana\",\"S-1-5-21-1004336348-1177238915-682003330-1001\"\r\n"),
            Some("S-1-5-21-1004336348-1177238915-682003330-1001")
        );
        // A name with a comma or a quote does not move it.
        assert_eq!(
            parse_sid("\"desk\\a,\"S-1-x\",b\",\"S-1-5-18\"\r\n"),
            Some("S-1-5-18")
        );
        assert_eq!(parse_sid("\"desk\\ana\",\"\"\r\n"), None);
        assert_eq!(parse_sid("ERROR: access denied"), None);
    }

    #[test]
    fn only_a_whole_identifier_is_one() {
        assert!(is_sid("S-1-5-18"));
        assert!(!is_sid("S-1-"));
        assert!(!is_sid("S-1-5-18:F /grant Everyone"));
        assert!(!is_sid("ana"));
    }

    #[test]
    fn a_new_file_is_open_only_to_its_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret file.key");
        let mut file = owner_only(&path).unwrap();
        file.write_all(b"secret").unwrap();
        drop(file);
        assert_only_its_owner(&path);
        // Its owner still reads and replaces it.
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        std::fs::write(&path, b"other").unwrap();
    }

    #[test]
    fn a_leftover_others_could_read_is_not_written_into() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.key.partial");
        std::fs::write(&path, b"old").unwrap();
        // Everyone, by its well-known identifier.
        icacls(&path, &["/grant", "*S-1-1-0:R"]).unwrap();
        drop(owner_only(&path).unwrap());
        assert_only_its_owner(&path);
    }

    #[test]
    fn a_key_file_others_can_read_is_closed_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = KeyFile::in_dir(dir.path());
        std::fs::write(file.path(), [1; 32]).unwrap();
        icacls(file.path(), &["/grant", "*S-1-1-0:R"]).unwrap();
        assert_eq!(file.get().unwrap(), Some(vec![1; 32]));
        assert_only_its_owner(file.path());
    }

    #[test]
    fn a_file_that_is_not_there_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(close(&dir.path().join("missing.key")).is_err());
    }
}
