/// Module pour l'exécution de la commande shred et le suivi de la progression
use std::process::{Command, Stdio, Child};
use std::io::{BufRead, BufReader};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use regex::Regex;
use std::sync::OnceLock;

/// Regex compilée une seule fois pour l'efficacité
static PASS_REGEX: OnceLock<Regex> = OnceLock::new();
static STEP_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_pass_regex() -> &'static Regex {
    PASS_REGEX.get_or_init(|| Regex::new(r"(\d+)\s*%").unwrap())
}

fn get_step_regex() -> &'static Regex {
    STEP_REGEX.get_or_init(|| Regex::new(r"étape\s+(\d+)/(\d+)").unwrap())
}

/// Représente l'état d'une opération de shred
#[derive(Debug, Clone)]
pub enum ShredStatus {
    Starting,
    InProgress { progress: f64, current_step: u32, total_steps: u32 },  // Progression de 0.0 à 1.0
    Completed,
    Stopped,
    FailedIoError { message: String, stderr: String },
    FailedOther { message: String, stderr: String },
}

/// Résultat d'une exécution de shred
#[derive(Debug, Clone)]
pub struct ShredResult {
    pub success: bool,
    pub had_io_error: bool,
    pub exit_status: std::process::ExitStatus,
    pub stderr: String,
}

/// Message envoyé lors de l'exécution de shred
#[derive(Debug, Clone)]
pub struct ShredMessage {
    pub status: ShredStatus,
}

/// Handle pour contrôler un processus shred en cours
pub struct ShredHandle {
    pub child: Arc<Mutex<Option<Child>>>,
    pub pid: Arc<Mutex<Option<u32>>>,
}

/// Lance l'opération de shred sur un disque
/// Retourne un canal pour recevoir les mises à jour et un handle pour contrôler le processus
pub fn start_shred(disk_path: String, disk_name: String) -> (std::sync::mpsc::Receiver<ShredMessage>, ShredHandle) {
    let (tx, rx) = channel();
    let child_handle = Arc::new(Mutex::new(None));
    let child_handle_clone = child_handle.clone();
    let pid_handle = Arc::new(Mutex::new(None));
    let pid_handle_clone = pid_handle.clone();

    thread::spawn(move || {
        execute_shred(&disk_path, &disk_name, tx, child_handle_clone, pid_handle_clone);
    });

    (rx, ShredHandle { child: child_handle, pid: pid_handle })
}

/// Exécute shred et envoie les mises à jour via le canal
fn execute_shred(disk_path: &str, disk_name: &str, tx: Sender<ShredMessage>, child_handle: Arc<Mutex<Option<Child>>>, pid_handle: Arc<Mutex<Option<u32>>>) {
    // Message de démarrage
    let _ = tx.send(ShredMessage {
        status: ShredStatus::Starting,
    });

    let result = match run_shred(
        disk_path,
        disk_name,
        3,
        &tx,
        child_handle,
        pid_handle,
    ) {
        Ok(result) => result,
        Err(e) => {
            let _ = tx.send(ShredMessage {
                status: ShredStatus::FailedOther {
                    message: format!("Impossible d'exécuter shred: {}", e),
                    stderr: String::new(),
                },
            });
            return;
        }
    };

    if result.had_io_error {
        let _ = tx.send(ShredMessage {
            status: ShredStatus::FailedIoError {
                message: "Erreur d'entrée/sortie détectée".to_string(),
                stderr: result.stderr.clone(),
            },
        });
        return;
    }

    if result.success {
        let _ = tx.send(ShredMessage {
            status: ShredStatus::Completed,
        });
        return;
    }

    // Code 143 ou signal = processus tué
    if result.exit_status.code() == Some(143) || result.exit_status.code().is_none() {
        let _ = tx.send(ShredMessage {
            status: ShredStatus::Stopped,
        });
    } else {
        let _ = tx.send(ShredMessage {
            status: ShredStatus::FailedOther {
                message: "Le processus a échoué".to_string(),
                stderr: result.stderr.clone(),
            },
        });
    }
}

fn run_shred(
    disk_path: &str,
    disk_name: &str,
    passes: u8,
    tx: &Sender<ShredMessage>,
    child_handle: Arc<Mutex<Option<Child>>>,
    pid_handle: Arc<Mutex<Option<u32>>>,
) -> Result<ShredResult, String> {
    let passes_str = passes.to_string();
    // Exécuter shred avec les options demandées
    // -n N: N passes
    // -z: ajouter une passe finale de zéros
    // -v: verbeux (pour suivre la progression)
    let helper = crate::paths::helper_path();
    let mut child = Command::new("pkexec")
        .args([
            helper.as_str(),
            "shred",
            "-n", passes_str.as_str(),
            "-z",
            "-v",
            disk_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Impossible de démarrer shred: {}", e))?;

    // Lire le PID réel de shred depuis stdout du helper
    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() {
            let pid = line.trim().strip_prefix("PID:").and_then(|s| s.parse::<u32>().ok());
            if let Ok(mut pid_lock) = pid_handle.lock() {
                *pid_lock = pid;
            }
        }
    }

    // Fallback si on n'a pas récupéré le PID
    if let Ok(mut pid_lock) = pid_handle.lock() {
        if pid_lock.is_none() {
            *pid_lock = Some(child.id());
        }
    }

    // Stocker le handle du processus pour pouvoir le tuer plus tard
    if let Ok(mut handle) = child_handle.lock() {
        *handle = Some(child);
    } else {
        return Err("Impossible de stocker le handle du processus".to_string());
    }

    // Récupérer le child pour la suite
    let mut child = match child_handle.lock() {
        Ok(mut handle) => handle.take().unwrap(),
        Err(_) => return Err("Impossible d'accéder au handle du processus".to_string()),
    };

    let stderr = child.stderr.take().ok_or_else(|| "Impossible de capturer stderr".to_string())?;
    let reader = BufReader::new(stderr);

    // Utiliser la regex statique compilée une seule fois
    // Exemple de sortie: "shred: /dev/sdb : étape 1/4 (random)…2,7GiB/233GiB 1 %"
    let percent_regex = get_pass_regex();
    let step_regex = get_step_regex();
    let mut line_count = 0;
    let mut current_step = 1.0;
    let mut total_steps = 4.0;
    let mut had_io_error = false;
    let mut stderr_buffer = String::new();

    for line in reader.lines() {
        match line {
            Ok(line) => {
                line_count += 1;
                if is_io_error_line(&line) {
                    had_io_error = true;
                    // Interrompre immédiatement le processus en cas d'erreur E/S
                    if let Err(e) = child.kill() {
                        eprintln!("[{}] Impossible d'arrêter shred après erreur E/S: {}", disk_name, e);
                    }
                    // Conserver la ligne d'erreur et arrêter la lecture
                    if !line.is_empty() {
                        stderr_buffer.push_str(&line);
                        stderr_buffer.push('\n');
                    }
                    break;
                }

                if !line.is_empty() {
                    stderr_buffer.push_str(&line);
                    stderr_buffer.push('\n');
                }

                // D'abord, capturer le numéro de l'étape (pour savoir quelle passe on est)
                if let Some(caps) = step_regex.captures(&line) {
                    if let (Some(step_str), Some(total_str)) = (caps.get(1), caps.get(2)) {
                        if let (Ok(step), Ok(total)) = (step_str.as_str().parse::<f64>(), total_str.as_str().parse::<f64>()) {
                            current_step = step;
                            total_steps = total;
                        }
                    }
                }

                // Ensuite, capturer la progression du pourcentage
                if let Some(caps) = percent_regex.captures(&line) {
                    if let Some(percent_match) = caps.get(1) {
                        if let Ok(percent) = percent_match.as_str().parse::<f64>() {
                            // Calculer la progression globale en fonction des passes
                            let progress_in_step = percent / 100.0;
                            let global_progress = ((current_step - 1.0) + progress_in_step) / total_steps;
                            eprintln!("[{}] Étape {}/{}: {:.2}% → Progression globale: {:.2}%", 
                                disk_name, current_step as i32, total_steps as i32, percent, global_progress * 100.0);
                            let _ = tx.send(ShredMessage {
                                status: ShredStatus::InProgress {
                                    progress: global_progress,
                                    current_step: current_step as u32,
                                    total_steps: total_steps as u32,
                                },
                            });
                        }
                    }
                } else if !line.is_empty() {
                    // Log toutes les lignes non vides pour debug
                    eprintln!("[{}] Ligne {}: {}", disk_name, line_count, line);
                }
            }
            Err(e) => {
                eprintln!("Erreur de lecture pour {}: {}", disk_name, e);
            }
        }
    }

    eprintln!("[{}] Total de {} lignes lues", disk_name, line_count);

    let status = child.wait().map_err(|e| format!("Erreur lors de l'attente du processus: {}", e))?;

    Ok(ShredResult {
        success: status.success(),
        had_io_error,
        exit_status: status,
        stderr: stderr_buffer,
    })
}

fn is_io_error_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("erreur d'entrée/sortie")
        || lower.contains("fdatasync")
        || lower.contains("erreur d'écriture au décalage")
        || lower.contains("input/output error")
        || lower.contains("i/o error")
}

/// Arrête un processus shred en cours
pub fn stop_shred(handle: &ShredHandle) -> Result<(), String> {
    // Essayer d'abord via le PID
    if let Ok(pid_opt) = handle.pid.lock() {
        if let Some(pid) = *pid_opt {
            let helper = crate::paths::helper_path();
            let output = Command::new("pkexec")
                .args([helper.as_str(), "kill", "-9", &pid.to_string()])
                .output()
                .map_err(|e| format!("Impossible de lancer pkexec kill: {}", e))?;

            if output.status.success() {
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                "L'arrêt a été annulé ou refusé".to_string()
            } else {
                format!("L'arrêt a échoué: {}", stderr)
            };
            return Err(message);
        }
    }
    
    // Fallback : essayer via le child handle
    if let Ok(mut child_opt) = handle.child.lock() {
        if let Some(child) = child_opt.as_mut() {
            child.kill().map_err(|e| format!("Impossible de tuer le processus: {}", e))?;
            Ok(())
        } else {
            Err("Aucun processus en cours".to_string())
        }
    } else {
        Err("Impossible d'accéder au handle du processus".to_string())
    }
}

/// Pré-authentifie l'utilisateur pour éviter un prompt par disque
pub fn preauthorize_shred() -> Result<(), String> {
    let helper = crate::paths::helper_path();
    let status = Command::new("pkexec")
        .args([helper.as_str(), "true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Impossible de lancer pkexec: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err("Autorisation refusée".to_string())
    }
}

/// Démarre un keepalive polkit pour éviter plusieurs prompts
pub fn start_polkit_keepalive(interval_seconds: u64) -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let running_thread = running.clone();

    std::thread::spawn(move || {
        while running_thread.load(Ordering::Relaxed) {
            let helper = crate::paths::helper_path();
            let _ = Command::new("pkexec")
                .args([helper.as_str(), "true"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            std::thread::sleep(Duration::from_secs(interval_seconds));
        }
    });

    running
}

pub fn stop_polkit_keepalive(flag: &Arc<AtomicBool>) {
    flag.store(false, Ordering::Relaxed);
}

/// Vérifie si shred est disponible sur le système
pub fn check_shred_available() -> bool {
    Command::new("which")
        .arg("shred")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Vérifie si pkexec est disponible
pub fn check_pkexec_available() -> bool {
    Command::new("which")
        .arg("pkexec")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shred_available() {
        // shred devrait être disponible sur la plupart des systèmes Linux
        assert!(check_shred_available(), "shred n'est pas disponible");
    }

    #[test]
    fn test_pkexec_available() {
        // pkexec devrait etre disponible sur un systeme avec polkit
        assert!(check_pkexec_available(), "pkexec n'est pas disponible");
    }
}
