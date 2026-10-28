//! `quipu._quipu`: the native half of the `quipu` Python package.
//!
//! One class, `Engine`, wraps the shared [`quipu::bindings::Engine`] facade.
//! Every call dispatches through that registry, so handlers, transactions,
//! vocabulary and the SHACL write gate are the native ones. The Pythonic
//! surface (context manager, typed results) lives in `python/quipu`.
//!
//! Thread affinity is explicit, not inferred from the GIL: a handle refuses
//! any call from a thread other than the one that opened it, with a typed
//! `ThreadAffinityError` whose `commit_state` is `not_started`. The GIL is
//! held for the whole call (no `allow_threads`), but Python code CAN run
//! while the store mutex is held: building a typed exception calls
//! `quipu.errors._from_native`, and allocations may run finalizers. Either can
//! hand the GIL to another thread, so nothing reachable from another thread
//! may block on that mutex: every acquisition is a `try_lock`.

use std::sync::{Mutex, TryLockError};
use std::thread::ThreadId;

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use quipu::bindings::{BindingError, Engine as Facade};
use serde_json::{Map, Number, Value};

/// A local Quipu store handle, bound to the thread that opened it.
#[pyclass(name = "Engine", module = "quipu._quipu", frozen)]
struct Engine {
    owner: ThreadId,
    inner: Mutex<Option<Facade>>,
}

/// A binding refusal raised before the facade is entered.
struct Refusal {
    code: &'static str,
    operation: String,
    message: String,
}

fn refusal(code: &'static str, operation: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        operation: operation.into(),
        message: message.into(),
    }
}

/// Build the matching `quipu.errors` exception, carrying the facade's fields.
fn raise(py: Python<'_>, error: &Value) -> PyErr {
    let build = || -> PyResult<PyErr> {
        let errors = py.import("quipu.errors")?;
        let instance = errors
            .getattr("_from_native")?
            .call1((to_py(py, error)?,))?;
        Ok(PyErr::from_value(instance))
    };
    build().unwrap_or_else(|e| e)
}

fn raise_binding(py: Python<'_>, error: &BindingError) -> PyErr {
    match serde_json::to_value(error) {
        Ok(value) => raise(py, &value),
        Err(e) => raise_refusal(
            py,
            &refusal("serialization_error", &error.operation, e.to_string()),
        ),
    }
}

fn raise_refusal(py: Python<'_>, r: &Refusal) -> PyErr {
    raise(
        py,
        &serde_json::json!({
            "code": r.code,
            "operation": r.operation,
            "commit_state": "not_started",
            "retryable": false,
            "message": r.message,
            "details": null,
        }),
    )
}

impl Engine {
    fn new(facade: Facade) -> Self {
        Self {
            owner: std::thread::current().id(),
            inner: Mutex::new(Some(facade)),
        }
    }

    /// Run `f` on the open facade from the owning thread, or refuse.
    fn with<T>(
        &self,
        py: Python<'_>,
        operation: &str,
        f: impl FnOnce(&mut Facade) -> PyResult<T>,
    ) -> PyResult<T> {
        if std::thread::current().id() != self.owner {
            return Err(raise_refusal(
                py,
                &refusal(
                    "thread_affinity",
                    operation,
                    "this store handle belongs to the thread that opened it; open a separate store per thread",
                ),
            ));
        }
        let mut guard = match self.inner.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => {
                return Err(raise_refusal(
                    py,
                    &refusal(
                        "busy",
                        operation,
                        "store handle is already in a call (no reentrant calls)",
                    ),
                ));
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(raise_refusal(
                    py,
                    &refusal(
                        "poisoned",
                        operation,
                        "an earlier call panicked; reopen the store",
                    ),
                ));
            }
        };
        match guard.as_mut() {
            Some(facade) => f(facade),
            None => Err(raise_refusal(
                py,
                &refusal("store_closed", operation, "store is closed"),
            )),
        }
    }
}

#[pymethods]
impl Engine {
    /// A fresh memory store with no adopted schema.
    #[staticmethod]
    fn memory(py: Python<'_>) -> PyResult<Self> {
        Facade::open_in_memory()
            .map(Self::new)
            .map_err(|e| raise_refusal(py, &refusal("open_failed", "open", e.to_string())))
    }

    /// Open (or create) a file-backed store at `path`. Never a file a running
    /// quipu-server serves: see `quipu::bindings::Engine::open`.
    #[staticmethod]
    fn open(py: Python<'_>, path: &str) -> PyResult<Self> {
        Facade::open(path)
            .map(Self::new)
            .map_err(|e| raise_refusal(py, &refusal("open_failed", "open", e.to_string())))
    }

    /// The capability manifest as Python objects.
    fn capabilities(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.with(py, "capabilities", |facade| {
            to_py(py, &facade.capabilities())
        })
    }

    /// Invoke one exact native operation with a dict input; returns Python objects.
    fn invoke(
        &self,
        py: Python<'_>,
        operation: &str,
        input: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let input = from_py(input)
            .map_err(|message| raise_refusal(py, &refusal("invalid_input", operation, message)))?;
        self.with(py, operation, |facade| {
            match facade.invoke(operation, &input) {
                Ok(value) => to_py(py, &value),
                Err(error) => Err(raise_binding(py, &error)),
            }
        })
    }

    /// Invoke with JSON text in and out, preserving native lexemes exactly.
    fn invoke_json(&self, py: Python<'_>, operation: &str, input: &str) -> PyResult<String> {
        self.with(py, operation, |facade| {
            facade
                .invoke_json(operation, input)
                .map_err(|error| raise_binding(py, &error))
        })
    }

    /// Release the store. Idempotent from the owning thread.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        if std::thread::current().id() != self.owner {
            return self.with(py, "close", |_| Ok(()));
        }
        match self.inner.try_lock() {
            Ok(mut guard) => {
                guard.take();
                Ok(())
            }
            Err(_) => self.with(py, "close", |_| Ok(())),
        }
    }

    /// Whether `close` has run. Never blocks: a handle mid-call (mutex held,
    /// possibly with the GIL handed to this thread) is open, so `False`.
    #[getter]
    fn closed(&self) -> bool {
        match self.inner.try_lock() {
            Ok(guard) => guard.is_none(),
            Err(TryLockError::WouldBlock) => false,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().is_none(),
        }
    }
}

fn to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    Ok(match value {
        Value::Null => py.None(),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any().unbind(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py)?.into_any().unbind()
            } else if let Some(u) = n.as_u64() {
                u.into_pyobject(py)?.into_any().unbind()
            } else {
                PyFloat::new(py, n.as_f64().unwrap_or(f64::NAN))
                    .into_any()
                    .unbind()
            }
        }
        Value::String(s) => PyString::new(py, s).into_any().unbind(),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(to_py(py, item)?)?;
            }
            list.into_any().unbind()
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, to_py(py, v)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

/// Convert a Python input to JSON. Refuses (rather than coerces) anything
/// without an exact JSON form: non-str keys, non-finite floats, ints beyond
/// 64 bits, and arbitrary objects.
fn from_py(obj: &Bound<'_, PyAny>) -> Result<Value, String> {
    if obj.is_none() {
        return Ok(Value::Null);
    }
    // bool is a subclass of int in Python: test it first.
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.is_instance_of::<PyInt>() {
        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::from(i));
        }
        if let Ok(u) = obj.extract::<u64>() {
            return Ok(Value::from(u));
        }
        return Err("integer does not fit in 64 bits".into());
    }
    if let Ok(f) = obj.cast::<PyFloat>() {
        return Number::from_f64(f.value())
            .map(Value::Number)
            .ok_or_else(|| "non-finite float has no JSON form".into());
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return s
            .to_str()
            .map(|s| Value::String(s.into()))
            .map_err(|e| e.to_string());
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut map = Map::new();
        for (k, v) in dict.iter() {
            let key = k
                .cast::<PyString>()
                .map_err(|_| "dict keys must be str".to_string())?
                .to_str()
                .map_err(|e| e.to_string())?
                .to_owned();
            map.insert(key, from_py(&v)?);
        }
        return Ok(Value::Object(map));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        return list
            .iter()
            .map(|v| from_py(&v))
            .collect::<Result<_, _>>()
            .map(Value::Array);
    }
    if let Ok(tuple) = obj.cast::<PyTuple>() {
        return tuple
            .iter()
            .map(|v| from_py(&v))
            .collect::<Result<_, _>>()
            .map(Value::Array);
    }
    let type_name = obj
        .get_type()
        .name()
        .map_or_else(|_| "?".to_string(), |n| n.to_string());
    Err(format!("{type_name} has no JSON form"))
}

#[pymodule]
fn _quipu(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Engine>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("CORE_VERSION", quipu::VERSION)?;
    Ok(())
}
