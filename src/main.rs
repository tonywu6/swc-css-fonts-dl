use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use log::info;
use relative_path::RelativePathBuf;
use swc_core::{
    common::{
        errors::{ColorConfig, Handler},
        sync::Lrc,
        FileName, SourceFile, SourceMap,
    },
    css::{
        ast::Stylesheet,
        codegen::{writer::basic::BasicCssWriter, CodeGenerator, CodegenConfig, Emit as _},
        parser::parse_file,
        visit::VisitMutWith as _,
    },
};

mod options;
mod visitor;

use options::{CommandLineArgs, DownloaderOptions, SourceLocation};
use tokio::{sync::Semaphore, task::JoinSet};
use visitor::{rewrite_remote_fonts, RemoteFont};

async fn run() -> anyhow::Result<()> {
    let args = CommandLineArgs::parse();

    let config_path = args
        .config
        .and_then(|p| Some(std::env::current_dir().unwrap().join(p)))
        .or_else(|| Some(default_config_path()))
        .unwrap();

    let config: DownloaderOptions = serde_yaml::from_str(
        &std::fs::read_to_string(config_path.clone()).with_context(|| {
            format!(
                "failed to read config from {}. does the file exist?",
                config_path.to_string_lossy()
            )
        })?,
    )?;

    let http_client = reqwest::Client::new();

    let css_sources: Lrc<SourceMap> = Default::default();

    let error_reporter =
        Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(css_sources.clone()));

    let out_dir = resolve_path_from_file(&RelativePathBuf::from(config.out_dir), &config_path)?;

    let mut collected: Vec<(PathBuf, Lrc<SourceFile>, Stylesheet, Vec<RemoteFont>)> = vec![];

    for item in config.sources {
        let (source_file, base) = match item.from {
            SourceLocation::Remote(src) => {
                info!("fetching stylesheet from {}", src);
                let response = http_client
                    .get(src.clone())
                    .header("user-agent", item.user_agent)
                    .send()
                    .await?;
                response.error_for_status_ref()?;
                let content = response
                    .text()
                    .await
                    .with_context(|| format!("failed to fetch stylesheet from {}", src))?;
                let file = css_sources.new_source_file(FileName::Custom(src.to_string()), content);
                (file, Some(src))
            }
            SourceLocation::Local(path) => {
                info!("reading stylesheet from {}", path);
                let resolved_path = resolve_path_from_file(&path, &config_path)?;
                let content =
                    std::fs::read_to_string(resolved_path.clone()).with_context(|| {
                        format!(
                            "failed to read source from {}. does the file exist?",
                            resolved_path.to_string_lossy()
                        )
                    })?;
                let file =
                    css_sources.new_source_file(FileName::Real(resolved_path.clone()), content);
                (file, None)
            }
        };

        let mut errors = vec![];

        let css: anyhow::Result<Stylesheet> =
            match parse_file(&source_file, None, Default::default(), &mut errors) {
                Ok(css) => {
                    if errors.len() > 0 {
                        for err in errors {
                            err.to_diagnostics(&error_reporter).emit();
                        }
                        Err(anyhow::anyhow!(
                            "failed to parse stylesheet from {}",
                            source_file.name
                        ))
                    } else {
                        Ok(css)
                    }
                }
                Err(err) => {
                    for err in errors {
                        err.to_diagnostics(&error_reporter).emit();
                    }
                    err.to_diagnostics(&error_reporter).emit();
                    Err(anyhow::anyhow!(
                        "failed to parse stylesheet from {}",
                        source_file.name
                    ))
                }
            };

        info!("preparing {}", item.into);

        let mut css = css?;
        let mut urls = vec![];
        let out_path = out_dir.join(item.into);

        {
            let mut visitor = rewrite_remote_fonts(&mut urls, base);
            css.visit_mut_with(&mut visitor);
        }

        collected.push((out_path, source_file, css, urls));
    }

    std::fs::remove_dir_all(&out_dir).or_else(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(e),
    })?;

    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let mut downloads = JoinSet::new();

    for (css_out_path, original_css, transformed_css, fonts) in collected {
        let out_dir = css_out_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("could not get config dir"))?;

        std::fs::create_dir_all(out_dir)?;

        {
            let mut output = String::new();
            let writer = BasicCssWriter::new(&mut output, None, Default::default());
            let mut generator = CodeGenerator::new(writer, CodegenConfig { minify: false });
            generator.emit(&transformed_css)?;
            std::fs::write(css_out_path.clone(), output)?;
            info!("wrote {}", css_out_path.to_string_lossy());
        }

        let original_file = css_out_path.with_extension("original.css");
        std::fs::write(original_file, original_css.src.as_ref())?;

        for RemoteFont {
            url: src,
            path: dst,
        } in fonts
        {
            let sem = semaphore.clone().acquire_owned().await?;
            let client = http_client.clone();
            let css_out_path = css_out_path.clone();
            let asset_path = resolve_path_from_file(&dst, &css_out_path)?;

            downloads.spawn(async move {
                let response = client.get(src).send().await?;

                response.error_for_status_ref()?;
                let content = response.bytes().await?;

                std::fs::create_dir_all(asset_path.parent().unwrap())?;
                std::fs::write(asset_path, content)?;

                info!("downloaded {}", dst);

                drop(client);
                drop(sem);

                Ok::<(), anyhow::Error>(())
            });
        }
    }

    while let Some(result) = downloads.join_next().await {
        let _ = result?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    run().await
}

fn default_config_path() -> PathBuf {
    std::env::current_dir()
        .unwrap()
        .join(format!(".{}.config.yaml", env!("CARGO_PKG_NAME")))
}

fn resolve_path_from_file(path: &RelativePathBuf, reference: &PathBuf) -> anyhow::Result<PathBuf> {
    Ok(path.to_path(
        reference
            .parent()
            .ok_or_else(|| anyhow::anyhow!("could not get config dir"))?,
    ))
}
