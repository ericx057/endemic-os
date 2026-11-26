use super::*;
use serde_json::json;

fn spec(paths: Value, components: Value) -> Value {
    json!({"openapi": "3.0.3", "paths": paths, "components": components})
}

fn error(spec: &Value) -> String {
    format!(
        "{:#}",
        operations(spec).err().expect("conversion must fail")
    )
}

#[test]
fn builds_path_query_and_body_with_nested_refs_inlined() {
    let spec = spec(
        json!({"/items/{id}": {
            "parameters": [{"$ref": "#/components/parameters/Id"}],
            "put": {
                "operationId": "updateItem",
                "summary": "Update an item",
                "description": "Long form",
                "parameters": [
                    {"name": "dry_run", "in": "query", "schema": {"type": "boolean"}},
                    {"name": "reason", "in": "query", "required": true, "schema": {"type": "string"}},
                    {"name": "x-tenant", "in": "header", "schema": {"type": "string"}}
                ],
                "requestBody": {"$ref": "#/components/requestBodies/Item"}
            }
        }}),
        json!({
            "parameters": {"Id": {"name": "id", "in": "path", "required": true,
                "schema": {"$ref": "#/components/schemas/Id"}}},
            "requestBodies": {"Item": {"required": true, "content": {"application/json":
                {"schema": {"$ref": "#/components/schemas/Item"}}}}},
            "schemas": {
                "Id": {"type": "string"},
                "Tag": {"type": "string", "maxLength": 8},
                "Item": {"type": "object", "properties": {
                    "tags": {"type": "array", "items": {"$ref": "#/components/schemas/Tag"}}
                }}
            }
        }),
    );
    let converted = operations(&spec).unwrap();
    let (tool, operation) = &converted[0];
    assert_eq!(tool.name, "updateItem");
    assert_eq!(tool.description.as_deref(), Some("Update an item"));
    assert_eq!(
        tool.schema,
        json!({"type": "object", "properties": {
            "path": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]},
            "query": {"type": "object", "properties": {
                "dry_run": {"type": "boolean"}, "reason": {"type": "string"}
            }, "required": ["reason"]},
            "body": {"type": "object", "properties": {
                "tags": {"type": "array", "items": {"type": "string", "maxLength": 8}}
            }}
        }, "required": ["path", "query", "body"]})
    );
    assert_eq!(operation.method, Method::PUT);
    assert_eq!(operation.path, "/items/{id}");
    assert_eq!(operation.query, ["dry_run", "reason"]);
    assert!(operation.body);
    assert_eq!(operation.label, "updateItem (PUT /items/{id})");
    assert!(!tool.read_only);
}

#[test]
fn every_operation_is_a_tool_and_only_get_and_head_are_read_only() {
    let spec = spec(
        json!({
            "/items": {"get": {"operationId": "list"}, "post": {"operationId": "create"}},
            "/items/{id}": {"head": {"operationId": "exists"}, "delete": {"operationId": "remove"}},
            "/search": {"options": {"operationId": "probe"}}
        }),
        json!({}),
    );
    let mut tools: Vec<_> = operations(&spec)
        .unwrap()
        .into_iter()
        .map(|(tool, _)| (tool.name, tool.read_only))
        .collect();
    tools.sort_unstable();
    assert_eq!(
        tools,
        [
            ("create".into(), false),
            ("exists".into(), true),
            ("list".into(), true),
            ("probe".into(), false),
            ("remove".into(), false),
        ]
    );
}

#[test]
fn omits_empty_parts_and_falls_back_to_a_bounded_description() {
    let long = "d".repeat(DESCRIPTION_LIMIT + 10);
    let spec = spec(
        json!({"/health": {"get": {"operationId": "health", "description": long}}}),
        json!({}),
    );
    let (tool, operation) = operations(&spec).unwrap().remove(0);
    assert_eq!(tool.schema, json!({"type": "object", "properties": {}}));
    assert_eq!(tool.description.unwrap().len(), DESCRIPTION_LIMIT);
    assert!(!operation.body);
    assert!(operation.query.is_empty());
}

#[test]
fn cyclic_or_too_deep_refs_are_rejected() {
    let cyclic = spec(
        json!({"/a": {"post": {"operationId": "a", "requestBody": {"content": {"application/json":
            {"schema": {"$ref": "#/components/schemas/Node"}}}}}}}),
        json!({"schemas": {"Node": {"type": "object", "properties": {
            "next": {"$ref": "#/components/schemas/Node"}
        }}}}),
    );
    assert!(error(&cyclic).contains("$ref depth"));
}

#[test]
fn refs_must_be_local_components_that_exist() {
    for reference in [
        "other.json#/components/schemas/X",
        "#/paths/~1a",
        "#/components/schemas/Missing",
    ] {
        let spec = spec(
            json!({"/a": {"get": {"operationId": "a", "parameters": [{"$ref": reference}]}}}),
            json!({}),
        );
        assert!(error(&spec).contains(reference), "{reference}");
    }
}

#[test]
fn rejects_duplicate_unnamed_or_uncallable_operations() {
    for (paths, expected) in [
        (
            json!({"/a": {"get": {"operationId": "a"}, "post": {"operationId": "a"}}}),
            "operationId a appears more than once",
        ),
        (
            json!({"/b": {"get": {"summary": "no id"}}}),
            "GET /b has no operationId",
        ),
        (json!({"/b": {"get": {"operationId": "b.get"}}}), "b.get"),
        (
            json!({"/c": {"post": {"operationId": "c", "requestBody": {"content": {"text/plain": {}}}}}}),
            "application/json",
        ),
        (
            json!({"/d": {"get": {"operationId": "d", "parameters": [{"in": "query"}]}}}),
            "parameter has no name",
        ),
    ] {
        let error = error(&spec(paths, json!({})));
        assert!(error.contains(expected), "{error}");
    }
    let swagger = json!({"swagger": "2.0", "paths": {}});
    assert!(error(&swagger).contains("OpenAPI 3"));
}
