//! Validation shared by SQLite clients and native integration commands.
use crate::{Result, ServiceError};
use serde_json::Value;

pub fn validate_desktop_integration_setting(key: &str, value: &Value) -> Result<()> {
    match key {
        "launch_at_login" | "launch_minimized" if value.is_boolean() => Ok(()),
        "global_quick_add_shortcut" | "global_show_hide_shortcut" => {
            let chord = value.as_str().ok_or_else(|| {
                ServiceError::Invalid("The global shortcut must be a string.".into())
            })?;
            if chord.is_empty() {
                return Ok(());
            }
            if chord.len() > 64 || chord.chars().any(char::is_whitespace) {
                return Err(ServiceError::Invalid(
                    "Use one global shortcut combination, not a sequence.".into(),
                ));
            }
            let parts: Vec<_> = chord.split('+').collect();
            let Some((key, modifiers)) = parts.split_last() else {
                unreachable!()
            };
            let order = ["mod", "ctrl", "alt", "shift"];
            let positions: Option<Vec<_>> = modifiers
                .iter()
                .map(|m| order.iter().position(|item| item == m))
                .collect();
            let valid_modifiers = positions.is_some_and(|positions| {
                positions.windows(2).all(|pair| pair[0] < pair[1])
                    && modifiers.iter().any(|m| ["mod", "ctrl", "alt"].contains(m))
            });
            let valid_key = (key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase())
                || (key.len() == 1 && key.as_bytes()[0].is_ascii_digit())
                || key.strip_prefix('f').is_some_and(|n| {
                    n.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) && !n.starts_with('0')
                })
                || [
                    "space",
                    "enter",
                    "tab",
                    "backspace",
                    "delete",
                    "escape",
                    "arrowup",
                    "arrowdown",
                    "arrowleft",
                    "arrowright",
                    "home",
                    "end",
                    "pageup",
                    "pagedown",
                ]
                .contains(key);
            if !valid_modifiers || !valid_key {
                return Err(ServiceError::Invalid(
                    "Use Command, Control or Option with a letter, number or navigation key."
                        .into(),
                ));
            }
            if [
                "mod+q",
                "mod+w",
                "mod+h",
                "mod+m",
                "mod+tab",
                "mod+space",
                "mod+ctrl+q",
            ]
            .contains(&chord)
            {
                return Err(ServiceError::Invalid(
                    "This shortcut is reserved by macOS.".into(),
                ));
            }
            Ok(())
        }
        "launch_at_login" | "launch_minimized" => {
            Err(ServiceError::Invalid(format!("{key} must be a boolean.")))
        }
        _ => Err(ServiceError::Invalid(
            "Unknown desktop integration setting.".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_disabled_and_canonical_modified_chords() {
        for chord in [
            "",
            "ctrl+shift+a",
            "mod+alt+space",
            "mod+ctrl+alt+shift+f24",
            "alt+arrowleft",
            "ctrl+9",
        ] {
            for key in ["global_quick_add_shortcut", "global_show_hide_shortcut"] {
                validate_desktop_integration_setting(key, &json!(chord)).unwrap();
            }
        }
        for key in ["launch_at_login", "launch_minimized"] {
            validate_desktop_integration_setting(key, &json!(true)).unwrap();
            validate_desktop_integration_setting(key, &json!(false)).unwrap();
        }
    }

    #[test]
    fn rejects_reserved_unmodified_duplicate_and_noncanonical_chords() {
        for chord in [
            "a",
            "shift+a",
            "g t",
            "mod+q",
            "mod+ctrl+q",
            "mod+space",
            "ctrl+mod+a",
            "ctrl+ctrl+a",
            "ctrl+A",
            "ctrl+f0",
            "ctrl+f25",
            "ctrl+f01",
            "ctrl+",
            "meta+a",
            "ctrl+é",
        ] {
            for key in ["global_quick_add_shortcut", "global_show_hide_shortcut"] {
                assert!(
                    validate_desktop_integration_setting(key, &json!(chord)).is_err(),
                    "{key}: {chord}"
                );
            }
        }
        assert!(validate_desktop_integration_setting("launch_at_login", &json!("true")).is_err());
        assert!(validate_desktop_integration_setting("unknown", &json!(false)).is_err());
    }
}
