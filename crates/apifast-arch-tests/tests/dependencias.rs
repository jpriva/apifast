//! Reglas de dependencia de `docs/03-arquitectura.md`, comprobadas con `cargo metadata`.

use std::collections::{BTreeMap, BTreeSet};

use cargo_metadata::{DependencyKind, MetadataCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capa {
    Dominio,
    Aplicacion,
    Adaptador,
    Driver,
    Herramienta,
}

/// Capa de cada crate del workspace. Un crate nuevo sin clasificar hace fallar el test.
fn capa(nombre: &str) -> Option<Capa> {
    match nombre {
        "apifast-domain" | "apifast-template" => Some(Capa::Dominio),
        "apifast-app" => Some(Capa::Aplicacion),
        "apifast-storage-fs" | "apifast-http" | "apifast-vault" | "apifast-watch"
        | "apifast-history" | "apifast-import" | "apifast-ws" | "apifast-grpc"
        | "apifast-script-js" => Some(Capa::Adaptador),
        "apifast-gui" | "apifast-cli" => Some(Capa::Driver),
        "apifast-arch-tests" => Some(Capa::Herramienta),
        _ => None,
    }
}

/// Crates externos que el dominio no puede usar (regla 1).
const PROHIBIDAS_EN_DOMINIO: &[&str] = &[
    "tokio", "serde", "reqwest", "egui", "eframe", "wgpu", "oneyaml",
];

/// Dependencias normales y de compilación de cada crate del workspace.
/// Las de desarrollo no cuentan: un test puede usar lo que necesite.
#[expect(
    clippy::expect_used,
    reason = "sin metadata el test no puede comprobar nada"
)]
fn grafo() -> BTreeMap<String, BTreeSet<String>> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .expect("cargo metadata falló");
    metadata
        .workspace_packages()
        .into_iter()
        .map(|p| {
            let deps = p
                .dependencies
                .iter()
                .filter(|d| d.kind != DependencyKind::Development)
                .map(|d| d.name.clone())
                .collect();
            (p.name.to_string(), deps)
        })
        .collect()
}

/// Comprueba si `origen` puede depender del crate del workspace `destino`.
fn permitido(origen: &str, a: Capa, destino: &str, b: Capa) -> bool {
    match a {
        // El dominio no depende de nada, salvo template, que puede usar domain.
        Capa::Dominio => origen == "apifast-template" && destino == "apifast-domain",
        Capa::Aplicacion => b == Capa::Dominio,
        Capa::Adaptador => matches!(b, Capa::Dominio | Capa::Aplicacion),
        Capa::Driver => b != Capa::Driver && b != Capa::Herramienta,
        Capa::Herramienta => false,
    }
}

#[test]
fn todos_los_crates_tienen_capa() {
    let sin_capa: Vec<_> = grafo().into_keys().filter(|n| capa(n).is_none()).collect();
    assert!(
        sin_capa.is_empty(),
        "crates sin capa en tests/dependencias.rs: {sin_capa:?}"
    );
}

#[test]
fn las_dependencias_respetan_las_capas() {
    let grafo = grafo();
    let mut violaciones = Vec::new();
    for (origen, deps) in &grafo {
        let Some(a) = capa(origen) else { continue };
        for destino in deps.iter().filter(|d| grafo.contains_key(*d)) {
            let Some(b) = capa(destino) else { continue };
            if !permitido(origen, a, destino, b) {
                violaciones.push(format!("{origen} ({a:?}) -> {destino} ({b:?})"));
            }
        }
    }
    assert!(
        violaciones.is_empty(),
        "dependencias prohibidas:\n{}",
        violaciones.join("\n")
    );
}

#[test]
fn el_dominio_no_usa_entrada_salida_ni_serializacion() {
    let mut violaciones = Vec::new();
    for (origen, deps) in grafo() {
        if capa(&origen) != Some(Capa::Dominio) {
            continue;
        }
        for dep in deps
            .iter()
            .filter(|d| PROHIBIDAS_EN_DOMINIO.contains(&d.as_str()))
        {
            violaciones.push(format!("{origen} -> {dep}"));
        }
    }
    assert!(
        violaciones.is_empty(),
        "el dominio usa crates prohibidos: {violaciones:?}"
    );
}

#[test]
fn las_reglas_detectan_violaciones() {
    use Capa::{Adaptador, Aplicacion, Dominio, Driver};
    assert!(!permitido(
        "apifast-domain",
        Dominio,
        "apifast-template",
        Dominio
    ));
    assert!(!permitido(
        "apifast-app",
        Aplicacion,
        "apifast-http",
        Adaptador
    ));
    assert!(!permitido(
        "apifast-http",
        Adaptador,
        "apifast-storage-fs",
        Adaptador
    ));
    assert!(!permitido("apifast-http", Adaptador, "apifast-cli", Driver));
    assert!(permitido("apifast-cli", Driver, "apifast-http", Adaptador));
    assert!(permitido(
        "apifast-template",
        Dominio,
        "apifast-domain",
        Dominio
    ));
}
