//! Per-op type rule resolution for all ComputeNodeOp variants.
//!
//! Each operation defines what types it expects on its input ports and what
//! type it produces. The [`resolve_type_rule`] function performs exhaustive
//! matching on all op variants with NO wildcard match arms.

use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::ops::{ComputeNodeOp, ComputeOp, StructuredOp};
use lmlang_core::type_id::TypeId;
use lmlang_core::types::{ConstValue, LmType};

use super::coercion::{
    can_coerce, common_numeric_type, is_integer, is_numeric, is_numeric_or_bool,
};
use super::diagnostics::TypeError;

/// Result of resolving the type rule for an op.
///
/// Contains the expected input types (by port) and the output type (if any).
#[derive(Debug, Clone)]
pub struct OpTypeRule {
    /// Expected type at each input port: (port_index, expected_type).
    pub expected_inputs: Vec<(u16, TypeId)>,
    /// Output type produced by this op. `None` for void ops (Store, Return, etc.).
    pub output_type: Option<TypeId>,
}

/// Resolve the type rule for an op given its incoming edge types.
///
/// This is the core type-checking logic: for each op variant, it defines what
/// types are expected on each input port and what output type is produced.
///
/// # Parameters
/// - `op`: The operation to resolve rules for.
/// - `input_types`: The types currently connected to input ports: `(port, type)`.
/// - `graph`: The program graph, needed for function lookups and type registry.
/// - `node_id`: The node being checked (for error context).
/// - `function_id`: The function containing this node (for error context).
pub fn resolve_type_rule(
    op: &ComputeNodeOp,
    input_types: &[(u16, TypeId)],
    graph: &ProgramGraph,
    node_id: NodeId,
    function_id: FunctionId,
) -> Result<OpTypeRule, TypeError> {
    match op {
        ComputeNodeOp::Core(core_op) => {
            resolve_core_rule(core_op, input_types, graph, node_id, function_id)
        }
        ComputeNodeOp::Structured(struct_op) => {
            resolve_structured_rule(struct_op, input_types, graph, node_id, function_id)
        }
    }
}

/// Resolve type rule for a Tier 1 (core) operation.
fn resolve_core_rule(
    op: &ComputeOp,
    input_types: &[(u16, TypeId)],
    graph: &ProgramGraph,
    node_id: NodeId,
    function_id: FunctionId,
) -> Result<OpTypeRule, TypeError> {
    let registry = &graph.types;

    match op {
        // -- Constants & Literals --
        ComputeOp::Const { value } => {
            let output = const_value_type(value);
            Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: Some(output),
            })
        }

        // -- Arithmetic (grouped) --
        ComputeOp::BinaryArith { .. } => {
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);

            match (port0, port1) {
                (Some(t0), Some(t1)) => {
                    if !is_numeric_or_bool(t0) {
                        return Err(TypeError::NonNumericArithmetic {
                            node: node_id,
                            type_id: t0,
                            function_id,
                        });
                    }
                    if !is_numeric_or_bool(t1) {
                        return Err(TypeError::NonNumericArithmetic {
                            node: node_id,
                            type_id: t1,
                            function_id,
                        });
                    }
                    match common_numeric_type(t0, t1, registry) {
                        Some(common) => Ok(OpTypeRule {
                            expected_inputs: vec![(0, common), (1, common)],
                            output_type: Some(common),
                        }),
                        None => Err(TypeError::TypeMismatch {
                            source_node: node_id,
                            target_node: node_id,
                            source_port: 0,
                            target_port: 1,
                            expected: t0,
                            actual: t1,
                            function_id,
                            suggestion: None,
                        }),
                    }
                }
                _ => {
                    // Not enough inputs yet -- return rule with placeholders
                    Ok(OpTypeRule {
                        expected_inputs: vec![],
                        output_type: None,
                    })
                }
            }
        }

        ComputeOp::UnaryArith { .. } => match find_port_type(input_types, 0) {
            Some(t) => {
                if !is_numeric(t) {
                    return Err(TypeError::NonNumericArithmetic {
                        node: node_id,
                        type_id: t,
                        function_id,
                    });
                }
                Ok(OpTypeRule {
                    expected_inputs: vec![(0, t)],
                    output_type: Some(t),
                })
            }
            None => Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: None,
            }),
        },

        // -- Comparison --
        ComputeOp::Compare { .. } => {
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);

            match (port0, port1) {
                (Some(t0), Some(t1)) => {
                    // Both must be the same type after coercion
                    if can_coerce(t0, t1, registry) || can_coerce(t1, t0, registry) {
                        let common = if can_coerce(t0, t1, registry) { t1 } else { t0 };
                        Ok(OpTypeRule {
                            expected_inputs: vec![(0, common), (1, common)],
                            output_type: Some(TypeId::BOOL),
                        })
                    } else {
                        Err(TypeError::TypeMismatch {
                            source_node: node_id,
                            target_node: node_id,
                            source_port: 0,
                            target_port: 1,
                            expected: t0,
                            actual: t1,
                            function_id,
                            suggestion: None,
                        })
                    }
                }
                _ => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: Some(TypeId::BOOL),
                }),
            }
        }

        // -- Logic --
        ComputeOp::BinaryLogic { .. } => {
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);

            match (port0, port1) {
                (Some(t0), Some(t1)) => {
                    // Both must be Bool or same integer type (bitwise)
                    let ok =
                        (t0 == TypeId::BOOL && t1 == TypeId::BOOL) || (is_integer(t0) && t0 == t1);
                    if ok {
                        Ok(OpTypeRule {
                            expected_inputs: vec![(0, t0), (1, t1)],
                            output_type: Some(t0),
                        })
                    } else {
                        Err(TypeError::TypeMismatch {
                            source_node: node_id,
                            target_node: node_id,
                            source_port: 0,
                            target_port: 1,
                            expected: t0,
                            actual: t1,
                            function_id,
                            suggestion: None,
                        })
                    }
                }
                _ => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        ComputeOp::Not => match find_port_type(input_types, 0) {
            Some(t) => {
                if t == TypeId::BOOL || is_integer(t) {
                    Ok(OpTypeRule {
                        expected_inputs: vec![(0, t)],
                        output_type: Some(t),
                    })
                } else {
                    Err(TypeError::TypeMismatch {
                        source_node: node_id,
                        target_node: node_id,
                        source_port: 0,
                        target_port: 0,
                        expected: TypeId::BOOL,
                        actual: t,
                        function_id,
                        suggestion: None,
                    })
                }
            }
            None => Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: None,
            }),
        },

        // -- Bitwise --
        ComputeOp::Shift { .. } => {
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);

            match (port0, port1) {
                (Some(t0), Some(t1)) => {
                    if !is_integer(t0) {
                        return Err(TypeError::NonNumericArithmetic {
                            node: node_id,
                            type_id: t0,
                            function_id,
                        });
                    }
                    if !is_integer(t1) {
                        return Err(TypeError::NonNumericArithmetic {
                            node: node_id,
                            type_id: t1,
                            function_id,
                        });
                    }
                    Ok(OpTypeRule {
                        expected_inputs: vec![(0, t0), (1, t1)],
                        output_type: Some(t0),
                    })
                }
                _ => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        // -- Control Flow (high-level structured) --
        ComputeOp::IfElse => {
            // Port 0 = condition (Bool)
            match find_port_type(input_types, 0) {
                Some(t) => {
                    if t != TypeId::BOOL {
                        return Err(TypeError::NonBooleanCondition {
                            node: node_id,
                            actual: t,
                            function_id,
                        });
                    }
                    Ok(OpTypeRule {
                        expected_inputs: vec![(0, TypeId::BOOL)],
                        output_type: None, // Output determined by branch results via control edges
                    })
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        ComputeOp::Loop => {
            // Port 0 = initial value
            match find_port_type(input_types, 0) {
                Some(t) => Ok(OpTypeRule {
                    expected_inputs: vec![(0, t)],
                    output_type: Some(t),
                }),
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        ComputeOp::Match => {
            // Port 0 = discriminant (integer or enum)
            match find_port_type(input_types, 0) {
                Some(t) => {
                    // Accept integer types and enum types
                    Ok(OpTypeRule {
                        expected_inputs: vec![(0, t)],
                        output_type: None, // Branch selection, no direct output
                    })
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        // -- Control Flow (low-level) --
        ComputeOp::Branch => {
            // Port 0 = condition (Bool)
            match find_port_type(input_types, 0) {
                Some(t) => {
                    if t != TypeId::BOOL {
                        return Err(TypeError::NonBooleanCondition {
                            node: node_id,
                            actual: t,
                            function_id,
                        });
                    }
                    Ok(OpTypeRule {
                        expected_inputs: vec![(0, TypeId::BOOL)],
                        output_type: None,
                    })
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        ComputeOp::Jump => Ok(OpTypeRule {
            expected_inputs: vec![],
            output_type: None,
        }),

        ComputeOp::Phi => {
            // N data inputs, all must be same type
            if input_types.is_empty() {
                return Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                });
            }
            let first_type = input_types[0].1;
            let mut expected = Vec::new();
            for &(port, ty) in input_types {
                if !can_coerce(ty, first_type, registry) && !can_coerce(first_type, ty, registry) {
                    return Err(TypeError::TypeMismatch {
                        source_node: node_id,
                        target_node: node_id,
                        source_port: port,
                        target_port: 0,
                        expected: first_type,
                        actual: ty,
                        function_id,
                        suggestion: None,
                    });
                }
                expected.push((port, first_type));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(first_type),
            })
        }

        // -- Memory --
        ComputeOp::Alloc => {
            // 0 data inputs. Output = pointer type (mutable).
            // The actual pointer type depends on what gets stored; for now the
            // output type is determined by the context (Store will provide the value type).
            // We return UNIT as a placeholder -- the caller should use context to
            // determine the actual allocated type.
            Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: None, // Type determined by usage context
            })
        }

        ComputeOp::Load => {
            // Port 0 = pointer. Output = pointee type.
            match find_port_type(input_types, 0) {
                Some(ptr_type) => {
                    match registry.get(ptr_type) {
                        Some(LmType::Pointer { pointee, .. }) => Ok(OpTypeRule {
                            expected_inputs: vec![(0, ptr_type)],
                            output_type: Some(*pointee),
                        }),
                        _ => {
                            // Not a pointer type -- accept for now, actual validation
                            // happens at edge level
                            Ok(OpTypeRule {
                                expected_inputs: vec![(0, ptr_type)],
                                output_type: None,
                            })
                        }
                    }
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        ComputeOp::Store => {
            // Port 0 = pointer, Port 1 = value. No output.
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);

            let mut expected = Vec::new();
            if let Some(t0) = port0 {
                expected.push((0, t0));
            }
            if let Some(t1) = port1 {
                expected.push((1, t1));
            }

            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: None,
            })
        }

        ComputeOp::GetElementPtr => {
            // Port 0 = pointer to aggregate, Port 1 = index (integer)
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);

            let mut expected = Vec::new();
            if let Some(t0) = port0 {
                expected.push((0, t0));
            }
            if let Some(t1) = port1 {
                if !is_integer(t1) {
                    return Err(TypeError::NonNumericArithmetic {
                        node: node_id,
                        type_id: t1,
                        function_id,
                    });
                }
                expected.push((1, t1));
            }

            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: port0, // Output is a pointer to element (simplified)
            })
        }

        // -- Functions --
        ComputeOp::Call { target } => {
            match graph.get_function(*target) {
                Some(func_def) => {
                    let expected: Vec<(u16, TypeId)> = func_def
                        .params
                        .iter()
                        .enumerate()
                        .map(|(i, (_, ty))| (i as u16, *ty))
                        .collect();
                    Ok(OpTypeRule {
                        expected_inputs: expected,
                        output_type: Some(func_def.return_type),
                    })
                }
                None => {
                    // Function not found -- let the caller handle this
                    Ok(OpTypeRule {
                        expected_inputs: vec![],
                        output_type: None,
                    })
                }
            }
        }

        ComputeOp::IndirectCall => {
            // Port 0 = function pointer, remaining ports = arguments
            // Output = return type from function type
            match find_port_type(input_types, 0) {
                Some(fn_ptr_type) => match registry.get(fn_ptr_type) {
                    Some(LmType::Function {
                        params,
                        return_type,
                    }) => {
                        let mut expected = vec![(0, fn_ptr_type)];
                        for (i, param_ty) in params.iter().enumerate() {
                            expected.push(((i + 1) as u16, *param_ty));
                        }
                        Ok(OpTypeRule {
                            expected_inputs: expected,
                            output_type: Some(*return_type),
                        })
                    }
                    _ => Ok(OpTypeRule {
                        expected_inputs: vec![(0, fn_ptr_type)],
                        output_type: None,
                    }),
                },
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        ComputeOp::Return => {
            // 0 or 1 data input. No output.
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: None,
            })
        }

        ComputeOp::Parameter { index } => {
            // 0 data inputs. Output = parameter type from FunctionDef.
            match graph.get_function(function_id) {
                Some(func_def) => {
                    let param_type = func_def.params.get(*index as usize).map(|(_, ty)| *ty);
                    Ok(OpTypeRule {
                        expected_inputs: vec![],
                        output_type: param_type,
                    })
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        // -- I/O (console) --
        ComputeOp::Print => {
            // 1 data input (any type). No output (Unit).
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(TypeId::UNIT),
            })
        }

        ComputeOp::ReadLine => {
            // 0 data inputs. Output = I64 as placeholder (no string type yet).
            Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: Some(TypeId::I64),
            })
        }

        // -- I/O (file) --
        ComputeOp::FileOpen => {
            // Accept inputs (file path). Output = I64 (file handle placeholder).
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(TypeId::I64),
            })
        }

        ComputeOp::FileRead => {
            // Port 0 = file handle. Output = I64 (data placeholder).
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(TypeId::I64),
            })
        }

        ComputeOp::FileWrite => {
            // Port 0 = file handle, Port 1 = data. Output = I64 (bytes written).
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            if let Some(t) = find_port_type(input_types, 1) {
                expected.push((1, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(TypeId::I64),
            })
        }

        ComputeOp::FileClose => {
            // Port 0 = file handle. No output.
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(TypeId::UNIT),
            })
        }

        // -- Closures --
        ComputeOp::MakeClosure { function } => {
            // N data inputs (captured values). Output = function type.
            match graph.get_function(*function) {
                Some(func_def) => {
                    let expected: Vec<(u16, TypeId)> = func_def
                        .captures
                        .iter()
                        .enumerate()
                        .map(|(i, cap)| (i as u16, cap.captured_type))
                        .collect();

                    // Output is a function type based on the closure's signature
                    let fn_type_id = graph.types.iter()
                        .find(|(_, ty)| matches!(ty, LmType::Function { params, return_type }
                            if params.len() == func_def.params.len()
                            && params.iter().zip(func_def.params.iter()).all(|(p, (_, t))| *p == *t)
                            && *return_type == func_def.return_type
                        ))
                        .map(|(id, _)| id);

                    Ok(OpTypeRule {
                        expected_inputs: expected,
                        output_type: fn_type_id,
                    })
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        ComputeOp::CaptureAccess { index } => {
            // 0 data inputs. Output = captured variable type from FunctionDef.
            match graph.get_function(function_id) {
                Some(func_def) => {
                    let cap_type = func_def
                        .captures
                        .get(*index as usize)
                        .map(|cap| cap.captured_type);
                    Ok(OpTypeRule {
                        expected_inputs: vec![],
                        output_type: cap_type,
                    })
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }

        // -- Contracts (development-time only, no output) --
        ComputeOp::Precondition { .. } => {
            // Port 0 = condition (Bool). No output.
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                if t != TypeId::BOOL {
                    return Err(TypeError::NonBooleanCondition {
                        node: node_id,
                        actual: t,
                        function_id,
                    });
                }
                expected.push((0, TypeId::BOOL));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: None,
            })
        }

        ComputeOp::Postcondition { .. } => {
            // Port 0 = condition (Bool), Port 1 = return value (any type). No output.
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                if t != TypeId::BOOL {
                    return Err(TypeError::NonBooleanCondition {
                        node: node_id,
                        actual: t,
                        function_id,
                    });
                }
                expected.push((0, TypeId::BOOL));
            }
            if let Some(t) = find_port_type(input_types, 1) {
                expected.push((1, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: None,
            })
        }

        ComputeOp::Invariant { .. } => {
            // Port 0 = condition (Bool), Port 1 = value being checked (any type). No output.
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                if t != TypeId::BOOL {
                    return Err(TypeError::NonBooleanCondition {
                        node: node_id,
                        actual: t,
                        function_id,
                    });
                }
                expected.push((0, TypeId::BOOL));
            }
            if let Some(t) = find_port_type(input_types, 1) {
                expected.push((1, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: None,
            })
        }
    }
}

/// Resolve type rule for a Tier 2 (structured) operation.
fn resolve_structured_rule(
    op: &StructuredOp,
    input_types: &[(u16, TypeId)],
    graph: &ProgramGraph,
    node_id: NodeId,
    function_id: FunctionId,
) -> Result<OpTypeRule, TypeError> {
    let registry = &graph.types;

    match op {
        StructuredOp::StructCreate { type_id } => match registry.get(*type_id) {
            Some(LmType::Struct(struct_def)) => {
                let expected: Vec<(u16, TypeId)> = struct_def
                    .fields
                    .values()
                    .enumerate()
                    .map(|(i, ty)| (i as u16, *ty))
                    .collect();
                Ok(OpTypeRule {
                    expected_inputs: expected,
                    output_type: Some(*type_id),
                })
            }
            _ => Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: Some(*type_id),
            }),
        },

        StructuredOp::StructGet { field_index } => match find_port_type(input_types, 0) {
            Some(struct_type) => match registry.get(struct_type) {
                Some(LmType::Struct(struct_def)) => {
                    let field_type = struct_def
                        .fields
                        .values()
                        .nth(*field_index as usize)
                        .copied();
                    Ok(OpTypeRule {
                        expected_inputs: vec![(0, struct_type)],
                        output_type: field_type,
                    })
                }
                _ => Ok(OpTypeRule {
                    expected_inputs: vec![(0, struct_type)],
                    output_type: None,
                }),
            },
            None => Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: None,
            }),
        },

        StructuredOp::StructSet { field_index } => match find_port_type(input_types, 0) {
            Some(struct_type) => {
                let mut expected = vec![(0, struct_type)];
                if let Some(LmType::Struct(struct_def)) = registry.get(struct_type) {
                    if let Some(field_type) = struct_def.fields.values().nth(*field_index as usize)
                    {
                        expected.push((1, *field_type));
                    }
                }
                Ok(OpTypeRule {
                    expected_inputs: expected,
                    output_type: Some(struct_type),
                })
            }
            None => Ok(OpTypeRule {
                expected_inputs: vec![],
                output_type: None,
            }),
        },

        StructuredOp::ArrayCreate { length: _ } => {
            // N inputs all same type. Output = array type.
            if input_types.is_empty() {
                return Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                });
            }
            let elem_type = input_types[0].1;
            let expected: Vec<(u16, TypeId)> = input_types
                .iter()
                .map(|&(port, _)| (port, elem_type))
                .collect();

            // Find or infer array type
            let array_type = registry
                .iter()
                .find(
                    |(_, ty)| matches!(ty, LmType::Array { element, .. } if *element == elem_type),
                )
                .map(|(id, _)| id);

            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: array_type,
            })
        }

        StructuredOp::ArrayGet => {
            // Port 0 = array, Port 1 = index (integer)
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);

            let mut expected = Vec::new();
            let mut output = None;

            if let Some(arr_type) = port0 {
                expected.push((0, arr_type));
                if let Some(LmType::Array { element, .. }) = registry.get(arr_type) {
                    output = Some(*element);
                }
            }
            if let Some(idx_type) = port1 {
                if !is_integer(idx_type) {
                    return Err(TypeError::NonNumericArithmetic {
                        node: node_id,
                        type_id: idx_type,
                        function_id,
                    });
                }
                expected.push((1, idx_type));
            }

            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: output,
            })
        }

        StructuredOp::ArraySet => {
            // Port 0 = array, Port 1 = index, Port 2 = new element
            let port0 = find_port_type(input_types, 0);
            let port1 = find_port_type(input_types, 1);
            let port2 = find_port_type(input_types, 2);

            let mut expected = Vec::new();

            if let Some(arr_type) = port0 {
                expected.push((0, arr_type));

                if let Some(idx_type) = port1 {
                    if !is_integer(idx_type) {
                        return Err(TypeError::NonNumericArithmetic {
                            node: node_id,
                            type_id: idx_type,
                            function_id,
                        });
                    }
                    expected.push((1, idx_type));
                }

                if let Some(elem_type) = port2 {
                    expected.push((2, elem_type));
                }

                Ok(OpTypeRule {
                    expected_inputs: expected,
                    output_type: Some(arr_type),
                })
            } else {
                Ok(OpTypeRule {
                    expected_inputs: expected,
                    output_type: None,
                })
            }
        }

        StructuredOp::Cast { target_type } => {
            // 1 input (any numeric/pointer type). Output = target_type.
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(*target_type),
            })
        }

        StructuredOp::EnumCreate {
            type_id,
            variant_index,
        } => {
            // 0 or 1 input (variant payload). Output = enum type.
            match registry.get(*type_id) {
                Some(LmType::Enum(enum_def)) => {
                    let variant = enum_def.variants.values().nth(*variant_index as usize);
                    let mut expected = Vec::new();
                    if let Some(v) = variant {
                        if let Some(payload_type) = v.payload {
                            expected.push((0, payload_type));
                        }
                    }
                    Ok(OpTypeRule {
                        expected_inputs: expected,
                        output_type: Some(*type_id),
                    })
                }
                _ => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: Some(*type_id),
                }),
            }
        }

        StructuredOp::EnumDiscriminant => {
            // 1 input (enum value). Output = I32 (discriminant integer).
            let mut expected = Vec::new();
            if let Some(t) = find_port_type(input_types, 0) {
                expected.push((0, t));
            }
            Ok(OpTypeRule {
                expected_inputs: expected,
                output_type: Some(TypeId::I32),
            })
        }

        StructuredOp::EnumPayload { variant_index } => {
            // 1 input (enum value). Output = variant payload type.
            match find_port_type(input_types, 0) {
                Some(enum_type) => {
                    let output = match registry.get(enum_type) {
                        Some(LmType::Enum(enum_def)) => enum_def
                            .variants
                            .values()
                            .nth(*variant_index as usize)
                            .and_then(|v| v.payload),
                        _ => None,
                    };
                    Ok(OpTypeRule {
                        expected_inputs: vec![(0, enum_type)],
                        output_type: output,
                    })
                }
                None => Ok(OpTypeRule {
                    expected_inputs: vec![],
                    output_type: None,
                }),
            }
        }
    }
}

/// Helper: find the type connected to a specific port in the input list.
fn find_port_type(input_types: &[(u16, TypeId)], port: u16) -> Option<TypeId> {
    input_types
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, t)| *t)
}

/// Map a ConstValue to its output TypeId.
fn const_value_type(value: &ConstValue) -> TypeId {
    match value {
        ConstValue::Bool(_) => TypeId::BOOL,
        ConstValue::I8(_) => TypeId::I8,
        ConstValue::I16(_) => TypeId::I16,
        ConstValue::I32(_) => TypeId::I32,
        ConstValue::I64(_) => TypeId::I64,
        ConstValue::F32(_) => TypeId::F32,
        ConstValue::F64(_) => TypeId::F64,
        ConstValue::Unit => TypeId::UNIT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::ops::ArithOp;
    use lmlang_core::types::Visibility;

    /// Helper: create a ProgramGraph with a single function and return (graph, function_id).
    fn test_graph_with_function() -> (ProgramGraph, FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();
        let func_id = graph
            .add_function(
                "test_fn".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        (graph, func_id)
    }

    #[test]
    fn const_bool_output_type() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Const {
            value: ConstValue::Bool(true),
        });
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::BOOL));
        assert!(rule.expected_inputs.is_empty());
    }

    #[test]
    fn const_i32_output_type() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Const {
            value: ConstValue::I32(42),
        });
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn const_f64_output_type() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Const {
            value: ConstValue::F64(std::f64::consts::PI),
        });
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::F64));
    }

    #[test]
    fn const_unit_output_type() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Const {
            value: ConstValue::Unit,
        });
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::UNIT));
    }

    #[test]
    fn binary_arith_same_type_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryArith { op: ArithOp::Add });
        let inputs = vec![(0, TypeId::I32), (1, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
        assert_eq!(rule.expected_inputs.len(), 2);
    }

    #[test]
    fn binary_arith_mismatched_types_errors() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryArith { op: ArithOp::Add });
        let inputs = vec![(0, TypeId::I32), (1, TypeId::F64)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(result.is_err());
    }

    #[test]
    fn binary_arith_non_numeric_errors() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryArith { op: ArithOp::Add });
        let inputs = vec![(0, TypeId::UNIT), (1, TypeId::I32)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(matches!(
            result,
            Err(TypeError::NonNumericArithmetic { .. })
        ));
    }

    #[test]
    fn binary_arith_bool_coerces_to_integer() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryArith { op: ArithOp::Add });
        // Bool + Bool should coerce to I8 arithmetic
        let inputs = vec![(0, TypeId::BOOL), (1, TypeId::BOOL)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I8));
    }

    #[test]
    fn binary_arith_bool_and_i32_coerces() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryArith { op: ArithOp::Add });
        let inputs = vec![(0, TypeId::BOOL), (1, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn compare_same_type_produces_bool() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Compare {
            op: lmlang_core::ops::CmpOp::Eq,
        });
        let inputs = vec![(0, TypeId::I32), (1, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::BOOL));
    }

    #[test]
    fn parameter_output_matches_function_def() {
        let (graph, func_id) = test_graph_with_function();
        // Parameter 0 should be I32 (first param of test function)
        let op = ComputeNodeOp::Core(ComputeOp::Parameter { index: 0 });
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn call_matches_target_function_params() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let callee = graph
            .add_function(
                "callee".into(),
                root,
                vec![("x".into(), TypeId::I64), ("y".into(), TypeId::F64)],
                TypeId::BOOL,
                Visibility::Public,
            )
            .unwrap();

        let caller = graph
            .add_function(
                "caller".into(),
                root,
                vec![],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();

        let op = ComputeNodeOp::Core(ComputeOp::Call { target: callee });
        let inputs = vec![(0, TypeId::I64), (1, TypeId::F64)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), caller).unwrap();
        assert_eq!(rule.expected_inputs.len(), 2);
        assert_eq!(rule.expected_inputs[0], (0, TypeId::I64));
        assert_eq!(rule.expected_inputs[1], (1, TypeId::F64));
        assert_eq!(rule.output_type, Some(TypeId::BOOL));
    }

    #[test]
    fn cast_produces_target_type() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Structured(StructuredOp::Cast {
            target_type: TypeId::I64,
        });
        let inputs = vec![(0, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I64));
    }

    #[test]
    fn enum_discriminant_produces_i32() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Structured(StructuredOp::EnumDiscriminant);
        let inputs = vec![(0, TypeId(100))];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn branch_requires_bool_condition() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Branch);
        let inputs = vec![(0, TypeId::I32)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(matches!(result, Err(TypeError::NonBooleanCondition { .. })));
    }

    #[test]
    fn ifelse_requires_bool_condition() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::IfElse);
        let inputs = vec![(0, TypeId::I32)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(matches!(result, Err(TypeError::NonBooleanCondition { .. })));
    }

    #[test]
    fn ifelse_with_bool_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::IfElse);
        let inputs = vec![(0, TypeId::BOOL)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.expected_inputs, vec![(0, TypeId::BOOL)]);
    }

    #[test]
    fn phi_all_same_type_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Phi);
        let inputs = vec![(0, TypeId::I32), (1, TypeId::I32), (2, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn phi_mismatched_types_errors() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Phi);
        let inputs = vec![(0, TypeId::I32), (1, TypeId::F64)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(result.is_err());
    }

    #[test]
    fn jump_has_no_inputs_or_output() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Jump);
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert!(rule.expected_inputs.is_empty());
        assert!(rule.output_type.is_none());
    }

    #[test]
    fn print_accepts_any_type() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Print);
        let inputs = vec![(0, TypeId::F64)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.expected_inputs, vec![(0, TypeId::F64)]);
        assert_eq!(rule.output_type, Some(TypeId::UNIT));
    }

    #[test]
    fn readline_produces_i64() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::ReadLine);
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I64));
    }

    #[test]
    fn shift_requires_integer_types() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Shift {
            op: lmlang_core::ops::ShiftOp::Shl,
        });
        let inputs = vec![(0, TypeId::I32), (1, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn shift_with_float_errors() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Shift {
            op: lmlang_core::ops::ShiftOp::Shl,
        });
        let inputs = vec![(0, TypeId::F32), (1, TypeId::I32)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(matches!(
            result,
            Err(TypeError::NonNumericArithmetic { .. })
        ));
    }

    #[test]
    fn not_with_bool_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Not);
        let inputs = vec![(0, TypeId::BOOL)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::BOOL));
    }

    #[test]
    fn not_with_integer_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Not);
        let inputs = vec![(0, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn return_with_value() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Return);
        let inputs = vec![(0, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.expected_inputs, vec![(0, TypeId::I32)]);
        assert!(rule.output_type.is_none());
    }

    #[test]
    fn return_without_value() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Return);
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), func_id).unwrap();
        assert!(rule.expected_inputs.is_empty());
        assert!(rule.output_type.is_none());
    }

    #[test]
    fn struct_create_expects_field_types() {
        use indexmap::IndexMap;
        use lmlang_core::types::{StructDef, Visibility};

        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();
        let func_id = graph
            .add_function("f".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();

        let struct_id = graph
            .types
            .register_named(
                "Point",
                LmType::Struct(StructDef {
                    name: "Point".into(),
                    type_id: TypeId(0), // placeholder
                    fields: IndexMap::from([("x".into(), TypeId::F64), ("y".into(), TypeId::F64)]),
                    module: root,
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();

        let op = ComputeNodeOp::Structured(StructuredOp::StructCreate { type_id: struct_id });
        let inputs = vec![(0, TypeId::F64), (1, TypeId::F64)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.expected_inputs.len(), 2);
        assert_eq!(rule.expected_inputs[0], (0, TypeId::F64));
        assert_eq!(rule.expected_inputs[1], (1, TypeId::F64));
        assert_eq!(rule.output_type, Some(struct_id));
    }

    #[test]
    fn unary_arith_numeric_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::UnaryArith {
            op: lmlang_core::ops::UnaryArithOp::Neg,
        });
        let inputs = vec![(0, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn unary_arith_non_numeric_errors() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::UnaryArith {
            op: lmlang_core::ops::UnaryArithOp::Neg,
        });
        let inputs = vec![(0, TypeId::BOOL)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(matches!(
            result,
            Err(TypeError::NonNumericArithmetic { .. })
        ));
    }

    #[test]
    fn loop_output_matches_input() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::Loop);
        let inputs = vec![(0, TypeId::I64)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I64));
    }

    #[test]
    fn capture_access_returns_captured_type() {
        use lmlang_core::function::{Capture, CaptureMode};

        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();
        let parent = graph
            .add_function(
                "parent".into(),
                root,
                vec![],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();
        let closure = graph
            .add_closure(
                "closure".into(),
                root,
                parent,
                vec![],
                TypeId::I32,
                vec![Capture {
                    name: "x".into(),
                    captured_type: TypeId::F64,
                    mode: CaptureMode::ByValue,
                }],
            )
            .unwrap();

        let op = ComputeNodeOp::Core(ComputeOp::CaptureAccess { index: 0 });
        let rule = resolve_type_rule(&op, &[], &graph, NodeId(0), closure).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::F64));
    }

    #[test]
    fn binary_logic_bool_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryLogic {
            op: lmlang_core::ops::LogicOp::And,
        });
        let inputs = vec![(0, TypeId::BOOL), (1, TypeId::BOOL)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::BOOL));
    }

    #[test]
    fn binary_logic_same_integer_succeeds() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryLogic {
            op: lmlang_core::ops::LogicOp::Or,
        });
        let inputs = vec![(0, TypeId::I32), (1, TypeId::I32)];
        let rule = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id).unwrap();
        assert_eq!(rule.output_type, Some(TypeId::I32));
    }

    #[test]
    fn binary_logic_mixed_types_errors() {
        let (graph, func_id) = test_graph_with_function();
        let op = ComputeNodeOp::Core(ComputeOp::BinaryLogic {
            op: lmlang_core::ops::LogicOp::And,
        });
        let inputs = vec![(0, TypeId::BOOL), (1, TypeId::I32)];
        let result = resolve_type_rule(&op, &inputs, &graph, NodeId(0), func_id);
        assert!(result.is_err());
    }
}
