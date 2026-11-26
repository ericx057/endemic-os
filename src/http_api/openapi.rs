use crate::target::Tool;
use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::Method;
use serde_json::{Map, Value, json};

const REF_DEPTH: usize = 8;
const DESCRIPTION_LIMIT: usize = 1024;
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "patch", "head", "options", "trace",
];

pub(super) struct Operation {
    pub(super) method: Method,
    pub(super) path: String,
    pub(super) query: Vec<String>,
    pub(super) body: bool,
    pub(super) label: String,
}

pub(super) fn operations(spec: &Value) -> Result<Vec<(Tool, Operation)>> {
    let version = spec["openapi"].as_str().unwrap_or_default();
    ensure!(
        version.starts_with("3."),
        "the spec must be an OpenAPI 3 document"
    );
    let paths = spec["paths"]
        .as_object()
        .context("OpenAPI document has no paths")?;
    let converted = paths
        .iter()
        .flat_map(|(path, item)| {
            METHODS
                .iter()
                .filter(|method| item[**method].is_object())
                .map(move |method| operation(spec, path, method, item))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut names: Vec<_> = converted.iter().map(|(tool, _)| &tool.name).collect();
    names.sort_unstable();
    if let Some(pair) = names.windows(2).find(|pair| pair[0] == pair[1]) {
        bail!("operationId {} appears more than once", pair[0]);
    }
    Ok(converted)
}

fn operation(spec: &Value, path: &str, method: &str, item: &Value) -> Result<(Tool, Operation)> {
    let op = &item[method];
    let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())?;
    let name = op["operationId"]
        .as_str()
        .with_context(|| format!("{method} {path} has no operationId"))?;
    ensure!(
        crate::mcp::valid_name(name),
        "operationId {name} must be 1 to 64 ASCII letters, digits, '_' or '-'"
    );
    let context = || format!("operation {name}");
    let mut params = Params::default();
    for param in parameters(item).chain(parameters(op)) {
        params
            .add(inline(param, spec, 0).with_context(context)?)
            .with_context(context)?;
    }
    let body = match op.get("requestBody") {
        Some(body) => Some(json_body(
            &inline(body, spec, 0).with_context(context)?,
            name,
        )?),
        None => None,
    };
    let query = params.query.keys().cloned().collect();
    let tool = Tool {
        name: name.to_owned(),
        description: description(op),
        schema: params.schema(body.as_ref()),
        read_only: method == Method::GET || method == Method::HEAD,
    };
    let operation = Operation {
        label: format!("{name} ({method} {path})"),
        method,
        path: path.to_owned(),
        query,
        body: body.is_some(),
    };
    Ok((tool, operation))
}

fn parameters(item: &Value) -> impl Iterator<Item = &Value> {
    item["parameters"].as_array().into_iter().flatten()
}

#[derive(Default)]
struct Params {
    path: Map<String, Value>,
    query: Map<String, Value>,
    required_query: Vec<Value>,
}

impl Params {
    fn add(&mut self, param: Value) -> Result<()> {
        let name = param["name"].as_str().context("parameter has no name")?;
        let schema = param["schema"].clone();
        let schema = if schema.is_null() { json!({}) } else { schema };
        match param["in"].as_str() {
            Some("path") => _ = self.path.insert(name.to_owned(), schema),
            Some("query") => {
                self.query.insert(name.to_owned(), schema);
                self.required_query.retain(|required| required != name);
                if param["required"] == true {
                    self.required_query.push(name.into());
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn schema(self, body: Option<&(Value, bool)>) -> Value {
        let (mut properties, mut required) = (Map::new(), Vec::new());
        if !self.path.is_empty() {
            let names: Vec<Value> = self.path.keys().map(|name| name.as_str().into()).collect();
            properties.insert("path".into(), object(self.path, names));
            required.push("path".into());
        }
        if !self.query.is_empty() {
            if !self.required_query.is_empty() {
                required.push("query".into());
            }
            properties.insert("query".into(), object(self.query, self.required_query));
        }
        if let Some((schema, body_required)) = body {
            properties.insert("body".into(), schema.clone());
            if *body_required {
                required.push("body".into());
            }
        }
        object(properties, required)
    }
}

fn object(properties: Map<String, Value>, required: Vec<Value>) -> Value {
    let mut schema = json!({"type": "object", "properties": properties});
    if !required.is_empty() {
        schema["required"] = required.into();
    }
    schema
}

fn json_body(body: &Value, name: &str) -> Result<(Value, bool)> {
    let schema = body["content"]["application/json"]
        .as_object()
        .map(|media| media.get("schema").cloned().unwrap_or_else(|| json!({})))
        .with_context(|| format!("operation {name} request body must be application/json"))?;
    Ok((schema, body["required"] == true))
}

fn description(op: &Value) -> Option<String> {
    let text = [&op["summary"], &op["description"]]
        .into_iter()
        .filter_map(Value::as_str)
        .find(|text| !text.trim().is_empty())?;
    let mut end = text.len().min(DESCRIPTION_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Some(text[..end].to_owned())
}

fn inline(value: &Value, spec: &Value, depth: usize) -> Result<Value> {
    match value {
        Value::Object(map) => match map.get("$ref").and_then(Value::as_str) {
            Some(reference) => {
                ensure!(
                    depth < REF_DEPTH,
                    "$ref depth exceeds {REF_DEPTH} at {reference}"
                );
                inline(resolve(reference, spec)?, spec, depth + 1)
            }
            None => map
                .iter()
                .map(|(key, value)| Ok((key.clone(), inline(value, spec, depth)?)))
                .collect::<Result<_>>()
                .map(Value::Object),
        },
        Value::Array(items) => items.iter().map(|item| inline(item, spec, depth)).collect(),
        scalar => Ok(scalar.clone()),
    }
}

fn resolve<'a>(reference: &str, spec: &'a Value) -> Result<&'a Value> {
    if !reference.starts_with("#/components/") {
        bail!("$ref {reference} is not a local #/components/ reference");
    }
    spec.pointer(&reference[1..])
        .ok_or_else(|| anyhow!("$ref {reference} does not exist"))
}

#[cfg(test)]
mod tests;
