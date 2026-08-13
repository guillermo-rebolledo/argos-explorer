#[cfg(not(windows))]
use std::env;
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    Native,
    CommandScript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VscodeCli {
    path: PathBuf,
    kind: CommandKind,
}

impl VscodeCli {
    pub fn detect() -> Option<Self> {
        #[cfg(windows)]
        {
            let output = Command::new("where.exe")
                .arg("code")
                .stdin(Stdio::null())
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            select_windows_command(&String::from_utf8_lossy(&output.stdout))
        }

        #[cfg(not(windows))]
        {
            env::var_os("PATH")
                .into_iter()
                .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
                .map(|directory| directory.join("code"))
                .find(|candidate| candidate.is_file())
                .map(|path| Self {
                    path,
                    kind: CommandKind::Native,
                })
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open_workspace(&self, workspace_root: &Path) -> io::Result<Child> {
        match self.kind {
            CommandKind::Native => {
                let mut command = Command::new(&self.path);
                command
                    .arg("--new-window")
                    .arg(workspace_root)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                hide_console_window(&mut command);
                command.spawn()
            }
            CommandKind::CommandScript => {
                let mut command = Command::new("cmd.exe");
                command
                    .args(["/d", "/c", "call"])
                    .arg(&self.path)
                    .arg("--new-window")
                    .arg(workspace_root)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                hide_console_window(&mut command);
                command.spawn()
            }
        }
    }
}

#[cfg(windows)]
fn select_windows_command(output: &str) -> Option<VscodeCli> {
    let mut candidates: Vec<_> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect();
    candidates.sort_by_key(|path| match path.extension().and_then(OsStr::to_str) {
        Some(extension) if extension.eq_ignore_ascii_case("exe") => 0,
        Some(extension)
            if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") =>
        {
            1
        }
        _ => 2,
    });
    candidates.into_iter().find_map(|path| {
        let extension = path.extension().and_then(OsStr::to_str)?;
        let kind = if extension.eq_ignore_ascii_case("exe") {
            CommandKind::Native
        } else if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
            CommandKind::CommandScript
        } else {
            return None;
        };
        Some(VscodeCli { path, kind })
    })
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn chooses_code_cmd_over_extensionless_shell_script() {
        let temp = tempfile::tempdir().unwrap();
        let shell = temp.path().join("code");
        let command = temp.path().join("code.cmd");
        std::fs::write(&shell, "shell").unwrap();
        std::fs::write(&command, "batch").unwrap();
        let output = format!("{}\r\n{}\r\n", shell.display(), command.display());

        let detected = select_windows_command(&output).unwrap();

        assert_eq!(detected.path, command);
        assert_eq!(detected.kind, CommandKind::CommandScript);
    }

    #[cfg(windows)]
    #[test]
    fn launches_code_command_with_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let command = temp.path().join("code.cmd");
        let arguments = temp.path().join("args.txt");
        let workspace = temp.path().join("workspace with spaces");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(&command, "@echo off\r\necho %* > \"%~dp0args.txt\"\r\n").unwrap();
        let cli = VscodeCli {
            path: command,
            kind: CommandKind::CommandScript,
        };

        let status = cli.open_workspace(&workspace).unwrap().wait().unwrap();
        let captured = std::fs::read_to_string(arguments).unwrap();

        assert!(status.success());
        assert!(captured.contains("--new-window"));
        assert!(captured.contains(&workspace.display().to_string()));
    }
}
