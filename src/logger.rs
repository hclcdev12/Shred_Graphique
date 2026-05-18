/// Module pour le logging des opérations
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;

#[derive(Debug, Serialize, Deserialize)]
struct LogEntry {
    timestamp: String,
    operation: String,
    disks: Vec<String>,
    results: Vec<DiskResult>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiskResult {
    disk_name: String,
    success: bool,
    message: String,
}

/// Chemin du fichier de log
fn get_log_file_path() -> PathBuf {
    let mut path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(".shred_graphique_logs");
    
    // Créer le dossier s'il n'existe pas
    if !path.exists() {
        let _ = fs::create_dir_all(&path);
    }
    
    let log_filename = format!("shred_log_{}.json", Local::now().format("%Y%m%d_%H%M%S"));
    path.push(log_filename);
    path
}

/// Log global pour une session (thread-safe)
static CURRENT_LOG: OnceLock<Mutex<Option<LogEntry>>> = OnceLock::new();

fn get_current_log() -> &'static Mutex<Option<LogEntry>> {
    CURRENT_LOG.get_or_init(|| Mutex::new(None))
}

/// Initialise une nouvelle entrée de log au début de l'opération
pub fn log_operation_start(disks: &[String]) {
    let entry = LogEntry {
        timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        operation: "Secure disk wiping".to_string(),
        disks: disks.to_vec(),
        results: Vec::new(),
    };

    if let Ok(mut log) = get_current_log().lock() {
        *log = Some(entry);
    }
}

/// Ajoute le résultat d'un disque au log
pub fn log_disk_result(disk_name: &str, success: bool, message: &str) {
    if let Ok(mut current) = get_current_log().lock() {
        if let Some(ref mut log) = *current {
            log.results.push(DiskResult {
                disk_name: disk_name.to_string(),
                success,
                message: message.to_string(),
            });
        }
    }
}

/// Finalise et écrit le log dans un fichier
pub fn log_operation_end() {
    if let Ok(mut current) = get_current_log().lock() {
        if let Some(log) = current.take() {
            let log_path = get_log_file_path();
            
            match OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&log_path)
            {
                Ok(mut file) => {
                    match serde_json::to_string_pretty(&log) {
                        Ok(json) => {
                            if let Err(e) = file.write_all(json.as_bytes()) {
                                eprintln!("Erreur d'écriture du log: {}", e);
                            } else {
                                println!("Log enregistré dans: {:?}", log_path);
                                
                                // Écrire aussi dans un fichier texte lisible
                                write_human_readable_log(&log, &log_path);
                            }
                        }
                        Err(e) => eprintln!("Erreur de sérialisation du log: {}", e),
                    }
                }
                Err(e) => eprintln!("Impossible de créer le fichier de log: {}", e),
            }
        }
    }
}

/// Écrit un log lisible par un humain
fn write_human_readable_log(log: &LogEntry, json_path: &Path) {
    let mut txt_path = json_path.to_path_buf();
    txt_path.set_extension("txt");
    
    let mut content = String::new();
    content.push_str("═══════════════════════════════════════════════════════════════\n");
    content.push_str("          JOURNAL DE SUPPRESSION SÉCURISÉE DE DISQUES\n");
    content.push_str("═══════════════════════════════════════════════════════════════\n\n");
    
    content.push_str(&format!("Date et heure : {}\n", log.timestamp));
    content.push_str(&format!("Opération     : {}\n\n", log.operation));
    
    content.push_str("Disques traités :\n");
    for disk in &log.disks {
        content.push_str(&format!("  • {}\n", disk));
    }
    
    content.push_str("\n───────────────────────────────────────────────────────────────\n");
    content.push_str("                          RÉSULTATS\n");
    content.push_str("───────────────────────────────────────────────────────────────\n\n");
    
    for result in &log.results {
        let status = if result.success { "✅ SUCCÈS" } else { "❌ ÉCHEC" };
        content.push_str(&format!("Disque : {}\n", result.disk_name));
        content.push_str(&format!("Statut : {}\n", status));
        content.push_str(&format!("Message: {}\n\n", result.message));
    }
    
    let success_count = log.results.iter().filter(|r| r.success).count();
    let total_count = log.results.len();
    
    content.push_str("───────────────────────────────────────────────────────────────\n");
    content.push_str(&format!("BILAN : {}/{} opérations réussies\n", success_count, total_count));
    content.push_str("───────────────────────────────────────────────────────────────\n");
    
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&txt_path)
    {
        let _ = file.write_all(content.as_bytes());
        println!("Log lisible enregistré dans: {:?}", txt_path);
    }
}

