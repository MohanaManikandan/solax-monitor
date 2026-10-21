//! The TypeScript the screens are written against is generated from these
//! types and committed beside them. A type changed without regenerating its
//! file fails here, by name, rather than leaving the screens to drift; run
//! `cargo test -p solax-api -- --ignored write_bindings` to write them afresh.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ts_rs::TS;

use crate::*;

/// Every type the screens read, each with the types it depends on.
fn export_every_type_to(dir: &Path) {
    macro_rules! export {
        ($($type:ty),* $(,)?) => {
            $( <$type>::export_all_to(dir).expect("a binding exports"); )*
        };
    }
    export!(
        Chart, Curve, DataSlots, Day, DaySummary, Device, Fault, FaultCode, Feature, Health,
        LiveEvent, Metric, MetricValue, Now, Peak, PollWindow, PollerState, Projection, Reading,
        Registry, Roles, Section, Severity, Site, Stretches,
    );
}

fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("bindings")
}

/// Each `.ts` file in a folder, by name.
fn files_in(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_dir(dir)
        .expect("the folder exists")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&path).expect("a binding reads"))
        })
        .collect()
}

#[test]
fn the_committed_bindings_match_the_rust_types() {
    let fresh = std::env::temp_dir().join(format!("solax-bindings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fresh);
    export_every_type_to(&fresh);
    let generated = files_in(&fresh);
    let on_disk = files_in(&committed());
    let _ = std::fs::remove_dir_all(&fresh);

    let stale: Vec<&String> = generated
        .iter()
        .filter(|(name, text)| on_disk.get(*name) != Some(text))
        .map(|(name, _)| name)
        .collect();
    let leftover: Vec<&String> = on_disk.keys().filter(|name| !generated.contains_key(*name)).collect();
    assert!(
        stale.is_empty() && leftover.is_empty(),
        "bindings out of date: {stale:?}, no longer generated: {leftover:?}. \
         Run `cargo test -p solax-api -- --ignored write_bindings` and commit them."
    );
}

#[test]
#[ignore = "writes the committed bindings; run it after changing a type"]
fn write_bindings() {
    export_every_type_to(&committed());
}
