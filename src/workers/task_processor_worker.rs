//! CesiumJS `createTaskProcessorWorker`: the Web Worker adapter that turns a
//! calculation into the `onmessage` / `onmessageerror` contract `TaskProcessor`
//! posts.
//!
//! The adapter copies task id, result, and error into one reply object, records
//! the task's `baseUrl` where the worker would assign `self.CESIUM_BASE_URL`,
//! and drops the transfer list when `canTransferArrayBuffer` is false. A failed
//! `postMessage` replaces the reply with `formatError`'s string, stringifying
//! the reply only after `result` is cleared and while the previous `error` is
//! still in place. `onmessageerror` posts `JSON.stringify(event)` and does not
//! assign `CESIUM_BASE_URL`.
//!
//! Rust workers return owned values. This module does not install a process-wide
//! base URL and does not change [`crate::core::TaskProcessor`], which already
//! moves those owned values.

use serde_json::{Map, Value};

/// One task `postMessage` body: `{ id, parameters, baseUrl, canTransferArrayBuffer }`.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskMessage {
    pub id: Option<Value>,
    /// `None` when the property is missing, which the worker sees as `undefined`.
    pub parameters: Option<Value>,
    /// `None` for a missing or `null` `baseUrl`. Both assignments record as unset.
    pub base_url: Option<String>,
    pub can_transfer_array_buffer: bool,
}

/// A thrown `Error`. `name`, `message`, and `stack` are omitted from the reply
/// when `None`, matching `JSON.stringify` dropping `undefined`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerException {
    pub name: Option<String>,
    pub message: Option<String>,
    pub stack: Option<String>,
}

/// What the calculation threw. `Other(None)` is `throw undefined`, which leaves
/// the reply's `error` property unset.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkerThrown {
    Exception(WorkerException),
    Other(Option<Value>),
}

/// The calculation's reply before the adapter posts it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkerResponse {
    pub result: Result<Option<Value>, WorkerThrown>,
    /// Objects the calculation pushed for `postMessage`'s transfer list.
    pub transfers: Vec<Value>,
}

/// The value caught from `postMessage`, formatted by Cesium `formatError`.
///
/// When both `name` and `message` are present, the text is `name: message`.
/// Otherwise it is `to_string`, which is the caught value's `toString()`.
/// A present `stack` is appended on its own line, including an empty stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostMessageError {
    pub name: Option<String>,
    pub message: Option<String>,
    pub stack: Option<String>,
    pub to_string: String,
}

/// The final `postMessage` arguments and the base URL this handler assigns.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskProcessorReply {
    /// Reply object with `undefined` fields omitted, in `id`, `result`, `error` order.
    pub message: Value,
    /// `None` when the final `postMessage` is called with no transfer argument.
    pub transfers: Option<Vec<Value>>,
    /// `self.CESIUM_BASE_URL` after `onmessage`. `onmessageerror` leaves this `None`.
    pub base_url: Option<String>,
}

/// `onmessage`: run one task and build the reply `TaskProcessor` receives.
pub fn deliver_task(
    task: &TaskMessage,
    response: WorkerResponse,
    post_failure: Option<&PostMessageError>,
) -> TaskProcessorReply {
    let transfers = if task.can_transfer_array_buffer { response.transfers } else { Vec::new() };
    let (result, error, error_json) = match response.result {
        Ok(result) => (result, None, None),
        Err(WorkerThrown::Other(value)) => {
            let json = value.as_ref().map(|v| serde_json::to_string(v).expect("thrown value"));
            (None, value, json)
        }
        Err(WorkerThrown::Exception(exception)) => {
            let json = exception_json(&exception);
            let value = exception_value(&exception);
            (None, Some(value), Some(json))
        }
    };
    if let Some(failure) = post_failure {
        // `result` is cleared before the previous error is stringified.
        // `preserve_order` keeps object keys in insertion order, matching JSON.stringify.
        let embedded = stringify_message(task.id.as_ref(), None, error_json.as_deref());
        let text =
            format!("postMessage failed with error: {}\n  with responseMessage: {embedded}", format_error(failure));
        return TaskProcessorReply {
            message: message_value(task.id.as_ref(), None, Some(&Value::String(text))),
            transfers: None,
            base_url: task.base_url.clone(),
        };
    }
    TaskProcessorReply {
        message: message_value(task.id.as_ref(), result.as_ref(), error.as_ref()),
        transfers: Some(transfers),
        base_url: task.base_url.clone(),
    }
}

/// `onmessageerror`: `postMessage failed with error: ${JSON.stringify(event)}`.
pub fn deliver_message_error(id: Option<Value>, event: &Value) -> TaskProcessorReply {
    let event_json = serde_json::to_string(event).expect("message error event");
    let error = format!("postMessage failed with error: {event_json}");
    TaskProcessorReply {
        message: message_value(id.as_ref(), None, Some(&Value::String(error))),
        transfers: None,
        base_url: None,
    }
}

fn exception_json(exception: &WorkerException) -> String {
    let mut parts = Vec::new();
    if let Some(name) = &exception.name {
        parts.push(format!("\"name\":{}", serde_json::to_string(name).expect("name")));
    }
    if let Some(message) = &exception.message {
        parts.push(format!("\"message\":{}", serde_json::to_string(message).expect("message")));
    }
    if let Some(stack) = &exception.stack {
        parts.push(format!("\"stack\":{}", serde_json::to_string(stack).expect("stack")));
    }
    format!("{{{}}}", parts.join(","))
}

fn exception_value(exception: &WorkerException) -> Value {
    let mut error = Map::new();
    if let Some(name) = &exception.name {
        error.insert("name".to_string(), Value::String(name.clone()));
    }
    if let Some(message) = &exception.message {
        error.insert("message".to_string(), Value::String(message.clone()));
    }
    if let Some(stack) = &exception.stack {
        error.insert("stack".to_string(), Value::String(stack.clone()));
    }
    Value::Object(error)
}

fn format_error(error: &PostMessageError) -> String {
    let mut result = match (&error.name, &error.message) {
        (Some(name), Some(message)) => format!("{name}: {message}"),
        _ => error.to_string.clone(),
    };
    if let Some(stack) = &error.stack {
        result.push('\n');
        result.push_str(stack);
    }
    result
}

fn message_value(id: Option<&Value>, result: Option<&Value>, error: Option<&Value>) -> Value {
    let mut message = Map::new();
    if let Some(id) = id {
        message.insert("id".to_string(), id.clone());
    }
    if let Some(result) = result {
        message.insert("result".to_string(), result.clone());
    }
    if let Some(error) = error {
        message.insert("error".to_string(), error.clone());
    }
    Value::Object(message)
}

/// `JSON.stringify` field order: `id`, `result`, `error`. `error_json` is already encoded.
fn stringify_message(id: Option<&Value>, result: Option<&Value>, error_json: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(id) = id {
        parts.push(format!("\"id\":{}", serde_json::to_string(id).expect("id")));
    }
    if let Some(result) = result {
        parts.push(format!("\"result\":{}", serde_json::to_string(result).expect("result")));
    }
    if let Some(error_json) = error_json {
        parts.push(format!("\"error\":{error_json}"));
    }
    format!("{{{}}}", parts.join(","))
}
