#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

#[cfg(not(windows))]
compile_error!("The argos-explorer installer must be compiled for Windows");

use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self, Write},
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

const ARGOS_EXPLORER_EXE: &[u8] = include_bytes!(env!("ARGOS_EXPLORER_INSTALLER_BINARY"));
const CONFIG_EXAMPLE: &[u8] = include_bytes!(env!("ARGOS_EXPLORER_INSTALLER_CONFIG"));
const THIRD_PARTY_NOTICES: &[u8] = include_bytes!(env!("ARGOS_EXPLORER_INSTALLER_NOTICES"));
const INSTALL_INSTRUCTIONS: &[u8] = include_bytes!(env!("ARGOS_EXPLORER_INSTALLER_INSTRUCTIONS"));
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const INSTALL_DIRECTORY_NAME: &str = "argos-explorer";

#[derive(Debug)]
struct Options {
    uninstall: bool,
    quiet: bool,
    update_path: bool,
    install_dir: PathBuf,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().collect();
    if arguments.iter().skip(1).any(|argument| {
        matches!(
            argument.to_ascii_lowercase().as_str(),
            "--help" | "-h" | "/?"
        )
    }) {
        print_help();
        return ExitCode::SUCCESS;
    }
    let quiet_requested = arguments
        .iter()
        .any(|argument| argument.eq_ignore_ascii_case("--quiet"));
    match parse_options().and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("argos-explorer Setup: {error}");
            if !quiet_requested {
                wait_for_enter();
            }
            ExitCode::FAILURE
        }
    }
}

fn parse_options() -> Result<Options, String> {
    let local_app_data = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is unavailable".to_owned())?;
    let invoked_as_uninstaller = env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy().to_lowercase())
        })
        .is_some_and(|name| name.contains("uninstall"));
    let mut options = Options {
        uninstall: invoked_as_uninstaller,
        quiet: false,
        update_path: true,
        install_dir: local_app_data.join("Programs").join(INSTALL_DIRECTORY_NAME),
    };

    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_string_lossy().to_ascii_lowercase().as_str() {
            "--uninstall" => options.uninstall = true,
            "--quiet" => options.quiet = true,
            "--no-path" => options.update_path = false,
            "--install-dir" => {
                options.install_dir = arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--install-dir requires a directory".to_owned())?;
            }
            "--help" | "-h" | "/?" => unreachable!("help is handled before parsing"),
            unknown => return Err(format!("unknown option: {unknown}")),
        }
    }
    Ok(options)
}

fn run(options: Options) -> Result<(), String> {
    if options.uninstall {
        uninstall(&options)?;
    } else {
        install(&options)?;
    }
    if !options.quiet {
        wait_for_enter();
    }
    Ok(())
}

fn install(options: &Options) -> Result<(), String> {
    println!(
        "Installing argos-explorer to {}",
        options.install_dir.display()
    );
    fs::create_dir_all(&options.install_dir).map_err(|error| {
        format!(
            "could not create {}: {error}",
            options.install_dir.display()
        )
    })?;

    write_atomically(
        &options.install_dir.join("argos-explorer.exe"),
        ARGOS_EXPLORER_EXE,
    )?;
    write_atomically(
        &options.install_dir.join("config.example.toml"),
        CONFIG_EXAMPLE,
    )?;
    write_atomically(
        &options.install_dir.join("THIRD-PARTY-NOTICES.txt"),
        THIRD_PARTY_NOTICES,
    )?;
    write_atomically(
        &options.install_dir.join("INSTALL-WINDOWS.txt"),
        INSTALL_INSTRUCTIONS,
    )?;

    let current_executable = env::current_exe()
        .map_err(|error| format!("could not locate the setup executable: {error}"))?;
    let uninstaller = options.install_dir.join("argos-explorer-uninstall.exe");
    if !same_path(&current_executable, &uninstaller) {
        fs::copy(&current_executable, &uninstaller)
            .map_err(|error| format!("could not create {}: {error}", uninstaller.display()))?;
    }

    if options.update_path {
        add_to_user_path(&options.install_dir)?;
    }

    let installed = options.install_dir.join("argos-explorer.exe");
    let version = Command::new(&installed)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not start installed argos-explorer: {error}"))?;
    if !version.status.success() {
        return Err("the installed executable failed its version check".to_owned());
    }

    let version = String::from_utf8_lossy(&version.stdout);
    println!("Installed {}", version.trim());
    if git_is_available() {
        println!("Git for Windows detected; Changes View is available.");
    } else {
        println!(
            "Git was not found. File browsing works; install Git for Windows to enable Changes View."
        );
    }
    if options.update_path {
        println!("argos-explorer was added to your user PATH.");
    }
    println!("Open a new terminal and run: argos-explorer C:\\path\\to\\workspace");
    println!("Uninstall with: {} --uninstall", uninstaller.display());
    Ok(())
}

fn uninstall(options: &Options) -> Result<(), String> {
    println!(
        "Uninstalling argos-explorer from {}",
        options.install_dir.display()
    );
    if options.update_path {
        remove_from_user_path(&options.install_dir)?;
    }
    if !options.install_dir.exists() {
        println!("argos-explorer is already uninstalled.");
        return Ok(());
    }

    let current_executable =
        env::current_exe().map_err(|error| format!("could not locate the uninstaller: {error}"))?;
    if current_executable
        .parent()
        .is_some_and(|parent| same_path(parent, &options.install_dir))
    {
        for file in [
            "argos-explorer.exe",
            "config.example.toml",
            "THIRD-PARTY-NOTICES.txt",
            "INSTALL-WINDOWS.txt",
        ] {
            let path = options.install_dir.join(file);
            if path.exists() {
                fs::remove_file(&path)
                    .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
            }
        }
        schedule_self_removal(&options.install_dir)?;
    } else {
        fs::remove_dir_all(&options.install_dir).map_err(|error| {
            format!(
                "could not remove {}: {error}",
                options.install_dir.display()
            )
        })?;
    }
    println!("argos-explorer was uninstalled.");
    Ok(())
}

fn write_atomically(path: &Path, content: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("new");
    fs::write(&temporary, content)
        .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| format!("could not install {}: {error}", path.display()))
}

fn add_to_user_path(install_dir: &Path) -> Result<(), String> {
    let existing = read_user_path()?;
    if path_contains(&existing, install_dir) {
        return Ok(());
    }
    let install = install_dir.to_string_lossy();
    let updated = if existing.trim().is_empty() {
        install.into_owned()
    } else {
        format!("{};{}", existing.trim_end_matches(';'), install)
    };
    write_user_path(&updated)
}

fn remove_from_user_path(install_dir: &Path) -> Result<(), String> {
    let existing = read_user_path()?;
    let updated = existing
        .split(';')
        .filter(|entry| !entry.trim().is_empty())
        .filter(|entry| !same_path(Path::new(entry.trim()), install_dir))
        .collect::<Vec<_>>()
        .join(";");
    if updated != existing.trim_matches(';') {
        write_user_path(&updated)?;
    }
    Ok(())
}

fn path_contains(path_value: &str, install_dir: &Path) -> bool {
    path_value
        .split(';')
        .any(|entry| same_path(Path::new(entry.trim()), install_dir))
}

fn read_user_path() -> Result<String, String> {
    let output = Command::new("reg.exe")
        .args(["query", "HKCU\\Environment", "/v", "Path"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("could not query the user PATH: {error}"))?;
    if !output.status.success() {
        return Ok(String::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        for value_type in ["REG_EXPAND_SZ", "REG_SZ"] {
            if let Some((_, value)) = line.split_once(value_type) {
                return Ok(value.trim().to_owned());
            }
        }
    }
    Ok(String::new())
}

fn write_user_path(value: &str) -> Result<(), String> {
    let status = Command::new("reg.exe")
        .args([
            "add",
            "HKCU\\Environment",
            "/v",
            "Path",
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            value,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("could not update the user PATH: {error}"))?;
    if !status.success() {
        return Err("reg.exe could not update the user PATH".to_owned());
    }
    broadcast_environment_change();
    Ok(())
}

fn git_is_available() -> bool {
    Command::new("git")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|status| status.success())
}

fn schedule_self_removal(install_dir: &Path) -> Result<(), String> {
    let cleanup = env::temp_dir().join(format!(
        "argos-explorer-uninstall-cleanup-{}.cmd",
        std::process::id()
    ));
    let script = format!(
        "@echo off\r\nset attempts=0\r\n:retry\r\nset /a attempts+=1 >nul\r\nrmdir /s /q \"{0}\"\r\nif not exist \"{0}\" goto done\r\nif %attempts% GEQ 30 goto done\r\nping 127.0.0.1 -n 2 >nul\r\ngoto retry\r\n:done\r\ndel \"%~f0\"\r\n",
        install_dir.display()
    );
    fs::write(&cleanup, script)
        .map_err(|error| format!("could not create cleanup script: {error}"))?;
    Command::new("cmd.exe")
        .args(["/d", "/c"])
        .arg(&cleanup)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("could not schedule uninstaller cleanup: {error}"))?;
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn wait_for_enter() {
    print!("Press Enter to close argos-explorer Setup...");
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

fn print_help() {
    println!(
        "argos-explorer Setup\n\n  argos-explorer-setup.exe [--quiet] [--no-path] [--install-dir DIRECTORY]\n  argos-explorer-setup.exe --uninstall [--quiet] [--no-path] [--install-dir DIRECTORY]"
    );
}

fn broadcast_environment_change() {
    const HWND_BROADCAST: isize = 0xffff;
    const WM_SETTINGCHANGE: u32 = 0x001a;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    let environment: Vec<u16> = OsStr::new("Environment")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut result = 0_usize;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            environment.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            2_000,
            &mut result,
        );
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SendMessageTimeoutW(
        window: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
}
