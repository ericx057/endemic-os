use super::Engine;
use crate::{runner::Failure, target::Tool};
use serde_json::{Value, json};

const TASK_LIMIT: usize = crate::request::USE_CASE_LIMIT;

impl Engine {
    pub async fn tasks(
        &self,
        tools: &[Tool],
        use_case: Option<&str>,
        count: usize,
    ) -> Result<Vec<String>, String> {
        let system = format!(
            "You plan a load test of a staging application. Write tasks for simulated users: \
             each task is one concrete, self-contained goal an average user could reach with \
             the listed tools, written as an instruction to the agent playing that user. When \
             a use case is given, the tasks follow it. Tasks must differ from each other. \
             Reply with a JSON array of exactly {count} strings and nothing else."
        );
        let tools: Vec<Value> = tools
            .iter()
            .map(|tool| json!({"name": tool.name, "description": tool.description}))
            .collect();
        let user = json!({"tools": tools, "use_case": use_case}).to_string();
        let messages = [
            json!({"role": "system", "content": system}),
            json!({"role": "user", "content": user}),
        ];
        let format = json!({"type": "json_schema", "json_schema": {
            "name": "tasks",
            "schema": {"type": "array", "items": {"type": "string"}},
        }});
        let text = self
            .client
            .answer(&messages, &format)
            .await
            .map_err(|failure| format!("task generation failed: {}", reason(failure)))?;
        parse(&text, count).map_err(|error| format!("task generation failed: {error}"))
    }
}

fn reason(failure: Failure) -> &'static str {
    match failure {
        Failure::InferenceThrottled => "the inference server throttled the request",
        Failure::Runner => "the request exceeds VLLM_REQUEST_LIMIT_BYTES",
        _ => "the inference server returned an error or no usable completion",
    }
}

fn parse(text: &str, count: usize) -> Result<Vec<String>, String> {
    let tasks: Vec<String> = serde_json::from_str(text.trim())
        .map_err(|_| "the model's reply is not a JSON array of strings".to_owned())?;
    if tasks.len() != count {
        return Err(format!(
            "the model returned {} tasks, expected {count}",
            tasks.len()
        ));
    }
    for (index, task) in tasks.iter().enumerate() {
        let number = index + 1;
        if task.trim().is_empty() {
            return Err(format!("task {number} is blank"));
        }
        if task.len() > TASK_LIMIT {
            return Err(format!("task {number} is longer than {TASK_LIMIT} bytes"));
        }
        if tasks[..index].contains(task) {
            return Err(format!("task {number} repeats an earlier task"));
        }
    }
    Ok(tasks)
}

#[cfg(test)]
mod tests;
