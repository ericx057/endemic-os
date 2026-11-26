use std::{cmp::Reverse, collections::HashSet, ops::Range};

use serde::{Deserialize, Serialize};

use crate::request::check_use_case;

pub const WEIGHT_TOTAL: usize = 100;
const NAME_LIMIT: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persona {
    pub name: String,
    pub weight: u8,
    pub max_model_turns: Option<usize>,
    pub max_tool_calls: Option<usize>,
    pub use_case: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Group {
    pub name: String,
    pub weight: u8,
    pub max_model_turns: usize,
    pub max_tool_calls: usize,
    pub tasks: Range<usize>,
}

pub struct TaskMix {
    groups: Vec<Group>,
    slots: Vec<(usize, usize)>,
}

pub fn validate(personas: &[Persona]) -> Result<(), String> {
    if personas.is_empty() {
        return Err("task_mix needs at least one persona".into());
    }
    for persona in personas {
        persona.check()?;
    }
    let names: HashSet<&str> = personas
        .iter()
        .map(|persona| persona.name.as_str())
        .collect();
    if names.len() != personas.len() {
        return Err("task_mix persona names must be unique".into());
    }
    let total: usize = personas
        .iter()
        .map(|persona| usize::from(persona.weight))
        .sum();
    if total != WEIGHT_TOTAL {
        return Err(format!(
            "task_mix weights must sum to {WEIGHT_TOTAL}, not {total}"
        ));
    }
    Ok(())
}

impl Persona {
    fn check(&self) -> Result<(), String> {
        let name = &self.name;
        if name.trim().is_empty() || name.len() > NAME_LIMIT {
            return Err(format!(
                "task_mix persona names must be 1 to {NAME_LIMIT} bytes and not blank"
            ));
        }
        let fail = |problem: &str| Err(format!("persona {name} {problem}"));
        if self.weight == 0 {
            return fail("needs a weight of at least 1");
        }
        if self.max_model_turns == Some(0) || self.max_tool_calls == Some(0) {
            return fail("needs max_model_turns and max_tool_calls of at least 1");
        }
        match &self.use_case {
            Some(use_case) => check_use_case(use_case)
                .map_err(|problem| format!("persona {name} use_case {problem}")),
            None => Ok(()),
        }
    }
}

impl TaskMix {
    pub fn new(groups: Vec<Group>, task_count: usize) -> Result<Self, String> {
        let total: usize = groups.iter().map(|group| usize::from(group.weight)).sum();
        let covered = groups.iter().all(|group| {
            group.weight > 0 && !group.tasks.is_empty() && group.tasks.end <= task_count
        });
        if total != WEIGHT_TOTAL || !covered {
            return Err("the run's task mix is invalid".into());
        }
        let mut taken = vec![0; groups.len()];
        let slots = (1..=WEIGHT_TOTAL)
            .map(|filled| {
                let behind = |index: &usize| {
                    let due = usize::from(groups[*index].weight) * filled;
                    (
                        due as i64 - (taken[*index] * WEIGHT_TOTAL) as i64,
                        Reverse(*index),
                    )
                };
                let chosen = (0..groups.len())
                    .max_by_key(behind)
                    .expect("weights summing to 100 name a group");
                taken[chosen] += 1;
                (chosen, taken[chosen] - 1)
            })
            .collect();
        Ok(Self { groups, slots })
    }

    pub fn assign(&self, agent: usize) -> (&Group, usize) {
        let (index, rank) = self.slots[agent % WEIGHT_TOTAL];
        let group = &self.groups[index];
        let ordinal = agent / WEIGHT_TOTAL * usize::from(group.weight) + rank;
        (group, group.tasks.start + ordinal % group.tasks.len())
    }
}

#[cfg(test)]
mod tests;
