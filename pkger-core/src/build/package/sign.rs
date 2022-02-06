use crate::build::container::Context;
use crate::container::ExecOpts;
use crate::{ErrContext, Result};

use crate::gpg::GpgKey;
use log::info;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Uploads the `gpg_key` to `destination` in the container and returns the
/// full path of the key in the container.
pub(crate) async fn upload_gpg_key(
    ctx: &Context<'_>,
    gpg_key: &GpgKey,
    destination: &Path,
) -> Result<PathBuf> {
    info!("uploading GPG key to {}", destination.display());
    let key = fs::read(&gpg_key.path()).context("reading the GPG key")?;

    ctx.container
        .upload_files(
            vec![("./GPG-SIGN-KEY", key.as_slice())],
            &destination,
            ctx.build.quiet,
        )
        .await
        .map(|_| destination.join("GPG-SIGN-KEY"))
        .context("uploading GPG key")
}

/// Imports the gpg key located at `path` to the database in the container.
pub(crate) async fn import_gpg_key(ctx: &Context<'_>, gpg_key: &GpgKey, path: &Path) -> Result<()> {
    info!("importing GPG key from {}", path.display());
    ctx.checked_exec(&exec!(&format!(
        r#"gpg --pinentry-mode=loopback --passphrase {} --import {}"#,
        gpg_key.pass(),
        path.display(),
    )))
    .await
    .map(|_| ())
}
