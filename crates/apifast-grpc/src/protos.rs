//! `.proto` files (docs/07, F8), compiled by protox: no external `protoc`.
//! Imports are looked for next to the file, then from the collection's
//! folder; Google's well-known types are built in.

use std::path::Path;

use apifast_app::TransportError;
use prost_reflect::DescriptorPool;

/// The descriptors of the `.proto` file at `proto`, relative to `root`
/// (the collection's folder), with everything it imports.
///
/// # Errors
///
/// If the file cannot be read, or it or an import does not compile; the
/// message says where.
pub(crate) fn compile(root: &Path, proto: &str) -> Result<DescriptorPool, TransportError> {
    let path = root.join(proto.trim());
    if !path.is_file() {
        return Err(TransportError::InvalidRequest(format!(
            "the .proto file `{}` does not exist",
            path.display()
        )));
    }
    let dir = path.parent().unwrap_or(root);
    let invalid = |e: protox::Error| {
        TransportError::InvalidRequest(format!("the .proto file `{}`: {e}", path.display()))
    };
    let mut compiler = protox::Compiler::new([dir, root]).map_err(invalid)?;
    compiler.include_imports(true);
    compiler.open_file(&path).map_err(invalid)?;
    Ok(compiler.descriptor_pool())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proto_compiles_with_its_imports_and_errors_say_where() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = dir.path();
        std::fs::create_dir_all(root.join("protos/common")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            root.join("protos/common/types.proto"),
            "syntax = \"proto3\";\npackage common;\nmessage Name { string value = 1; }\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            root.join("protos/hello.proto"),
            "syntax = \"proto3\";\npackage hello;\nimport \"common/types.proto\";\nimport \"google/protobuf/empty.proto\";\nservice Greeter { rpc Hi(common.Name) returns (google.protobuf.Empty); }\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let pool = compile(root, "protos/hello.proto").unwrap_or_else(|e| panic!("{e}"));
        assert!(pool.get_service_by_name("hello.Greeter").is_some());

        std::fs::write(root.join("bad.proto"), "syntax = \"proto3\";\nmessage {")
            .unwrap_or_else(|e| panic!("{e}"));
        let error = compile(root, "bad.proto").unwrap_err().to_string();
        assert!(error.contains("bad.proto"), "{error}");
        let error = compile(root, "nope.proto").unwrap_err().to_string();
        assert!(error.contains("does not exist"), "{error}");
    }
}
