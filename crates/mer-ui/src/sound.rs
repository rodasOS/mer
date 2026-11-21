//! Short sounds for agents that need the user. Played by an external player in a thread of its
//! own, so the interface never waits for audio.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Two sounds closer than this are one: several agents asking at once do not pile up.
const MIN_GAP: Duration = Duration::from_millis(1500);

/// Folders of the desktop's sound themes, most specific first.
const THEME_DIRS: [&str; 3] = [
    "/usr/share/sounds/ocean/stereo",
    "/usr/share/sounds/freedesktop/stereo",
    "/usr/share/sounds/gnome/default/alerts",
];
const EXTENSIONS: [&str; 3] = ["oga", "ogg", "wav"];

/// Should a sound play now, given when the last one did?
pub fn gap_has_passed(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| now.duration_since(last) >= MIN_GAP)
}

/// The audio file for a configured sound: a path as it is, or a theme sound name looked up in
/// `dirs`. `None` when there is no such file.
pub fn resolve(sound: &str, dirs: &[&Path]) -> Option<PathBuf> {
    let sound = sound.trim();
    if sound.is_empty() {
        return None;
    }
    if sound.contains('/') {
        let path = expand_home(sound);
        return path.is_file().then_some(path);
    }
    // A name never climbs out of the theme folders.
    if sound.contains("..") {
        return None;
    }
    dirs.iter()
        .flat_map(|dir| EXTENSIONS.iter().map(move |ext| dir.join(format!("{sound}.{ext}"))))
        .find(|path| path.is_file())
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// A volume in percent, as each player wants it: `pw-play` takes 0.0 to 1.0 and `paplay` a
/// linear value up to 65536. Above 100 % is cut to 100 %.
pub fn player_volume(percent: u32) -> (String, String) {
    let percent = percent.min(100);
    let pipewire = format!("{:.2}", f64::from(percent) / 100.0);
    let pulse = (u64::from(percent) * 65_536 / 100).to_string();
    (pipewire, pulse)
}

/// Plays the sound in the background at `volume` percent. Does nothing when the volume is 0,
/// when the file is missing, when no player is installed, or while KDE's "Do not disturb" is on.
pub fn play(sound: &str, volume: u32) {
    if volume == 0 {
        return;
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/sounds"));
    }
    dirs.extend(THEME_DIRS.iter().map(PathBuf::from));
    let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    let Some(file) = resolve(sound, &refs) else {
        tracing::debug!(sound, "sound not found");
        return;
    };
    std::thread::spawn(move || {
        if do_not_disturb() {
            return;
        }
        // PipeWire's player first, then PulseAudio's.
        let (pipewire, pulse) = player_volume(volume);
        for (player, level) in [("pw-play", pipewire), ("paplay", pulse)] {
            let started = Command::new(player)
                .arg(format!("--volume={level}"))
                .arg(&file)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if started.is_ok_and(|status| status.success()) {
                return;
            }
        }
        tracing::debug!("no sound player found (pw-play, paplay)");
    });
}

/// Is the desktop asking not to be disturbed? Asks the notification service; any failure
/// means "no".
fn do_not_disturb() -> bool {
    Command::new("busctl")
        .args([
            "--user",
            "get-property",
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Inhibited",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "b true")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mer-sound-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_theme_name_is_found_with_any_known_extension() {
        let dir = scratch("theme");
        std::fs::write(dir.join("ding.wav"), b"x").unwrap();
        assert_eq!(resolve("ding", &[&dir]), Some(dir.join("ding.wav")));
        assert_eq!(resolve("missing", &[&dir]), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_first_folder_with_the_sound_wins() {
        let (first, second) = (scratch("first"), scratch("second"));
        std::fs::write(first.join("a.oga"), b"x").unwrap();
        std::fs::write(second.join("a.oga"), b"x").unwrap();
        assert_eq!(resolve("a", &[&first, &second]), Some(first.join("a.oga")));
        std::fs::remove_dir_all(first).unwrap();
        std::fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn a_path_is_used_as_it_is_when_the_file_exists() {
        let dir = scratch("path");
        let file = dir.join("custom.ogg");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(resolve(file.to_str().unwrap(), &[]), Some(file));
        assert_eq!(resolve("/nonexistent/none.ogg", &[]), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_names_and_names_that_climb_out_are_refused() {
        let dir = scratch("refuse");
        assert_eq!(resolve("", &[&dir]), None);
        assert_eq!(resolve("  ", &[&dir]), None);
        assert_eq!(resolve("..", &[&dir]), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn volumes_are_converted_for_each_player() {
        assert_eq!(player_volume(100), ("1.00".to_owned(), "65536".to_owned()));
        assert_eq!(player_volume(50), ("0.50".to_owned(), "32768".to_owned()));
        assert_eq!(player_volume(10), ("0.10".to_owned(), "6553".to_owned()));
        assert_eq!(player_volume(0), ("0.00".to_owned(), "0".to_owned()));
        // Past full volume is full volume.
        assert_eq!(player_volume(900), player_volume(100));
    }

    #[test]
    fn sounds_closer_than_the_gap_are_dropped() {
        let now = Instant::now();
        assert!(gap_has_passed(None, now));
        assert!(!gap_has_passed(Some(now), now + Duration::from_millis(500)));
        assert!(gap_has_passed(Some(now), now + Duration::from_millis(1600)));
    }
}
