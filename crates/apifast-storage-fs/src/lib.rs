//! Reads and writes YAML collections on disk.
//!
//! Collections are plain OpenCollection YAML, the format Bruno uses, read the
//! way Bruno reads it. apifast's own data lives in `.apifast/`.
//!
//! Saving never rewrites a file from scratch. It re-reads the file, writes
//! into it only the fields that changed, turns that into a JSON Patch applied
//! element by element to the original text, checks that reading the result
//! back gives exactly what was saved, and replaces the file atomically. So
//! comments, formatting and keys apifast does not know survive, and saving an
//! unchanged request leaves its file byte for byte the same.

mod convert;
mod diff;
mod examples;
mod fs;
mod keymap;
mod read;
mod session;
mod settings;
mod source;
mod workspace;
mod write;
mod yaml;

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use apifast_app::{
    CollectionRepository, CollectionTree, EnvironmentRepository, LoadedRequest, Random,
    RepositoryError, RequestPath, SharedSecretFiles, TreeItem,
};
use apifast_domain::{Collection, CollectionId, DnsOverride, Environment, Folder, HttpRequest};
use oneyaml::Value;

pub use convert::yaml_to_json;
pub use keymap::FsKeymapStore;
pub use session::FsSessionStore;
pub use settings::FsSettingsStore;
pub use source::{import_source, import_source_path};
pub use workspace::{
    FsKnownWorkspaces, FsWorkspace, FsWorkspaceState, KnownWorkspaces, WORKSPACE_FILE,
    WorkspaceStateData,
};

const COLLECTION_FILE: &str = "opencollection.yml";
const FOLDER_FILE: &str = "folder.yml";
const ENVIRONMENTS_DIR: &str = "environments";
const APIFAST_DIR: &str = ".apifast";
const APIFAST_COLLECTION_FILE: &str = ".apifast/collection.yml";

/// A collection folder on disk.
#[derive(Debug, Clone)]
pub struct FsCollection {
    root: PathBuf,
}

impl FsCollection {
    /// Opens the collection whose root folder holds `opencollection.yml`.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] if there is no `opencollection.yml`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, RepositoryError> {
        let root = root.into();
        if !root.join(COLLECTION_FILE).is_file() {
            return Err(RepositoryError::NotFound(
                root.join(COLLECTION_FILE).display().to_string(),
            ));
        }
        Ok(Self { root })
    }

    /// Creates a new collection in `root`, which must not hold one already.
    ///
    /// # Errors
    ///
    /// If a collection exists there or the files cannot be written.
    pub fn create(
        root: impl Into<PathBuf>,
        collection: &Collection,
    ) -> Result<Self, RepositoryError> {
        let root = root.into();
        if root.join(COLLECTION_FILE).exists() {
            return Err(RepositoryError::Invalid {
                path: COLLECTION_FILE.into(),
                message: "a collection already exists here".into(),
            });
        }
        std::fs::create_dir_all(&root).map_err(|e| io_error(&root.display().to_string(), &e))?;
        let repo = Self { root };
        repo.save_collection(collection)?;
        Ok(repo)
    }

    /// The collection's root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The collection's stable identifier, created in `.apifast/` the first
    /// time (which also adds `.apifast` to Bruno's ignore list).
    ///
    /// # Errors
    ///
    /// If the files cannot be read or written.
    pub fn ensure_id(&self, random: &dyn Random) -> Result<CollectionId, RepositoryError> {
        let mut collection = self.collection()?;
        if let Some(id) = &collection.id {
            return Ok(id.clone());
        }
        let id = new_collection_id(random);
        collection.id = Some(id.clone());
        self.save_collection(&collection)?;
        Ok(id)
    }

    fn path(&self, relative: &str) -> Result<PathBuf, RepositoryError> {
        fs::resolve(&self.root, relative).map_err(|problem| RepositoryError::Invalid {
            path: relative.to_owned(),
            message: problem.message().into(),
        })
    }

    /// The full path of a request file or folder. Refuses the collection's
    /// own files (`opencollection.yml`, `environments`, `.env`) and reserved
    /// folders, so moving or deleting can never touch them.
    fn item_path(&self, relative: &str) -> Result<PathBuf, RepositoryError> {
        let full = self.path(relative)?;
        if fs::is_ignored(relative, &[]) {
            return Err(RepositoryError::Invalid {
                path: relative.to_owned(),
                message: "this is not a request or folder of the collection".into(),
            });
        }
        Ok(full)
    }

    /// The file's text, or `None` if it does not exist. `relative` is trusted
    /// (built by this crate), so it is not validated again.
    fn read_text(&self, relative: &str) -> Result<Option<String>, RepositoryError> {
        read_text_in(&self.root, relative)
    }

    fn read_value(&self, relative: &str) -> Result<Option<Value>, RepositoryError> {
        self.read_text(relative)?
            .map(|text| parse(relative, &text))
            .transpose()
    }

    /// Saves `new` into the file at `relative` (trusted), as described in the
    /// crate docs. `read` turns a document into the domain value and `update`
    /// writes the differences into a document.
    fn save_document<T: PartialEq + Debug>(
        &self,
        relative: &str,
        new: &T,
        read: impl Fn(&Value) -> Result<T, RepositoryError>,
        update: impl Fn(&mut Value, &T, &T),
        create: impl Fn(&T) -> Value,
    ) -> Result<(), RepositoryError> {
        save_document_in(&self.root, relative, new, (read, update, create))
    }

    fn read_folder(&self, dir: &str) -> Result<Folder, RepositoryError> {
        let name = dir.rsplit('/').next().unwrap_or(dir);
        let file = format!("{dir}/{FOLDER_FILE}");
        Ok(match self.read_value(&file)? {
            Some(doc) => read::folder(&doc, name),
            // Without `folder.yml`, a folder inherits everything.
            None => Folder::new(name),
        })
    }

    fn ignore_patterns(&self) -> Result<Vec<String>, RepositoryError> {
        Ok(self
            .read_value(COLLECTION_FILE)?
            .map(|doc| read::ignore_patterns(&doc))
            .unwrap_or_default())
    }

    fn list_dir(&self, dir: &str, patterns: &[String]) -> Result<Vec<TreeItem>, RepositoryError> {
        let full = if dir.is_empty() {
            self.root.clone()
        } else {
            self.root.join(dir)
        };
        let mut entries: Vec<_> = std::fs::read_dir(&full)
            .map_err(|e| io_error(dir, &e))?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);

        let mut items = Vec::new();
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue; // Not valid UTF-8: cannot be a path in the format.
            };
            let relative = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            if fs::is_ignored(&relative, patterns) {
                continue;
            }
            // `file_type` does not follow symlinks, so linked folders cannot loop.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let mut children = self.list_dir(&relative, patterns)?;
                let folder = match self.read_folder(&relative) {
                    Ok(folder) => folder,
                    Err(e) => {
                        children.push(TreeItem::Invalid {
                            path: RequestPath::new(format!("{relative}/{FOLDER_FILE}")),
                            message: e.to_string(),
                        });
                        Folder::new(name)
                    }
                };
                items.push(TreeItem::Folder {
                    path: relative,
                    folder,
                    children,
                });
            } else if kind.is_file() && is_yaml(&name) && name != FOLDER_FILE {
                items.push(self.tree_item(&relative));
            }
        }
        Ok(items)
    }

    fn tree_item(&self, relative: &str) -> TreeItem {
        let path = RequestPath::new(relative);
        match self.read_value(relative) {
            Ok(Some(doc)) => match read::item(&doc) {
                read::Item::Http(request) => TreeItem::Request {
                    kind: request.kind(),
                    path,
                    name: request.name,
                    method: request.method,
                    seq: request.seq,
                },
                read::Item::Other { name, kind, seq } => TreeItem::Other {
                    path,
                    name,
                    kind,
                    seq,
                },
            },
            Ok(None) => TreeItem::Invalid {
                path,
                message: "the file disappeared".into(),
            },
            Err(e) => TreeItem::Invalid {
                path,
                message: e.to_string(),
            },
        }
    }

    /// Environment files with their parsed content, sorted by file name.
    /// Writes `environment` to the file at `relative`, keeping what the file
    /// has that the model does not.
    fn save_environment_at(
        &self,
        relative: &str,
        environment: &Environment,
    ) -> Result<(), RepositoryError> {
        let stem = relative
            .trim_start_matches("environments/")
            .rsplit_once('.')
            .map_or(relative, |(s, _)| s)
            .to_owned();
        self.save_document(
            relative,
            environment,
            |doc| Ok(read::environment(doc, &stem)),
            write::update_environment,
            write::new_environment,
        )
    }

    fn environment_files(&self) -> Result<Vec<(String, Environment)>, RepositoryError> {
        let dir = self.root.join(ENVIRONMENTS_DIR);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error(ENVIRONMENTS_DIR, &e)),
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| is_yaml(n))
            .collect();
        names.sort();
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let relative = format!("{ENVIRONMENTS_DIR}/{name}");
            if let Some(doc) = self.read_value(&relative)? {
                let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
                out.push((relative.clone(), read::environment(&doc, stem)));
            }
        }
        Ok(out)
    }

    /// Writes the DNS overrides into `.apifast/collection.yml`, keeping the
    /// rest of it; creates the file for a first override.
    fn save_dns_overrides(&self, overrides: &[DnsOverride]) -> Result<(), RepositoryError> {
        let exists = self.read_value(APIFAST_COLLECTION_FILE)?.is_some();
        if !exists && overrides.is_empty() {
            return Ok(());
        }
        self.save_document(
            APIFAST_COLLECTION_FILE,
            &overrides.to_vec(),
            |doc| Ok(read::dns_overrides(doc)),
            |doc, _, new| yaml::set_or_remove(doc, "dns", write::dns_value(new)),
            |new| write::apifast_with_dns(new),
        )
    }

    fn save_apifast_id(&self, id: &CollectionId) -> Result<(), RepositoryError> {
        let read_id = |doc: &Value| Ok(yaml::text(yaml::get(doc, "id")).map(CollectionId::new));
        self.save_document(
            APIFAST_COLLECTION_FILE,
            &Some(id.clone()),
            read_id,
            |doc, _, new| {
                if let Some(id) = new {
                    yaml::set(doc, "id", yaml::string(id.as_str()));
                }
            },
            |new| write::apifast_collection(new.as_ref().map_or("", CollectionId::as_str)),
        )
    }
}

impl CollectionRepository for FsCollection {
    fn tree(&self) -> Result<CollectionTree, RepositoryError> {
        Ok(CollectionTree {
            collection: self.collection()?,
            items: self.list_dir("", &self.ignore_patterns()?)?,
        })
    }

    fn collection(&self) -> Result<Collection, RepositoryError> {
        let doc = self
            .read_value(COLLECTION_FILE)?
            .ok_or_else(|| RepositoryError::NotFound(COLLECTION_FILE.into()))?;
        let mut collection = read::collection(&doc);
        let apifast = self.read_value(APIFAST_COLLECTION_FILE)?;
        collection.id = (apifast.as_ref())
            .and_then(|doc| yaml::text(yaml::get(doc, "id")))
            .filter(|id| !id.is_empty())
            .map(CollectionId::new);
        collection.dns_overrides = apifast
            .as_ref()
            .map(read::dns_overrides)
            .unwrap_or_default();
        Ok(collection)
    }

    fn request(&self, path: &RequestPath) -> Result<LoadedRequest, RepositoryError> {
        self.path(path.as_str())?;
        let doc = self
            .read_value(path.as_str())?
            .ok_or_else(|| RepositoryError::NotFound(path.to_string()))?;
        let request = http_only(path.as_str(), &doc)?;
        let parts: Vec<&str> = path.as_str().split('/').collect();
        let folders = (1..parts.len())
            .map(|depth| self.read_folder(&parts[..depth].join("/")))
            .collect::<Result<_, _>>()?;
        Ok(LoadedRequest { request, folders })
    }

    fn save_request(
        &self,
        path: &RequestPath,
        request: &HttpRequest,
    ) -> Result<(), RepositoryError> {
        self.path(path.as_str())?;
        if !is_yaml(path.as_str()) {
            return Err(RepositoryError::Invalid {
                path: path.to_string(),
                message: "request files end in .yml".into(),
            });
        }
        let relative = path.as_str();
        // A request that changes between HTTP and GraphQL keeps its block
        // where it is: a patch would add the renamed key at the end.
        let block = write::block_of(request);
        save_document_prepared(
            &self.root,
            relative,
            request,
            (
                |doc: &Value| http_only(relative, doc),
                write::update_request,
                write::new_request,
            ),
            |text| write::retype_request_text(text, block),
        )
    }

    fn save_folder(&self, path: &str, folder: &Folder) -> Result<(), RepositoryError> {
        self.path(path)?;
        let name = path.rsplit('/').next().unwrap_or(path);
        self.save_document(
            &format!("{path}/{FOLDER_FILE}"),
            folder,
            |doc| Ok(read::folder(doc, name)),
            write::update_folder,
            write::new_folder,
        )
    }

    fn save_collection(&self, collection: &Collection) -> Result<(), RepositoryError> {
        let current_id = self
            .read_value(APIFAST_COLLECTION_FILE)?
            .and_then(|doc| yaml::text(yaml::get(&doc, "id")));
        let new_id = collection.id.as_ref().map(CollectionId::as_str);
        let id_changes = new_id.is_some() && current_id.as_deref() != new_id;
        if let (true, Some(id)) = (id_changes, &collection.id) {
            self.save_apifast_id(id)?;
        }

        self.save_dns_overrides(&collection.dns_overrides)?;

        // The identifier and the DNS overrides live in `.apifast/`, not in
        // `opencollection.yml`.
        let mut without_id = collection.clone();
        without_id.id = None;
        without_id.dns_overrides = Vec::new();
        let add_ignore = collection.id.is_some() || !collection.dns_overrides.is_empty();
        self.save_document(
            COLLECTION_FILE,
            &without_id,
            |doc| Ok(read::collection(doc)),
            |doc, old, new| {
                write::update_collection(doc, old, new);
                if add_ignore {
                    write::add_ignore(doc, APIFAST_DIR);
                }
            },
            |new| {
                let mut doc = write::new_collection(new);
                if add_ignore {
                    write::add_ignore(&mut doc, APIFAST_DIR);
                }
                doc
            },
        )
    }

    fn exists(&self, path: &str) -> Result<bool, RepositoryError> {
        Ok(std::fs::symlink_metadata(self.item_path(path)?).is_ok())
    }

    fn rename_path(&self, from: &str, to: &str) -> Result<(), RepositoryError> {
        let (source, target) = (self.item_path(from)?, self.item_path(to)?);
        if std::fs::symlink_metadata(&source).is_err() {
            return Err(RepositoryError::NotFound(from.to_owned()));
        }
        // `rename` would silently replace a file at `target`.
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(RepositoryError::Invalid {
                path: to.to_owned(),
                message: "something already exists there".into(),
            });
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io_error(to, &e))?;
        }
        std::fs::rename(&source, &target).map_err(|e| io_error(from, &e))
    }

    fn delete_path(&self, path: &str) -> Result<(), RepositoryError> {
        let full = self.item_path(path)?;
        let kind = std::fs::symlink_metadata(&full)
            .map_err(|_| RepositoryError::NotFound(path.to_owned()))?
            .file_type();
        // A symlink is removed itself, never what it points to.
        if kind.is_dir() {
            std::fs::remove_dir_all(&full)
        } else {
            std::fs::remove_file(&full)
        }
        .map_err(|e| io_error(path, &e))
    }
}

impl EnvironmentRepository for FsCollection {
    fn environments(&self) -> Result<Vec<Environment>, RepositoryError> {
        Ok(self
            .environment_files()?
            .into_iter()
            .map(|(_, env)| env)
            .collect())
    }

    fn save_environment(&self, environment: &Environment) -> Result<(), RepositoryError> {
        let files = self.environment_files()?;
        let relative = match files.iter().find(|(_, e)| e.name == environment.name) {
            Some((relative, _)) => relative.clone(),
            None => free_environment_file(&files, &environment.name),
        };
        self.save_environment_at(&relative, environment)
    }

    fn rename_environment(&self, from: &str, to: &str) -> Result<(), RepositoryError> {
        let files = self.environment_files()?;
        if files.iter().any(|(_, e)| e.name == to) {
            return Err(RepositoryError::Invalid {
                path: to.to_owned(),
                message: "an environment with that name already exists".into(),
            });
        }
        let (relative, environment) = files
            .iter()
            .find(|(_, e)| e.name == from)
            .ok_or_else(|| RepositoryError::NotFound(from.to_owned()))?;
        // Bruno names the file after the environment: follow the new name.
        let others: Vec<_> = files
            .iter()
            .filter(|(r, _)| r != relative)
            .cloned()
            .collect();
        let target = free_environment_file(&others, to);
        if target != *relative {
            std::fs::rename(self.root.join(relative), self.root.join(&target))
                .map_err(|e| io_error(relative, &e))?;
        }
        let renamed = Environment {
            name: to.to_owned(),
            ..environment.clone()
        };
        self.save_environment_at(&target, &renamed)
    }

    fn delete_environment(&self, name: &str) -> Result<(), RepositoryError> {
        let files = self.environment_files()?;
        let (relative, _) = files
            .iter()
            .find(|(_, e)| e.name == name)
            .ok_or_else(|| RepositoryError::NotFound(name.to_owned()))?;
        std::fs::remove_file(self.root.join(relative)).map_err(|e| io_error(relative, &e))
    }

    fn process_env(&self) -> Result<BTreeMap<String, String>, RepositoryError> {
        let mut vars: BTreeMap<String, String> = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect();
        if let Some(text) = self.read_text(".env")? {
            vars.extend(fs::parse_dotenv(&text));
        }
        Ok(vars)
    }
}

/// A path in `environments/` for an environment named `name`, after the
/// name and not taken by any of `files`.
fn free_environment_file(files: &[(String, Environment)], name: &str) -> String {
    let base = fs::file_name_for(name);
    let taken = |candidate: &str| files.iter().any(|(r, _)| r == candidate);
    let mut candidate = format!("{ENVIRONMENTS_DIR}/{base}.yml");
    let mut n = 2;
    while taken(&candidate) {
        candidate = format!("{ENVIRONMENTS_DIR}/{base}-{n}.yml");
        n += 1;
    }
    candidate
}

/// Folder of the shared secrets, one file per environment.
const SECRETS_DIR: &str = ".apifast/secrets";

/// A file of shared secrets: its path, environment and `(name, envelope)`s.
type SecretsFile = (String, String, Vec<(String, String)>);

impl FsCollection {
    /// Every file of shared secrets.
    fn secrets_files(&self) -> Result<Vec<SecretsFile>, RepositoryError> {
        let entries = match std::fs::read_dir(self.root.join(SECRETS_DIR)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error(SECRETS_DIR, &e)),
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| is_yaml(n))
            .collect();
        names.sort();
        let mut out = Vec::new();
        for name in names {
            let relative = format!("{SECRETS_DIR}/{name}");
            let Some(doc) = self.read_value(&relative)? else {
                continue;
            };
            let Some(environment) = yaml::text(yaml::get(&doc, "environment")) else {
                continue;
            };
            let secrets = yaml::items(yaml::get(&doc, "secrets"))
                .iter()
                .filter_map(|item| {
                    Some((
                        yaml::text(yaml::get(item, "name"))?,
                        yaml::text(yaml::get(item, "value"))?,
                    ))
                })
                .collect();
            out.push((relative, environment, secrets));
        }
        Ok(out)
    }

    fn write_secrets_file(
        &self,
        relative: &str,
        environment: &str,
        secrets: &[(String, String)],
    ) -> Result<(), RepositoryError> {
        let path = self.root.join(relative);
        if secrets.is_empty() {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(io_error(relative, &e)),
            };
        }
        let items: Vec<Value> = secrets
            .iter()
            .map(|(name, value)| {
                yaml::mapping([
                    ("name", Some(yaml::string(name))),
                    ("value", Some(yaml::string(value))),
                ])
            })
            .collect();
        let doc = yaml::mapping([
            ("environment", Some(yaml::string(environment))),
            ("secrets", Some(Value::from(items))),
        ]);
        let text = write::to_text(&doc).map_err(|e| invalid(relative, &e))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io_error(relative, &e))?;
        }
        fs::write_atomic(&path, text.as_bytes()).map_err(|e| io_error(relative, &e))
    }
}

impl SharedSecretFiles for FsCollection {
    fn shared_secret(
        &self,
        environment: &str,
        name: &str,
    ) -> Result<Option<String>, RepositoryError> {
        Ok(self
            .secrets_files()?
            .into_iter()
            .filter(|(_, env, _)| env == environment)
            .flat_map(|(_, _, secrets)| secrets)
            .find(|(n, _)| n == name)
            .map(|(_, value)| value))
    }

    fn set_shared_secret(
        &self,
        environment: &str,
        name: &str,
        envelope: &str,
    ) -> Result<(), RepositoryError> {
        let files = self.secrets_files()?;
        let (relative, mut secrets) = if let Some((relative, _, secrets)) =
            files.iter().find(|(_, env, _)| env == environment)
        {
            (relative.clone(), secrets.clone())
        } else {
            (free_secrets_file(&files, environment), Vec::new())
        };
        match secrets.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => envelope.clone_into(&mut entry.1),
            None => secrets.push((name.to_owned(), envelope.to_owned())),
        }
        self.write_secrets_file(&relative, environment, &secrets)
    }

    fn remove_shared_secret(&self, environment: &str, name: &str) -> Result<bool, RepositoryError> {
        for (relative, env, mut secrets) in self.secrets_files()? {
            if env != environment {
                continue;
            }
            let before = secrets.len();
            secrets.retain(|(n, _)| n != name);
            if secrets.len() != before {
                self.write_secrets_file(&relative, &env, &secrets)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn shared_secret_entries(&self) -> Result<Vec<(String, String)>, RepositoryError> {
        Ok(self
            .secrets_files()?
            .into_iter()
            .flat_map(|(_, env, secrets)| {
                secrets
                    .into_iter()
                    .map(move |(name, _)| (env.clone(), name))
            })
            .collect())
    }

    /// The vault's challenge in `.apifast/collection.yml`, if the
    /// collection has shared secrets.
    ///
    /// # Errors
    ///
    /// If the file exists but cannot be read.
    fn vault_challenge(&self) -> Result<Option<String>, RepositoryError> {
        Ok(self
            .read_value(APIFAST_COLLECTION_FILE)?
            .and_then(|doc| yaml::text(yaml::get_in(&doc, &["vault", "challenge"])))
            .filter(|challenge| !challenge.is_empty()))
    }

    /// Writes the vault's challenge, keeping the rest of
    /// `.apifast/collection.yml`.
    ///
    /// # Errors
    ///
    /// `NotFound` if the collection has no id yet (see [`Self::ensure_id`]),
    /// or if the file cannot be written.
    fn set_vault_challenge(&self, challenge: &str) -> Result<(), RepositoryError> {
        if self.collection()?.id.is_none() {
            return Err(RepositoryError::NotFound(APIFAST_COLLECTION_FILE.into()));
        }
        let read = |doc: &Value| {
            Ok(yaml::text(yaml::get_in(doc, &["vault", "challenge"])).unwrap_or_default())
        };
        self.save_document(
            APIFAST_COLLECTION_FILE,
            &challenge.to_owned(),
            read,
            |doc, _, new| {
                yaml::set(
                    yaml::child_mut(doc, "vault"),
                    "challenge",
                    yaml::string(new),
                );
            },
            // The file exists: the id is in it.
            |_| Value::Null,
        )
    }
}

/// A path in `.apifast/secrets/` for `environment`, after its name and not
/// taken by any of `files`.
fn free_secrets_file(files: &[SecretsFile], environment: &str) -> String {
    let base = fs::file_name_for(environment);
    let taken = |candidate: &str| files.iter().any(|(r, _, _)| r == candidate);
    let mut candidate = format!("{SECRETS_DIR}/{base}.yml");
    let mut n = 2;
    while taken(&candidate) {
        candidate = format!("{SECRETS_DIR}/{base}-{n}.yml");
        n += 1;
    }
    candidate
}

fn http_only(relative: &str, doc: &Value) -> Result<HttpRequest, RepositoryError> {
    match read::item(doc) {
        read::Item::Http(request) => Ok(request),
        read::Item::Other { kind, .. } => Err(RepositoryError::Unsupported {
            path: relative.to_owned(),
            what: format!("`{kind}` requests"),
        }),
    }
}

fn is_yaml(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
}

/// The text of the file at `relative` inside `root`; `None` if missing.
pub(crate) fn read_text_in(root: &Path, relative: &str) -> Result<Option<String>, RepositoryError> {
    match std::fs::read_to_string(root.join(relative)) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(relative, &e)),
    }
}

/// Saves `new` into the file at `relative` inside `root` (trusted), as
/// described in the crate docs. `read` turns a document into the domain value and `update`
/// writes the differences into a document.
pub(crate) fn save_document_in<T: PartialEq + Debug>(
    root: &Path,
    relative: &str,
    new: &T,
    functions: (
        impl Fn(&Value) -> Result<T, RepositoryError>,
        impl Fn(&mut Value, &T, &T),
        impl Fn(&T) -> Value,
    ),
) -> Result<(), RepositoryError> {
    save_document_prepared(root, relative, new, functions, |text| text)
}

/// [`save_document_in`], with the file's text first passed through
/// `prepare` (in memory: the file is only written at the end, checked).
fn save_document_prepared<T: PartialEq + Debug>(
    root: &Path,
    relative: &str,
    new: &T,
    (read, update, create): (
        impl Fn(&Value) -> Result<T, RepositoryError>,
        impl Fn(&mut Value, &T, &T),
        impl Fn(&T) -> Value,
    ),
    prepare: impl Fn(String) -> String,
) -> Result<(), RepositoryError> {
    let path = root.join(relative);
    let original = read_text_in(root, relative)?.map(prepare);
    let (text, expected) = if let Some(original_text) = original {
        let original = parse(relative, &original_text)?;
        let old = read(&original)?;
        let mut updated = original.clone();
        update(&mut updated, &old, new);
        let ops = diff::diff(&original, &updated);
        if ops.is_empty() {
            return if old == *new {
                Ok(())
            } else {
                Err(unrepresentable(relative))
            };
        }
        let mut doc = oneyaml::Document::parse_with(original_text.as_str(), &write::settings())
            .map_err(|e| invalid(relative, &e))?;
        doc.apply_patch(&oneyaml::Patch::new(ops))
            .map_err(|e| RepositoryError::Invalid {
                path: relative.to_owned(),
                message: format!("cannot apply the change: {e}"),
            })?;
        (doc.to_string(), updated)
    } else {
        let value = create(new);
        let text = write::to_text(&value).map_err(|e| invalid(relative, &e))?;
        (text, value)
    };

    // Never write something that does not read back as what was saved.
    let written = parse(relative, &text)?;
    if written != expected || read(&written)? != *new {
        return Err(unrepresentable(relative));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io_error(relative, &e))?;
    }
    fs::write_atomic(&path, text.as_bytes()).map_err(|e| io_error(relative, &e))
}

fn parse(relative: &str, text: &str) -> Result<Value, RepositoryError> {
    oneyaml::from_str(text).map_err(|e| invalid(relative, &e))
}

fn invalid(relative: &str, error: &dyn std::fmt::Display) -> RepositoryError {
    RepositoryError::Invalid {
        path: relative.to_owned(),
        message: error.to_string(),
    }
}

fn io_error(relative: &str, error: &std::io::Error) -> RepositoryError {
    RepositoryError::Io {
        path: relative.to_owned(),
        message: error.to_string(),
    }
}

fn unrepresentable(relative: &str) -> RepositoryError {
    RepositoryError::Invalid {
        path: relative.to_owned(),
        message: "the change cannot be written to this file without losing data".into(),
    }
}

/// `col_` and ten base-62 characters.
fn new_collection_id(random: &dyn Random) -> CollectionId {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut n = random.next_u64();
    let mut id = String::from("col_");
    for _ in 0..10 {
        // 62^10 < 2^64, so one random number is enough.
        let index = usize::try_from(n % 62).unwrap_or(0);
        id.push(char::from(ALPHABET[index]));
        n /= 62;
    }
    CollectionId::new(id)
}

/// What the last import wrote, in `.apifast/import.yml`, versioned with the
/// collection so anyone can import the source again.
const IMPORT_FILE: &str = ".apifast/import.yml";

impl apifast_app::ImportRecords for FsCollection {
    fn import_record(&self) -> Result<Option<apifast_app::ImportRecord>, RepositoryError> {
        Ok(self
            .read_value(IMPORT_FILE)?
            .map(|doc| apifast_app::ImportRecord {
                source: yaml::text_or_empty(yaml::get(&doc, "source")),
                requests: yaml::items(yaml::get(&doc, "requests"))
                    .iter()
                    .map(|r| apifast_app::RecordedRequest {
                        key: yaml::text_or_empty(yaml::get(r, "key")),
                        path: yaml::text_or_empty(yaml::get(r, "path")),
                        hash: yaml::text_or_empty(yaml::get(r, "hash")),
                    })
                    .collect(),
            }))
    }

    fn save_import_record(
        &self,
        record: &apifast_app::ImportRecord,
    ) -> Result<(), RepositoryError> {
        let requests: Vec<Value> = (record.requests.iter())
            .map(|r| {
                yaml::mapping([
                    ("key", Some(yaml::string(r.key.clone()))),
                    ("path", Some(yaml::string(r.path.clone()))),
                    ("hash", Some(yaml::string(r.hash.clone()))),
                ])
            })
            .collect();
        let doc = yaml::mapping([
            ("source", Some(yaml::string(record.source.clone()))),
            ("requests", Some(Value::from(requests))),
        ]);
        // Like every file of `.apifast/`, hidden from Bruno.
        if let Some(mut collection) = self.read_value(COLLECTION_FILE)? {
            let before = collection.clone();
            write::add_ignore(&mut collection, APIFAST_DIR);
            if collection != before {
                let text = write::to_text(&collection).map_err(|e| invalid(COLLECTION_FILE, &e))?;
                fs::write_atomic(&self.root.join(COLLECTION_FILE), text.as_bytes())
                    .map_err(|e| io_error(COLLECTION_FILE, &e))?;
            }
        }
        let text = write::to_text(&doc).map_err(|e| invalid(IMPORT_FILE, &e))?;
        let path = self.root.join(IMPORT_FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io_error(IMPORT_FILE, &e))?;
        }
        fs::write_atomic(&path, text.as_bytes()).map_err(|e| io_error(IMPORT_FILE, &e))
    }
}
