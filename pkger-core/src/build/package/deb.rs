use crate::build::container::Context;
use crate::build::package::sign::{import_gpg_key, upload_gpg_key};
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::{ErrContext, Result};

use log::{debug, info, trace};
use std::path::{Path, PathBuf};

pub fn package_name(ctx: &Context<'_>, extension: bool) -> String {
    format!(
        "{}-{}.{}{}",
        &ctx.build.recipe.metadata.name,
        &ctx.build.recipe.metadata.version,
        ctx.build.recipe.metadata.arch.deb_name(),
        if extension { ".deb" } else { "" },
    )
}

/// Creates a final DEB packages and saves it to `output_dir`
pub async fn build(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
) -> Result<PathBuf> {
    let package_name = package_name(ctx, false);

    info!("building DEB package {}", &package_name);

    let debbld_dir = PathBuf::from("/root/debbuild");
    let tmp_dir = debbld_dir.join("tmp");
    let base_dir = debbld_dir.join(&package_name);
    let deb_dir = base_dir.join("DEBIAN");
    let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

    ctx.create_dirs(&dirs[..])
        .await
        .context("creating build directories")?;

    let size_out = ctx
        .checked_exec(
            &ExecOpts::default()
                .cmd("du -s .")
                .working_dir(&ctx.build.container_out_dir)
                .build(),
        )
        .await
        .context("checking size of package files")?
        .stdout
        .join("");
    let size = size_out.split_ascii_whitespace().next();

    let control = ctx
        .build
        .recipe
        .as_deb_control(&image_state.image, size)
        .render();
    debug!("control:\n{}", control);

    // Upload install scripts
    if let Some(deb) = &ctx.build.recipe.metadata.deb {
        let mut scripts = vec![];
        if let Some(postinst) = &deb.postinst_script {
            scripts.push(("./postinst", postinst.as_bytes()));
        }
        if !scripts.is_empty() {
            let scripts_paths: String = scripts
                .iter()
                .map(|s| s.0.trim_start_matches("./"))
                .collect::<Vec<_>>()
                .join(" ");

            ctx.container
                .upload_files(scripts, &deb_dir, ctx.build.quiet)
                .await
                .context("uploading install scripts to container")?;

            ctx.checked_exec(
                &ExecOpts::default()
                    .cmd(&format!("chmod 0755 {}", scripts_paths))
                    .working_dir(&deb_dir)
                    .build(),
            )
            .await
            .context("changing ownership of build scripts")?;
        }
    }

    ctx.container
        .upload_files(
            vec![("./control", control.as_bytes())],
            &deb_dir,
            ctx.build.quiet,
        )
        .await
        .context("uploading control file to container")?;

    trace!("copy source files to build directory");
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!("cp -rv . {}", base_dir.display()))
            .working_dir(&ctx.build.container_out_dir)
            .build(),
    )
    .await
    .context("copying source files to build directory")?;

    let dpkg_deb_opts = if image_state.os.version().parse::<u8>().unwrap_or_default() < 10 {
        "--build"
    } else {
        "--build --root-owner-group"
    };

    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!(
                "dpkg-deb {} {}",
                dpkg_deb_opts,
                base_dir.display()
            ))
            .build(),
    )
    .await
    .context("running dpkg-deb to assemble final package")?;

    let deb_name = [&package_name, ".deb"].join("");
    let package_file = debbld_dir.join(&deb_name);

    sign_package(ctx, &package_file).await?;

    ctx.container
        .download_files(&package_file, output_dir)
        .await
        .map(|_| output_dir.join(deb_name))
        .context("downloading output files from container")
}

pub(crate) async fn sign_package(ctx: &Context<'_>, package: &Path) -> Result<()> {
    let gpg_key = if let Some(key) = &ctx.build.gpg_key {
        key
    } else {
        return Ok(());
    };

    let key_file = upload_gpg_key(ctx, gpg_key, &ctx.build.container_tmp_dir)
        .await
        .context("failed to upload gpg key to container")?;

    import_gpg_key(ctx, gpg_key, &key_file)
        .await
        .context("failed to import gpg key")?;

    trace!("get key id");
    let key_id = ctx
        .checked_exec(
            &ExecOpts::default()
                .cmd("gpg --list-keys --with-colons")
                .build(),
        )
        .await
        .map(|out| {
            let stdout = out.stdout.join("");
            for line in stdout.split('\n') {
                if !line.contains(gpg_key.name()) {
                    continue;
                }

                return line.split(':').nth(7).map(ToString::to_string);
            }
            None
        })
        .context("failed to get gpg key id")?
        .unwrap_or_default();

    trace!("add signature");
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!(
                r#"dpkg-sig -k {} -g "--pinentry-mode=loopback --passphrase {}" --sign {} {}"#,
                key_id,
                gpg_key.pass(),
                gpg_key.name().to_lowercase(),
                package.display()
            ))
            .build(),
    )
    .await
    .map(|_| ())
}
