//! Terraform and OpenTofu (`*.tf`, `.terraform-version`, `.opentofu-version`) detection.

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
    let tofu_ver = dir
        .read_first_line(".opentofu-version")
        .and_then(|line| scan::clean_version(&line));
    let tf_ver = dir
        .read_first_line(".terraform-version")
        .and_then(|line| scan::clean_version(&line));
    let has_lock = dir.has_file(".terraform.lock.hcl");
    let has_tf = dir.has_file_ext("tf");

    if tofu_ver.is_none() && tf_ver.is_none() && !has_lock && !has_tf {
        return None;
    }

    let is_tofu = tofu_ver.is_some();
    let version = tofu_ver.or(tf_ver);
    let short_ver = version.as_deref().map(scan::short_major_minor);

    let prefix = if is_tofu { "tofu" } else { "tf" };
    let label = if is_tofu { "OpenTofu" } else { "Terraform" };

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("v{ver}"));
    }
    if has_lock {
        details.push(".terraform.lock.hcl".to_owned());
    } else if has_tf {
        details.push("*.tf".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        prefix,
        short_ver,
        format!("{label}: {}", details.join(" · ")),
    ))
}
