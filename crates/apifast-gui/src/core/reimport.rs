//! Importing again into a collection that came from an import (ADR 0010):
//! the user picks the source's new version, reviews what happens to each
//! request, chooses, and applies. The source read is kept until they apply,
//! so what is written is what they saw.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use apifast_app::{
    ImportRecords, ImportedCollection, ReimportAction, ReimportPlan, apply_reimport, plan_reimport,
};

use super::State;

/// What the interface shows of a reimport being reviewed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReimportView {
    /// The collection's folder.
    pub root: PathBuf,
    /// The file it was last imported from, if it says.
    pub previous_source: Option<PathBuf>,
    /// The file the plan was made from.
    pub file: Option<PathBuf>,
    /// What importing it again would do, or why it cannot be.
    pub plan: Option<Result<Arc<ReimportPlan>, String>>,
}

impl State {
    /// The commands about a reimport.
    pub(super) fn handle_reimport(&mut self, command: super::Command) {
        match command {
            super::Command::OpenReimport(root) => self.open_reimport(root),
            super::Command::PlanReimport(file) => self.plan_reimport(&file),
            super::Command::ApplyReimport(chosen) => self.apply_reimport(&chosen),
            super::Command::CloseReimport => {
                self.reimporting = None;
                self.snapshot.reimport = None;
            }
            _ => {}
        }
    }

    /// Starts a reimport into the collection at `root`; without a record
    /// of its import, the view says why it cannot.
    fn open_reimport(&mut self, root: PathBuf) {
        self.reimporting = None;
        let record = self.open.get(&root).map(|open| open.repo.import_record());
        let (previous_source, plan) = match record {
            Some(Ok(Some(record))) => {
                let file = apifast_storage_fs::import_source_path(&root, &record.source);
                ((!record.source.is_empty()).then_some(file), None)
            }
            Some(Ok(None)) => (None, Some(Err(not_imported(&root)))),
            Some(Err(e)) => (None, Some(Err(e.to_string()))),
            None => (None, Some(Err(format!("`{}` is not open", root.display())))),
        };
        self.snapshot.reimport = Some(ReimportView {
            root,
            previous_source,
            file: None,
            plan,
        });
    }

    /// Reads `file` and plans importing it again.
    fn plan_reimport(&mut self, file: &Path) {
        let Some(view) = self.snapshot.reimport.as_mut() else {
            return;
        };
        let planned = (|| {
            let open = self
                .open
                .get(&view.root)
                .ok_or_else(|| not_imported(&view.root))?;
            let record = (open.repo.import_record())
                .map_err(|e| e.to_string())?
                .ok_or_else(|| not_imported(&view.root))?;
            let imported = super::import::read_source(file, &[])?;
            let plan = plan_reimport(&*open.repo, &record, &imported).map_err(|e| e.to_string())?;
            Ok::<_, String>((imported, plan))
        })();
        view.file = Some(file.to_path_buf());
        match planned {
            Ok((imported, plan)) => {
                view.plan = Some(Ok(Arc::new(plan)));
                self.reimporting = Some(imported);
            }
            Err(why) => {
                view.plan = Some(Err(why));
                self.reimporting = None;
            }
        }
    }

    /// Applies the reviewed plan, with the requests of `chosen` (by key)
    /// selected and the rest not, then reloads the collection.
    fn apply_reimport(&mut self, chosen: &[String]) {
        let (Some(view), Some(imported)) =
            (self.snapshot.reimport.clone(), self.reimporting.take())
        else {
            return;
        };
        let (Some(Ok(plan)), Some(file)) = (view.plan.as_ref(), view.file.as_ref()) else {
            return;
        };
        let mut plan = plan.as_ref().clone();
        for entry in &mut plan.entries {
            entry.selected =
                entry.action != ReimportAction::Unchanged && chosen.contains(&entry.key);
        }
        let source = apifast_storage_fs::import_source(&view.root, file);
        let applied = self.organize(&view.root, |repo| {
            apply_reimport((repo, repo), &imported, &plan, &source)
        });
        self.reload_environments(&view.root);
        self.refresh_collection(&view.root);
        if applied.is_some() {
            self.snapshot.reimport = None;
        } else {
            // Kept with its plan, to try again.
            self.reimporting = Some(imported);
        }
    }
}

fn not_imported(root: &Path) -> String {
    format!(
        "`{}` did not come from an import: there is nothing to import again",
        root.display()
    )
}

/// The source read for a reimport.
pub(super) type Reimporting = Option<ImportedCollection>;
