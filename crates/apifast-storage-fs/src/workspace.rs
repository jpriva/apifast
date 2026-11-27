//! Workspaces (ADR 0009): `workspace.yml`, the shared file that groups
//! collections, as Bruno writes it; and what stays on this machine: the
//! personal state of each workspace and the list of known workspaces.
//!
//! ```yaml
//! opencollection: 1.0.0
//! info:
//!   name: "My team"
//!   type: workspace
//! collections:
//!   - name: "Shop API"
//!     path: "collections/shop"
//! specs:
//! docs: ''
//! ```

use std::path::{Component, Path, PathBuf};

use apifast_app::{
    RepositoryError, RequestPath, StoreError, Workspace, WorkspaceEntry, WorkspaceRepository,
};
use oneyaml::Value;

use crate::yaml::{child_mut, get, get_in, items, mapping, set, string, text, text_or_empty};
use crate::{fs, save_document_in, write};

/// The file that makes a folder a workspace.
pub const WORKSPACE_FILE: &str = "workspace.yml";

/// A workspace folder.
#[derive(Debug, Clone)]
pub struct FsWorkspace {
    folder: PathBuf,
}

impl FsWorkspace {
    /// The workspace whose folder holds `workspace.yml`.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] if there is no `workspace.yml`.
    pub fn open(folder: impl Into<PathBuf>) -> Result<Self, RepositoryError> {
        let folder = folder.into();
        let file = folder.join(WORKSPACE_FILE);
        if !file.is_file() {
            return Err(RepositoryError::NotFound(file.display().to_string()));
        }
        Ok(Self { folder })
    }

    /// Creates a workspace named `name` in `folder` (created if needed).
    ///
    /// # Errors
    ///
    /// If the folder already holds a workspace, or cannot be written.
    pub fn create(folder: impl Into<PathBuf>, name: &str) -> Result<Self, RepositoryError> {
        let folder = folder.into();
        if folder.join(WORKSPACE_FILE).exists() {
            return Err(RepositoryError::Invalid {
                path: folder.join(WORKSPACE_FILE).display().to_string(),
                message: "a workspace already exists here".into(),
            });
        }
        let workspace = Self { folder };
        workspace.save_workspace(&Workspace {
            name: name.to_owned(),
            collections: Vec::new(),
        })?;
        Ok(workspace)
    }

    /// Its folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The folder of `entry`: a relative path is inside the workspace's.
    pub fn resolve(&self, entry: &WorkspaceEntry) -> PathBuf {
        let path = Path::new(&entry.path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.folder.join(path)
        }
    }

    /// The entry for the collection in `root`: its path relative to the
    /// workspace when it is inside its folder, absolute otherwise.
    pub fn entry_for(&self, root: &Path, name: &str) -> WorkspaceEntry {
        let canonical = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let (root, folder) = (canonical(root), canonical(&self.folder));
        let path = match root.strip_prefix(&folder) {
            Ok(inside)
                if inside
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))) =>
            {
                let parts: Vec<String> = inside
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                parts.join("/")
            }
            _ => root.display().to_string(),
        };
        WorkspaceEntry {
            name: name.to_owned(),
            path,
        }
    }
}

impl WorkspaceRepository for FsWorkspace {
    fn workspace(&self) -> Result<Workspace, RepositoryError> {
        let text = crate::read_text_in(&self.folder, WORKSPACE_FILE)?
            .ok_or_else(|| RepositoryError::NotFound(WORKSPACE_FILE.into()))?;
        let doc = crate::parse(WORKSPACE_FILE, &text)?;
        Ok(read(&doc))
    }

    fn save_workspace(&self, workspace: &Workspace) -> Result<(), RepositoryError> {
        save_document_in(
            &self.folder,
            WORKSPACE_FILE,
            workspace,
            (|doc: &Value| Ok(read(doc)), update, new_document),
        )
    }
}

/// The workspace's environments, in its `environments/`: those Bruno
/// calls global, which every collection of the workspace sees.
impl apifast_app::EnvironmentRepository for FsWorkspace {
    fn environments(&self) -> Result<Vec<apifast_domain::Environment>, RepositoryError> {
        crate::FsEnvironments::new(&self.folder).all()
    }

    fn save_environment(
        &self,
        environment: &apifast_domain::Environment,
    ) -> Result<(), RepositoryError> {
        crate::FsEnvironments::new(&self.folder).save(environment)
    }

    fn rename_environment(&self, from: &str, to: &str) -> Result<(), RepositoryError> {
        crate::FsEnvironments::new(&self.folder).rename(from, to)
    }

    fn delete_environment(&self, name: &str) -> Result<(), RepositoryError> {
        crate::FsEnvironments::new(&self.folder).delete(name)
    }

    /// The process's: a workspace has no `.env` of its own.
    fn process_env(&self) -> Result<std::collections::BTreeMap<String, String>, RepositoryError> {
        Ok(std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect())
    }
}

fn read(doc: &Value) -> Workspace {
    Workspace {
        name: text_or_empty(get_in(doc, &["info", "name"])),
        collections: items(get(doc, "collections"))
            .iter()
            .map(|item| WorkspaceEntry {
                name: text_or_empty(get(item, "name")),
                path: text_or_empty(get(item, "path")),
            })
            .collect(),
    }
}

fn entry_item(entry: &WorkspaceEntry) -> Value {
    mapping([
        ("name", Some(string(&entry.name))),
        ("path", Some(string(&entry.path))),
    ])
}

fn merge_entry(item: &mut Value, old: &WorkspaceEntry, new: &WorkspaceEntry) {
    if old.name != new.name {
        set(item, "name", string(&new.name));
    }
    if old.path != new.path {
        set(item, "path", string(&new.path));
    }
}

fn update(doc: &mut Value, old: &Workspace, new: &Workspace) {
    if old.name != new.name {
        set(child_mut(doc, "info"), "name", string(&new.name));
    }
    if old.collections != new.collections {
        write::update_list(
            doc,
            "collections",
            &old.collections,
            &new.collections,
            entry_item,
            merge_entry,
        );
    }
}

fn new_document(workspace: &Workspace) -> Value {
    let mut doc = mapping([
        ("opencollection", Some(string("1.0.0"))),
        (
            "info",
            Some(mapping([
                ("name", Some(string(&workspace.name))),
                ("type", Some(string("workspace"))),
            ])),
        ),
    ]);
    update(
        &mut doc,
        &Workspace {
            name: workspace.name.clone(),
            collections: Vec::new(),
        },
        workspace,
    );
    doc
}

/// What this machine keeps of a workspace: which collections are
/// expanded and which tabs were open. Never in the shared file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceStateData {
    /// Folders of the expanded collections.
    pub expanded: Vec<PathBuf>,
    /// Open tabs, in order: a collection's folder and the request.
    pub tabs: Vec<(PathBuf, RequestPath)>,
    /// The active tab.
    pub active: Option<(PathBuf, RequestPath)>,
}

/// The personal state of one workspace, in its own file.
#[derive(Debug, Clone)]
pub struct FsWorkspaceState {
    file: PathBuf,
}

impl FsWorkspaceState {
    /// The state kept in `file`, created on the first save.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// The saved state; empty if there is none. Entries that are not
    /// understood are skipped.
    ///
    /// # Errors
    ///
    /// If the file exists but cannot be read or is not YAML.
    pub fn load(&self) -> Result<WorkspaceStateData, StoreError> {
        let Some(doc) = load_yaml(&self.file)? else {
            return Ok(WorkspaceStateData::default());
        };
        let tab = |item: &Value| {
            Some((
                PathBuf::from(text(get(item, "collection"))?),
                RequestPath::new(text(get(item, "request"))?),
            ))
        };
        Ok(WorkspaceStateData {
            expanded: items(get(&doc, "expanded"))
                .iter()
                .filter_map(|v| text(Some(v)).map(PathBuf::from))
                .collect(),
            tabs: items(get(&doc, "tabs")).iter().filter_map(tab).collect(),
            active: get(&doc, "active").and_then(tab),
        })
    }

    /// Replaces the saved state.
    ///
    /// # Errors
    ///
    /// If the file or its folder cannot be written.
    pub fn save(&self, state: &WorkspaceStateData) -> Result<(), StoreError> {
        let tab = |(root, path): &(PathBuf, RequestPath)| {
            mapping([
                ("collection", Some(string(root.display().to_string()))),
                ("request", Some(string(path.as_str()))),
            ])
        };
        let doc = mapping([
            (
                "expanded",
                Some(Value::Sequence(
                    state
                        .expanded
                        .iter()
                        .map(|p| string(p.display().to_string()))
                        .collect(),
                )),
            ),
            (
                "tabs",
                Some(Value::Sequence(state.tabs.iter().map(tab).collect())),
            ),
            ("active", state.active.as_ref().map(tab)),
        ]);
        save_yaml(&self.file, &doc)
    }
}

/// The workspaces this machine knows, and the one last used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownWorkspaces {
    /// Their folders, in the order they were added.
    pub known: Vec<PathBuf>,
    /// The one last used.
    pub active: Option<PathBuf>,
}

/// The list of known workspaces, in the configuration directory.
#[derive(Debug, Clone)]
pub struct FsKnownWorkspaces {
    file: PathBuf,
}

impl FsKnownWorkspaces {
    /// The list kept in `file`, created on the first save.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// The list; empty if there is none.
    ///
    /// # Errors
    ///
    /// If the file exists but cannot be read or is not YAML.
    pub fn load(&self) -> Result<KnownWorkspaces, StoreError> {
        let Some(doc) = load_yaml(&self.file)? else {
            return Ok(KnownWorkspaces::default());
        };
        Ok(KnownWorkspaces {
            known: items(get(&doc, "known"))
                .iter()
                .filter_map(|v| text(Some(v)).map(PathBuf::from))
                .collect(),
            active: text(get(&doc, "active")).map(PathBuf::from),
        })
    }

    /// Replaces the list.
    ///
    /// # Errors
    ///
    /// If the file or its folder cannot be written.
    pub fn save(&self, list: &KnownWorkspaces) -> Result<(), StoreError> {
        let doc = mapping([
            (
                "active",
                list.active
                    .as_ref()
                    .map(|p| string(p.display().to_string())),
            ),
            (
                "known",
                Some(Value::Sequence(
                    list.known
                        .iter()
                        .map(|p| string(p.display().to_string()))
                        .collect(),
                )),
            ),
        ]);
        save_yaml(&self.file, &doc)
    }
}

fn load_yaml(file: &Path) -> Result<Option<Value>, StoreError> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(store_error("cannot read", file, &e)),
    };
    oneyaml::from_str(&text)
        .map(Some)
        .map_err(|e| store_error("not valid", file, &e))
}

fn save_yaml(file: &Path, doc: &Value) -> Result<(), StoreError> {
    let text = write::to_text(doc).map_err(|e| store_error("cannot write", file, &e))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| store_error("cannot write", file, &e))?;
    }
    fs::write_atomic(file, text.as_bytes()).map_err(|e| store_error("cannot write", file, &e))
}

fn store_error(what: &str, file: &Path, error: &dyn std::fmt::Display) -> StoreError {
    StoreError(format!("{what} `{}`: {error}", file.display()))
}
