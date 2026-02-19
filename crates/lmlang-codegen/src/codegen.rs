//! Per-function code generation: lowers computational graph ops to LLVM IR.
//!
//! The [`compile_function`] entry point creates an LLVM function from a
//! `FunctionDef`, topologically sorts its nodes by data edges, and dispatches
//! each op to the appropriate IR emitter. SSA values are tracked in a
//! `HashMap<NodeId, BasicValueEnum>` and basic blocks in a parallel map.

use std::collections::{HashMap, HashSet, VecDeque};

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::values::{AggregateValueEnum, BasicValueEnum, FunctionValue, IntValue};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use lmlang_core::edge::FlowEdge;
use lmlang_core::function::FunctionDef;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::ops::{
    ArithOp, CmpOp, ComputeNodeOp, ComputeOp, LogicOp, ShiftOp, StructuredOp, UnaryArithOp,
};
use lmlang_core::type_id::{TypeId, TypeRegistry};
use lmlang_core::types::ConstValue;

use crate::error::CodegenError;
use crate::runtime;
use crate::types::lm_type_to_llvm;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compile a single function from the program graph into LLVM IR.
///
/// Steps:
/// 1. Create LLVM function with correct signature (params + return type).
/// 2. Create entry basic block and position builder.
/// 3. Collect function nodes, topologically sort by data edges.
/// 4. Iterate sorted nodes, dispatch to per-op emit functions.
/// 5. Return the LLVM FunctionValue.
pub fn compile_function<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    func_id: FunctionId,
    func_def: &FunctionDef,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    let registry = &graph.types;

    // 1. Build LLVM function signature
    let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = func_def
        .params
        .iter()
        .map(|(_, tid)| lm_type_to_llvm(context, *tid, registry).map(|t| t.into()))
        .collect::<Result<Vec<_>, _>>()?;

    // For closures, add an extra environment pointer parameter
    if func_def.is_closure && !func_def.captures.is_empty() {
        // The env pointer is added as the last parameter below
    }

    let mut all_params = param_types.clone();
    if func_def.is_closure && !func_def.captures.is_empty() {
        // Add environment pointer parameter (opaque ptr)
        all_params.push(context.ptr_type(AddressSpace::default()).into());
    }

    let ret_type = lm_type_to_llvm(context, func_def.return_type, registry)?;
    let fn_type = if func_def.return_type == TypeId::UNIT {
        context.void_type().fn_type(&all_params, false)
    } else {
        ret_type.fn_type(&all_params, false)
    };

    // Reuse existing forward-declaration if present; otherwise add a new function.
    let function = match module.get_function(&func_def.name) {
        Some(existing) => existing,
        None => module.add_function(&func_def.name, fn_type, None),
    };

    // 2. Create entry basic block
    let entry_bb = context.append_basic_block(function, "entry");
    builder.position_at_end(entry_bb);

    // 3. Collect and sort function nodes
    let func_nodes = graph.function_nodes(func_id);
    let sorted_nodes = topological_sort(&func_nodes, graph)?;

    // 4. Track SSA values and basic blocks
    let mut values: HashMap<NodeId, BasicValueEnum<'ctx>> = HashMap::new();
    let mut basic_blocks: HashMap<NodeId, inkwell::basic_block::BasicBlock<'ctx>> = HashMap::new();

    // Store the entry block for reference
    basic_blocks.insert(NodeId(u32::MAX), entry_bb);

    // 5. Emit each node
    for &node_id in &sorted_nodes {
        let node = graph
            .get_compute_node(node_id)
            .ok_or_else(|| CodegenError::InvalidGraph(format!("node {} not found", node_id)))?;

        emit_node(
            context,
            module,
            builder,
            graph,
            function,
            node_id,
            &node.op,
            &mut values,
            &mut basic_blocks,
        )?;
    }

    Ok(function)
}

// ---------------------------------------------------------------------------
// Topological sort
// ---------------------------------------------------------------------------

/// Topologically sort nodes within a function by data and control edges
/// using Kahn's algorithm.
///
/// Considers both data and control edges where both source and target are
/// in the function node set. Nodes with no dependencies (Const, Parameter,
/// Alloc, CaptureAccess, ReadLine) naturally sort first.
fn topological_sort(
    func_nodes: &[NodeId],
    graph: &ProgramGraph,
) -> Result<Vec<NodeId>, CodegenError> {
    let node_set: HashSet<NodeId> = func_nodes.iter().copied().collect();
    let compute = graph.compute();

    // Build in-degree map (data + control edges within the function)
    let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
    for &nid in func_nodes {
        in_degree.insert(nid, 0);
    }

    for &nid in func_nodes {
        let idx = petgraph::graph::NodeIndex::from(nid);
        for edge in compute.edges_directed(idx, Direction::Incoming) {
            let source_nid = NodeId::from(edge.source());
            if node_set.contains(&source_nid) {
                *in_degree.entry(nid).or_insert(0) += 1;
            }
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    for &nid in func_nodes {
        if in_degree[&nid] == 0 {
            queue.push_back(nid);
        }
    }

    let mut sorted = Vec::with_capacity(func_nodes.len());
    while let Some(nid) = queue.pop_front() {
        sorted.push(nid);

        let idx = petgraph::graph::NodeIndex::from(nid);
        for edge in compute.edges_directed(idx, Direction::Outgoing) {
            let target_nid = NodeId::from(edge.target());
            if node_set.contains(&target_nid) {
                let deg = in_degree.get_mut(&target_nid).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(target_nid);
                }
            }
        }
    }

    if sorted.len() != func_nodes.len() {
        return Err(CodegenError::InvalidGraph(
            "cycle detected in data flow graph".to_string(),
        ));
    }

    Ok(sorted)
}

// ---------------------------------------------------------------------------
// Input value helper
// ---------------------------------------------------------------------------

/// Look up the value produced by the node connected to `node_id` at input `port`.
///
/// Searches incoming data edges for one with `target_port == port`, then
/// looks up the source node's value in the `values` map.
fn get_input<'ctx>(
    graph: &ProgramGraph,
    node_id: NodeId,
    port: u16,
    values: &HashMap<NodeId, BasicValueEnum<'ctx>>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);

    for edge in compute.edges_directed(idx, Direction::Incoming) {
        if let FlowEdge::Data { target_port, .. } = edge.weight() {
            if *target_port == port {
                let source_nid = NodeId::from(edge.source());
                return values.get(&source_nid).copied().ok_or_else(|| {
                    CodegenError::InvalidGraph(format!(
                        "value for node {} (input to {} port {}) not yet computed",
                        source_nid, node_id, port
                    ))
                });
            }
        }
    }

    Err(CodegenError::InvalidGraph(format!(
        "no incoming data edge at port {} for node {}",
        port, node_id
    )))
}

/// Get the TypeId of the value flowing into `node_id` at input `port`.
fn get_input_type(
    graph: &ProgramGraph,
    node_id: NodeId,
    port: u16,
) -> Result<TypeId, CodegenError> {
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);

    for edge in compute.edges_directed(idx, Direction::Incoming) {
        if let FlowEdge::Data {
            target_port,
            value_type,
            ..
        } = edge.weight()
        {
            if *target_port == port {
                return Ok(*value_type);
            }
        }
    }

    Err(CodegenError::InvalidGraph(format!(
        "no incoming data edge at port {} for node {}",
        port, node_id
    )))
}

/// Get the TypeId of the value flowing out of `node_id` at output `port`.
fn get_output_type(
    graph: &ProgramGraph,
    node_id: NodeId,
    port: u16,
) -> Result<TypeId, CodegenError> {
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);

    for edge in compute.edges_directed(idx, Direction::Outgoing) {
        if let FlowEdge::Data {
            source_port,
            value_type,
            ..
        } = edge.weight()
        {
            if *source_port == port {
                return Ok(*value_type);
            }
        }
    }

    Err(CodegenError::InvalidGraph(format!(
        "no outgoing data edge at port {} for node {}",
        port, node_id
    )))
}

/// Count the number of incoming data edges to a node.
fn count_data_inputs(graph: &ProgramGraph, node_id: NodeId) -> usize {
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);
    compute
        .edges_directed(idx, Direction::Incoming)
        .filter(|e| e.weight().is_data())
        .count()
}

// ---------------------------------------------------------------------------
// Checked arithmetic intrinsic helper
// ---------------------------------------------------------------------------

/// Emit a checked arithmetic operation using LLVM overflow intrinsics.
///
/// Returns the result value. If overflow is detected, branches to a runtime
/// error block.
fn emit_checked_int_arith<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    function: FunctionValue<'ctx>,
    lhs: IntValue<'ctx>,
    rhs: IntValue<'ctx>,
    intrinsic_name: &str,
    node_id: NodeId,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let int_type = lhs.get_type();

    // Build the intrinsic function type: { iN, i1 } fn(iN, iN)
    let overflow_struct = context.struct_type(
        &[int_type.into(), context.bool_type().into()],
        false,
    );
    let intrinsic_fn_type = overflow_struct.fn_type(&[int_type.into(), int_type.into()], false);

    // Get or declare the intrinsic
    let full_name = format!(
        "llvm.{}.with.overflow.i{}",
        intrinsic_name,
        int_type.get_bit_width()
    );
    let intrinsic_fn = match module.get_function(&full_name) {
        Some(f) => f,
        None => module.add_function(&full_name, intrinsic_fn_type, None),
    };

    // Call the intrinsic
    let result = builder
        .build_call(intrinsic_fn, &[lhs.into(), rhs.into()], "checked")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?
        .try_as_basic_value()
        .basic()
        .ok_or_else(|| CodegenError::LlvmError("overflow intrinsic returned void".into()))?;

    // Extract result value (index 0) and overflow flag (index 1)
    let value = builder
        .build_extract_value(result.into_struct_value(), 0, "result")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    let overflow_flag = builder
        .build_extract_value(result.into_struct_value(), 1, "overflow")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    // Emit overflow guard
    runtime::emit_overflow_guard(
        builder,
        context,
        module,
        function,
        overflow_flag.into_int_value(),
        node_id.0,
    )?;

    Ok(value)
}

// ---------------------------------------------------------------------------
// Per-op emit dispatch
// ---------------------------------------------------------------------------

/// Emit LLVM IR for a single compute node.
///
/// Dispatches on the `ComputeNodeOp` variant and emits the appropriate
/// LLVM instructions. Results are stored in the `values` map.
#[allow(clippy::too_many_arguments)]
fn emit_node<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    function: FunctionValue<'ctx>,
    node_id: NodeId,
    op: &ComputeNodeOp,
    values: &mut HashMap<NodeId, BasicValueEnum<'ctx>>,
    basic_blocks: &mut HashMap<NodeId, inkwell::basic_block::BasicBlock<'ctx>>,
) -> Result<(), CodegenError> {
    let registry = &graph.types;

    match op {
        ComputeNodeOp::Core(core_op) => match core_op {
            // ----- Constants -----
            ComputeOp::Const { value } => {
                let val = emit_const(context, value)?;
                values.insert(node_id, val);
            }

            // ----- Parameters -----
            ComputeOp::Parameter { index } => {
                let param = function
                    .get_nth_param(*index)
                    .ok_or_else(|| {
                        CodegenError::InvalidGraph(format!(
                            "parameter index {} out of range for function with {} params",
                            index,
                            function.count_params()
                        ))
                    })?;
                values.insert(node_id, param);
            }

            // ----- Binary Arithmetic -----
            ComputeOp::BinaryArith { op: arith_op } => {
                let lhs = get_input(graph, node_id, 0, values)?;
                let rhs = get_input(graph, node_id, 1, values)?;
                let val = emit_binary_arith(
                    context, module, builder, function, lhs, rhs, arith_op, node_id,
                )?;
                values.insert(node_id, val);
            }

            // ----- Unary Arithmetic -----
            ComputeOp::UnaryArith { op: unary_op } => {
                let operand = get_input(graph, node_id, 0, values)?;
                let val = emit_unary_arith(context, module, builder, operand, unary_op)?;
                values.insert(node_id, val);
            }

            // ----- Comparison -----
            ComputeOp::Compare { op: cmp_op } => {
                let lhs = get_input(graph, node_id, 0, values)?;
                let rhs = get_input(graph, node_id, 1, values)?;
                let val = emit_compare(builder, lhs, rhs, cmp_op)?;
                values.insert(node_id, val);
            }

            // ----- Binary Logic -----
            ComputeOp::BinaryLogic { op: logic_op } => {
                let lhs = get_input(graph, node_id, 0, values)?;
                let rhs = get_input(graph, node_id, 1, values)?;
                let val = emit_binary_logic(builder, lhs, rhs, logic_op)?;
                values.insert(node_id, val);
            }

            // ----- Not -----
            ComputeOp::Not => {
                let operand = get_input(graph, node_id, 0, values)?;
                let val = builder
                    .build_not(operand.into_int_value(), "not")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                values.insert(node_id, val.into());
            }

            // ----- Shifts -----
            ComputeOp::Shift { op: shift_op } => {
                let val = get_input(graph, node_id, 0, values)?;
                let amt = get_input(graph, node_id, 1, values)?;
                let result = emit_shift(builder, val, amt, shift_op)?;
                values.insert(node_id, result);
            }

            // ----- Print -----
            ComputeOp::Print => {
                let val = get_input(graph, node_id, 0, values)?;
                let type_id = get_input_type(graph, node_id, 0)?;
                runtime::emit_print_value(builder, context, module, val, type_id)?;
                // Print produces no SSA value (or Unit)
            }

            // ----- Return -----
            ComputeOp::Return => {
                let func_def = graph.get_function(
                    graph.get_compute_node(node_id).unwrap().owner
                ).ok_or_else(|| CodegenError::InvalidGraph("return node has no owner function".into()))?;

                if func_def.return_type == TypeId::UNIT {
                    builder.build_return(None)
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                } else {
                    let ret_val = get_input(graph, node_id, 0, values)?;
                    builder.build_return(Some(&ret_val))
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                }
            }

            // ----- Control Flow: IfElse -----
            ComputeOp::IfElse => {
                emit_if_else(context, module, builder, graph, function, node_id, values, basic_blocks)?;
            }

            // ----- Control Flow: Loop -----
            ComputeOp::Loop => {
                emit_loop(context, module, builder, graph, function, node_id, values, basic_blocks)?;
            }

            // ----- Control Flow: Match -----
            ComputeOp::Match => {
                emit_match(context, module, builder, graph, function, node_id, values, basic_blocks)?;
            }

            // ----- Control Flow: Branch -----
            ComputeOp::Branch => {
                let cond = get_input(graph, node_id, 0, values)?;
                let compute = graph.compute();
                let idx = petgraph::graph::NodeIndex::from(node_id);

                let mut true_bb = None;
                let mut false_bb = None;
                for edge in compute.edges_directed(idx, Direction::Outgoing) {
                    if let FlowEdge::Control { branch_index } = edge.weight() {
                        let target_nid = NodeId::from(edge.target());
                        let bb = *basic_blocks.entry(target_nid).or_insert_with(|| {
                            context.append_basic_block(function, &format!("bb_{}", target_nid))
                        });
                        match branch_index {
                            Some(0) => true_bb = Some(bb),
                            Some(1) => false_bb = Some(bb),
                            _ => {}
                        }
                    }
                }

                let true_bb = true_bb.ok_or_else(|| {
                    CodegenError::InvalidGraph(format!("Branch {} missing true target", node_id))
                })?;
                let false_bb = false_bb.ok_or_else(|| {
                    CodegenError::InvalidGraph(format!("Branch {} missing false target", node_id))
                })?;

                builder
                    .build_conditional_branch(cond.into_int_value(), true_bb, false_bb)
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            }

            // ----- Control Flow: Jump -----
            ComputeOp::Jump => {
                let compute = graph.compute();
                let idx = petgraph::graph::NodeIndex::from(node_id);

                let mut target_bb = None;
                for edge in compute.edges_directed(idx, Direction::Outgoing) {
                    if let FlowEdge::Control { .. } = edge.weight() {
                        let target_nid = NodeId::from(edge.target());
                        target_bb = Some(*basic_blocks.entry(target_nid).or_insert_with(|| {
                            context.append_basic_block(function, &format!("bb_{}", target_nid))
                        }));
                        break;
                    }
                }

                let target_bb = target_bb.ok_or_else(|| {
                    CodegenError::InvalidGraph(format!("Jump {} missing target", node_id))
                })?;

                builder
                    .build_unconditional_branch(target_bb)
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            }

            // ----- Control Flow: Phi -----
            ComputeOp::Phi => {
                emit_phi(context, builder, graph, function, node_id, values, basic_blocks)?;
            }

            // ----- Memory: Alloc -----
            ComputeOp::Alloc => {
                // Determine the pointee type from the outgoing data edge
                let pointee_type_id = get_output_type(graph, node_id, 0)?;
                // The output type of Alloc is Pointer; we need to find what it points to.
                // Look at what Load/Store uses this as -- infer from the first Load successor's output type
                // or from the value_type on the outgoing edge which should be Pointer type.
                // For simplicity, look up the pointer type and extract pointee.
                let lm_type = registry.get(pointee_type_id);
                let alloc_type = match lm_type {
                    Some(lmlang_core::types::LmType::Pointer { pointee, .. }) => {
                        lm_type_to_llvm(context, *pointee, registry)?
                    }
                    _ => {
                        // If the edge type is not a pointer, allocate the type directly
                        lm_type_to_llvm(context, pointee_type_id, registry)?
                    }
                };

                let ptr = builder
                    .build_alloca(alloc_type, &format!("alloc_{}", node_id))
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                values.insert(node_id, ptr.into());
            }

            // ----- Memory: Load -----
            ComputeOp::Load => {
                let ptr = get_input(graph, node_id, 0, values)?;
                // Determine loaded type from outgoing data edge
                let loaded_type_id = get_output_type(graph, node_id, 0)?;
                let loaded_type = lm_type_to_llvm(context, loaded_type_id, registry)?;
                let val = builder
                    .build_load(loaded_type, ptr.into_pointer_value(), &format!("load_{}", node_id))
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                values.insert(node_id, val);
            }

            // ----- Memory: Store -----
            ComputeOp::Store => {
                let ptr = get_input(graph, node_id, 0, values)?;
                let val = get_input(graph, node_id, 1, values)?;
                builder
                    .build_store(ptr.into_pointer_value(), val)
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                // Store produces no SSA value
            }

            // ----- Memory: GetElementPtr -----
            ComputeOp::GetElementPtr => {
                let base = get_input(graph, node_id, 0, values)?;
                let index = get_input(graph, node_id, 1, values)?;

                // Determine element type from the output edge
                let out_type_id = get_output_type(graph, node_id, 0)?;
                let lm_type = registry.get(out_type_id);
                let elem_type = match lm_type {
                    Some(lmlang_core::types::LmType::Pointer { pointee, .. }) => {
                        lm_type_to_llvm(context, *pointee, registry)?
                    }
                    _ => lm_type_to_llvm(context, out_type_id, registry)?,
                };

                let gep = unsafe {
                    builder
                        .build_in_bounds_gep(
                            elem_type,
                            base.into_pointer_value(),
                            &[index.into_int_value()],
                            &format!("gep_{}", node_id),
                        )
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                };
                values.insert(node_id, gep.into());
            }

            // ----- Functions: Call -----
            ComputeOp::Call { target } => {
                let target_def = graph.get_function(*target).ok_or_else(|| {
                    CodegenError::InvalidGraph(format!("call target function {} not found", target))
                })?;

                let target_fn = module.get_function(&target_def.name).ok_or_else(|| {
                    CodegenError::InvalidGraph(format!(
                        "LLVM function '{}' not found in module",
                        target_def.name
                    ))
                })?;

                // Collect arguments from data input edges by port order
                let num_inputs = count_data_inputs(graph, node_id);
                let mut args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                    Vec::with_capacity(num_inputs);
                for port in 0..num_inputs as u16 {
                    let arg = get_input(graph, node_id, port, values)?;
                    args.push(arg.into());
                }

                let call_result = builder
                    .build_call(target_fn, &args, &format!("call_{}", node_id))
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                // Extract return value if non-void
                if target_def.return_type != TypeId::UNIT {
                    if let Some(val) = call_result.try_as_basic_value().basic() {
                        values.insert(node_id, val);
                    }
                }
            }

            // ----- Functions: IndirectCall -----
            ComputeOp::IndirectCall => {
                // Port 0 = function pointer, remaining ports = arguments
                let fn_ptr = get_input(graph, node_id, 0, values)?;
                let num_inputs = count_data_inputs(graph, node_id);

                let mut args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                    Vec::with_capacity(num_inputs - 1);
                for port in 1..num_inputs as u16 {
                    let arg = get_input(graph, node_id, port, values)?;
                    args.push(arg.into());
                }

                // Determine function type from the output edge
                let ret_type_id = get_output_type(graph, node_id, 0)
                    .unwrap_or(TypeId::UNIT);

                let ret_llvm = if ret_type_id == TypeId::UNIT {
                    None
                } else {
                    Some(lm_type_to_llvm(context, ret_type_id, registry)?)
                };

                // Build argument types for the function type
                let arg_types: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = args
                    .iter()
                    .map(|a| {
                        let bv: BasicValueEnum<'ctx> = (*a).try_into().unwrap();
                        bv.get_type().into()
                    })
                    .collect();

                let fn_type = match ret_llvm {
                    Some(rt) => rt.fn_type(&arg_types, false),
                    None => context.void_type().fn_type(&arg_types, false),
                };

                let call_result = builder
                    .build_indirect_call(
                        fn_type,
                        fn_ptr.into_pointer_value(),
                        &args,
                        &format!("icall_{}", node_id),
                    )
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                if ret_llvm.is_some() {
                    if let Some(val) = call_result.try_as_basic_value().basic() {
                        values.insert(node_id, val);
                    }
                }
            }

            // ----- I/O: ReadLine -----
            ComputeOp::ReadLine => {
                // Stub: declare lmlang_readline if not present, return empty ptr
                let readline_fn = match module.get_function("lmlang_readline") {
                    Some(f) => f,
                    None => {
                        let ptr_type = context.ptr_type(AddressSpace::default());
                        let fn_type = ptr_type.fn_type(&[], false);
                        module.add_function(
                            "lmlang_readline",
                            fn_type,
                            Some(inkwell::module::Linkage::External),
                        )
                    }
                };

                let result = builder
                    .build_call(readline_fn, &[], &format!("readline_{}", node_id))
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                if let Some(val) = result.try_as_basic_value().basic() {
                    values.insert(node_id, val);
                }
            }

            // ----- I/O: File operations (stubs) -----
            ComputeOp::FileOpen => {
                emit_file_io_stub(context, module, builder, node_id, "fopen", values)?;
            }
            ComputeOp::FileRead => {
                emit_file_io_stub(context, module, builder, node_id, "fread", values)?;
            }
            ComputeOp::FileWrite => {
                emit_file_io_stub(context, module, builder, node_id, "fwrite", values)?;
            }
            ComputeOp::FileClose => {
                emit_file_io_stub(context, module, builder, node_id, "fclose", values)?;
            }

            // ----- Closures: MakeClosure -----
            ComputeOp::MakeClosure { function: closure_fn_id } => {
                emit_make_closure(
                    context, module, builder, graph, function, node_id,
                    *closure_fn_id, values,
                )?;
            }

            // ----- Closures: CaptureAccess -----
            ComputeOp::CaptureAccess { index } => {
                // The environment pointer is the last parameter of the closure function
                let env_ptr = function
                    .get_last_param()
                    .ok_or_else(|| {
                        CodegenError::InvalidGraph(
                            "CaptureAccess but function has no parameters".into(),
                        )
                    })?
                    .into_pointer_value();

                // Determine capture type from the output edge
                let cap_type_id = get_output_type(graph, node_id, 0)?;
                let cap_type = lm_type_to_llvm(context, cap_type_id, registry)?;

                // GEP into the environment struct and load
                let gep = builder
                    .build_struct_gep(
                        // We need the environment struct type; build it from captures
                        build_env_struct_type(context, graph, function, registry)?,
                        env_ptr,
                        *index,
                        &format!("cap_gep_{}", index),
                    )
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                let val = builder
                    .build_load(cap_type, gep, &format!("cap_load_{}", index))
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                values.insert(node_id, val);
            }
        },

        ComputeNodeOp::Structured(struct_op) => match struct_op {
            // ----- StructCreate -----
            StructuredOp::StructCreate { type_id } => {
                let struct_llvm_type = lm_type_to_llvm(context, *type_id, registry)?;
                let struct_type = struct_llvm_type.into_struct_type();

                // Build struct by sequential insertvalue
                let mut agg = struct_type.get_undef();
                let num_fields = struct_type.count_fields();
                for i in 0..num_fields {
                    let field_val = get_input(graph, node_id, i as u16, values)?;
                    agg = builder
                        .build_insert_value(agg, field_val, i, &format!("sfield_{}", i))
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                        .into_struct_value();
                }
                values.insert(node_id, agg.into());
            }

            // ----- StructGet -----
            StructuredOp::StructGet { field_index } => {
                let struct_val = get_input(graph, node_id, 0, values)?;
                let val = builder
                    .build_extract_value(struct_val.into_struct_value(), *field_index, "sget")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                values.insert(node_id, val);
            }

            // ----- StructSet -----
            StructuredOp::StructSet { field_index } => {
                let struct_val = get_input(graph, node_id, 0, values)?;
                let new_val = get_input(graph, node_id, 1, values)?;
                let val = builder
                    .build_insert_value(
                        struct_val.into_struct_value(),
                        new_val,
                        *field_index,
                        "sset",
                    )
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                values.insert(node_id, aggregate_to_basic(val));
            }

            // ----- ArrayCreate -----
            StructuredOp::ArrayCreate { length } => {
                // Determine element type from first input
                let first_val = get_input(graph, node_id, 0, values)?;
                let elem_type = first_val.get_type();
                let arr_type = elem_type.array_type(*length);
                let mut agg = arr_type.get_undef();

                for i in 0..*length {
                    let elem = get_input(graph, node_id, i as u16, values)?;
                    agg = builder
                        .build_insert_value(agg, elem, i, &format!("aelem_{}", i))
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                        .into_array_value();
                }
                values.insert(node_id, agg.into());
            }

            // ----- ArrayGet -----
            StructuredOp::ArrayGet => {
                let arr = get_input(graph, node_id, 0, values)?;
                let index = get_input(graph, node_id, 1, values)?;
                let index_int = index.into_int_value();

                // Check if index is a constant
                if let Some(const_val) = index_int.get_zero_extended_constant() {
                    let val = builder
                        .build_extract_value(
                            arr.into_array_value(),
                            const_val as u32,
                            "aget",
                        )
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    values.insert(node_id, val);
                } else {
                    // Dynamic index: alloca + GEP + load + bounds guard
                    let arr_val = arr.into_array_value();
                    let arr_type = arr_val.get_type();
                    let arr_len = arr_type.len();
                    let length = context.i32_type().const_int(arr_len as u64, false);

                    // Bounds check
                    runtime::emit_bounds_guard(
                        builder, context, module, function,
                        index_int, length, node_id.0,
                    )?;

                    // Alloca the array, GEP, load
                    let alloca = builder
                        .build_alloca(arr_type, "arr_tmp")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    builder.build_store(alloca, arr_val)
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                    let elem_type = arr_type.get_element_type();
                    let zero = context.i32_type().const_zero();
                    let gep = unsafe {
                        builder
                            .build_in_bounds_gep(
                                arr_type,
                                alloca,
                                &[zero, index_int],
                                "aget_gep",
                            )
                            .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                    };

                    let val = builder
                        .build_load(elem_type, gep, "aget_load")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    values.insert(node_id, val);
                }
            }

            // ----- ArraySet -----
            StructuredOp::ArraySet => {
                let arr = get_input(graph, node_id, 0, values)?;
                let index = get_input(graph, node_id, 1, values)?;
                let new_val = get_input(graph, node_id, 2, values)?;
                let index_int = index.into_int_value();

                if let Some(const_val) = index_int.get_zero_extended_constant() {
                    let val = builder
                        .build_insert_value(
                            arr.into_array_value(),
                            new_val,
                            const_val as u32,
                            "aset",
                        )
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    values.insert(node_id, aggregate_to_basic(val));
                } else {
                    // Dynamic index: alloca, GEP, store, reload
                    let arr_val = arr.into_array_value();
                    let arr_type = arr_val.get_type();
                    let arr_len = arr_type.len();
                    let length = context.i32_type().const_int(arr_len as u64, false);

                    runtime::emit_bounds_guard(
                        builder, context, module, function,
                        index_int, length, node_id.0,
                    )?;

                    let alloca = builder
                        .build_alloca(arr_type, "arr_set_tmp")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    builder.build_store(alloca, arr_val)
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                    let zero = context.i32_type().const_zero();
                    let gep = unsafe {
                        builder
                            .build_in_bounds_gep(
                                arr_type,
                                alloca,
                                &[zero, index_int],
                                "aset_gep",
                            )
                            .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                    };
                    builder.build_store(gep, new_val)
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                    let reloaded = builder
                        .build_load(arr_type, alloca, "aset_reload")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    values.insert(node_id, reloaded);
                }
            }

            // ----- Cast -----
            StructuredOp::Cast { target_type } => {
                let src = get_input(graph, node_id, 0, values)?;
                let src_type_id = get_input_type(graph, node_id, 0)?;
                let val = emit_cast(context, builder, src, src_type_id, *target_type, registry)?;
                values.insert(node_id, val);
            }

            // ----- EnumCreate -----
            StructuredOp::EnumCreate { type_id, variant_index } => {
                let enum_llvm_type = lm_type_to_llvm(context, *type_id, registry)?;
                let enum_struct = enum_llvm_type.into_struct_type();

                // Set discriminant
                let disc_val = context.i32_type().const_int(*variant_index as u64, false);
                let mut agg = enum_struct.get_undef();
                agg = builder
                    .build_insert_value(agg, disc_val, 0, "disc")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                    .into_struct_value();

                // If there's a payload, store it
                if enum_struct.count_fields() > 1 {
                    // Check if this variant has a payload input
                    if count_data_inputs(graph, node_id) > 0 {
                        let payload = get_input(graph, node_id, 0, values)?;

                        // Alloca the payload, bitcast to [N x i8], load, insertvalue
                        let payload_alloca = builder
                            .build_alloca(payload.get_type(), "payload_tmp")
                            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                        builder.build_store(payload_alloca, payload)
                            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                        let payload_field_type = enum_struct.get_field_type_at_index(1).unwrap();
                        let payload_bytes = builder
                            .build_load(payload_field_type, payload_alloca, "payload_bytes")
                            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                        agg = builder
                            .build_insert_value(agg, payload_bytes, 1, "payload")
                            .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                            .into_struct_value();
                    }
                }
                values.insert(node_id, agg.into());
            }

            // ----- EnumDiscriminant -----
            StructuredOp::EnumDiscriminant => {
                let enum_val = get_input(graph, node_id, 0, values)?;
                let disc = builder
                    .build_extract_value(enum_val.into_struct_value(), 0, "disc")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                values.insert(node_id, disc);
            }

            // ----- EnumPayload -----
            StructuredOp::EnumPayload { variant_index: _ } => {
                let enum_val = get_input(graph, node_id, 0, values)?;
                let enum_struct = enum_val.into_struct_value();

                if enum_struct.get_type().count_fields() > 1 {
                    let raw_payload = builder
                        .build_extract_value(enum_struct, 1, "payload_raw")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                    // Determine target payload type from the output edge
                    let out_type_id = get_output_type(graph, node_id, 0)?;
                    let target_type = lm_type_to_llvm(context, out_type_id, registry)?;

                    // Alloca the raw bytes, load as target type (reinterpret cast)
                    let raw_alloca = builder
                        .build_alloca(raw_payload.get_type(), "payload_raw_alloca")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    builder.build_store(raw_alloca, raw_payload)
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

                    let val = builder
                        .build_load(target_type, raw_alloca, "payload_cast")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                    values.insert(node_id, val);
                } else {
                    // All-unit enum, no payload -- return undef of output type
                    let out_type_id = get_output_type(graph, node_id, 0)?;
                    let target_type = lm_type_to_llvm(context, out_type_id, registry)?;
                    values.insert(node_id, target_type.const_zero());
                }
            }
        },
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// AggregateValueEnum -> BasicValueEnum helper
// ---------------------------------------------------------------------------

fn aggregate_to_basic<'ctx>(agg: AggregateValueEnum<'ctx>) -> BasicValueEnum<'ctx> {
    match agg {
        AggregateValueEnum::ArrayValue(v) => v.into(),
        AggregateValueEnum::StructValue(v) => v.into(),
    }
}

// ---------------------------------------------------------------------------
// Constant emission
// ---------------------------------------------------------------------------

fn emit_const<'ctx>(
    context: &'ctx Context,
    value: &ConstValue,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match value {
        ConstValue::Bool(v) => Ok(context.bool_type().const_int(*v as u64, false).into()),
        ConstValue::I8(v) => Ok(context.i8_type().const_int(*v as u64, true).into()),
        ConstValue::I16(v) => Ok(context.i16_type().const_int(*v as u64, true).into()),
        ConstValue::I32(v) => Ok(context.i32_type().const_int(*v as u64, true).into()),
        ConstValue::I64(v) => Ok(context.i64_type().const_int(*v as u64, true).into()),
        ConstValue::F32(v) => Ok(context.f32_type().const_float(*v).into()),
        ConstValue::F64(v) => Ok(context.f64_type().const_float(*v).into()),
        ConstValue::Unit => Ok(context.struct_type(&[], false).const_zero().into()),
    }
}

// ---------------------------------------------------------------------------
// Binary arithmetic emission
// ---------------------------------------------------------------------------

fn emit_binary_arith<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    function: FunctionValue<'ctx>,
    lhs: BasicValueEnum<'ctx>,
    rhs: BasicValueEnum<'ctx>,
    op: &ArithOp,
    node_id: NodeId,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    if lhs.is_int_value() {
        let lhs_int = lhs.into_int_value();
        let rhs_int = rhs.into_int_value();

        match op {
            ArithOp::Add => {
                emit_checked_int_arith(
                    context, module, builder, function, lhs_int, rhs_int, "sadd", node_id,
                )
            }
            ArithOp::Sub => {
                emit_checked_int_arith(
                    context, module, builder, function, lhs_int, rhs_int, "ssub", node_id,
                )
            }
            ArithOp::Mul => {
                emit_checked_int_arith(
                    context, module, builder, function, lhs_int, rhs_int, "smul", node_id,
                )
            }
            ArithOp::Div => {
                runtime::emit_div_guard(
                    builder, context, module, function, rhs_int, node_id.0,
                )?;
                let val = builder
                    .build_int_signed_div(lhs_int, rhs_int, "sdiv")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                Ok(val.into())
            }
            ArithOp::Rem => {
                runtime::emit_div_guard(
                    builder, context, module, function, rhs_int, node_id.0,
                )?;
                let val = builder
                    .build_int_signed_rem(lhs_int, rhs_int, "srem")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                Ok(val.into())
            }
        }
    } else {
        let lhs_float = lhs.into_float_value();
        let rhs_float = rhs.into_float_value();

        let val = match op {
            ArithOp::Add => builder
                .build_float_add(lhs_float, rhs_float, "fadd")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
            ArithOp::Sub => builder
                .build_float_sub(lhs_float, rhs_float, "fsub")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
            ArithOp::Mul => builder
                .build_float_mul(lhs_float, rhs_float, "fmul")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
            ArithOp::Div => builder
                .build_float_div(lhs_float, rhs_float, "fdiv")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
            ArithOp::Rem => builder
                .build_float_rem(lhs_float, rhs_float, "frem")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
        };
        Ok(val.into())
    }
}

// ---------------------------------------------------------------------------
// Unary arithmetic emission
// ---------------------------------------------------------------------------

fn emit_unary_arith<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    operand: BasicValueEnum<'ctx>,
    op: &UnaryArithOp,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match op {
        UnaryArithOp::Neg => {
            if operand.is_int_value() {
                let val = builder
                    .build_int_neg(operand.into_int_value(), "neg")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                Ok(val.into())
            } else {
                let val = builder
                    .build_float_neg(operand.into_float_value(), "fneg")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                Ok(val.into())
            }
        }
        UnaryArithOp::Abs => {
            if operand.is_int_value() {
                let int_val = operand.into_int_value();
                let zero = int_val.get_type().const_zero();
                let neg_val = builder
                    .build_int_neg(int_val, "neg_for_abs")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                let is_neg = builder
                    .build_int_compare(IntPredicate::SLT, int_val, zero, "is_neg")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                let val = builder
                    .build_select(is_neg, neg_val, int_val, "abs")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                Ok(val)
            } else {
                // Use llvm.fabs intrinsic
                let float_val = operand.into_float_value();
                let float_type = float_val.get_type();
                let intrinsic_name = if float_type == context.f32_type() {
                    "llvm.fabs.f32"
                } else {
                    "llvm.fabs.f64"
                };

                let fabs_fn = match module.get_function(intrinsic_name) {
                    Some(f) => f,
                    None => {
                        let fn_type = float_type.fn_type(&[float_type.into()], false);
                        module.add_function(intrinsic_name, fn_type, None)
                    }
                };

                let result = builder
                    .build_call(fabs_fn, &[float_val.into()], "fabs")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                    .try_as_basic_value()
                    .basic()
                    .ok_or_else(|| CodegenError::LlvmError("fabs returned void".into()))?;

                Ok(result)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Comparison emission
// ---------------------------------------------------------------------------

fn emit_compare<'ctx>(
    builder: &Builder<'ctx>,
    lhs: BasicValueEnum<'ctx>,
    rhs: BasicValueEnum<'ctx>,
    op: &CmpOp,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    if lhs.is_int_value() {
        let predicate = match op {
            CmpOp::Eq => IntPredicate::EQ,
            CmpOp::Ne => IntPredicate::NE,
            CmpOp::Lt => IntPredicate::SLT,
            CmpOp::Le => IntPredicate::SLE,
            CmpOp::Gt => IntPredicate::SGT,
            CmpOp::Ge => IntPredicate::SGE,
        };
        let val = builder
            .build_int_compare(predicate, lhs.into_int_value(), rhs.into_int_value(), "cmp")
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        Ok(val.into())
    } else {
        let predicate = match op {
            CmpOp::Eq => FloatPredicate::OEQ,
            CmpOp::Ne => FloatPredicate::UNE,
            CmpOp::Lt => FloatPredicate::OLT,
            CmpOp::Le => FloatPredicate::OLE,
            CmpOp::Gt => FloatPredicate::OGT,
            CmpOp::Ge => FloatPredicate::OGE,
        };
        let val = builder
            .build_float_compare(
                predicate,
                lhs.into_float_value(),
                rhs.into_float_value(),
                "cmp",
            )
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        Ok(val.into())
    }
}

// ---------------------------------------------------------------------------
// Binary logic emission
// ---------------------------------------------------------------------------

fn emit_binary_logic<'ctx>(
    builder: &Builder<'ctx>,
    lhs: BasicValueEnum<'ctx>,
    rhs: BasicValueEnum<'ctx>,
    op: &LogicOp,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let lhs_int = lhs.into_int_value();
    let rhs_int = rhs.into_int_value();

    let val = match op {
        LogicOp::And => builder
            .build_and(lhs_int, rhs_int, "and")
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
        LogicOp::Or => builder
            .build_or(lhs_int, rhs_int, "or")
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
        LogicOp::Xor => builder
            .build_xor(lhs_int, rhs_int, "xor")
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
    };
    Ok(val.into())
}

// ---------------------------------------------------------------------------
// Shift emission
// ---------------------------------------------------------------------------

fn emit_shift<'ctx>(
    builder: &Builder<'ctx>,
    val: BasicValueEnum<'ctx>,
    amt: BasicValueEnum<'ctx>,
    op: &ShiftOp,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let val_int = val.into_int_value();
    let amt_int = amt.into_int_value();

    let result = match op {
        ShiftOp::Shl => builder
            .build_left_shift(val_int, amt_int, "shl")
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
        ShiftOp::ShrLogical => builder
            .build_right_shift(val_int, amt_int, false, "lshr")
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
        ShiftOp::ShrArith => builder
            .build_right_shift(val_int, amt_int, true, "ashr")
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?,
    };
    Ok(result.into())
}

// ---------------------------------------------------------------------------
// Control flow: IfElse
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn emit_if_else<'ctx>(
    context: &'ctx Context,
    _module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    function: FunctionValue<'ctx>,
    node_id: NodeId,
    values: &mut HashMap<NodeId, BasicValueEnum<'ctx>>,
    basic_blocks: &mut HashMap<NodeId, inkwell::basic_block::BasicBlock<'ctx>>,
) -> Result<(), CodegenError> {
    let cond = get_input(graph, node_id, 0, values)?;

    let then_bb = context.append_basic_block(function, &format!("then_{}", node_id));
    let else_bb = context.append_basic_block(function, &format!("else_{}", node_id));
    let merge_bb = context.append_basic_block(function, &format!("merge_{}", node_id));

    builder
        .build_conditional_branch(cond.into_int_value(), then_bb, else_bb)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    // Record blocks for control edge targets
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);
    for edge in compute.edges_directed(idx, Direction::Outgoing) {
        if let FlowEdge::Control { branch_index } = edge.weight() {
            let target_nid = NodeId::from(edge.target());
            match branch_index {
                Some(0) => { basic_blocks.insert(target_nid, then_bb); }
                Some(1) => { basic_blocks.insert(target_nid, else_bb); }
                _ => {}
            }
        }
    }

    // Store merge block for use by Phi nodes or subsequent code
    basic_blocks.insert(node_id, merge_bb);

    // Position builder at then_bb for the next nodes
    // (The actual positioning will be handled by the control region logic)
    builder.position_at_end(then_bb);

    Ok(())
}

// ---------------------------------------------------------------------------
// Control flow: Loop
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn emit_loop<'ctx>(
    context: &'ctx Context,
    _module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    function: FunctionValue<'ctx>,
    node_id: NodeId,
    values: &mut HashMap<NodeId, BasicValueEnum<'ctx>>,
    basic_blocks: &mut HashMap<NodeId, inkwell::basic_block::BasicBlock<'ctx>>,
) -> Result<(), CodegenError> {
    let header_bb = context.append_basic_block(function, &format!("loop_hdr_{}", node_id));
    let body_bb = context.append_basic_block(function, &format!("loop_body_{}", node_id));
    let exit_bb = context.append_basic_block(function, &format!("loop_exit_{}", node_id));

    // Branch to header
    builder
        .build_unconditional_branch(header_bb)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    builder.position_at_end(header_bb);

    // Get loop condition from port 0
    let cond = get_input(graph, node_id, 0, values)?;
    builder
        .build_conditional_branch(cond.into_int_value(), body_bb, exit_bb)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    // Record blocks for body (branch 0) and exit
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);
    for edge in compute.edges_directed(idx, Direction::Outgoing) {
        if let FlowEdge::Control { branch_index } = edge.weight() {
            let target_nid = NodeId::from(edge.target());
            match branch_index {
                Some(0) => { basic_blocks.insert(target_nid, body_bb); }
                _ => {}
            }
        }
    }

    basic_blocks.insert(node_id, exit_bb);
    builder.position_at_end(body_bb);

    Ok(())
}

// ---------------------------------------------------------------------------
// Control flow: Match
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn emit_match<'ctx>(
    context: &'ctx Context,
    _module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    function: FunctionValue<'ctx>,
    node_id: NodeId,
    values: &mut HashMap<NodeId, BasicValueEnum<'ctx>>,
    basic_blocks: &mut HashMap<NodeId, inkwell::basic_block::BasicBlock<'ctx>>,
) -> Result<(), CodegenError> {
    let discriminant = get_input(graph, node_id, 0, values)?;
    let disc_int = discriminant.into_int_value();

    let default_bb = context.append_basic_block(function, &format!("match_default_{}", node_id));
    let merge_bb = context.append_basic_block(function, &format!("match_merge_{}", node_id));

    // Collect switch arms from control edges
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);
    let mut cases: Vec<(IntValue<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> = Vec::new();

    for edge in compute.edges_directed(idx, Direction::Outgoing) {
        if let FlowEdge::Control { branch_index } = edge.weight() {
            if let Some(arm_idx) = branch_index {
                let target_nid = NodeId::from(edge.target());
                let arm_bb = context.append_basic_block(
                    function,
                    &format!("match_arm_{}_{}", node_id, arm_idx),
                );
                basic_blocks.insert(target_nid, arm_bb);
                let case_val = disc_int
                    .get_type()
                    .const_int(*arm_idx as u64, false);
                cases.push((case_val, arm_bb));
            }
        }
    }

    builder
        .build_switch(disc_int, default_bb, &cases)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    // Default block branches to merge
    builder.position_at_end(default_bb);
    builder
        .build_unconditional_branch(merge_bb)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    basic_blocks.insert(node_id, merge_bb);

    // Position at first arm or merge if no arms
    if let Some((_, first_arm_bb)) = cases.first() {
        builder.position_at_end(*first_arm_bb);
    } else {
        builder.position_at_end(merge_bb);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Control flow: Phi
// ---------------------------------------------------------------------------

fn emit_phi<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    _function: FunctionValue<'ctx>,
    node_id: NodeId,
    values: &mut HashMap<NodeId, BasicValueEnum<'ctx>>,
    basic_blocks: &mut HashMap<NodeId, inkwell::basic_block::BasicBlock<'ctx>>,
) -> Result<(), CodegenError> {
    let registry = &graph.types;

    // Determine type from first incoming data edge
    let phi_type_id = get_input_type(graph, node_id, 0)?;
    let phi_type = lm_type_to_llvm(context, phi_type_id, registry)?;

    let phi = builder
        .build_phi(phi_type, &format!("phi_{}", node_id))
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    // Add incoming values: each data input port corresponds to a predecessor
    let compute = graph.compute();
    let idx = petgraph::graph::NodeIndex::from(node_id);

    for edge in compute.edges_directed(idx, Direction::Incoming) {
        if let FlowEdge::Data { target_port, .. } = edge.weight() {
            let source_nid = NodeId::from(edge.source());
            if let Some(&val) = values.get(&source_nid) {
                // Find the basic block the source value was emitted in
                let src_bb = basic_blocks.get(&source_nid).copied().unwrap_or_else(|| {
                    // Fallback: use entry block
                    basic_blocks[&NodeId(u32::MAX)]
                });
                phi.add_incoming(&[(&val, src_bb)]);
            }
            let _ = target_port; // used for matching
        }
    }

    values.insert(node_id, phi.as_basic_value());

    Ok(())
}

// ---------------------------------------------------------------------------
// File I/O stubs
// ---------------------------------------------------------------------------

fn emit_file_io_stub<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    _builder: &Builder<'ctx>,
    node_id: NodeId,
    fn_name: &str,
    values: &mut HashMap<NodeId, BasicValueEnum<'ctx>>,
) -> Result<(), CodegenError> {
    // Declare a minimal stub that returns a null pointer
    let ptr_type = context.ptr_type(AddressSpace::default());
    let result_fn = match module.get_function(fn_name) {
        Some(f) => f,
        None => {
            let fn_type = ptr_type.fn_type(&[ptr_type.into()], true);
            module.add_function(fn_name, fn_type, Some(inkwell::module::Linkage::External))
        }
    };

    // Just produce a null pointer as the result for Phase 5
    let null_ptr = ptr_type.const_null();
    values.insert(node_id, null_ptr.into());

    // Keep the function declared so it links
    let _ = result_fn;

    Ok(())
}

// ---------------------------------------------------------------------------
// MakeClosure
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn emit_make_closure<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    _function: FunctionValue<'ctx>,
    node_id: NodeId,
    closure_fn_id: FunctionId,
    values: &mut HashMap<NodeId, BasicValueEnum<'ctx>>,
) -> Result<(), CodegenError> {
    let registry = &graph.types;
    let closure_def = graph.get_function(closure_fn_id).ok_or_else(|| {
        CodegenError::InvalidGraph(format!("closure function {} not found", closure_fn_id))
    })?;

    // Build environment struct type from captures
    let capture_types: Vec<BasicTypeEnum<'ctx>> = closure_def
        .captures
        .iter()
        .map(|cap| lm_type_to_llvm(context, cap.captured_type, registry))
        .collect::<Result<Vec<_>, _>>()?;

    let env_struct_type = context.struct_type(&capture_types, false);

    // Allocate environment struct on stack
    let env_alloca = builder
        .build_alloca(env_struct_type, &format!("env_{}", node_id))
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    // Store each capture value into the struct fields
    let num_captures = closure_def.captures.len();
    for i in 0..num_captures {
        let capture_val = get_input(graph, node_id, i as u16, values)?;
        let field_ptr = builder
            .build_struct_gep(env_struct_type, env_alloca, i as u32, &format!("cap_{}", i))
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        builder.build_store(field_ptr, capture_val)
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    }

    // Get the closure function pointer
    let closure_fn = module.get_function(&closure_def.name).ok_or_else(|| {
        CodegenError::InvalidGraph(format!(
            "LLVM function '{}' not found for closure",
            closure_def.name
        ))
    })?;

    // Produce a closure pair: { function_pointer, environment_pointer }
    let ptr_type = context.ptr_type(AddressSpace::default());
    let closure_struct_type = context.struct_type(&[ptr_type.into(), ptr_type.into()], false);
    let mut closure_val = closure_struct_type.get_undef();

    closure_val = builder
        .build_insert_value(
            closure_val,
            closure_fn.as_global_value().as_pointer_value(),
            0,
            "fn_ptr",
        )
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?
        .into_struct_value();

    closure_val = builder
        .build_insert_value(closure_val, env_alloca, 1, "env_ptr")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?
        .into_struct_value();

    values.insert(node_id, closure_val.into());

    Ok(())
}

// ---------------------------------------------------------------------------
// CaptureAccess helper: build environment struct type
// ---------------------------------------------------------------------------

fn build_env_struct_type<'ctx>(
    context: &'ctx Context,
    graph: &ProgramGraph,
    function: FunctionValue<'ctx>,
    registry: &TypeRegistry,
) -> Result<inkwell::types::StructType<'ctx>, CodegenError> {
    // Find the function definition for this LLVM function
    let fn_name = function
        .get_name()
        .to_str()
        .map_err(|_| CodegenError::LlvmError("invalid function name encoding".into()))?;

    // Look up the FunctionDef by name
    for (_, func_def) in graph.functions() {
        if func_def.name == fn_name && func_def.is_closure {
            let capture_types: Vec<BasicTypeEnum<'ctx>> = func_def
                .captures
                .iter()
                .map(|cap| lm_type_to_llvm(context, cap.captured_type, registry))
                .collect::<Result<Vec<_>, _>>()?;

            return Ok(context.struct_type(&capture_types, false));
        }
    }

    Err(CodegenError::InvalidGraph(format!(
        "no closure FunctionDef found for LLVM function '{}'",
        fn_name
    )))
}

// ---------------------------------------------------------------------------
// Cast emission
// ---------------------------------------------------------------------------

fn emit_cast<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    src: BasicValueEnum<'ctx>,
    src_type_id: TypeId,
    target_type_id: TypeId,
    registry: &TypeRegistry,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let target_type = lm_type_to_llvm(context, target_type_id, registry)?;

    // Determine cast kind based on source and target types
    let is_src_int = src.is_int_value();
    let is_src_float = src.is_float_value();

    if is_src_int && target_type.is_int_type() {
        // Int -> Int cast
        let src_int = src.into_int_value();
        let target_int_type = target_type.into_int_type();
        let src_width = src_int.get_type().get_bit_width();
        let target_width = target_int_type.get_bit_width();

        if src_width < target_width {
            // Widen: use sext (sign extend) for signed ints, zext for bool
            if src_type_id == TypeId::BOOL {
                let val = builder
                    .build_int_z_extend(src_int, target_int_type, "zext")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                Ok(val.into())
            } else {
                let val = builder
                    .build_int_s_extend(src_int, target_int_type, "sext")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
                Ok(val.into())
            }
        } else if src_width > target_width {
            // Narrow: truncate
            let val = builder
                .build_int_truncate(src_int, target_int_type, "trunc")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            Ok(val.into())
        } else {
            // Same width
            Ok(src)
        }
    } else if is_src_float && target_type.is_float_type() {
        // Float -> Float cast
        let src_float = src.into_float_value();
        let target_float_type = target_type.into_float_type();

        if src_float.get_type() == context.f32_type()
            && target_float_type == context.f64_type()
        {
            let val = builder
                .build_float_ext(src_float, target_float_type, "fpext")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            Ok(val.into())
        } else if src_float.get_type() == context.f64_type()
            && target_float_type == context.f32_type()
        {
            let val = builder
                .build_float_trunc(src_float, target_float_type, "fptrunc")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            Ok(val.into())
        } else {
            Ok(src)
        }
    } else if is_src_int && target_type.is_float_type() {
        // Int -> Float
        let val = builder
            .build_signed_int_to_float(
                src.into_int_value(),
                target_type.into_float_type(),
                "sitofp",
            )
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        Ok(val.into())
    } else if is_src_float && target_type.is_int_type() {
        // Float -> Int
        let val = builder
            .build_float_to_signed_int(
                src.into_float_value(),
                target_type.into_int_type(),
                "fptosi",
            )
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        Ok(val.into())
    } else {
        Err(CodegenError::TypeMapping(format!(
            "unsupported cast from {:?} to {:?}",
            src_type_id, target_type_id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;
    use lmlang_core::graph::ProgramGraph;
    use lmlang_core::ops::{ArithOp, CmpOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    /// Helper: create a minimal program graph with a single function, compile it, verify module.
    fn compile_and_verify(
        build_fn: impl FnOnce(&mut ProgramGraph, FunctionId),
        params: Vec<(&str, TypeId)>,
        return_type: TypeId,
    ) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let param_pairs: Vec<(String, TypeId)> =
            params.iter().map(|(n, t)| (n.to_string(), *t)).collect();

        let func_id = graph
            .add_function("test_fn".into(), root, param_pairs, return_type, Visibility::Public)
            .unwrap();

        build_fn(&mut graph, func_id);

        let context = Context::create();
        let module = context.create_module("test_mod");
        let builder = context.create_builder();

        crate::runtime::declare_runtime_functions(&context, &module);

        let func_def = graph.get_function(func_id).unwrap().clone();
        let result = compile_function(&context, &module, &builder, &graph, func_id, &func_def);
        assert!(result.is_ok(), "compile_function failed: {:?}", result.err());

        let verify = module.verify();
        assert!(verify.is_ok(), "Module verification failed: {:?}", verify);
    }

    #[test]
    fn test_const_return_i32() {
        compile_and_verify(
            |graph, func_id| {
                let c = graph
                    .add_core_op(ComputeOp::Const { value: ConstValue::I32(42) }, func_id)
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(c, ret, 0, 0, TypeId::I32).unwrap();
            },
            vec![],
            TypeId::I32,
        );
    }

    #[test]
    fn test_add_two_params() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let b = graph
                    .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                    .unwrap();
                let add = graph
                    .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, add, 0, 0, TypeId::I32).unwrap();
                graph.add_data_edge(b, add, 0, 1, TypeId::I32).unwrap();
                graph.add_data_edge(add, ret, 0, 0, TypeId::I32).unwrap();
            },
            vec![("a", TypeId::I32), ("b", TypeId::I32)],
            TypeId::I32,
        );
    }

    #[test]
    fn test_float_arithmetic() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let b = graph
                    .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                    .unwrap();
                let mul = graph
                    .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Mul }, func_id)
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, mul, 0, 0, TypeId::F64).unwrap();
                graph.add_data_edge(b, mul, 0, 1, TypeId::F64).unwrap();
                graph.add_data_edge(mul, ret, 0, 0, TypeId::F64).unwrap();
            },
            vec![("a", TypeId::F64), ("b", TypeId::F64)],
            TypeId::F64,
        );
    }

    #[test]
    fn test_comparison() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let b = graph
                    .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                    .unwrap();
                let cmp = graph
                    .add_core_op(ComputeOp::Compare { op: CmpOp::Lt }, func_id)
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, cmp, 0, 0, TypeId::I32).unwrap();
                graph.add_data_edge(b, cmp, 0, 1, TypeId::I32).unwrap();
                graph.add_data_edge(cmp, ret, 0, 0, TypeId::BOOL).unwrap();
            },
            vec![("a", TypeId::I32), ("b", TypeId::I32)],
            TypeId::BOOL,
        );
    }

    #[test]
    fn test_division_with_guard() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let b = graph
                    .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                    .unwrap();
                let div = graph
                    .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Div }, func_id)
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, div, 0, 0, TypeId::I32).unwrap();
                graph.add_data_edge(b, div, 0, 1, TypeId::I32).unwrap();
                graph.add_data_edge(div, ret, 0, 0, TypeId::I32).unwrap();
            },
            vec![("a", TypeId::I32), ("b", TypeId::I32)],
            TypeId::I32,
        );
    }

    #[test]
    fn test_topological_sort_simple() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();
        let func_id = graph
            .add_function(
                "f".into(), root,
                vec![("a".into(), TypeId::I32)],
                TypeId::I32, Visibility::Public,
            )
            .unwrap();

        let p = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let c = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(1) }, func_id).unwrap();
        let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(p, add, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c, add, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(add, ret, 0, 0, TypeId::I32).unwrap();

        let nodes = graph.function_nodes(func_id);
        let sorted = topological_sort(&nodes, &graph).unwrap();

        // p and c should come before add, add before ret
        let p_pos = sorted.iter().position(|n| *n == p).unwrap();
        let c_pos = sorted.iter().position(|n| *n == c).unwrap();
        let add_pos = sorted.iter().position(|n| *n == add).unwrap();
        let ret_pos = sorted.iter().position(|n| *n == ret).unwrap();

        assert!(p_pos < add_pos);
        assert!(c_pos < add_pos);
        assert!(add_pos < ret_pos);
    }

    #[test]
    fn test_void_return() {
        compile_and_verify(
            |graph, func_id| {
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                let _ = ret;
            },
            vec![],
            TypeId::UNIT,
        );
    }

    #[test]
    fn test_logic_ops() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let b = graph
                    .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                    .unwrap();
                let and_op = graph
                    .add_core_op(
                        ComputeOp::BinaryLogic {
                            op: lmlang_core::ops::LogicOp::And,
                        },
                        func_id,
                    )
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, and_op, 0, 0, TypeId::BOOL).unwrap();
                graph.add_data_edge(b, and_op, 0, 1, TypeId::BOOL).unwrap();
                graph.add_data_edge(and_op, ret, 0, 0, TypeId::BOOL).unwrap();
            },
            vec![("a", TypeId::BOOL), ("b", TypeId::BOOL)],
            TypeId::BOOL,
        );
    }

    #[test]
    fn test_shift_ops() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let b = graph
                    .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                    .unwrap();
                let shl = graph
                    .add_core_op(
                        ComputeOp::Shift {
                            op: lmlang_core::ops::ShiftOp::Shl,
                        },
                        func_id,
                    )
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, shl, 0, 0, TypeId::I32).unwrap();
                graph.add_data_edge(b, shl, 0, 1, TypeId::I32).unwrap();
                graph.add_data_edge(shl, ret, 0, 0, TypeId::I32).unwrap();
            },
            vec![("a", TypeId::I32), ("b", TypeId::I32)],
            TypeId::I32,
        );
    }

    // -----------------------------------------------------------------------
    // Task 2: Memory ops
    // -----------------------------------------------------------------------

    #[test]
    fn test_alloc_store_load() {
        compile_and_verify(
            |graph, func_id| {
                // alloc -> store param -> load -> return
                let ptr_type_id = graph.types.register(lmlang_core::types::LmType::Pointer {
                    pointee: TypeId::I32,
                    mutable: true,
                });

                let param = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let alloc = graph.add_core_op(ComputeOp::Alloc, func_id).unwrap();
                let store = graph.add_core_op(ComputeOp::Store, func_id).unwrap();
                let load = graph.add_core_op(ComputeOp::Load, func_id).unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

                // Alloc outputs a pointer
                graph.add_data_edge(alloc, store, 0, 0, ptr_type_id).unwrap();
                graph.add_data_edge(param, store, 0, 1, TypeId::I32).unwrap();
                graph.add_data_edge(alloc, load, 0, 0, ptr_type_id).unwrap();
                // Load outputs an i32
                graph.add_data_edge(load, ret, 0, 0, TypeId::I32).unwrap();
                // Force store before load via data dependency through alloc
            },
            vec![("x", TypeId::I32)],
            TypeId::I32,
        );
    }

    // -----------------------------------------------------------------------
    // Task 2: Function calls
    // -----------------------------------------------------------------------

    #[test]
    fn test_function_call() {
        // Build a program with two functions: callee(i32) -> i32, caller(i32) -> i32
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // Callee: identity function
        let callee_id = graph
            .add_function(
                "callee".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let callee_param = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, callee_id)
            .unwrap();
        let callee_ret = graph.add_core_op(ComputeOp::Return, callee_id).unwrap();
        graph
            .add_data_edge(callee_param, callee_ret, 0, 0, TypeId::I32)
            .unwrap();

        // Caller: calls callee
        let caller_id = graph
            .add_function(
                "caller".into(),
                root,
                vec![("y".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let caller_param = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, caller_id)
            .unwrap();
        let call_node = graph
            .add_core_op(ComputeOp::Call { target: callee_id }, caller_id)
            .unwrap();
        let caller_ret = graph.add_core_op(ComputeOp::Return, caller_id).unwrap();

        graph
            .add_data_edge(caller_param, call_node, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(call_node, caller_ret, 0, 0, TypeId::I32)
            .unwrap();

        // Compile both functions
        let context = Context::create();
        let module = context.create_module("test_call");
        let builder = context.create_builder();
        crate::runtime::declare_runtime_functions(&context, &module);

        let callee_def = graph.get_function(callee_id).unwrap().clone();
        compile_function(&context, &module, &builder, &graph, callee_id, &callee_def).unwrap();

        let caller_def = graph.get_function(caller_id).unwrap().clone();
        compile_function(&context, &module, &builder, &graph, caller_id, &caller_def).unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    // -----------------------------------------------------------------------
    // Task 2: Struct ops
    // -----------------------------------------------------------------------

    #[test]
    fn test_struct_create_and_get() {
        use indexmap::IndexMap;
        use lmlang_core::types::{StructDef, Visibility as LmVisibility};
        use lmlang_core::id::ModuleId;

        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // Register a Point struct { x: i32, y: i32 }
        let point_type_id = graph
            .types
            .register_named(
                "Point",
                lmlang_core::types::LmType::Struct(StructDef {
                    name: "Point".into(),
                    type_id: TypeId(0),
                    fields: IndexMap::from([
                        ("x".into(), TypeId::I32),
                        ("y".into(), TypeId::I32),
                    ]),
                    module: ModuleId(0),
                    visibility: LmVisibility::Public,
                }),
            )
            .unwrap();

        let func_id = graph
            .add_function(
                "test_struct".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
            .unwrap();

        // StructCreate { x: a, y: b }
        let create = graph
            .add_structured_op(
                lmlang_core::ops::StructuredOp::StructCreate {
                    type_id: point_type_id,
                },
                func_id,
            )
            .unwrap();

        // StructGet field 0 (x)
        let get_x = graph
            .add_structured_op(
                lmlang_core::ops::StructuredOp::StructGet { field_index: 0 },
                func_id,
            )
            .unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(param_a, create, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(param_b, create, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(create, get_x, 0, 0, point_type_id).unwrap();
        graph.add_data_edge(get_x, ret, 0, 0, TypeId::I32).unwrap();

        let context = Context::create();
        let module = context.create_module("test_struct_mod");
        let builder = context.create_builder();
        crate::runtime::declare_runtime_functions(&context, &module);

        let func_def = graph.get_function(func_id).unwrap().clone();
        compile_function(&context, &module, &builder, &graph, func_id, &func_def).unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    // -----------------------------------------------------------------------
    // Task 2: Cast ops
    // -----------------------------------------------------------------------

    #[test]
    fn test_cast_i32_to_f64() {
        compile_and_verify(
            |graph, func_id| {
                let param = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let cast = graph
                    .add_structured_op(
                        lmlang_core::ops::StructuredOp::Cast {
                            target_type: TypeId::F64,
                        },
                        func_id,
                    )
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(param, cast, 0, 0, TypeId::I32).unwrap();
                graph.add_data_edge(cast, ret, 0, 0, TypeId::F64).unwrap();
            },
            vec![("x", TypeId::I32)],
            TypeId::F64,
        );
    }

    #[test]
    fn test_cast_f64_to_i32() {
        compile_and_verify(
            |graph, func_id| {
                let param = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let cast = graph
                    .add_structured_op(
                        lmlang_core::ops::StructuredOp::Cast {
                            target_type: TypeId::I32,
                        },
                        func_id,
                    )
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(param, cast, 0, 0, TypeId::F64).unwrap();
                graph.add_data_edge(cast, ret, 0, 0, TypeId::I32).unwrap();
            },
            vec![("x", TypeId::F64)],
            TypeId::I32,
        );
    }

    #[test]
    fn test_cast_i8_to_i64() {
        compile_and_verify(
            |graph, func_id| {
                let param = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let cast = graph
                    .add_structured_op(
                        lmlang_core::ops::StructuredOp::Cast {
                            target_type: TypeId::I64,
                        },
                        func_id,
                    )
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(param, cast, 0, 0, TypeId::I8).unwrap();
                graph.add_data_edge(cast, ret, 0, 0, TypeId::I64).unwrap();
            },
            vec![("x", TypeId::I8)],
            TypeId::I64,
        );
    }

    // -----------------------------------------------------------------------
    // Task 2: Not and unary abs ops
    // -----------------------------------------------------------------------

    #[test]
    fn test_not_op() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let not = graph.add_core_op(ComputeOp::Not, func_id).unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, not, 0, 0, TypeId::BOOL).unwrap();
                graph.add_data_edge(not, ret, 0, 0, TypeId::BOOL).unwrap();
            },
            vec![("a", TypeId::BOOL)],
            TypeId::BOOL,
        );
    }

    #[test]
    fn test_unary_neg() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let neg = graph
                    .add_core_op(
                        ComputeOp::UnaryArith {
                            op: lmlang_core::ops::UnaryArithOp::Neg,
                        },
                        func_id,
                    )
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, neg, 0, 0, TypeId::I32).unwrap();
                graph.add_data_edge(neg, ret, 0, 0, TypeId::I32).unwrap();
            },
            vec![("x", TypeId::I32)],
            TypeId::I32,
        );
    }

    #[test]
    fn test_unary_abs_float() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let abs = graph
                    .add_core_op(
                        ComputeOp::UnaryArith {
                            op: lmlang_core::ops::UnaryArithOp::Abs,
                        },
                        func_id,
                    )
                    .unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(a, abs, 0, 0, TypeId::F64).unwrap();
                graph.add_data_edge(abs, ret, 0, 0, TypeId::F64).unwrap();
            },
            vec![("x", TypeId::F64)],
            TypeId::F64,
        );
    }

    // -----------------------------------------------------------------------
    // Task 2: Print op
    // -----------------------------------------------------------------------

    #[test]
    fn test_print_op() {
        compile_and_verify(
            |graph, func_id| {
                let param = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                graph.add_data_edge(param, print, 0, 0, TypeId::I32).unwrap();
                // Control edge from print -> ret ensures print is emitted before return
                graph.add_control_edge(print, ret, None).unwrap();
            },
            vec![("x", TypeId::I32)],
            TypeId::UNIT,
        );
    }

    // -----------------------------------------------------------------------
    // Task 2: All arithmetic ops produce valid IR
    // -----------------------------------------------------------------------

    #[test]
    fn test_all_arith_ops() {
        for arith_op in [ArithOp::Add, ArithOp::Sub, ArithOp::Mul, ArithOp::Div, ArithOp::Rem] {
            compile_and_verify(
                |graph, func_id| {
                    let a = graph
                        .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                        .unwrap();
                    let b = graph
                        .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                        .unwrap();
                    let op_node = graph
                        .add_core_op(ComputeOp::BinaryArith { op: arith_op }, func_id)
                        .unwrap();
                    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                    graph.add_data_edge(a, op_node, 0, 0, TypeId::I32).unwrap();
                    graph.add_data_edge(b, op_node, 0, 1, TypeId::I32).unwrap();
                    graph.add_data_edge(op_node, ret, 0, 0, TypeId::I32).unwrap();
                },
                vec![("a", TypeId::I32), ("b", TypeId::I32)],
                TypeId::I32,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 2: All comparison ops produce valid IR
    // -----------------------------------------------------------------------

    #[test]
    fn test_all_cmp_ops() {
        for cmp_op in [CmpOp::Eq, CmpOp::Ne, CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge] {
            compile_and_verify(
                |graph, func_id| {
                    let a = graph
                        .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                        .unwrap();
                    let b = graph
                        .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                        .unwrap();
                    let cmp_node = graph
                        .add_core_op(ComputeOp::Compare { op: cmp_op }, func_id)
                        .unwrap();
                    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
                    graph.add_data_edge(a, cmp_node, 0, 0, TypeId::I32).unwrap();
                    graph.add_data_edge(b, cmp_node, 0, 1, TypeId::I32).unwrap();
                    graph.add_data_edge(cmp_node, ret, 0, 0, TypeId::BOOL).unwrap();
                },
                vec![("a", TypeId::I32), ("b", TypeId::I32)],
                TypeId::BOOL,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 2: ArrayCreate with constant index extract
    // -----------------------------------------------------------------------

    #[test]
    fn test_array_create_and_get_const() {
        compile_and_verify(
            |graph, func_id| {
                let a = graph
                    .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
                    .unwrap();
                let b = graph
                    .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
                    .unwrap();

                // Create [i32; 2] from params
                let arr = graph
                    .add_structured_op(
                        lmlang_core::ops::StructuredOp::ArrayCreate { length: 2 },
                        func_id,
                    )
                    .unwrap();

                let arr_type_id = graph.types.register(lmlang_core::types::LmType::Array {
                    element: TypeId::I32,
                    length: 2,
                });

                // Index 0 constant
                let idx = graph
                    .add_core_op(
                        ComputeOp::Const {
                            value: ConstValue::I32(0),
                        },
                        func_id,
                    )
                    .unwrap();

                let get = graph
                    .add_structured_op(lmlang_core::ops::StructuredOp::ArrayGet, func_id)
                    .unwrap();

                let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

                graph.add_data_edge(a, arr, 0, 0, TypeId::I32).unwrap();
                graph.add_data_edge(b, arr, 0, 1, TypeId::I32).unwrap();
                graph.add_data_edge(arr, get, 0, 0, arr_type_id).unwrap();
                graph.add_data_edge(idx, get, 0, 1, TypeId::I32).unwrap();
                graph.add_data_edge(get, ret, 0, 0, TypeId::I32).unwrap();
            },
            vec![("a", TypeId::I32), ("b", TypeId::I32)],
            TypeId::I32,
        );
    }
}
