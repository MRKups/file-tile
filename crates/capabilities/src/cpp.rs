//! C / C++ build system (`cmake`, `meson`, `vcpkg`, `conan`) detection.

use crate::scan::DirSnapshot;
use crate::{CapabilityItem, CapabilityKind, scan};

pub fn detect(ancestors: &[DirSnapshot]) -> Option<CapabilityItem> {
    for dir in ancestors {
        if let Some(item) = inspect_directory(dir) {
            return Some(item);
        }
    }
    None
}

fn inspect_directory(dir: &DirSnapshot) -> Option<CapabilityItem> {
    let has_cmake = dir.has_file("CMakeLists.txt") || dir.has_file("CMakePresets.json");
    let has_meson = dir.has_file("meson.build");
    let has_vcpkg = dir.has_file("vcpkg.json");
    let has_conan = dir.has_file("conanfile.txt") || dir.has_file("conanfile.py");
    let has_compile_db = dir.has_file("compile_commands.json");

    if !has_cmake && !has_meson && !has_vcpkg && !has_conan && !has_compile_db {
        return None;
    }

    let primary = if has_cmake {
        "cmake"
    } else if has_meson {
        "meson"
    } else if has_vcpkg {
        "vcpkg"
    } else if has_conan {
        "conan"
    } else {
        "clangd"
    };

    let pkg_mgr = if has_vcpkg && primary != "vcpkg" {
        Some("vcpkg")
    } else if has_conan && primary != "conan" {
        Some("conan")
    } else {
        None
    };

    let value = match pkg_mgr {
        Some(mgr) => Some(format!("{primary} ({mgr})")),
        None => Some(primary.to_owned()),
    };

    let mut details = Vec::new();
    if has_cmake {
        if let Some(ver) = read_cmake_version(dir) {
            details.push(format!("CMake {ver}"));
        } else {
            details.push("CMake".to_owned());
        }
    }
    if has_meson {
        details.push("Meson".to_owned());
    }
    if has_vcpkg {
        details.push("vcpkg".to_owned());
    }
    if has_conan {
        details.push("Conan".to_owned());
    }
    if has_compile_db && details.is_empty() {
        details.push("compile_commands.json".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "cpp",
        value,
        format!("C/C++: {}", details.join(" · ")),
    ))
}

fn read_cmake_version(dir: &DirSnapshot) -> Option<String> {
    let cmake = dir.read_bounded("CMakeLists.txt")?;
    for line in cmake.lines() {
        let lower = line.trim().to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("cmake_minimum_required")
            && let Some(idx) = rest.find("version")
        {
            let after = &rest[idx + "version".len()..];
            let cleaned = after
                .trim_start_matches([' ', '(', ':'])
                .trim_end_matches(')');
            if let Some(ver) = scan::clean_version(cleaned) {
                return Some(ver);
            }
        }
    }
    None
}
