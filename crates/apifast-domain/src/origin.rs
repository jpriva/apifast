/// Where a resolved value comes from.
///
/// The UI uses it to show the source of each value, flag when one value
/// overrides another and jump to the file that defines it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Origin {
    /// apifast's built-in default: nobody defined it.
    Default,
    /// The workspace's global environment.
    Global,
    /// `opencollection.yml`.
    Collection,
    /// The environment with this name. With `extends`, the one that defines the value.
    Environment(String),
    /// A folder, by its position in [`Chain::folders`](crate::Chain): 0 is the outermost.
    Folder(usize),
    /// The request file.
    Request,
    /// Captures from previous responses, in memory.
    Runtime,
    /// A value the user typed in for this send.
    Prompt,
}
