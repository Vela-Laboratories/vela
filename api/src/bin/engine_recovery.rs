use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

fn usage() -> &'static str {
    "usage:\n  engine-recovery generate-keys <new-key-dir>\n  engine-recovery verify <bundle-dir> <trusted-verifying-key-file>\n  engine-recovery verify-latest <replica-dir> <trusted-verifying-key-file>\n  engine-recovery restore <bundle-dir> <empty-snapshot-dir> <trusted-verifying-key-file>\n  engine-recovery export <bundle-dir> <offsite-dir> <age-recipient-file> <trusted-verifying-key-file>\n  engine-recovery import <archive.age> <replica-dir> <age-identity-file> <trusted-verifying-key-file>"
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [command, target] if command == "generate-keys" => {
            let generated =
                api::recovery_replication::generate_recovery_key_files(Path::new(target))?;
            println!(
                "created recovery key files in {} key_id={} age_recipient={}",
                target, generated.key_id, generated.age_recipient
            );
        }
        [command, bundle, trusted_key] if command == "verify" => {
            let manifest = api::recovery_replication::verify_bundle_with_trusted_key(
                Path::new(bundle),
                &read_key_file(trusted_key)?,
            )?;
            print_manifest(&manifest);
        }
        [command, replica, trusted_key] if command == "verify-latest" => {
            let manifest = api::recovery_replication::verify_latest_with_trusted_key(
                Path::new(replica),
                &read_key_file(trusted_key)?,
            )?
            .1;
            print_manifest(&manifest);
        }
        [command, bundle, target, trusted_key] if command == "restore" => {
            let manifest = api::recovery_replication::restore_bundle_with_trusted_key(
                Path::new(bundle),
                Path::new(target),
                &read_key_file(trusted_key)?,
            )?;
            print_manifest(&manifest);
        }
        [command, bundle, offsite, recipient, trusted_key] if command == "export" => {
            let archive = api::recovery_replication::export_encrypted_bundle(
                Path::new(bundle),
                Path::new(offsite),
                &read_key_file(recipient)?,
                &read_key_file(trusted_key)?,
            )?;
            println!("published encrypted recovery archive {}", archive.display());
        }
        [command, archive, replica, identity, trusted_key] if command == "import" => {
            let mut identity_value = read_key_file(identity)?;
            let imported = api::recovery_replication::import_encrypted_bundle(
                Path::new(archive),
                Path::new(replica),
                &identity_value,
                &read_key_file(trusted_key)?,
            );
            identity_value.zeroize();
            let (bundle, manifest) = imported?;
            println!("imported signed recovery bundle {}", bundle.display());
            print_manifest(&manifest);
        }
        _ => bail!(usage()),
    }
    Ok(())
}

fn read_key_file(path: impl Into<PathBuf>) -> Result<String> {
    let path = path.into();
    let metadata = std::fs::symlink_metadata(&path)
        .with_context(|| format!("inspect key file {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.len() > 16 * 1024 {
        bail!("key file must be a regular file no larger than 16 KiB");
    }
    let value = std::fs::read_to_string(&path)
        .with_context(|| format!("read key file {}", path.display()))?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        bail!("key file is empty");
    }
    Ok(value)
}

fn print_manifest(manifest: &api::recovery_replication::RecoveryManifest) {
    println!(
        "verified recovery bundle {} build={} checkpoint={} last={} clean={}",
        manifest.bundle_id,
        manifest.build_id,
        manifest.checkpoint_sequence,
        manifest.last_sequence,
        manifest.clean_shutdown
    );
}
