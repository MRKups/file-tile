//! Strict scope records and lossless boundaries. No native resolution during decode.
use super::path::{PathRecord, PathRef};
use crate::domain::{NavigationRange, Scope, ScopeKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    kind: Kind,
    root: Option<PathRecord>,
    range: Option<RangeRecord>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeRecord {
    start: PathRecord,
    stops: Vec<PathRecord>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Free,
    Pinned,
    Branch,
    Range,
}
pub(super) fn restore(value: Value) -> Result<Scope, &'static str> {
    if value.get("root").is_none() || value.get("range").is_none() {
        return Err("missing explicit scope boundaries");
    }
    let record: Record =
        serde_json::from_value(value).map_err(|_| "invalid or unsupported scope fields")?;
    let kind = match record.kind {
        Kind::Free => ScopeKind::Free,
        Kind::Pinned => ScopeKind::Pinned,
        Kind::Branch => ScopeKind::Branch,
        Kind::Range => ScopeKind::Range,
    };
    let root = record.root.map(PathRecord::into_path).transpose()?;
    let range = record
        .range
        .map(|range| {
            NavigationRange::new(
                range.start.into_path()?,
                range
                    .stops
                    .into_iter()
                    .map(PathRecord::into_path)
                    .collect::<Result<Vec<_>, _>>()?,
            )
        })
        .transpose()?;
    Scope::restore(kind, root, range)
}

pub(super) struct ScopeRef<'a>(pub &'a Scope);
impl Serialize for ScopeRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Range<'a> {
            start: PathRef<'a>,
            stops: Vec<PathRef<'a>>,
        }
        #[derive(Serialize)]
        struct Record<'a> {
            kind: Kind,
            root: Option<PathRef<'a>>,
            range: Option<Range<'a>>,
        }
        Record {
            kind: match self.0.kind() {
                ScopeKind::Free => Kind::Free,
                ScopeKind::Pinned => Kind::Pinned,
                ScopeKind::Branch => Kind::Branch,
                ScopeKind::Range => Kind::Range,
            },
            root: self.0.root().map(PathRef),
            range: self.0.range().map(|range| Range {
                start: PathRef(range.start()),
                stops: range.stops().iter().map(|path| PathRef(path)).collect(),
            }),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::contracts::configuration::{ConfigurationDocument, WorkspaceId};
    use crate::persistence::{ConfigKey, decode, encode};
    fn id() -> Result<WorkspaceId, crate::application::contracts::configuration::InvalidWorkspaceId>
    {
        WorkspaceId::parse("11111111-1111-4111-8111-111111111111")
    }
    fn example() -> Result<Value, serde_json::Error> {
        serde_json::from_str(include_str!(
            "../../../../schemas/examples/workspace-11111111-1111-4111-8111-111111111111.json"
        ))
    }
    #[test]
    fn all_scopes_and_retained_ranges_round_trip_without_disk_or_broadening()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = |value| serde_json::json!({"encoding": "utf8", "value": value});
        let range =
            serde_json::json!({"start": path("/fixture"), "stops": [path("/fixture/Projects")]});
        for (kind, root, range) in [
            ("free", Value::Null, Value::Null),
            ("pinned", path("/fixture/Projects"), Value::Null),
            ("branch", path("/fixture"), range.clone()),
            ("range", Value::Null, range),
        ] {
            let mut value = example()?;
            if kind == "pinned" {
                value["boxes"][0]["standalone_view"]["history"]["back"] = serde_json::json!([]);
                value["boxes"][0]["standalone_view"]["starting_folder"] = path("/fixture/Projects");
            }
            value["boxes"][0]["standalone_view"]["scope"] =
                serde_json::json!({"kind": kind, "root": root, "range": range});
            let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::Workspace(id()?))?;
            assert!(decoded.can_save());
            assert_eq!(
                serde_json::from_slice::<Value>(&encode(&decoded.document)?)?,
                value
            );
        }
        Ok(())
    }
    #[test]
    fn malformed_restrictions_remain_read_only_and_cannot_turn_into_free()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = |value| serde_json::json!({"encoding": "utf8", "value": value});
        let baseline = example()?;
        let decoded = decode(&serde_json::to_vec(&baseline)?, ConfigKey::Workspace(id()?))?;
        assert!(decoded.can_save());
        assert_eq!(
            serde_json::from_slice::<Value>(&encode(&decoded.document)?)?,
            baseline
        );
        for scope in [
            serde_json::json!({"kind": "pinned", "root": null, "range": null}),
            serde_json::json!({"kind": "branch", "root": null, "range": null}),
            serde_json::json!({"kind": "range", "root": null, "range": null}),
            serde_json::json!({"kind": "free", "root": path("/fixture"), "range": null}),
            serde_json::json!({"kind": "range", "root": null, "range": {"start": path("/fixture"), "stops": []}}),
            serde_json::json!({"kind": "range", "root": null, "range": {"start": path("/fixture"), "stops": [path("/Outside")]}}),
            serde_json::json!({"kind": "branch", "root": path("/Outside"), "range": null}),
            serde_json::json!({"kind": "branch", "root": path("/fixture/../Outside"), "range": null}),
        ] {
            let mut value = baseline.clone();
            value["boxes"][0]["standalone_view"]["scope"] = scope;
            let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::Workspace(id()?))?;
            assert!(!decoded.can_save());
            let ConfigurationDocument::Workspace(document) = decoded.document else {
                return Err("Missing viewing workspace".into());
            };
            assert!(
                document.workspace.boxes().is_empty(),
                "Invalid restrictive box must not become Free"
            );
        }
        Ok(())
    }
}
