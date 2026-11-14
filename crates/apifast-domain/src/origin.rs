/// De dónde sale un valor resuelto.
///
/// La interfaz lo usa para mostrar el origen de cada valor, indicar cuándo uno
/// sobrescribe a otro y saltar al archivo que lo define.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Origin {
    /// Valor por defecto de apifast: nadie lo definió.
    Default,
    /// Entorno global del workspace.
    Global,
    /// `opencollection.yml`.
    Collection,
    /// Entorno con ese nombre. Con `extends`, es el entorno que define el valor.
    Environment(String),
    /// Carpeta, por su posición en [`Chain::folders`](crate::Chain): 0 es la más externa.
    Folder(usize),
    /// El archivo de la request.
    Request,
    /// Capturas de respuestas anteriores, en memoria.
    Runtime,
    /// Valor introducido por el usuario para este envío.
    Prompt,
}
