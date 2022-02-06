use crate::build::{container, remote};
use crate::container::ExecOpts;
use crate::recipe::{Patch, Patches};
use crate::Result;

use log::{debug, trace, warn};
use std::path::PathBuf;

pub async fn apply(ctx: &container::Context<'_>, patches: Vec<(Patch, PathBuf)>) -> Result<()> {
    trace!("Patches: {:?}", patches);
    for (patch, location) in patches {
        debug!("applying {:?}", patch);
        if let Err(e) = ctx
            .checked_exec(
                &ExecOpts::default()
                    .cmd(&format!(
                        "patch -p{} < {}",
                        patch.strip_level(),
                        location.display()
                    ))
                    .working_dir(&ctx.build.container_bld_dir)
                    .build(),
            )
            .await
        {
            warn!("applying {:?} failed, reason: {:?}", patch, e)
        }
    }

    Ok(())
}

pub async fn collect(
    ctx: &container::Context<'_>,
    patches: &Patches,
) -> Result<Vec<(Patch, PathBuf)>> {
    let mut out = Vec::new();
    let patch_dir = ctx.build.container_tmp_dir.join("patches");
    ctx.create_dirs(&[patch_dir.as_path()]).await?;

    let mut to_copy = Vec::new();

    for patch in patches.resolve_names(ctx.build.target.image()) {
        let src = patch.patch();
        if src.starts_with("http") {
            trace!("found http source '{}'", src);
            remote::fetch_http_source(ctx, src, &patch_dir).await?;
            out.push((
                patch.clone(),
                patch_dir.join(src.split('/').last().unwrap_or_default()),
            ));
            continue;
        }

        let patch_p = PathBuf::from(src);
        if patch_p.is_absolute() {
            trace!("found absolute path source '{}'", patch_p.display());
            out.push((
                patch.clone(),
                patch_dir.join(patch_p.file_name().unwrap_or_default()),
            ));
            to_copy.push(patch_p);
            continue;
        }

        let patch_recipe_p = ctx.build.recipe.recipe_dir.join(src);
        trace!(
            "using patch from recipe_dir: '{}'",
            patch_recipe_p.display()
        );
        out.push((patch.clone(), patch_dir.join(src)));
        to_copy.push(patch_recipe_p);
    }

    let to_copy = to_copy.iter().map(PathBuf::as_path).collect::<Vec<_>>();

    let patches_archive = ctx.build.container_tmp_dir.join("patches.tar");
    remote::fetch_fs_source(ctx, &to_copy, &patches_archive).await?;

    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!(
                "tar xf {} -C {}",
                patches_archive.display(),
                patch_dir.display()
            ))
            .build(),
    )
    .await
    .map(|_| out)
}
