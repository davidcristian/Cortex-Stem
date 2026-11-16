use super::*;

#[test]
fn barrel_is_sorted_and_deterministic() {
    let cfg = Config::from_env();
    let mut entries = exported(&cfg);
    let a = barrel(&entries);
    entries.reverse();
    let b = barrel(&entries);
    assert_eq!(a, b);
    let lines: Vec<&str> = a.lines().filter(|l| l.starts_with("export ")).collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted);
    assert_eq!(lines.len(), entries.len());
    assert!(a.contains("export type { DockState } from \"./DockState\";"));
}

/// Ids are `#[serde(transparent)]` newtypes: the frontend sees the primitive.
#[test]
fn ids_cross_ipc_as_plain_number_and_string() {
    let cfg = Config::from_env();
    assert_eq!(<WindowId as TS>::decl(&cfg), "type WindowId = number;");
    assert_eq!(<AppId as TS>::decl(&cfg), "type AppId = string;");
    assert_eq!(<IconKey as TS>::decl(&cfg), "type IconKey = string;");
    assert_eq!(<MonitorId as TS>::decl(&cfg), "type MonitorId = string;");
}

/// JSON numbers are doubles; `u64` fields are annotated `ts(type = "number")`
/// so no binding asks the frontend for a `bigint`.
#[test]
fn no_binding_uses_bigint() {
    for e in exported(&Config::from_env()) {
        assert!(
            !e.decl.contains("bigint"),
            "{} exports a bigint; add `#[cfg_attr(feature = \"ts\", ts(type = \"number\"))]`: {}",
            e.name,
            e.decl
        );
    }
}

#[test]
fn every_ipc_payload_type_is_exported_and_internal_ones_are_not() {
    let names: BTreeSet<String> = exported(&Config::from_env())
        .into_iter()
        .map(|e| e.name)
        .collect();
    for ty in IPC_PAYLOAD_TYPES {
        assert!(
            names.contains(*ty),
            "{ty} is named in docs/ipc/ but not exported"
        );
    }
    // Engine inputs never cross IPC.
    assert!(!names.contains("PlatformEvent"));
    assert!(!names.contains("EngineInput"));
}
