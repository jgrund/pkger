use crate::build::container::Context;
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, trace, Instrument};

pub fn package_name(ctx: &Context<'_>, extension: bool) -> String {
    format!(
        "{}-{}-{}-{}{}",
        &ctx.build.recipe.metadata.name,
        &ctx.build.recipe.metadata.version,
        &ctx.build.recipe.metadata.release(),
        ctx.build.recipe.metadata.arch.pkg_name(),
        if extension { ".pkg" } else { "" },
    )
}

/// Creates a final PKG package and saves it to `output_dir`
pub(crate) async fn build(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
) -> Result<PathBuf> {
    let package_name = package_name(ctx, false);

    let span = info_span!("PKG", package = %package_name);
    async move {
        info!("building PKG package");

        let tmp_dir = PathBuf::from(format!("/tmp/{}", package_name));
        let src_dir = tmp_dir.join("src");
        let bld_dir = tmp_dir.join("bld");

        let source_tar_name = [&package_name, ".tar.gz"].join("");
        let source_tar_path = bld_dir.join(source_tar_name);

        let dirs = [tmp_dir.as_path(), bld_dir.as_path(), src_dir.as_path()];

        ctx.create_dirs(&dirs[..])
            .await
            .context("failed to create dirs")?;

        trace!("copy source files to temporary location");
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!("cp -rv . {}", src_dir.display()))
                .working_dir(&ctx.build.container_out_dir)
                .build(),
        )
        .await
        .context("failed to copy source files to temp directory")?;

        trace!("prepare archived source files");
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!("tar -zcvf {} .", source_tar_path.display()))
                .working_dir(src_dir.as_path())
                .build(),
        )
        .await?;

        trace!("calculate source MD5 checksum");
        let sum = ctx
            .checked_exec(
                &ExecOpts::default()
                    .cmd(&format!("md5sum {}", source_tar_path.display()))
                    .build(),
            )
            .await
            .map(|out| out.stdout.join(""))?;
        let sum = sum
            .split_ascii_whitespace()
            .next()
            .map(|s| s.to_string())
            .context("failed to calculate MD5 checksum of source")?;

        let sources = vec![source_tar_path.to_string_lossy().to_string()];
        let checksums = vec![sum];
        static BUILD_USER: &str = "builduser";

        let pkgbuild = ctx
            .build
            .recipe
            .as_pkgbuild(&image_state.image, &sources, &checksums)
            .render();
        debug!(PKGBUILD = %pkgbuild);

        ctx.container
            .upload_files(
                vec![("PKGBUILD".to_string(), pkgbuild.as_bytes())],
                &bld_dir,
                ctx.build.quiet,
            )
            .await
            .context("failed to upload PKGBUILD to container")?;

        trace!("create build user");
        ctx.script_exec([
            (
                &exec!(&format!("useradd -m {}", BUILD_USER)),
                Some("failed to create build user"),
            ),
            (
                &exec!(&format!("passwd -d {}", BUILD_USER)),
                Some("failed to create build user"),
            ),
            (
                &exec!(&format!("chown -Rv {0}:{0} .", BUILD_USER), &bld_dir),
                Some("failed to change ownership of build directory"),
            ),
            (
                &exec!("chmod 644 PKGBUILD", &bld_dir),
                Some("failed to change mode of PKGBUILD"),
            ),
            (
                &exec!("makepkg", &bld_dir, BUILD_USER),
                Some("failed to makepkg"),
            ),
        ])
        .await?;

        let pkg = format!("{}.pkg.tar.zst", package_name);
        let pkg_path = bld_dir.join(&pkg);

        ctx.container
            .download_files(&pkg_path, output_dir)
            .await
            .map(|_| output_dir.join(pkg))
            .context("failed to download finished package")
    }
    .instrument(span)
    .await
}
