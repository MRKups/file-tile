//! Java / JVM version and build tool (`maven`, `gradle`, `sbt`) detection.

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
    let tool = detect_build_tool(dir);
    let version = read_java_version(dir);

    if tool.is_none() && version.is_none() {
        return None;
    }

    let short_ver = version.as_deref().map(major_or_short);
    let value = match (short_ver.as_deref(), tool) {
        (Some(ver), Some(t)) => Some(format!("{ver} ({t})")),
        (Some(ver), None) => Some(ver.to_owned()),
        (None, Some(t)) => Some(t.to_owned()),
        (None, None) => None,
    };

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("JDK {ver}"));
    }
    if let Some(t) = tool {
        details.push(t.to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "jvm",
        value,
        format!("JVM: {}", details.join(" · ")),
    ))
}

fn detect_build_tool(dir: &DirSnapshot) -> Option<&'static str> {
    if dir.has_file("build.gradle.kts")
        || dir.has_file("build.gradle")
        || dir.has_file("settings.gradle.kts")
        || dir.has_file("settings.gradle")
    {
        Some("gradle")
    } else if dir.has_file("pom.xml") {
        Some("maven")
    } else if dir.has_file("build.sbt") {
        Some("sbt")
    } else {
        None
    }
}

fn read_java_version(dir: &DirSnapshot) -> Option<String> {
    if let Some(line) = dir.read_first_line(".java-version")
        && let Some(ver) = scan::clean_version(&line)
    {
        return Some(ver);
    }
    if let Some(sdkman) = dir.read_bounded(".sdkmanrc") {
        for line in sdkman.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("java=")
                && let Some(ver) = scan::clean_version(rest)
            {
                return Some(ver);
            }
        }
    }
    None
}

fn major_or_short(version: &str) -> String {
    let mut parts = version.split('.');
    let Some(first) = parts.next() else {
        return version.to_owned();
    };
    if first == "1"
        && let Some(second) = parts.next()
    {
        return format!("1.{second}");
    }
    if first.chars().all(|c| c.is_ascii_digit()) {
        first.to_owned()
    } else {
        version.to_owned()
    }
}
