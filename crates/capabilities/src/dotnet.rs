//! .NET SDK (`global.json`), solution (`*.sln`, `*.slnx`), and project (`*.csproj`, `*.fsproj`) detection.

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
    let has_global = dir.has_file("global.json");
    let has_sln = dir.has_file_ext("sln") || dir.has_file_ext("slnx");
    let proj_file = dir
        .first_file_with_ext("csproj")
        .or_else(|| dir.first_file_with_ext("fsproj"));

    if !has_global && !has_sln && proj_file.is_none() {
        return None;
    }

    let tfm = proj_file.and_then(|file| read_target_framework(dir, file));
    let sdk = has_global.then(|| read_global_sdk(dir)).flatten();

    let value = tfm
        .clone()
        .or_else(|| sdk.as_deref().map(scan::short_major_minor))
        .or_else(|| has_sln.then(|| "sln".to_owned()));

    let mut details = Vec::new();
    if let Some(t) = tfm {
        details.push(t);
    }
    if let Some(s) = sdk {
        details.push(format!("SDK {s}"));
    }
    if has_sln {
        details.push("solution".to_owned());
    } else if let Some(proj) = proj_file {
        details.push(proj.to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "dotnet",
        value,
        format!(".NET: {}", details.join(" · ")),
    ))
}

fn read_target_framework(dir: &DirSnapshot, file: &str) -> Option<String> {
    let xml = dir.read_bounded(file)?;
    for tag in ["<TargetFramework>", "<TargetFrameworks>"] {
        if let Some(start) = xml.find(tag) {
            let after = &xml[start + tag.len()..];
            if let Some(end) = after.find('<') {
                let first = after[..end].split(';').next()?.trim();
                if !first.is_empty() && first.len() <= 16 {
                    return Some(first.to_owned());
                }
            }
        }
    }
    None
}

fn read_global_sdk(dir: &DirSnapshot) -> Option<String> {
    let json = dir.read_bounded("global.json")?;
    for line in json.lines() {
        let trimmed = line.trim();
        if let Some((_, rhs)) = trimmed.split_once("\"version\"")
            && let Some((_, val)) = rhs.split_once(':')
            && let Some(ver) = scan::clean_version(val)
        {
            return Some(ver);
        }
    }
    None
}
