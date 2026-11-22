//! Use cases of the sidebar: create, rename, move and delete requests and
//! folders, and keep their order (`seq`).

use crate::Invalid;
use apifast_domain::{Folder, HttpRequest, Method};

use crate::{AppError, CollectionRepository, ItemPath, RequestPath, TreeItem, open_collection};

/// Where [`move_item`] puts an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Just before this item, in its folder.
    Before(ItemPath),
    /// Just after this item, in its folder.
    After(ItemPath),
    /// At the end of this folder (`""` for the root).
    Inside(String),
}

/// A file or folder name for `name`: lowercase letters and digits, with
/// anything else turned into single dashes. `fallback` if nothing is left.
pub fn slug(name: &str, fallback: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Creates an empty `GET` request called `name` at the end of `folder`
/// (`""` for the root). Its file name is the [`slug`] of `name`, with `-2`,
/// `-3`... if taken. If some items of the folder have no `seq`, they are
/// numbered too, since they would otherwise sort after the new one.
///
/// # Errors
///
/// [`AppError::Validation`] if `name` is blank, the repository error if
/// `folder` does not exist or the file cannot be written.
pub fn create_request(
    repo: &impl CollectionRepository,
    folder: &str,
    name: &str,
) -> Result<RequestPath, AppError> {
    let name = valid_name(name)?;
    let tree = open_collection(repo)?;
    let siblings = children(&tree.items, folder)?;
    let path = RequestPath::new(free_path(repo, folder, &slug(name, "request"), ".yml")?);
    let mut request = HttpRequest::new(name, Method::Get, "");
    request.seq = Some(next_seq(siblings));
    let unordered = has_unordered(siblings);
    repo.save_request(&path, &request)?;
    let item = ItemPath::Request(path.clone());
    if unordered {
        move_item(repo, &item, &Placement::Inside(folder.to_owned()))?;
    }
    Ok(path)
}

/// Creates a folder called `name` at the end of `parent` (`""` for the
/// root), named on disk like [`create_request`] names files.
///
/// # Errors
///
/// As [`create_request`].
pub fn create_folder(
    repo: &impl CollectionRepository,
    parent: &str,
    name: &str,
) -> Result<String, AppError> {
    let name = valid_name(name)?;
    let tree = open_collection(repo)?;
    let siblings = children(&tree.items, parent)?;
    let path = free_path(repo, parent, &slug(name, "folder"), "")?;
    let mut folder = Folder::new(name);
    folder.seq = Some(next_seq(siblings));
    let unordered = has_unordered(siblings);
    repo.save_folder(&path, &folder)?;
    if unordered {
        move_item(
            repo,
            &ItemPath::Folder(path.clone()),
            &Placement::Inside(parent.to_owned()),
        )?;
    }
    Ok(path)
}

/// Renames an item: its `info.name`, and its file or folder to the [`slug`]
/// of the new name. Returns where it is now.
///
/// # Errors
///
/// [`AppError::Validation`] if `name` is blank; the repository error if the
/// item cannot be read or written, such as a GraphQL request.
pub fn rename_item(
    repo: &impl CollectionRepository,
    item: &ItemPath,
    name: &str,
) -> Result<ItemPath, AppError> {
    let name = valid_name(name)?;
    let parent = item.parent();
    match item {
        ItemPath::Request(path) => {
            let mut request = repo.request(path)?.request;
            if request.name != name {
                name.clone_into(&mut request.name);
                repo.save_request(path, &request)?;
            }
            let base = slug(name, "request");
            if item.file_name() == format!("{base}.yml") {
                return Ok(item.clone());
            }
            let to = free_path(repo, parent, &base, ".yml")?;
            repo.rename_path(path.as_str(), &to)?;
            Ok(ItemPath::Request(RequestPath::new(to)))
        }
        ItemPath::Folder(path) => {
            let tree = open_collection(repo)?;
            let mut folder = find_folder(&tree.items, path)?;
            if folder.name != name {
                name.clone_into(&mut folder.name);
                repo.save_folder(path, &folder)?;
            }
            let base = slug(name, "folder");
            if item.file_name() == base {
                return Ok(item.clone());
            }
            let to = free_path(repo, parent, &base, "")?;
            repo.rename_path(path, &to)?;
            Ok(ItemPath::Folder(to))
        }
    }
}

/// Deletes an item, with everything inside it if it is a folder.
///
/// # Errors
///
/// The repository error if it does not exist or cannot be deleted.
pub fn delete_item(repo: &impl CollectionRepository, item: &ItemPath) -> Result<(), AppError> {
    Ok(repo.delete_path(item.as_str())?)
}

/// Moves an item to `placement`, into another folder if needed, and
/// renumbers the `seq` of that folder's items to keep the new order. Only
/// files whose `seq` changes are written. Returns where the item is now.
///
/// Items that cannot be rewritten (requests of other kinds and files that
/// cannot be read) keep their `seq`.
///
/// # Errors
///
/// [`AppError::Validation`] if a folder would go inside itself or the target
/// folder already holds an item with the same file name; the repository
/// error if something cannot be read or written.
pub fn move_item(
    repo: &impl CollectionRepository,
    item: &ItemPath,
    placement: &Placement,
) -> Result<ItemPath, AppError> {
    let folder = match placement {
        Placement::Before(other) | Placement::After(other) => other.parent(),
        Placement::Inside(folder) => folder.as_str(),
    };
    if matches!(item, ItemPath::Folder(_)) && crate::ports::is_within(folder, item.as_str()) {
        return Err(Invalid::FolderIntoItself.into());
    }
    let tree = open_collection(repo)?;
    let moved = find_tree_item(&tree.items, item)
        .cloned()
        .ok_or_else(|| not_found(item.as_str()))?;
    let siblings = children(&tree.items, folder)?;

    let new_path = if item.parent() == folder {
        item.clone()
    } else {
        let to = join(folder, item.file_name());
        if repo.exists(&to)? {
            return Err(Invalid::ExistsInFolder(item.file_name().to_owned()).into());
        }
        repo.rename_path(item.as_str(), &to)?;
        match item {
            ItemPath::Request(_) => ItemPath::Request(RequestPath::new(to)),
            ItemPath::Folder(_) => ItemPath::Folder(to),
        }
    };

    // The folder's items in their new order, each with how it was listed
    // (for its kind and current `seq`).
    let mut order: Vec<(ItemPath, &TreeItem)> = siblings
        .iter()
        .filter(|sibling| item_path(sibling) != *item)
        .map(|sibling| (item_path(sibling), sibling))
        .collect();
    let index = match placement {
        Placement::Inside(_) => order.len(),
        Placement::Before(other) | Placement::After(other) => {
            let at = order
                .iter()
                .position(|(path, _)| path == other)
                .unwrap_or(order.len());
            if matches!(placement, Placement::After(_)) {
                (at + 1).min(order.len())
            } else {
                at
            }
        }
    };
    order.insert(index, (new_path.clone(), &moved));

    for (position, (path, listed)) in order.iter().enumerate() {
        let seq = u32::try_from(position + 1).unwrap_or(u32::MAX);
        write_seq(repo, path, listed, seq)?;
    }
    Ok(new_path)
}

/// Gives the item now at `path` this `seq`, if it can and it changes.
/// `listed` is the item as listed before the move.
fn write_seq(
    repo: &impl CollectionRepository,
    path: &ItemPath,
    listed: &TreeItem,
    seq: u32,
) -> Result<(), AppError> {
    match (path, listed) {
        (ItemPath::Request(path), TreeItem::Request { seq: old, .. }) => {
            if *old != Some(seq) {
                let mut request = repo.request(path)?.request;
                request.seq = Some(seq);
                repo.save_request(path, &request)?;
            }
        }
        (ItemPath::Folder(path), TreeItem::Folder { folder, .. }) if folder.seq != Some(seq) => {
            let mut folder = folder.clone();
            folder.seq = Some(seq);
            repo.save_folder(path, &folder)?;
        }
        // Other kinds and unreadable files cannot be rewritten.
        _ => {}
    }
    Ok(())
}

fn valid_name(name: &str) -> Result<&str, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Invalid::NameEmpty.into());
    }
    Ok(name)
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// `folder/base{ext}`, or `folder/base-2{ext}`, `-3`... if taken.
fn free_path(
    repo: &impl CollectionRepository,
    folder: &str,
    base: &str,
    ext: &str,
) -> Result<String, AppError> {
    let mut candidate = join(folder, &format!("{base}{ext}"));
    let mut n = 2;
    while repo.exists(&candidate)? {
        candidate = join(folder, &format!("{base}-{n}{ext}"));
        n += 1;
    }
    Ok(candidate)
}

fn next_seq(siblings: &[TreeItem]) -> u32 {
    siblings
        .iter()
        .filter_map(item_seq)
        .max()
        .map_or(1, |max| max.saturating_add(1))
}

/// Whether an item that can be rewritten has no `seq`. Such items sort
/// after every numbered one, so a new item only goes last by numbering
/// them all.
fn has_unordered(siblings: &[TreeItem]) -> bool {
    siblings.iter().any(|item| {
        matches!(item, TreeItem::Folder { .. } | TreeItem::Request { .. })
            && item_seq(item).is_none()
    })
}

fn item_seq(item: &TreeItem) -> Option<u32> {
    match item {
        TreeItem::Folder { folder, .. } => folder.seq,
        TreeItem::Request { seq, .. } | TreeItem::Other { seq, .. } => *seq,
        TreeItem::Invalid { .. } => None,
    }
}

fn item_path(item: &TreeItem) -> ItemPath {
    match item {
        TreeItem::Folder { path, .. } => ItemPath::Folder(path.clone()),
        TreeItem::Request { path, .. }
        | TreeItem::Other { path, .. }
        | TreeItem::Invalid { path, .. } => ItemPath::Request(path.clone()),
    }
}

/// The items of `folder` (`""` for the root).
fn children<'a>(items: &'a [TreeItem], folder: &str) -> Result<&'a [TreeItem], AppError> {
    if folder.is_empty() {
        return Ok(items);
    }
    find_tree_item(items, &ItemPath::Folder(folder.to_owned()))
        .and_then(|item| match item {
            TreeItem::Folder { children, .. } => Some(children.as_slice()),
            _ => None,
        })
        .ok_or_else(|| not_found(folder))
}

fn find_folder(items: &[TreeItem], path: &str) -> Result<Folder, AppError> {
    match find_tree_item(items, &ItemPath::Folder(path.to_owned())) {
        Some(TreeItem::Folder { folder, .. }) => Ok(folder.clone()),
        _ => Err(not_found(path)),
    }
}

fn find_tree_item<'a>(items: &'a [TreeItem], path: &ItemPath) -> Option<&'a TreeItem> {
    for item in items {
        if item_path(item) == *path {
            return Some(item);
        }
        if let TreeItem::Folder {
            path: folder,
            children,
            ..
        } = item
            && crate::ports::is_within(path.as_str(), folder)
        {
            return find_tree_item(children, path);
        }
    }
    None
}

fn not_found(path: &str) -> AppError {
    AppError::Repository(crate::RepositoryError::NotFound(path.to_owned()))
}
