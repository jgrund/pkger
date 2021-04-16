use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::util::{create_tar_archive, unpack_archive};
use crate::Result;

use futures::TryStreamExt;
use std::path::Path;
use std::path::PathBuf;
use tracing::{debug, info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    /// Creates a final DEB packages and saves it to `output_dir`
    pub(crate) async fn build_deb(
        &self,
        image_state: &ImageState,
        output_dir: &Path,
    ) -> Result<()> {
        let name = [
            &self.recipe.metadata.name,
            "-",
            &self.recipe.metadata.version,
        ]
        .join("");
        let arch = if self.recipe.metadata.arch.is_empty() {
            "all"
        } else {
            &self.recipe.metadata.arch
        };
        let package_name = [&name, ".", &arch].join("");

        let span = info_span!("DEB", package = %package_name);
        let _enter = span.enter();

        info!(parent: &span, "building DEB package");

        let debbld_dir = PathBuf::from("/root/debbuild");
        let tmp_dir = debbld_dir.join("tmp");
        let base_dir = debbld_dir.join(&name);
        let deb_dir = base_dir.join("DEBIAN");
        let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

        self.create_dirs(&dirs[..]).instrument(span.clone()).await?;

        let control = self
            .recipe
            .as_deb_control(&image_state.image)
            .render_owned()?;
        debug!(parent: &span, control = %control);

        let entries = vec![("./control", control.as_bytes())];
        let control_tar = async move { create_tar_archive(entries) }
            .instrument(span.clone())
            .await?;
        let control_tar_path = tmp_dir.join([&name, "-control.tar"].join(""));

        trace!(parent: &span, "copy control archive to container");
        self.container
            .inner()
            .copy_file_into(control_tar_path.as_path(), &control_tar)
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "extract control archive");
        self.container
            .exec(format!(
                "tar -xvf {} -C {}",
                control_tar_path.display(),
                deb_dir.display(),
            ))
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "copy source files to build dir");
        self.container
            .exec(format!(
                "cp -r {}/ {}",
                self.container_out_dir.display(),
                base_dir.display()
            ))
            .instrument(span.clone())
            .await?;

        self.container
            .exec(format!(
                "dpkg-deb --build --root-owner-group {}",
                base_dir.display()
            ))
            .instrument(span.clone())
            .await?;

        let deb = self
            .container
            .inner()
            .copy_from(debbld_dir.join([&name, ".deb"].join("")).as_path())
            .try_concat()
            .instrument(span.clone())
            .await?;

        let mut archive = tar::Archive::new(&deb[..]);

        async move {
            unpack_archive(&mut archive, output_dir)
                .map_err(|e| anyhow!("failed to unpack archive - {}", e))
        }
        .instrument(span.clone())
        .await
    }
}
