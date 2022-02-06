use crate::app::Application;
use crate::job::{JobCtx, JobResult};
use crate::opts::BuildOpts;
use pkger_core::build::{container::SESSION_LABEL_KEY, Context};
use pkger_core::container;
use pkger_core::docker::DockerConnectionPool;
use pkger_core::image::Image;
use pkger_core::recipe::{BuildTarget, ImageTarget, Recipe};
use pkger_core::{err, ErrContext, Error, Result};

use futures::stream::FuturesUnordered;
use log::{debug, error, trace, warn};
use std::convert::TryFrom;
use std::sync::Arc;
use tokio::task;

#[derive(Debug, PartialEq)]
pub enum BuildTask {
    Simple {
        recipe: Arc<Recipe>,
        target: BuildTarget,
    },
    Custom {
        recipe: Arc<Recipe>,
        target: ImageTarget,
    },
}

impl Application {
    pub fn process_build_opts(&mut self, opts: BuildOpts) -> Result<Vec<BuildTask>> {
        let mut tasks = Vec::new();
        let mut recipes = Vec::new();

        if opts.all {
            recipes = self
                .recipes
                .load_all()
                .context("loading recipes")?
                .into_iter()
                .map(Arc::new)
                .collect();
        } else if !opts.recipes.is_empty() {
            for recipe_name in opts.recipes {
                trace!("loading {}", recipe_name);
                recipes.push(Arc::new(
                    self.recipes.load(&recipe_name).context("loading recipe")?,
                ));
            }
        } else {
            eprintln!("no recipes to build");
            eprintln!("if you meant to build all recipes run `pkger build --all`");
            eprintln!("or only specified recipes with `pkger build <RECIPES>...`");
            return Ok(tasks);
        }

        macro_rules! add_task_if_target_found {
            ($target:ident, $recipe:ident, $self:ident, $tasks:ident) => {
                if let Some(target) = $self
                    .config
                    .images
                    .iter()
                    .find(|target| &target.image == $target)
                {
                    $tasks.push(BuildTask::Custom {
                        recipe: $recipe.clone(),
                        target: target.clone(),
                    });
                } else {
                    warn!("image '{}' not found in configuration, skipping", $target);
                }
            };
        }

        if opts.all {
            debug!("building all recipes for all targets");
            for recipe in &recipes {
                if recipe.metadata.all_images {
                    for image in &self.config.images {
                        tasks.push(BuildTask::Custom {
                            target: image.clone(),
                            recipe: recipe.clone(),
                        });
                    }
                } else if !recipe.images().is_empty() {
                    for target_image in recipe.images() {
                        add_task_if_target_found!(target_image, recipe, self, tasks);
                    }
                } else {
                    warn!(
                        "recipe has no image targets, skipping recipe = {}",
                        &recipe.metadata.name
                    );
                }
            }
        } else if let Some(targets) = &opts.simple {
            debug!("building only specified recipes for simple targets");
            for target in targets {
                for recipe in &recipes {
                    let target = BuildTarget::try_from(target.as_str())?;
                    tasks.push(BuildTask::Simple {
                        recipe: recipe.clone(),
                        target,
                    })
                }
            }
        } else if let Some(opt_images) = &opts.images {
            debug!("building only specified recipes for specified images");
            for recipe in &recipes {
                if recipe.metadata.all_images {
                    for image in opt_images {
                        add_task_if_target_found!(image, recipe, self, tasks);
                    }
                } else if !recipe.images().is_empty() {
                    for image in opt_images {
                        // first we check if the recipe contains the image
                        if recipe.images().iter().any(|target| target == image) {
                            // then we fetch the target from configuration images
                            add_task_if_target_found!(image, recipe, self, tasks);
                        } else {
                            warn!(
                                "image '{}' not found in recipe '{}' targets",
                                image, &recipe.metadata.name
                            );
                        }
                    }
                } else {
                    warn!(
                        "recipe '{}' has no image targets, skipping",
                        &recipe.metadata.name
                    );
                }
            }
        } else {
            trace!("building only specified recipes for all targets");
            for recipe in &recipes {
                if recipe.metadata.all_images {
                    for image in &self.config.images {
                        tasks.push(BuildTask::Custom {
                            target: image.clone(),
                            recipe: recipe.clone(),
                        });
                    }
                } else if !recipe.images().is_empty() {
                    for target_image in recipe.images() {
                        add_task_if_target_found!(target_image, recipe, self, tasks);
                    }
                } else {
                    warn!(
                        "recipe '{}' has no image targets, skipping",
                        &recipe.metadata.name
                    );
                }
            }
        }

        self.docker = Arc::new(
            // check if docker uri provided as cli arg
            match &opts.docker {
                Some(uri) => {
                    trace!("using docker uri from opts, uri: {}", uri);
                    DockerConnectionPool::new(uri)
                }
                None => {
                    // otherwise check if available as config parameter
                    if let Some(uri) = &self.config.docker {
                        trace!("using docker uri from config, uri: {}", uri);
                        DockerConnectionPool::new(uri)
                    } else {
                        trace!("using default docker uri");
                        Ok(DockerConnectionPool::default())
                    }
                }
            }
            .context("Failed to initialize docker connection")?,
        );
        Ok(tasks)
    }

    pub async fn process_tasks(&mut self, tasks: Vec<BuildTask>, quiet: bool) -> Result<()> {
        let jobs = FuturesUnordered::new();
        let start = std::time::SystemTime::now();

        for task in tasks {
            let (recipe, image, target, is_simple) = match task {
                BuildTask::Custom { recipe, target } => {
                    let image = Image::new(
                        target.image.clone(),
                        self.user_images_dir.join(&target.image),
                    );
                    (recipe, image, target, false)
                }
                BuildTask::Simple { recipe, target } => {
                    let image = Image::try_get_or_new_simple(
                        &self.app_dir.path().join("images"),
                        target,
                        self.config
                            .custom_simple_images
                            .as_ref()
                            .and_then(|c| c.name_for_target(target)),
                    )?;
                    let name = image.name.clone();
                    (
                        recipe,
                        image,
                        ImageTarget::new(name, target, None::<&str>),
                        true,
                    )
                }
            };
            let ctx = Context::new(
                &self.session_id,
                recipe,
                image,
                self.docker.connect(),
                target,
                self.config.output_dir.as_path(),
                self.images_state.clone(),
                is_simple,
                self.gpg_key.clone(),
                self.config.ssh.clone(),
                quiet,
            );
            let id = ctx.id().to_string();

            println!("Starting job {}", &id);
            jobs.push((id, task::spawn(JobCtx::Build(ctx).run())));
        }

        let mut results = vec![];

        for (id, mut job) in jobs {
            tokio::select! {
                res = &mut job => {
                    if let Err(e) = res {
                        eprintln!("failed to join the handle for a job, reason: {:?}", e);
                        continue;
                    }
                    results.push(res.unwrap());
                }
                _ = self.is_running() => {
                    results.push(
                        JobResult::Failure {
                            id,
                            duration: start.elapsed().unwrap_or_default(),
                            reason: "job cancelled by ctrl-c signal".to_string()
                        }
                    );
                }
            }
        }

        let mut task_failed = false;

        results.iter().for_each(|err| match err {
            JobResult::Failure {
                id,
                duration,
                reason,
            } => {
                task_failed = true;
                println!(
                    "job '{}' failed, duration: {}s, reason: {}",
                    &id,
                    duration.as_secs_f32(),
                    reason
                );
            }
            JobResult::Success {
                id,
                duration,
                output,
            } => {
                println!(
                    "job '{}' succeded, duration: {}s, output: {}",
                    &id,
                    duration.as_secs_f32(),
                    output
                );
            }
        });

        if self.images_state.read().await.has_changed() {
            if let Err(e) = self.save_images_state().await {
                error!("failed to save images state, reason: {:?}", e);
            }
        } else {
            trace!("images state unchanged, not saving");
        }

        let docker = self.docker.connect();
        match container::cleanup(&docker, SESSION_LABEL_KEY, self.session_id.to_string()).await {
            Ok(info) => {
                trace!(
                    "successfuly removed containers, space reclaimed: {}B",
                    info.space_reclaimed
                );
            }
            Err(e) => {
                error!(
                    "failed to cleanup containers for session '{}', reason: {:?}",
                    &self.session_id, e
                );
            }
        }

        if task_failed {
            err!("at least one of the tasks failed")
        } else {
            Ok(())
        }
    }
}
