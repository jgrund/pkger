use crate::archive::{save_tar_gz, tar};
use crate::build::container::Context;
use crate::{ErrContext, Result};

use log::info;
use std::path::{Path, PathBuf};

pub fn package_name(ctx: &Context<'_>, extension: bool) -> String {
    format!(
        "{}-{}.{}",
        &ctx.build.recipe.metadata.name,
        &ctx.build.recipe.metadata.version,
        if extension { ".tar.gz" } else { "" },
    )
}

/// Creates a final GZIP package and saves it to `output_dir` returning the path of the final
/// archive as String.
pub async fn build(ctx: &Context<'_>, output_dir: &Path) -> Result<PathBuf> {
    let archive_name = package_name(ctx, true);

    info!("building GZIP package {}", archive_name);

    let package = ctx
        .container
        .copy_from(&ctx.build.container_out_dir)
        .await?;

    let archive = tar::Archive::new(&package[..]);

    save_tar_gz(archive, &archive_name, output_dir)
        .context("saving package as tar.gz")
        .map(|_| output_dir.join(archive_name))
}
