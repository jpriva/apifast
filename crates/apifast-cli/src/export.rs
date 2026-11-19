//! `apifast export`: a collection, and some of its environments, as another
//! tool's files, printing what could not be kept.

use std::path::PathBuf;

use anyhow::Result;
use apifast_app::{ImportWarning, collection_contents};
use apifast_storage_fs::FsCollection;

use crate::locate;

#[derive(clap::Args)]
pub struct ExportArgs {
    /// What to export to.
    #[arg(value_enum)]
    format: Format,
    /// The collection folder, or a folder inside it.
    #[arg(long, default_value = ".")]
    collection: PathBuf,
    /// The file to write. An existing one is replaced.
    #[arg(long, value_name = "FILE")]
    out: PathBuf,
    /// An environment to export too, by name (repeatable), as
    /// `<name>.postman_environment.json` next to `--out`.
    #[arg(long = "env", value_name = "NAME")]
    environments: Vec<String>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum Format {
    /// A Postman collection (v2.1) and Postman environments.
    Postman,
}

/// Exports the collection.
///
/// # Errors
///
/// If the collection cannot be read, an environment does not exist, or a
/// file cannot be written.
pub fn run(args: &ExportArgs) -> Result<()> {
    let root = locate::collection_root(&args.collection)?;
    let repo = FsCollection::open(&root)?;
    let contents = collection_contents(&repo, &repo)?;
    let Format::Postman = args.format;
    let files = apifast_import::write_postman(&contents, &args.out, &args.environments)?;
    println!("Exported `{}` to {}", contents.name, args.out.display());
    for (name, file) in args.environments.iter().zip(&files.environments) {
        println!("Exported environment `{name}` to {}", file.display());
    }
    print!("{}", warnings_text(&files.warnings));
    Ok(())
}

fn warnings_text(warnings: &[ImportWarning]) -> String {
    use std::fmt::Write as _;
    if warnings.is_empty() {
        return String::new();
    }
    let mut out = format!("Not kept ({}):\n", warnings.len());
    for warning in warnings {
        let at = if warning.at.is_empty() {
            "collection"
        } else {
            warning.at.as_str()
        };
        let _ = writeln!(out, "  - {at}: {}", warning.message);
    }
    out
}
