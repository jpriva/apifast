//! `apifast import`: a collection from another tool into a new folder, after
//! printing what will be created and what was not kept. Into a collection
//! it imported before, it imports again: what changed in the source is
//! updated, and the user's edits are kept unless `--overwrite`.

use std::path::PathBuf;

use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use apifast_app::{
    ImportPlan, ImportRecords, ImportedCollection, ReimportAction, ReimportPlan, apply_import,
    apply_reimport, plan_import, plan_reimport,
};
use apifast_domain::Collection;
use apifast_storage_fs::FsCollection;

#[derive(clap::Args)]
pub struct ImportArgs {
    /// Where the collection comes from.
    #[arg(value_enum)]
    format: Format,
    /// The file: a Postman collection (v2.0 or v2.1), or an OpenAPI 3 spec
    /// (JSON or YAML); for Bruno, the collection's folder.
    file: PathBuf,
    /// A Postman environment to import with it (repeatable; Postman only).
    #[arg(long = "env", value_name = "FILE")]
    environments: Vec<PathBuf>,
    /// The collection's folder: a new one, or one this source was
    /// imported into before, to update it.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
    /// Print the plan and write nothing.
    #[arg(long)]
    dry_run: bool,
    /// Importing again: take the source's version of requests edited
    /// here, and create again the ones deleted here.
    #[arg(long)]
    overwrite: bool,
    /// Importing again: delete the requests the source no longer has.
    #[arg(long)]
    prune: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum Format {
    /// A Postman collection export.
    Postman,
    /// An OpenAPI 3 spec.
    Openapi,
    /// A Bruno collection in `.bru` files: the folder with `bruno.json`.
    Bruno,
    /// An Insomnia export (v4 JSON or v5 YAML).
    Insomnia,
}

/// Imports, or prints what importing would do with `--dry-run`.
///
/// # Errors
///
/// If a file cannot be read or is not of its format, or the collection
/// cannot be written.
pub fn run(args: &ImportArgs) -> Result<()> {
    let imported = read(args)?;
    if args.out.join("opencollection.yml").exists() {
        let source = apifast_storage_fs::import_source(&args.out, &args.file);
        return reimport(args, &imported, &source);
    }
    if args.overwrite || args.prune {
        bail!("--overwrite and --prune are for importing again into a collection");
    }
    let plan = plan_import(&imported);
    print!("{}", plan_text(&plan, args));
    if args.dry_run {
        return Ok(());
    }
    let repo = FsCollection::create(&args.out, &Collection::new(imported.name.clone()))
        .with_context(|| format!("cannot create the collection in `{}`", args.out.display()))?;
    let source = apifast_storage_fs::import_source(&args.out, &args.file);
    let report = apply_import((&repo, &repo), &imported, &source)?;
    println!(
        "Imported {} folders, {} requests and {} environments into {}",
        report.folders,
        report.requests,
        report.environments,
        args.out.display()
    );
    Ok(())
}

/// Imports `imported` again into the collection at `--out`.
fn reimport(args: &ImportArgs, imported: &ImportedCollection, source: &str) -> Result<()> {
    let repo = FsCollection::open(&args.out)
        .with_context(|| format!("cannot open the collection in `{}`", args.out.display()))?;
    let Some(record) = repo.import_record()? else {
        bail!(
            "`{}` already holds a collection that was not imported: choose a new folder",
            args.out.display()
        );
    };
    let mut plan = plan_reimport(&repo, &record, imported)?;
    for entry in &mut plan.entries {
        match entry.action {
            ReimportAction::Conflict | ReimportAction::Deleted if args.overwrite => {
                entry.selected = true;
            }
            ReimportAction::Removed if args.prune => entry.selected = true,
            _ => {}
        }
    }
    print!("{}", reimport_text(&plan, args));
    if args.dry_run {
        return Ok(());
    }
    let report = apply_reimport((&repo, &repo), imported, &plan, source)?;
    println!(
        "Updated {}: {} requests written or deleted, {} new folders, {} new environments",
        args.out.display(),
        report.requests,
        report.folders,
        report.environments
    );
    Ok(())
}

/// A reimport plan, as the terminal shows it: what happens to each request
/// that is not unchanged, with `[skip]` on what is left as it is.
fn reimport_text(plan: &ReimportPlan, args: &ImportArgs) -> String {
    use ReimportAction as A;
    let verb = if args.dry_run {
        "Would update"
    } else {
        "Updating"
    };
    let mut out = format!(
        "{verb} {} (last imported from {}): {} new, {} changed, {} unchanged\n",
        args.out.display(),
        plan.previous_source,
        plan.count(A::Create),
        plan.count(A::Update) + plan.count(A::Conflict),
        plan.count(A::Unchanged),
    );
    for entry in plan.entries.iter().filter(|e| e.action != A::Unchanged) {
        let what = match entry.action {
            A::Create => "new",
            A::Update => "changed in the source",
            A::Conflict => "changed in the source and here",
            A::Deleted => "deleted here",
            A::Removed => "no longer in the source",
            A::Unchanged => continue,
        };
        let skip = if entry.selected { "" } else { " [skip]" };
        let _ = writeln!(out, "  {}: {what}{skip}", entry.path);
    }
    let hints = [
        (
            plan.count(A::Conflict) + plan.count(A::Deleted) > 0 && !args.overwrite,
            "--overwrite takes the source's version of what was edited or deleted here",
        ),
        (
            plan.count(A::Removed) > 0 && !args.prune,
            "--prune deletes what the source no longer has",
        ),
    ];
    for (_, hint) in hints.iter().filter(|(show, _)| *show) {
        let _ = writeln!(out, "  ({hint})");
    }
    for warning in &plan.warnings {
        let at = if warning.at.is_empty() {
            "collection"
        } else {
            warning.at.as_str()
        };
        let _ = writeln!(out, "  not kept: {at}: {}", warning.message);
    }
    out
}

fn read(args: &ImportArgs) -> Result<ImportedCollection> {
    let read_text = |path: &PathBuf| {
        std::fs::read_to_string(path).with_context(|| format!("cannot read `{}`", path.display()))
    };
    let context = || format!("`{}`", args.file.display());
    if !args.environments.is_empty() && !matches!(args.format, Format::Postman) {
        bail!("--env takes Postman environments, for a Postman collection");
    }
    let mut imported = match args.format {
        Format::Postman => {
            apifast_import::import_postman(&read_text(&args.file)?).with_context(context)?
        }
        Format::Bruno => apifast_import::import_bruno(&args.file).with_context(context)?,
        Format::Openapi | Format::Insomnia => {
            let text = read_text(&args.file)?;
            let value = apifast_storage_fs::yaml_to_json(&text)
                .map_err(anyhow::Error::msg)
                .with_context(context)?;
            if let Format::Insomnia = args.format {
                apifast_import::import_insomnia(&value).with_context(context)?
            } else {
                apifast_import::import_openapi(&value).with_context(context)?
            }
        }
    };
    for path in &args.environments {
        let text = read_text(path)?;
        let (environment, warnings) = apifast_import::import_postman_environment(&text)
            .with_context(|| format!("`{}`", path.display()))?;
        imported.environments.push(environment);
        imported.warnings.extend(warnings);
    }
    Ok(imported)
}

/// The plan, as the terminal shows it.
fn plan_text(plan: &ImportPlan, args: &ImportArgs) -> String {
    let (folders, requests) = plan.counts();
    let verb = if args.dry_run {
        "Would create"
    } else {
        "Creating"
    };
    let mut out = format!(
        "{verb} `{}` in {}: {folders} folders, {requests} requests, {} environments\n",
        plan.collection,
        args.out.display(),
        plan.environments.len()
    );
    for entry in &plan.entries {
        let depth = entry.path.matches('/').count();
        let mark = if entry.is_folder { "+ " } else { "  " };
        let _ = writeln!(out, "  {}{mark}{}", "  ".repeat(depth), entry.path);
    }
    for environment in &plan.environments {
        let _ = writeln!(out, "  environments/{environment}");
    }
    if !plan.warnings.is_empty() {
        let _ = writeln!(out, "Not kept ({}):", plan.warnings.len());
        for warning in &plan.warnings {
            let at = if warning.at.is_empty() {
                "collection"
            } else {
                warning.at.as_str()
            };
            let _ = writeln!(out, "  - {at}: {}", warning.message);
        }
    }
    out
}
