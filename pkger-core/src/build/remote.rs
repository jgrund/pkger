use crate::archive::create_tarball;
use crate::build::container::Context;
use crate::container::ExecOpts;
use crate::recipe::GitSource;
use crate::template;
use crate::Result;

use log::info;
use std::fs;
use std::path::{Path, PathBuf};

pub async fn fetch_git_source(ctx: &Context<'_>, repo: &GitSource) -> Result<()> {
    info!(
        "cloning git source repository '{}' branch {} to build directory {}",
        repo.url(),
        repo.branch(),
        ctx.build.container_bld_dir.display()
    );
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!(
                "git clone -j 8 --single-branch --branch {} --recurse-submodules -- {} {}",
                repo.branch(),
                repo.url(),
                ctx.build.container_bld_dir.display()
            ))
            .build(),
    )
    .await
    .map(|_| ())
}

pub async fn fetch_http_source(ctx: &Context<'_>, source: &str, dest: &Path) -> Result<()> {
    info!("fetching '{}' to {}", source, dest.display());
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!("curl -LO {}", source))
            .working_dir(dest)
            .build(),
    )
    .await
    .map(|_| ())
}

pub async fn fetch_fs_source(ctx: &Context<'_>, files: &[&Path], dest: &Path) -> Result<()> {
    let mut entries = Vec::new();
    for f in files {
        let filename = f
            .file_name()
            .map(|s| format!("./{}", s.to_string_lossy()))
            .unwrap_or_default();
        entries.push((filename, fs::read(f)?));
    }

    let archive = create_tarball(entries.iter().map(|(p, b)| (p, &b[..])))?;

    ctx.container.inner().copy_file_into(dest, &archive).await?;

    Ok(())
}

pub async fn fetch_source(ctx: &Context<'_>) -> Result<()> {
    if let Some(repo) = &ctx.build.recipe.metadata.git {
        fetch_git_source(ctx, repo).await?;
    } else if let Some(source) = &ctx.build.recipe.metadata.source {
        let source = template::render(source, ctx.vars.inner());
        if source.starts_with("http") {
            fetch_http_source(ctx, source.as_str(), &ctx.build.container_tmp_dir).await?;
        } else {
            let src_path = PathBuf::from(source);
            fetch_fs_source(ctx, &[src_path.as_path()], &ctx.build.container_tmp_dir).await?;
        }
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!(
                    r#"
                        for file in *;
                        do
                            if [[ $file =~ (.*[.]tar.*|.*[.](tgz|tbz|txz|tlz|tsz|taz|tz)) ]]
                            then
                                tar xvf $file -C {0}
                            elif [[ $file == *.zip ]]
                            then
                                unzip $file -d {0}
                            else
                                cp -v $file {0}
                            fi
                        done"#,
                    ctx.build.container_bld_dir.display(),
                ))
                .working_dir(&ctx.build.container_tmp_dir)
                .shell("/bin/bash")
                .build(),
        )
        .await?;
    }
    Ok(())
}
