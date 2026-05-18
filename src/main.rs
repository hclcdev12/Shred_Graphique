//! Application graphique de suppression sécurisée de disques
//!
//! Cette application utilise GTK4 pour fournir une interface graphique
//! permettant d'effacer de manière sécurisée des disques durs via la commande shred.

mod disk;
mod paths;
mod shred;
mod system;
mod ui;
mod logger;

use ui::ShredApp;

fn main() {
    // Vérifier que les outils nécessaires sont disponibles
    if !shred::check_shred_available() {
        eprintln!("❌ ERREUR: La commande 'shred' n'est pas disponible sur ce système.");
        eprintln!("Veuillez installer le paquet 'coreutils' qui contient shred.");
        std::process::exit(1);
    }

    if !shred::check_pkexec_available() {
        eprintln!("❌ ERREUR: La commande 'pkexec' n'est pas disponible sur ce système.");
        eprintln!("Veuillez installer le paquet 'policykit-1'.");
        std::process::exit(1);
    }

    // Lancer l'application GTK
    println!("🚀 Lancement de l'interface graphique...");
    let app = ShredApp::new();
    app.run();
}
