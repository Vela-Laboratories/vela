use anyhow::{bail, Result};
use std::path::Path;

fn usage() -> &'static str {
    "usage: engine-recovery verify <bundle-dir> | verify-latest <replica-dir> | restore <bundle-dir> <empty-snapshot-dir>"
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let manifest = match arguments.as_slice() {
        [command, bundle] if command == "verify" => {
            api::recovery_replication::verify_bundle(Path::new(bundle))?
        }
        [command, replica] if command == "verify-latest" => {
            api::recovery_replication::verify_latest(Path::new(replica))?.1
        }
        [command, bundle, target] if command == "restore" => {
            api::recovery_replication::restore_bundle(Path::new(bundle), Path::new(target))?
        }
        _ => bail!(usage()),
    };
    println!(
        "verified recovery bundle {} build={} checkpoint={} last={} clean={}",
        manifest.bundle_id,
        manifest.build_id,
        manifest.checkpoint_sequence,
        manifest.last_sequence,
        manifest.clean_shutdown
    );
    Ok(())
}
