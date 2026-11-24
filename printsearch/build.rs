//! 1. Intègre l'icône dans l'exécutable Windows (.exe). Sans effet sur les autres systèmes.
//!    `CARGO_CFG_TARGET_OS` désigne le système *cible* : ça marche aussi en compilation croisée depuis Linux
//!    (il faut alors `windres`, fourni par le paquet mingw-w64). Si l'intégration échoue, la compilation continue
//!    simplement sans icône.
//! 2. Active `has_tray` quand la barre d'état est disponible (fonction « tray » + Windows ou Linux).
fn main() {
    println!("cargo:rerun-if-changed=packaging/printsearch.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(has_tray)");

    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if std::env::var_os("CARGO_FEATURE_TRAY").is_some() && (os == "windows" || os == "linux") {
        println!("cargo:rustc-cfg=has_tray");
    }

    if os == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/printsearch.ico");
        res.set("ProductName", "PrintSearch");
        res.set("FileDescription", "PrintSearch - 3D model search");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon not embedded in the .exe: {e}");
        }
    }
}
