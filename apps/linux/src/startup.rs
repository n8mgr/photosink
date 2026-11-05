use gtk::glib;
use std::io;
use std::path::Path;

const GROUP: &str = "Desktop Entry";

fn entry(executable: &Path) -> glib::KeyFile {
    let file = glib::KeyFile::new();
    let mut command = String::from("\"");
    for c in executable.to_string_lossy().chars() {
        match c {
            '\\' | '"' | '`' | '$' => {
                command.push('\\');
                command.push(c);
            }
            '%' => command.push_str("%%"),
            _ => command.push(c),
        }
    }
    command.push('"');
    file.set_string(GROUP, "Type", "Application");
    file.set_string(GROUP, "Name", "Photosink");
    file.set_string(GROUP, "Exec", &command);
    file.set_string(GROUP, "Icon", "com.n8mgr.photosink");
    file.set_boolean(GROUP, "Terminal", false);
    file
}

pub fn enabled() -> Result<bool, glib::Error> {
    let path = glib::user_config_dir().join("autostart/com.n8mgr.photosink.desktop");
    if !path.exists() {
        return Ok(false);
    }
    let file = glib::KeyFile::new();
    file.load_from_file(path, glib::KeyFileFlags::NONE)?;
    Ok(!file.boolean(GROUP, "Hidden").unwrap_or(false))
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let path = glib::user_config_dir().join("autostart/com.n8mgr.photosink.desktop");
    if enabled {
        let file = entry(&std::env::current_exe()?);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, file.to_data().as_bytes())
    } else {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_executable_without_expanding_field_codes() {
        let file = entry(Path::new("/opt/My App/$photosink%f\"\\"));
        let copy = glib::KeyFile::new();
        copy.load_from_data(&file.to_data(), glib::KeyFileFlags::NONE)
            .unwrap();
        assert_eq!(
            copy.string(GROUP, "Exec").unwrap(),
            "\"/opt/My App/\\$photosink%%f\\\"\\\\\""
        );
        assert!(!copy.boolean(GROUP, "Terminal").unwrap());
    }
}
