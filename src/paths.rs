/// Chemins configurables pour l'environnement d'exécution.
pub fn helper_path() -> String {
    std::env::var("SHRED_GRAPHIQUE_HELPER")
        .unwrap_or_else(|_| "/usr/bin/shred-graphique-helper".to_string())
}
