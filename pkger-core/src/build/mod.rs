#[macro_use]
pub mod container;
pub mod deps;
pub mod image;
pub mod package;
pub mod patches;
pub mod remote;
pub mod scripts;

use crate::container::ExecOpts;
use crate::docker::Docker;
use crate::gpg::GpgKey;
use crate::image::{Image, ImageState, ImagesState};
use crate::recipe::{ImageTarget, Recipe, RecipeTarget};
use crate::ssh::SshConfig;
use crate::{ErrContext, Result};

use async_rwlock::RwLock;
use log::{info, trace, warn};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;

#[derive(Debug)]
/// Groups all data and functionality necessary to create an artifact
pub struct Context {
    id: String,
    session_id: Uuid,
    recipe: Arc<Recipe>,
    image: Image,
    docker: Docker,
    container_bld_dir: PathBuf,
    container_out_dir: PathBuf,
    container_tmp_dir: PathBuf,
    out_dir: PathBuf,
    target: RecipeTarget,
    image_state: Arc<RwLock<ImagesState>>,
    simple: bool,
    gpg_key: Option<GpgKey>,
    ssh: Option<SshConfig>,
    quiet: bool,
}

impl Context {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: &Uuid,
        recipe: Arc<Recipe>,
        image: Image,
        docker: Docker,
        target: ImageTarget,
        out_dir: &Path,
        image_state: Arc<RwLock<ImagesState>>,
        simple: bool,
        gpg_key: Option<GpgKey>,
        ssh: Option<SshConfig>,
        quiet: bool,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let id = format!(
            "pkger-{}-{}-{}",
            &recipe.metadata.name, &target.image, &timestamp,
        );
        let container_bld_dir = PathBuf::from(format!(
            "/tmp/{}-build-{}",
            &recipe.metadata.name, &timestamp,
        ));
        let container_out_dir =
            PathBuf::from(format!("/tmp/{}-out-{}", &recipe.metadata.name, &timestamp,));

        let container_tmp_dir =
            PathBuf::from(format!("/tmp/{}-tmp-{}", &recipe.metadata.name, &timestamp,));
        trace!("creating new build context, id: {}", id);

        let target = RecipeTarget::new(recipe.metadata.name.clone(), target);

        Context {
            id,
            session_id: session_id.clone(),
            recipe,
            image,
            docker,
            container_bld_dir,
            container_out_dir,
            container_tmp_dir,
            out_dir: out_dir.to_path_buf(),
            target,
            image_state,
            simple,
            gpg_key,
            ssh,
            quiet,
        }
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    async fn create_out_dir(&self, image: &ImageState) -> Result<PathBuf> {
        let out_dir = self.out_dir.join(&image.image);

        if out_dir.exists() {
            trace!(
                "output directory '{}' already exists, skipping",
                out_dir.display()
            );
            Ok(out_dir)
        } else {
            trace!("creating output directory '{}'", out_dir.display());
            fs::create_dir_all(out_dir.as_path())
                .map(|_| out_dir)
                .context("failed to create output directory")
        }
    }
}

pub async fn run(ctx: &mut Context) -> Result<PathBuf> {
    info!("running job, id: {}", &ctx.id());
    let image_state = image::build(ctx).await.context("failed to build image")?;

    let out_dir = ctx.create_out_dir(&image_state).await?;

    let mut container_ctx = container::spawn(ctx, &image_state).await?;

    let image_state = if image_state.tag != image::CACHED {
        let mut deps = deps::default(
            ctx.target.build_target(),
            &ctx.recipe,
            ctx.gpg_key.is_some(),
        );
        deps.extend(deps::recipe(&container_ctx, &image_state));
        let new_state =
            image::create_cache(&container_ctx, &ctx.docker, &image_state, &deps).await?;
        info!(
            "successfully cached image '{}', id: {}",
            new_state.image, new_state.id
        );

        trace!("saving image state");
        let mut state = ctx.image_state.write().await;
        (*state).update(ctx.target.clone(), new_state.clone());

        container_ctx.container.remove().await?;
        container_ctx = container::spawn(ctx, &new_state).await?;

        new_state
    } else {
        image_state
    };

    let dirs = vec![
        &ctx.container_out_dir,
        &ctx.container_bld_dir,
        &ctx.container_tmp_dir,
    ];

    container_ctx.create_dirs(&dirs[..]).await?;

    remote::fetch_source(&container_ctx).await?;

    if let Some(patches) = &ctx.recipe.metadata.patches {
        let patches = patches::collect(&container_ctx, patches).await?;
        patches::apply(&container_ctx, patches).await?;
    }

    scripts::run(&container_ctx).await?;

    exclude_paths(&container_ctx).await?;

    let package = package::build(&container_ctx, &image_state, out_dir.as_path()).await?;

    container_ctx.container.remove().await?;

    Ok(package)
}

pub async fn exclude_paths(ctx: &container::Context<'_>) -> Result<()> {
    if let Some(exclude) = &ctx.build.recipe.metadata.exclude {
        let exclude_paths = exclude
            .iter()
            .filter(|p| {
                let p = PathBuf::from(p);
                if p.is_absolute() {
                    warn!(
                        "invalid path '{}', reason: absolute paths are not allowed in excludes",
                        p.display()
                    );
                    false
                } else {
                    true
                }
            })
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        info!("Exclude directories: {:?}", exclude_paths);

        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!("rm -rvf {}", exclude_paths.join(" ")))
                .working_dir(&ctx.build.container_out_dir)
                .build(),
        )
        .await?;
    }

    Ok(())
}
