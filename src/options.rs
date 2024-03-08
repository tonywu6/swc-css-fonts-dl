use std::path::PathBuf;

use clap::Parser;
use relative_path::RelativePathBuf;
use serde::{de::Visitor, Deserialize};
use url::Url;

#[derive(Debug, Clone)]
pub enum SourceLocation {
    Local(RelativePathBuf),
    Remote(Url),
}

struct SourceLocationVisitor;

impl<'de> Visitor<'de> for SourceLocationVisitor {
    type Value = SourceLocation;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("either a local path or a remote URL")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match Url::parse(v) {
            Ok(parsed) => Ok(SourceLocation::Remote(parsed)),
            Err(_) => Ok(SourceLocation::Local(v.into())),
        }
    }
}

impl<'de> Deserialize<'de> for SourceLocation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SourceLocationVisitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CSSSource {
    pub from: SourceLocation,
    pub into: String,
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DownloaderOptions {
    pub out_dir: String,
    pub sources: Vec<CSSSource>,
}

#[derive(Parser, Debug)]
pub struct CommandLineArgs {
    #[arg(
        short,
        long,
        value_name = "FILE",
        help = format!("Path to a YAML config file; defaults to .{}.config.yaml in the working directory", env!("CARGO_PKG_NAME"))
    )]
    pub config: Option<PathBuf>,
    #[arg(
        long,
        default_value = "10",
        help = "Maximum number of concurrent downloads"
    )]
    pub concurrency: usize,
}

fn default_user_agent() -> String {
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36".to_string()
}
