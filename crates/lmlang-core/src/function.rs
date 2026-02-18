//! Function definitions with closure and nesting support.
//!
//! [`FunctionDef`] is the full function metadata -- the function body lives as
//! compute nodes owned by this function's ID in the flat computational graph.
//! See Pattern 4 in RESEARCH.md.
//!
//! Closures are functions with non-empty [`captures`](FunctionDef::captures)
//! and [`is_closure`](FunctionDef::is_closure) set to `true`. Nested functions
//! reference their enclosing function via [`parent_function`](FunctionDef::parent_function).

use serde::{Deserialize, Serialize};

use crate::id::{FunctionId, ModuleId, NodeId};
use crate::type_id::TypeId;
use crate::types::Visibility;

/// How a variable is captured by a closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureMode {
    /// Capture by value (move semantics).
    ByValue,
    /// Capture by immutable reference.
    ByRef,
    /// Capture by mutable reference.
    ByMutRef,
}

/// A single captured variable in a closure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    /// Original variable name in the enclosing scope.
    pub name: String,
    /// Type of the captured value.
    pub captured_type: TypeId,
    /// How this variable is captured.
    pub mode: CaptureMode,
}

/// Full function definition including identity, signature, closure captures,
/// and nesting information.
///
/// Functions are metadata -- the function body lives as compute nodes owned by
/// this function's ID in the flat computational graph. See Pattern 4 in
/// RESEARCH.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    /// Unique identity for this function.
    pub id: FunctionId,
    /// Function name.
    pub name: String,
    /// Which module owns this function.
    pub module: ModuleId,
    /// Visibility across module boundaries (pub/private).
    pub visibility: Visibility,
    /// Named, typed parameters in declaration order.
    pub params: Vec<(String, TypeId)>,
    /// Return type (use `TypeId::UNIT` for void-like functions).
    pub return_type: TypeId,
    /// Entry point in the compute graph. `None` if the function body has not
    /// yet been constructed.
    pub entry_node: Option<NodeId>,
    /// Captured variables for closures (empty for non-closures).
    ///
    /// Closures are functions with non-empty captures. Inside the function
    /// body, `CaptureAccess { index }` nodes reference this list by position.
    /// At the call site, `MakeClosure { function }` takes captured values as
    /// data flow inputs.
    pub captures: Vec<Capture>,
    /// `true` if this is a closure.
    pub is_closure: bool,
    /// For nested functions/closures, the enclosing function. `None` for
    /// top-level functions.
    ///
    /// Supports nesting per user decision. Inner functions can capture from
    /// enclosing scope. Nesting depth is unbounded.
    pub parent_function: Option<FunctionId>,
}

impl FunctionDef {
    /// Creates a non-closure, top-level, public function with no captures and
    /// no entry node yet.
    pub fn new(
        id: FunctionId,
        name: String,
        module: ModuleId,
        params: Vec<(String, TypeId)>,
        return_type: TypeId,
    ) -> Self {
        FunctionDef {
            id,
            name,
            module,
            visibility: Visibility::Public,
            params,
            return_type,
            entry_node: None,
            captures: Vec::new(),
            is_closure: false,
            parent_function: None,
        }
    }

    /// Creates a closure with `is_closure=true` and `parent_function` set.
    pub fn closure(
        id: FunctionId,
        name: String,
        module: ModuleId,
        parent: FunctionId,
        params: Vec<(String, TypeId)>,
        return_type: TypeId,
        captures: Vec<Capture>,
    ) -> Self {
        FunctionDef {
            id,
            name,
            module,
            visibility: Visibility::Private,
            params,
            return_type,
            entry_node: None,
            captures,
            is_closure: true,
            parent_function: Some(parent),
        }
    }

    /// Returns `true` if this function is a closure.
    pub fn is_closure(&self) -> bool {
        self.is_closure
    }

    /// Returns `true` if this function is nested inside another function.
    pub fn is_nested(&self) -> bool {
        self.parent_function.is_some()
    }

    /// Returns the number of parameters.
    pub fn arity(&self) -> usize {
        self.params.len()
    }

    /// Returns the number of captured variables.
    pub fn capture_count(&self) -> usize {
        self.captures.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_function_defaults() {
        let f = FunctionDef::new(
            FunctionId(1),
            "add".into(),
            ModuleId(0),
            vec![
                ("a".into(), TypeId::I32),
                ("b".into(), TypeId::I32),
            ],
            TypeId::I32,
        );

        assert_eq!(f.id, FunctionId(1));
        assert_eq!(f.name, "add");
        assert_eq!(f.module, ModuleId(0));
        assert_eq!(f.visibility, Visibility::Public);
        assert_eq!(f.arity(), 2);
        assert_eq!(f.return_type, TypeId::I32);
        assert!(f.entry_node.is_none());
        assert!(!f.is_closure());
        assert!(!f.is_nested());
        assert!(f.captures.is_empty());
        assert_eq!(f.capture_count(), 0);
    }

    #[test]
    fn closure_with_captures() {
        let captures = vec![
            Capture {
                name: "x".into(),
                captured_type: TypeId::I32,
                mode: CaptureMode::ByRef,
            },
            Capture {
                name: "y".into(),
                captured_type: TypeId::F64,
                mode: CaptureMode::ByValue,
            },
        ];

        let c = FunctionDef::closure(
            FunctionId(10),
            "lambda".into(),
            ModuleId(0),
            FunctionId(1),
            vec![("z".into(), TypeId::I32)],
            TypeId::F64,
            captures,
        );

        assert!(c.is_closure());
        assert!(c.is_nested());
        assert_eq!(c.parent_function, Some(FunctionId(1)));
        assert_eq!(c.capture_count(), 2);
        assert_eq!(c.arity(), 1);
        assert_eq!(c.captures[0].name, "x");
        assert_eq!(c.captures[0].mode, CaptureMode::ByRef);
        assert_eq!(c.captures[1].name, "y");
        assert_eq!(c.captures[1].mode, CaptureMode::ByValue);
    }

    #[test]
    fn serde_roundtrip_function_def_with_captures() {
        let captures = vec![
            Capture {
                name: "counter".into(),
                captured_type: TypeId::I64,
                mode: CaptureMode::ByMutRef,
            },
        ];

        let f = FunctionDef::closure(
            FunctionId(5),
            "incrementer".into(),
            ModuleId(2),
            FunctionId(3),
            vec![("step".into(), TypeId::I32)],
            TypeId::UNIT,
            captures,
        );

        let json = serde_json::to_string(&f).unwrap();
        let back: FunctionDef = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);

        // Verify deserialized values
        assert!(back.is_closure());
        assert_eq!(back.capture_count(), 1);
        assert_eq!(back.captures[0].mode, CaptureMode::ByMutRef);
    }
}
