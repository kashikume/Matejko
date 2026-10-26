//! The JPEG and PNG files in one folder, in case-insensitive ordinal order.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[link(name = "kernel32")]
extern "system" {
    fn CompareStringOrdinal(
        string1: *const u16,
        count1: i32,
        string2: *const u16,
        count2: i32,
        ignore_case: i32,
    ) -> i32;
}

#[derive(Debug)]
pub struct Catalog {
    dir: PathBuf,
    paths: Vec<PathBuf>,
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// The previous current path is still in the list.
    KeptCurrent,
    /// The previous current path is gone; index now points at the next/previous file.
    CurrentRemoved,
    /// No supported images remain in the folder.
    Empty,
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("Couldn't find {}.", path.display()));
        }
        if path.is_dir() {
            return Err("Matejko opens an image file. Pass a JPEG or PNG, not a folder.".into());
        }
        if !path.is_file() {
            return Err(format!("{} is not a file.", path.display()));
        }
        if !is_supported(path) {
            return Err("Matejko opens JPEG and PNG files only.".into());
        }

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let paths = scan_dir(&parent)?;
        let index = paths
            .iter()
            .position(|candidate| file_names_equal(candidate, path))
            .ok_or_else(|| format!("Couldn't find {} in its folder.", path.display()))?;
        Ok(Self {
            dir: parent,
            paths,
            index,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        self.paths.iter().any(|candidate| candidate == path)
    }

    /// Rescan the folder. Keeps the current path when it still exists; otherwise
    /// picks the next file the same way as deleting the current image in-app.
    pub fn refresh(&mut self) -> Refresh {
        let previous = if self.paths.is_empty() {
            None
        } else {
            Some(self.paths[self.index].clone())
        };
        let next_after = previous.as_ref().and_then(|_| {
            self.neighbor(1).map(Path::to_path_buf)
        });
        let prev_before = previous.as_ref().and_then(|_| {
            self.neighbor(-1).map(Path::to_path_buf)
        });

        let paths = match scan_dir(&self.dir) {
            Ok(paths) => paths,
            Err(_) => {
                self.paths.clear();
                self.index = 0;
                return Refresh::Empty;
            }
        };

        if paths.is_empty() {
            self.paths = paths;
            self.index = 0;
            return Refresh::Empty;
        }

        if let Some(current) = &previous {
            if let Some(index) = paths.iter().position(|path| path == current) {
                self.paths = paths;
                self.index = index;
                return Refresh::KeptCurrent;
            }
        }

        let index = next_after
            .and_then(|path| paths.iter().position(|candidate| candidate == &path))
            .or_else(|| {
                prev_before.and_then(|path| paths.iter().position(|candidate| candidate == &path))
            })
            .unwrap_or_else(|| previous.as_ref().map(|_| self.index.min(paths.len() - 1)).unwrap_or(0));

        self.paths = paths;
        self.index = index;
        if previous.is_some() {
            Refresh::CurrentRemoved
        } else {
            Refresh::KeptCurrent
        }
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn current_path(&self) -> PathBuf {
        self.paths[self.index].clone()
    }

    pub fn file_name(&self) -> String {
        file_name(&self.paths[self.index])
    }

    pub fn title(&self) -> String {
        format!(
            "Matejko — {} ({} / {}) — F1 for help",
            self.file_name(),
            self.index() + 1,
            self.len()
        )
    }

    pub fn neighbor(&self, delta: isize) -> Option<&Path> {
        let next = self.index as isize + delta;
        if next < 0 || next >= self.paths.len() as isize {
            None
        } else {
            Some(&self.paths[next as usize])
        }
    }

    /// Move by `delta` images. Does nothing if that would leave the list.
    pub fn move_by(&mut self, delta: isize) -> bool {
        let next = self.index as isize + delta;
        if next < 0 || next >= self.paths.len() as isize {
            return false;
        }
        self.index = next as usize;
        true
    }

    /// Move by `delta`, stopping at the first or last file instead of failing.
    pub fn move_clamped(&mut self, delta: isize) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let last = (self.paths.len() - 1) as isize;
        let next = (self.index as isize + delta).clamp(0, last) as usize;
        if next == self.index {
            return false;
        }
        self.index = next;
        true
    }

    pub fn go_to_first(&mut self) -> bool {
        if self.paths.is_empty() || self.index == 0 {
            return false;
        }
        self.index = 0;
        true
    }

    pub fn go_to_last(&mut self) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let last = self.paths.len() - 1;
        if self.index == last {
            return false;
        }
        self.index = last;
        true
    }

    /// Drop the current path. The index then points at the next file, or the
    /// new last file when the deleted one was at the end.
    pub fn remove_current(&mut self) -> PathBuf {
        let path = self.paths.remove(self.index);
        if self.index == self.paths.len() && self.index > 0 {
            self.index -= 1;
        }
        path
    }
}

pub fn is_supported(path: &Path) -> bool {
    let Some(extension) = path.extension() else {
        return false;
    };
    extension.eq_ignore_ascii_case("jpg")
        || extension.eq_ignore_ascii_case("jpeg")
        || extension.eq_ignore_ascii_case("png")
}

fn scan_dir(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        fs::read_dir(dir).map_err(|err| format!("Couldn't read {}: {err}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("Couldn't read {}: {err}", dir.display()))?;
        let candidate = entry.path();
        if !is_supported(&candidate) {
            continue;
        }
        let is_file = entry
            .metadata()
            .map(|metadata| metadata.is_file())
            .unwrap_or(false);
        if is_file {
            paths.push(candidate);
        }
    }
    paths.sort_by(|left, right| cmp_file_names(left, right));
    Ok(paths)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn cmp_file_names(left: &Path, right: &Path) -> Ordering {
    cmp_ordinal_ignore_case(
        left.file_name().unwrap_or(OsStr::new("")),
        right.file_name().unwrap_or(OsStr::new("")),
    )
}

fn file_names_equal(left: &Path, right: &Path) -> bool {
    cmp_file_names(left, right) == Ordering::Equal
}

fn cmp_ordinal_ignore_case(left: &OsStr, right: &OsStr) -> Ordering {
    let left: Vec<u16> = left.encode_wide().collect();
    let right: Vec<u16> = right.encode_wide().collect();
    let result = unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    };
    match result {
        1 => Ordering::Less,
        2 => Ordering::Equal,
        3 => Ordering::Greater,
        _ => left.cmp(&right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "matejko-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_supported_files_in_ordinal_order_and_stops_at_the_ends() {
        let dir = scratch("order");
        fs::write(dir.join("img2.JPG"), b"").unwrap();
        fs::write(dir.join("img10.jpg"), b"").unwrap();
        fs::write(dir.join("a.PNG"), b"").unwrap();
        fs::write(dir.join("B.jpeg"), b"").unwrap();
        fs::write(dir.join("notes.txt"), b"skip").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("nested.jpg"), b"").unwrap();

        let mut catalog = Catalog::open(&dir.join("img2.JPG")).unwrap();
        let names: Vec<_> = catalog
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.PNG", "B.jpeg", "img10.jpg", "img2.JPG"]);
        assert_eq!(catalog.index(), 3);
        assert_eq!(catalog.title(), "Matejko — img2.JPG (4 / 4) — F1 for help");
        assert!(!catalog.move_by(1));
        assert!(catalog.move_by(-1));
        assert_eq!(catalog.file_name(), "img10.jpg");
        assert!(catalog.move_by(-1));
        assert!(catalog.move_by(-1));
        assert!(!catalog.move_by(-1));
        assert_eq!(catalog.file_name(), "a.PNG");

        assert!(catalog.move_clamped(10));
        assert_eq!(catalog.file_name(), "img2.JPG");
        assert!(!catalog.move_clamped(10));
        assert!(catalog.go_to_first());
        assert_eq!(catalog.file_name(), "a.PNG");
        assert!(!catalog.go_to_first());
        assert!(catalog.go_to_last());
        assert_eq!(catalog.file_name(), "img2.JPG");
        assert!(!catalog.go_to_last());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_keeps_the_next_image_and_steps_back_at_the_end() {
        let mut catalog = Catalog {
            dir: PathBuf::from("."),
            paths: vec!["a.jpg".into(), "b.jpg".into(), "c.jpg".into()],
            index: 0,
        };
        catalog.remove_current();
        assert_eq!(catalog.file_name(), "b.jpg");
        catalog.remove_current();
        assert_eq!(catalog.file_name(), "c.jpg");
        catalog.remove_current();
        assert!(catalog.is_empty());
    }

    #[test]
    fn refresh_keeps_the_current_path_when_neighbors_change() {
        let dir = scratch("refresh-keep");
        fs::write(dir.join("a.jpg"), b"").unwrap();
        fs::write(dir.join("b.jpg"), b"").unwrap();
        fs::write(dir.join("c.jpg"), b"").unwrap();
        let mut catalog = Catalog::open(&dir.join("b.jpg")).unwrap();
        assert_eq!(catalog.index(), 1);

        fs::write(dir.join("aa.jpg"), b"").unwrap();
        assert_eq!(catalog.refresh(), Refresh::KeptCurrent);
        assert_eq!(catalog.file_name(), "b.jpg");
        assert_eq!(catalog.len(), 4);

        fs::remove_file(dir.join("a.jpg")).unwrap();
        assert_eq!(catalog.refresh(), Refresh::KeptCurrent);
        assert_eq!(catalog.file_name(), "b.jpg");
        assert_eq!(catalog.len(), 3);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refresh_moves_like_delete_when_the_current_file_vanishes() {
        let dir = scratch("refresh-gone");
        fs::write(dir.join("a.jpg"), b"").unwrap();
        fs::write(dir.join("b.jpg"), b"").unwrap();
        fs::write(dir.join("c.jpg"), b"").unwrap();
        let mut catalog = Catalog::open(&dir.join("b.jpg")).unwrap();

        fs::remove_file(dir.join("b.jpg")).unwrap();
        assert_eq!(catalog.refresh(), Refresh::CurrentRemoved);
        assert_eq!(catalog.file_name(), "c.jpg");

        fs::remove_file(dir.join("c.jpg")).unwrap();
        assert_eq!(catalog.refresh(), Refresh::CurrentRemoved);
        assert_eq!(catalog.file_name(), "a.jpg");

        fs::remove_file(dir.join("a.jpg")).unwrap();
        assert_eq!(catalog.refresh(), Refresh::Empty);
        assert!(catalog.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_a_folder_a_missing_path_and_other_extensions() {
        let dir = scratch("reject");
        fs::write(dir.join("notes.txt"), b"").unwrap();
        assert!(Catalog::open(&dir).unwrap_err().contains("folder"));
        assert!(Catalog::open(&dir.join("missing.jpg"))
            .unwrap_err()
            .contains("Couldn't find"));
        assert!(Catalog::open(&dir.join("notes.txt"))
            .unwrap_err()
            .contains("JPEG and PNG"));
        let _ = fs::remove_dir_all(dir);
    }
}
