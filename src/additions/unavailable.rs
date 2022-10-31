use sysinfo::{self, SystemExt};

pub const CHARACTERS_WINDOWS: &[&str] = &["<", ">", ":", "\"", "/", "\\", "|", "?", "*"];
pub const CHARACTERS_LINUX: &[&str] = &["/"];
pub const CHARACTERS_MACOS: &[&str] = &[":", "/"];

pub const FILENAMES_WINDOWS: &[&str] = &["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];

pub fn unavailable_characters_for_system() -> &'static [&'static str] {
    let syst_name = sysinfo::System::default().name().unwrap().to_lowercase();
    if syst_name == "windows" {
        CHARACTERS_WINDOWS
    }
    else if syst_name == "macos" {
        CHARACTERS_MACOS
    }
    else {
        CHARACTERS_LINUX
    }
}

// Return "true" when inside are unavailable characters for System File System
pub fn os_file_system_check_unavailable_characters_into(to_check: &str) -> bool {
    let forebidden_characters = self::unavailable_characters_for_system();

    let mut result: bool = false;
    
    for character in forebidden_characters {
        if to_check.contains(character) {
            result = true;
            break;
        }
    };

    result
}
