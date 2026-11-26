use super::*;
use std::collections::HashMap;

pub(crate) fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Result<String, VarError> {
    let vars: HashMap<String, String> = pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    move |name| vars.get(name).cloned().ok_or(VarError::NotPresent)
}

pub(crate) fn assert_each_required<T>(pairs: &[(&str, &str)], load: impl Fn(Env) -> Result<T>) {
    assert!(load(&env_of(pairs)).is_ok(), "complete settings load");
    for (index, (name, _)) in pairs.iter().enumerate() {
        let mut partial = pairs.to_vec();
        partial.remove(index);
        let error = load(&env_of(&partial))
            .err()
            .map(|error| format!("{error:#}"));
        let expected = format!("{name} is required");
        assert!(
            error
                .as_deref()
                .is_some_and(|error| error.starts_with(&expected)),
            "without {name}: {error:?}"
        );
    }
}

#[test]
fn missing_variables_are_named() {
    let env = env_of(&[]);
    assert_eq!(
        required(&env, "LISTEN_ADDR").unwrap_err().to_string(),
        "LISTEN_ADDR is required"
    );
    assert_eq!(optional(&env, "UNSET_SETTING").unwrap(), None);
}

#[test]
fn numbers_must_be_positive_integers() {
    let env = env_of(&[("A", "7"), ("B", "0"), ("C", "x"), ("D", "1.5")]);
    assert_eq!(positive::<u64>(&env, "A").unwrap(), 7);
    for name in ["B", "C", "D"] {
        assert_eq!(
            positive::<u64>(&env, name).unwrap_err().to_string(),
            format!("{name} must be a positive integer")
        );
    }
    assert_eq!(seconds(&env, "A").unwrap(), Duration::from_secs(7));
}
