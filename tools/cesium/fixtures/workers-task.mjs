// createTaskProcessorWorker against the real onmessage / onmessageerror
// adapter. Each script is the Rust test input. The recorded reply is what
// the CesiumJS adapter posts: undefined fields dropped, transfer list absent
// when postMessage is retried without one, and CESIUM_BASE_URL after the call.
import createTaskProcessorWorker from "@cesium/engine/Source/Workers/createTaskProcessorWorker.js";

function raiseWorker(spec) {
  if (spec.type === "undefined") throw undefined;
  if (spec.type === "value") throw spec.value;
  const error = new Error("placeholder");
  if ("name" in spec) error.name = spec.name === null ? undefined : spec.name;
  else error.name = "Error";
  error.message = "message" in spec && spec.message === null ? undefined : spec.message ?? "";
  error.stack = "stack" in spec && spec.stack !== null ? spec.stack : undefined;
  throw error;
}

function raisePost(spec) {
  if (spec.type === "string") throw spec.string;
  if (spec.type === "object") {
    const text = spec.string;
    throw { toString() { return text; } };
  }
  const error = new Error("placeholder");
  if ("name" in spec) error.name = spec.name === null ? undefined : spec.name;
  else error.name = "Error";
  if ("message" in spec) error.message = spec.message === null ? undefined : spec.message;
  else error.message = "";
  error.stack = "stack" in spec && spec.stack !== null ? spec.stack : undefined;
  throw error;
}

function restore(previousSelf, previousPost) {
  if (previousSelf === undefined) delete globalThis.self;
  else globalThis.self = previousSelf;
  if (previousPost === undefined) delete globalThis.postMessage;
  else globalThis.postMessage = previousPost;
}

async function runMessage(script) {
  const previousSelf = globalThis.self;
  const previousPost = globalThis.postMessage;
  globalThis.self = {};
  let last = null;
  let attempted = false;
  try {
    const worker = createTaskProcessorWorker(async (parameters, transferableObjects) => {
      for (const item of script.transfers ?? []) transferableObjects.push(item);
      if (script.throw) raiseWorker(script.throw);
      if (script.echoParameters) return parameters;
      if ("returns" in script) return script.returns;
      return undefined;
    });
    globalThis.self = worker;
    globalThis.postMessage = function (message, transfers) {
      if (script.postThrows && !attempted) {
        attempted = true;
        raisePost(script.postThrows);
      }
      last = {
        message: JSON.parse(JSON.stringify(message)),
        transfers: arguments.length > 1 ? JSON.parse(JSON.stringify(transfers)) : null,
      };
    };
    const data = {};
    if ("id" in script) data.id = script.id;
    if ("parameters" in script) data.parameters = script.parameters;
    if ("baseUrl" in script) data.baseUrl = script.baseUrl;
    if ("canTransferArrayBuffer" in script) data.canTransferArrayBuffer = script.canTransferArrayBuffer;
    await worker.onmessage({ data });
    if (!last) throw new Error("worker posted no reply");
    return { message: last.message, transfers: last.transfers, baseUrl: worker.CESIUM_BASE_URL ?? null };
  } finally {
    restore(previousSelf, previousPost);
  }
}

async function runMessageError(script) {
  const previousSelf = globalThis.self;
  const previousPost = globalThis.postMessage;
  globalThis.self = {};
  let last = null;
  try {
    const worker = createTaskProcessorWorker(async () => 1);
    globalThis.self = worker;
    globalThis.postMessage = function (message, transfers) {
      last = {
        message: JSON.parse(JSON.stringify(message)),
        transfers: arguments.length > 1 ? JSON.parse(JSON.stringify(transfers)) : null,
      };
    };
    const event = script.hideData ? {} : script.event;
    if (script.hideData) Object.defineProperty(event, "data", { value: { id: script.id }, enumerable: false });
    await worker.onmessageerror(event);
    if (!last) throw new Error("messageerror posted no reply");
    return { message: last.message, transfers: last.transfers, baseUrl: worker.CESIUM_BASE_URL ?? null };
  } finally {
    restore(previousSelf, previousPost);
  }
}

const messages = [
  { id: 7, parameters: { x: 1 }, baseUrl: "https://cesium.test/", canTransferArrayBuffer: true, returns: { v: 1 }, transfers: [{ m: "a" }] },
  { id: 7, parameters: {}, baseUrl: "https://cesium.test/", canTransferArrayBuffer: false, returns: { v: 1 }, transfers: [{ m: "a" }] },
  { id: 7, returns: { v: 1 }, transfers: ["a"] },
  { id: 0, baseUrl: "https://cesium.test/", canTransferArrayBuffer: true },
  { id: 0, canTransferArrayBuffer: true, returns: null },
  { id: 3, canTransferArrayBuffer: true, transfers: ["t"], throw: { type: "error", name: "RangeError", message: "bad" } },
  { id: 3, canTransferArrayBuffer: true, throw: { type: "error", name: "Error", message: "bad", stack: "at worker.js:1" } },
  { id: 3, canTransferArrayBuffer: true, throw: { type: "value", value: "nope" } },
  { id: 3, canTransferArrayBuffer: true, throw: { type: "value", value: 3 } },
  { id: 3, canTransferArrayBuffer: true, throw: { type: "value", value: null } },
  { id: 3, canTransferArrayBuffer: true, throw: { type: "value", value: { code: 1 } } },
  { id: 3, canTransferArrayBuffer: true, throw: { type: "undefined" } },
  { id: 4, canTransferArrayBuffer: true, transfers: ["kept"], throw: { type: "undefined" } },
  { id: 4, canTransferArrayBuffer: false, transfers: ["dropped"], throw: { type: "error", name: "Error", message: "bad" } },
  { id: 7, canTransferArrayBuffer: true, returns: { v: 1 }, transfers: ["a"], postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 7, canTransferArrayBuffer: true, returns: { v: 1 }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed", stack: "at post:1" } },
  { id: 7, canTransferArrayBuffer: true, throw: { type: "error", name: "Error", message: "worker failed" }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 7, canTransferArrayBuffer: true, throw: { type: "error", name: "Error", message: "bad", stack: "at worker.js:1" }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 7, canTransferArrayBuffer: true, throw: { type: "value", value: "nope" }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 7, canTransferArrayBuffer: true, returns: { ok: true }, postThrows: { type: "string", string: "boom" } },
  { id: 7, canTransferArrayBuffer: true, returns: { ok: true }, postThrows: { type: "object", string: "custom" } },
  { canTransferArrayBuffer: true, returns: { v: 1 }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 8, canTransferArrayBuffer: true, returns: null, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 1, canTransferArrayBuffer: true, returns: 1 },
  { id: 1, baseUrl: null, canTransferArrayBuffer: true, returns: 1 },
  { id: 1, canTransferArrayBuffer: true, returns: 1, postThrows: { type: "error", name: "", message: "", stack: "" } },
  { id: 1, canTransferArrayBuffer: true, returns: 1, postThrows: { type: "error", name: "OnlyName", message: null, string: "OnlyName" } },
  { id: 1, canTransferArrayBuffer: true, returns: 1, postThrows: { type: "error", name: null, message: "m", string: "Error: m" } },
  { id: 2, parameters: { x: 1, y: [2, 3] }, canTransferArrayBuffer: true, echoParameters: true },
  { id: 6, baseUrl: "https://cesium.test/thrown", canTransferArrayBuffer: true, throw: { type: "error", name: "Error", message: "bad" } },
  { id: 6, baseUrl: "https://cesium.test/post", canTransferArrayBuffer: true, returns: { v: 1 }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 7, canTransferArrayBuffer: true, throw: { type: "value", value: { z: 1, a: 2, m: 3 } }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
  { id: 3, canTransferArrayBuffer: true, throw: { type: "error", name: null, message: "bad" } },
  { id: 7, canTransferArrayBuffer: true, throw: { type: "error", name: null, message: "bad", stack: "at worker.js:1" }, postThrows: { type: "error", name: "DataCloneError", message: "clone failed" } },
];

const messageErrors = [
  { id: 5, event: {}, hideData: true },
  { id: 9, event: { data: { id: 9 }, type: "messageerror" } },
  { event: { type: "messageerror" } },
  { event: { data: null } },
  { id: 4, event: { z: 1, data: { id: 4 }, type: "messageerror" } },
];

export default async function workersTask() {
  const cases = [];
  for (const script of messages) cases.push({ f: "onmessage", a: [script], r: await runMessage(script) });
  for (const script of messageErrors) cases.push({ f: "onmessageerror", a: [script], r: await runMessageError(script) });
  return [{ module: "createTaskProcessorWorker", cases }];
}
