/// Module pour exécuter des commandes système (smartctl, blkid)
use std::process::Command;

#[derive(Debug, Clone)]
pub enum SmartSummary {
    Good,
    PreFail,
    Bad,
}

#[derive(Debug, Clone)]
pub enum SmartAttrStatus {
    Good,
    Warn,
    Bad,
}

#[derive(Debug, Clone)]
pub struct SmartAttr {
    pub name: String,
    pub raw_value: String,
    pub threshold: Option<String>,
    pub status: SmartAttrStatus,
    pub value: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SmartInfo {
    pub summary: SmartSummary,
    pub attributes: Vec<SmartAttr>,
    pub summary_reason: String,
}

#[derive(Debug, Clone)]
pub struct BlkidResult {
    pub ok: bool,
    pub output: String,
}

pub fn get_smart_info(disk_path: &str) -> Result<SmartInfo, String> {
    let helper = crate::paths::helper_path();
    let output = Command::new("pkexec")
        .args([helper.as_str(), "smartctl", "-a", disk_path])
        .output()
        .map_err(|e| format!("Impossible d'exécuter smartctl: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            "smartctl -a a échoué".to_string()
        } else {
            stderr
        };
        return Err(message);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let health_line = parse_health_line(&stdout);
    let mut attributes = parse_smart_attributes(&stdout);

    let summary = compute_summary(&health_line, &attributes);
    let summary_reason = compute_summary_reason(&summary, &health_line, &attributes);
    apply_attr_statuses(&mut attributes);

    Ok(SmartInfo {
        summary,
        attributes,
        summary_reason,
    })
}

pub fn verify_with_blkid(disk_path: &str) -> Result<BlkidResult, String> {
    let helper = crate::paths::helper_path();
    let output = Command::new("pkexec")
        .args([helper.as_str(), "blkid", disk_path])
        .output()
        .map_err(|e| format!("Impossible d'exécuter blkid: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() && stdout.is_empty() {
        if stderr.contains("No such file") || stderr.contains("not found") {
            return Err("blkid n'est pas disponible".to_string());
        }
    }

    if stdout.is_empty() {
        Ok(BlkidResult { ok: true, output: String::new() })
    } else {
        Ok(BlkidResult { ok: false, output: stdout })
    }
}

fn parse_smart_attributes(output: &str) -> Vec<SmartAttr> {
    let keys = [
        "Reallocated_Sector_Ct",
        "Current_Pending_Sector",
        "Offline_Uncorrectable",
        "Reported_Uncorrect",
        "Power_On_Hours",
        "Power_On_Minutes",
    ];

    let mut attrs = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let name = parts[1];
        if !keys.contains(&name) {
            continue;
        }

        let raw_value = parts.last().unwrap_or(&"-").to_string();
        let value = parts.get(3).and_then(|v| v.parse::<u64>().ok());
        let threshold = if parts.len() > 5 { Some(parts[5].to_string()) } else { None };

        attrs.push(SmartAttr {
            name: name.to_string(),
            raw_value,
            threshold,
            status: SmartAttrStatus::Good,
            value,
        });
    }

    attrs
}

fn compute_summary(health_line: &str, attributes: &[SmartAttr]) -> SmartSummary {
    let health_upper = health_line.to_uppercase();
    let health_passed = health_upper.contains("PASSED");
    let health_failed = health_upper.contains("FAILED");

    let pending = get_attr_number(attributes, "Current_Pending_Sector").unwrap_or(0);
    let uncorrect = get_attr_number(attributes, "Offline_Uncorrectable").unwrap_or(0);
    let reported = get_attr_number(attributes, "Reported_Uncorrect").unwrap_or(0);
    let realloc = get_attr_number(attributes, "Reallocated_Sector_Ct").unwrap_or(0);
    let realloc_value = get_attr_value(attributes, "Reallocated_Sector_Ct").unwrap_or(100);
    let power_hours = get_power_on_hours(attributes).unwrap_or(0);

    if pending > 0
        || uncorrect > 0
        || reported > 0
        || health_failed
        || (realloc > 50 && realloc_value < 50)
    {
        return SmartSummary::Bad;
    }

    if health_passed
        && pending == 0
        && uncorrect == 0
        && (realloc <= 5 || realloc_value >= 90)
    {
        return SmartSummary::Good;
    }

    if (realloc >= 6 && realloc <= 20 && realloc_value >= 70)
        || (power_hours > 30000 && pending == 0 && uncorrect == 0)
    {
        return SmartSummary::PreFail;
    }

    SmartSummary::PreFail
}

fn compute_summary_reason(
    summary: &SmartSummary,
    health_line: &str,
    attributes: &[SmartAttr],
) -> String {
    let pending = get_attr_number(attributes, "Current_Pending_Sector").unwrap_or(0);
    let uncorrect = get_attr_number(attributes, "Offline_Uncorrectable").unwrap_or(0);
    let reported = get_attr_number(attributes, "Reported_Uncorrect").unwrap_or(0);
    let realloc = get_attr_number(attributes, "Reallocated_Sector_Ct").unwrap_or(0);
    let realloc_value = get_attr_value(attributes, "Reallocated_Sector_Ct").unwrap_or(100);
    let power_hours = get_power_on_hours(attributes).unwrap_or(0);
    let health_upper = health_line.to_uppercase();

    match summary {
        SmartSummary::Good => {
            "Excellent - aucun secteur dégradé, santé SMART OK".to_string()
        }
        SmartSummary::PreFail => {
            if realloc >= 6 && realloc <= 20 {
                format!("Surveiller - secteurs réalloués modérés ({})", realloc)
            } else if power_hours > 30000 {
                format!("Surveiller - usage élevé ({} h)", power_hours)
            } else if !health_upper.contains("PASSED") {
                "Surveiller - statut SMART indéterminé".to_string()
            } else {
                "Surveiller - indicateurs limites".to_string()
            }
        }
        SmartSummary::Bad => {
            if pending > 0 {
                format!("Défaillant - secteurs en attente ({})", pending)
            } else if uncorrect > 0 || reported > 0 {
                "Défaillant - erreurs non corrigeables".to_string()
            } else if realloc > 50 && realloc_value < 50 {
                format!("Défaillant - secteurs réalloués élevés ({})", realloc)
            } else if health_upper.contains("FAILED") {
                "Défaillant - statut SMART FAILED".to_string()
            } else {
                "Défaillant - anomalies critiques".to_string()
            }
        }
    }
}

fn apply_attr_statuses(attributes: &mut [SmartAttr]) {
    for attr in attributes.iter_mut() {
        let num = get_raw_number(&attr.raw_value).unwrap_or(0);
        let value = attr.value.unwrap_or(100);
        let status = match attr.name.as_str() {
            "Reallocated_Sector_Ct" => {
                if num > 50 && value < 50 {
                    SmartAttrStatus::Bad
                } else if num >= 6 && num <= 20 && value >= 70 {
                    SmartAttrStatus::Warn
                } else {
                    SmartAttrStatus::Good
                }
            }
            "Current_Pending_Sector" | "Offline_Uncorrectable" | "Reported_Uncorrect" => {
                if num > 0 { SmartAttrStatus::Bad } else { SmartAttrStatus::Good }
            }
            "Power_On_Hours" | "Power_On_Minutes" => {
                let hours = get_power_on_hours(&[attr.clone()]).unwrap_or(0);
                if hours > 30000 { SmartAttrStatus::Warn } else { SmartAttrStatus::Good }
            }
            _ => SmartAttrStatus::Good,
        };
        attr.status = status;
    }
}

fn parse_health_line(output: &str) -> String {
    output
        .lines()
        .find(|line| line.to_lowercase().contains("overall-health") || line.to_lowercase().contains("smart overall-health"))
        .unwrap_or("Statut SMART inconnu")
        .trim()
        .to_string()
}

fn get_attr_number(attributes: &[SmartAttr], name: &str) -> Option<u64> {
    attributes
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| get_raw_number(&a.raw_value))
}

fn get_attr_value(attributes: &[SmartAttr], name: &str) -> Option<u64> {
    attributes
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| a.value)
}

fn get_power_on_hours(attributes: &[SmartAttr]) -> Option<u64> {
    if let Some(hours) = get_attr_number(attributes, "Power_On_Hours") {
        return Some(hours);
    }
    if let Some(minutes_raw) = attributes
        .iter()
        .find(|a| a.name == "Power_On_Minutes")
        .map(|a| a.raw_value.clone())
    {
        return parse_hours_from_minutes(&minutes_raw);
    }
    None
}

fn parse_hours_from_minutes(raw: &str) -> Option<u64> {
    if let Some((hours_part, minutes_part)) = raw.split_once('h') {
        let hours = hours_part.trim().parse::<u64>().ok()?;
        if let Some(minutes_str) = minutes_part.split('m').next() {
            let minutes = minutes_str.trim().parse::<u64>().unwrap_or(0);
            return Some(hours + (minutes / 60));
        }
        return Some(hours);
    }
    get_raw_number(raw).map(|m| m / 60)
}

fn get_raw_number(raw: &str) -> Option<u64> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

