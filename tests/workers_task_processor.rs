//! `createTaskProcessorWorker` against the real CesiumJS message adapter.
mod common;
use cesium_rs::workers::*;
use common::*;
use serde_json::{json, Value};

fn string_field(v: &Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn parse_throw(v: &Value) -> WorkerThrown {
    match v.get("type").and_then(Value::as_str) {
        Some("undefined") => WorkerThrown::Other(None),
        Some("value") => WorkerThrown::Other(Some(v.get("value").cloned().unwrap_or(Value::Null))),
        Some("error") => WorkerThrown::Exception(WorkerException {
            name: match v.get("name") {
                Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                None => Some("Error".to_string()),
                Some(other) => panic!("throw name: {other}"),
            },
            message: match v.get("message") {
                Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                None => Some(String::new()),
                Some(other) => panic!("throw message: {other}"),
            },
            stack: string_field(v, "stack"),
        }),
        other => panic!("throw type: {other:?}"),
    }
}

fn parse_post(v: &Value) -> PostMessageError {
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("error");
    let to_string = string_field(v, "string").unwrap_or_default();
    if kind == "string" || kind == "object" {
        return PostMessageError { name: None, message: None, stack: None, to_string };
    }
    PostMessageError {
        name: match v.get("name") {
            Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            None => Some("Error".to_string()),
            Some(other) => panic!("post name: {other}"),
        },
        message: match v.get("message") {
            Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            None => Some(String::new()),
            Some(other) => panic!("post message: {other}"),
        },
        stack: string_field(v, "stack"),
        to_string,
    }
}

fn encode_reply(reply: TaskProcessorReply) -> Value {
    json!({
        "message": reply.message,
        "transfers": reply.transfers,
        "baseUrl": reply.base_url,
    })
}

fn on_message(script: &Value) -> Value {
    let task = TaskMessage {
        id: script.get("id").cloned(),
        parameters: script.get("parameters").cloned(),
        base_url: string_field(script, "baseUrl"),
        can_transfer_array_buffer: script.get("canTransferArrayBuffer").and_then(Value::as_bool).unwrap_or(false),
    };
    let response = WorkerResponse {
        result: match script.get("throw") {
            Some(thrown) => Err(parse_throw(thrown)),
            None if script.get("echoParameters").and_then(Value::as_bool).unwrap_or(false) => {
                Ok(task.parameters.clone())
            }
            None if script.get("returns").is_some() => Ok(Some(script["returns"].clone())),
            None => Ok(None),
        },
        transfers: script.get("transfers").and_then(Value::as_array).cloned().unwrap_or_default(),
    };
    let post_failure = script.get("postThrows").map(parse_post);
    encode_reply(deliver_task(&task, response, post_failure.as_ref()))
}

fn on_message_error(script: &Value) -> Value {
    encode_reply(deliver_message_error(script.get("id").cloned(), script.get("event").unwrap_or(&Value::Null)))
}

#[test]
fn create_task_processor_worker_reference() {
    run(
        "workers",
        "createTaskProcessorWorker",
        |_| EXACT,
        |f, a| {
            let script = a.first().expect("script");
            Some(Out::Ok(match f {
                "onmessage" => on_message(script),
                "onmessageerror" => on_message_error(script),
                _ => return None,
            }))
        },
    );
}
