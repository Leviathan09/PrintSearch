//! Intègre l'icône dans l'exécutable Windows (.exe). Sans effet sur les autres systèmes.
//! `CARGO_CFG_TARGET_OS` désigne le système *cible* : ça marche aussi en compilation croisée depuis Linux
//! (il faut alors `windres`, fourni par le paquet mingw-w64). Si l'intégration échoue, la compilation continue
//! simplement sans icône.
fn main() {
    println!("cargo:rerun-if-changed=packaging/printsearch.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/printsearch.ico");
        res.set("ProductName", "PrintSearch");
        res.set("FileDescription", "PrintSearch - 3D model search");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon not embedded in the .exe: {e}");
        }
    }
}
