use super::*;
use serde_json::json;

fn group(name: &str, weight: u8, tasks: Range<usize>) -> Group {
    Group {
        name: name.into(),
        weight,
        max_model_turns: 1,
        max_tool_calls: 1,
        tasks,
    }
}

fn personas(value: serde_json::Value) -> Vec<Persona> {
    serde_json::from_value(value).unwrap()
}

#[test]
fn every_hundred_agents_split_exactly_by_weight_and_interleave() {
    let mix = TaskMix::new(vec![group("reads", 70, 0..2), group("writes", 30, 2..5)], 5).unwrap();
    let groups: Vec<&str> = (0..200)
        .map(|agent| mix.assign(agent).0.name.as_str())
        .collect();
    for period in groups.chunks(100) {
        assert_eq!(period.iter().filter(|name| **name == "writes").count(), 30);
    }
    assert!(groups[..4].contains(&"writes"), "{groups:?}");
    let writes: Vec<usize> = (0..200)
        .map(|agent| mix.assign(agent))
        .filter(|(group, _)| group.name == "writes")
        .map(|(_, task)| task)
        .collect();
    assert_eq!(&writes[..6], [2, 3, 4, 2, 3, 4]);
    let again = TaskMix::new(vec![group("reads", 70, 0..2), group("writes", 30, 2..5)], 5).unwrap();
    assert!((0..500).all(|agent| mix.assign(agent).1 == again.assign(agent).1));
}

#[test]
fn a_mix_that_does_not_cover_its_tasks_is_refused() {
    for groups in [
        vec![group("a", 60, 0..1)],
        vec![group("a", 100, 0..0)],
        vec![group("a", 100, 0..3)],
        Vec::new(),
    ] {
        assert!(TaskMix::new(groups, 2).is_err());
    }
}

#[test]
fn personas_are_validated_at_the_request_boundary() {
    let valid = personas(json!([
        {"name":"reads","weight":70,"use_case":"Browse."},
        {"name":"writes","weight":30,"max_tool_calls":4}
    ]));
    assert_eq!(validate(&valid), Ok(()));
    let cases = [
        (json!([]), "at least one persona"),
        (json!([{"name":"a","weight":60}]), "sum to 100, not 60"),
        (
            json!([{"name":"a","weight":50},{"name":"a","weight":50}]),
            "unique",
        ),
        (json!([{"name":" ","weight":100}]), "names must be 1 to 64"),
        (
            json!([{"name":"a","weight":0},{"name":"b","weight":100}]),
            "weight of at least 1",
        ),
        (
            json!([{"name":"a","weight":100,"max_model_turns":0}]),
            "at least 1",
        ),
        (
            json!([{"name":"a","weight":100,"use_case":""}]),
            "use_case must be 1 to 4096",
        ),
    ];
    for (body, expected) in cases {
        let error = validate(&personas(body)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}
