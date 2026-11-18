//! Allowlisted application settings. The only place that defines which keys
//! exist and which values are valid (used by the desktop IPC and the CLI).

pub const SETTINGS: &[(&str, fn(&str) -> bool, &str)] = &[
    ("theme", |v| matches!(v, "light" | "dark" | "system"), "light | dark | system"),
    ("accent", |v| v.parse::<u16>().is_ok_and(|h| h < 360), "matiz 0–359"),
    ("density", |v| matches!(v, "comfortable" | "compact"), "comfortable | compact"),
    ("font_scale", |v| matches!(v, "90" | "100" | "110" | "120"), "90 | 100 | 110 | 120"),
    ("units", |v| matches!(v, "binary" | "decimal"), "binary | decimal"),
    ("language", |v| v == "pt-BR", "pt-BR"),
    ("max_running", |v| v.parse::<u8>().is_ok_and(|n| (1..=8).contains(&n)), "1–8"),
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingError {
    #[error("configuração desconhecida: {0}")]
    UnknownKey(String),
    #[error("valor inválido para {key}: {value} (esperado: {expected})")]
    InvalidValue { key: String, value: String, expected: &'static str },
}

pub fn validate_setting(key: &str, value: &str) -> Result<(), SettingError> {
    let Some((_, valid, expected)) = SETTINGS.iter().find(|(k, _, _)| *k == key) else {
        return Err(SettingError::UnknownKey(key.to_string()));
    };
    if !valid(value) {
        return Err(SettingError::InvalidValue { key: key.into(), value: value.into(), expected });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allowlist() {
        assert!(validate_setting("theme", "dark").is_ok());
        assert!(validate_setting("theme", "purple").is_err());
        assert!(validate_setting("accent", "360").is_err());
        assert!(matches!(validate_setting("shell", "cmd"), Err(SettingError::UnknownKey(_))));
    }
}
