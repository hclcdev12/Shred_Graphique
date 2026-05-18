/// Module pour la détection et la gestion des disques
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone)]
pub struct Disk {
    pub name: String,          // ex: "sda"
    pub path: String,          // ex: "/dev/sda"
    pub size: String,          // ex: "500G"
    pub model: String,         // ex: "Samsung SSD 850"
    pub is_ssd: bool,          // true si disque non rotatif (SSD/NVMe)
    pub is_read_only: bool,    // true si RO=1 dans lsblk
    pub is_system: bool,       // true si disque système
    pub is_mounted: bool,      // true si au moins une partition est montée
}

impl Disk {
    /// Crée une nouvelle instance de Disk
    pub fn new(
        name: String,
        path: String,
        size: String,
        model: String,
        is_ssd: bool,
        is_read_only: bool,
    ) -> Self {
        let is_system = Self::check_if_system_disk(&name);
        let is_mounted = Self::check_if_mounted(&name);
        
        Disk {
            name,
            path,
            size,
            model,
            is_ssd,
            is_read_only,
            is_system,
            is_mounted,
        }
    }

    /// Vérifie si le disque est le disque système
    fn check_if_system_disk(disk_name: &str) -> bool {
        // D'abord, lire /proc/mounts pour trouver la partition racine
        if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let device = parts[0];
                    let mount_point = parts[1];
                    
                    // Vérifier si c'est la partition racine et si elle appartient au disque
                    if mount_point == "/" && device.contains(disk_name) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Vérifie si le disque (ou ses partitions) est monté
    fn check_if_mounted(disk_name: &str) -> bool {
        if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
            // Vérifier si le disque ou ses partitions apparaissent dans /proc/mounts
            for line in mounts.lines() {
                if line.starts_with(&format!("/dev/{}", disk_name)) {
                    return true;
                }
            }
        }
        false
    }

    /// Vérifie si le disque peut être supprimé en toute sécurité
    pub fn can_be_shredded(&self) -> (bool, String) {
        if self.is_system {
            return (false, "Ce disque est le disque système".to_string());
        }

        if self.is_ssd && self.is_read_only {
            return (false, "SSD en lecture seule (RO)".to_string());
        }
        
        if self.is_mounted {
            return (false, "Ce disque ou l'une de ses partitions est monté".to_string());
        }

        (true, "OK".to_string())
    }
}

/// Détecte tous les disques disponibles sur le système
pub fn detect_disks() -> Vec<Disk> {
    let mut disks = Vec::new();

    // Utiliser lsblk pour lister les disques avec un timeout court
    // Utiliser -P pour format parsable (format key="value") qui gère mieux les espaces dans les modèles
    let output = match std::process::Command::new("timeout")
        .args([
            "5",            // Timeout de 5 secondes (au lieu de bloquer indéfiniment)
            "lsblk",
            "-d",           // Seulement les disques, pas les partitions
            "-n",           // Pas de header
            "-P",           // Format parsable (key="value")
            "-o", "NAME,SIZE,MODEL,TYPE,ROTA,RO",  // Colonnes à afficher
        ])
        .output()
    {
        Ok(output) => {
            // Vérifier si la commande timeout a expiré (exit code 124)
            if !output.status.success() && output.status.code() == Some(124) {
                eprintln!("⚠️  Timeout lors de la détection des disques (5s)");
                return disks;
            }
            output
        },
        Err(e) => {
            eprintln!("Erreur lors de l'exécution de lsblk: {}", e);
            return disks;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        // Parser le format key="value" de lsblk -P, y compris les valeurs avec espaces.
        let parsed = parse_lsblk_pairs(line);
        let name = parsed.get("NAME").cloned().unwrap_or_default();
        let size = parsed.get("SIZE").cloned().unwrap_or_default();
        let mut model = parsed.get("MODEL").cloned().unwrap_or_default();
        let disk_type = parsed.get("TYPE").cloned().unwrap_or_default();
        let rota = parsed.get("ROTA").map(String::as_str).unwrap_or("1");
        let ro = parsed.get("RO").map(String::as_str).unwrap_or("0");

        // Ignorer les lignes incomplètes
        if name.is_empty() || disk_type.is_empty() {
            continue;
        }

        // Filtrer uniquement les disques (disk), pas les cd-rom, loopback, etc.
        if disk_type != "disk" {
            continue;
        }

        // Ignorer les disques loop et autres périphériques virtuels
        if name.starts_with("loop") || name.starts_with("ram") {
            continue;
        }

        let path = format!("/dev/{}", name);
        if model.is_empty() {
            model = "Unknown".to_string();
        }

        let is_ssd = rota == "0";
        let is_read_only = ro == "1";

        let disk = Disk::new(name, path, size, model, is_ssd, is_read_only);
        disks.push(disk);
    }

    disks
}

fn parse_lsblk_pairs(line: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut key = String::new();
    let mut value = String::new();
    let mut in_key = true;
    let mut in_value = false;
    let mut escape_next = false;

    for ch in line.chars() {
        if in_key {
            if ch == '=' {
                in_key = false;
            } else if !ch.is_whitespace() {
                key.push(ch);
            }
            continue;
        }

        if !in_value {
            if ch == '"' {
                in_value = true;
            }
            continue;
        }

        if escape_next {
            value.push(ch);
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => {
                escape_next = true;
            }
            '"' => {
                if !key.is_empty() {
                    result.insert(key.clone(), value.clone());
                }
                key.clear();
                value.clear();
                in_key = true;
                in_value = false;
            }
            _ => value.push(ch),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_disks() {
        let disks = detect_disks();
        // Au moins un disque doit être détecté sur un système normal
        assert!(!disks.is_empty(), "Aucun disque détecté");
        
        // Au moins un disque doit être le disque système
        assert!(disks.iter().any(|d| d.is_system), "Aucun disque système détecté");
    }

    #[test]
    fn test_disk_safety() {
        let disks = detect_disks();
        for disk in disks {
            let (can_shred, reason) = disk.can_be_shredded();
            if disk.is_system {
                assert!(!can_shred, "Le disque système ne devrait pas pouvoir être effacé");
                assert!(reason.contains("système"));
            }
        }
    }
}
