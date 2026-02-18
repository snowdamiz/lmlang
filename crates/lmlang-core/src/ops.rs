//! Op node enums for the computational graph.
//!
//! Defines the complete operation vocabulary in two tiers:
//! - **Tier 1 ([`ComputeOp`])**: ~24 core operations covering arithmetic, comparison,
//!   logic, shifts, control flow (both high-level and low-level), memory, functions,
//!   I/O (console + file), and closures.
//! - **Tier 2 ([`StructuredOp`])**: 10 operations for aggregate access, type casts,
//!   and enum operations.
//!
//! # Design: Type Inference from Edges
//!
//! Op nodes do NOT carry explicit type parameters. Types are inferred from the typed
//! data flow edges connected to each node. This follows the LLVM IR model where
//! operations are typed by their operands, not by annotations.
//!
//! **Exception:** Operations that create or convert to a specific type carry the
//! minimum necessary type info that cannot be inferred from inputs:
//! - [`StructuredOp::Cast`] carries `target_type`
//! - [`StructuredOp::StructCreate`] carries `type_id`
//! - [`StructuredOp::EnumCreate`] carries `type_id` and `variant_index`
//!
//! # LLVM Lowering
//!
//! Every operation has a documented LLVM IR lowering path. See the doc comments
//! on individual variants and the mapping table in the phase research document.

use serde::{Deserialize, Serialize};

use crate::id::FunctionId;
use crate::type_id::TypeId;
use crate::types::ConstValue;

// ---------------------------------------------------------------------------
// Sub-enums for grouped operations
// ---------------------------------------------------------------------------

/// Binary arithmetic operators.
///
/// # LLVM Lowering
/// Each variant maps to a pair of LLVM instructions selected by input type:
/// - Integer: `add`, `sub`, `mul`, `sdiv`/`udiv`, `srem`/`urem`
/// - Float: `fadd`, `fsub`, `fmul`, `fdiv`, `frem`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    /// Division semantics (signed vs unsigned) are determined by type context
    /// during LLVM lowering, following the LLVM approach where `iN` types have
    /// no inherent signedness. Lowers to `sdiv`/`udiv` (int) or `fdiv` (float).
    Div,
    Rem,
}

/// Unary arithmetic operators.
///
/// # LLVM Lowering
/// - `Neg`: integer `sub 0, %val` or float `fneg %val`
/// - `Abs`: intrinsic `llvm.abs` (int) or `llvm.fabs` (float)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryArithOp {
    Neg,
    Abs,
}

/// Comparison operators.
///
/// # LLVM Lowering
/// Signed/unsigned/float comparison is selected during lowering based on input type:
/// - Signed int: `icmp slt`, `icmp sle`, etc.
/// - Unsigned int: `icmp ult`, `icmp ule`, etc.
/// - Float: `fcmp olt`, `fcmp ole`, etc. (ordered comparisons)
/// - Equality (`Eq`/`Ne`): `icmp eq`/`icmp ne` (int) or `fcmp oeq`/`fcmp une` (float)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Binary logic operators.
///
/// # LLVM Lowering
/// - `And`: `and`
/// - `Or`: `or`
/// - `Xor`: `xor`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicOp {
    And,
    Or,
    Xor,
}

/// Bit shift operators.
///
/// # LLVM Lowering
/// - `Shl`: `shl` (shift left)
/// - `ShrLogical`: `lshr` (logical shift right, zero-fill)
/// - `ShrArith`: `ashr` (arithmetic shift right, sign-extend)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShiftOp {
    Shl,
    ShrLogical,
    ShrArith,
}

// ---------------------------------------------------------------------------
// Tier 1: Core operations (~24 grouped ops)
// ---------------------------------------------------------------------------

/// Tier 1: Core computational operations.
///
/// These are the fundamental building blocks of the executable graph. Each variant
/// maps directly to one or more LLVM IR instructions. Types are inferred from
/// input data flow edges -- no TypeId is stored on arithmetic, logic, or comparison
/// ops (see module-level docs for rationale).
///
/// The op set is intentionally "CISC-like" (richer, grouped) for agent usability:
/// fewer nodes per program means smaller graph representations that fit better
/// in AI context windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputeOp {
    // -- Constants & Literals --
    /// Produces a typed constant value.
    /// Lowers to: LLVM constant literal.
    Const { value: ConstValue },

    // -- Arithmetic (grouped) --
    /// Binary arithmetic: add, sub, mul, div, rem.
    /// Lowers to: `add`/`fadd`, `sub`/`fsub`, `mul`/`fmul`, `sdiv`/`udiv`/`fdiv`, `srem`/`urem`/`frem`.
    BinaryArith { op: ArithOp },
    /// Unary arithmetic: neg, abs.
    /// Lowers to: `sub 0, %val`/`fneg`, `llvm.abs`/`llvm.fabs`.
    UnaryArith { op: UnaryArithOp },

    // -- Comparison --
    /// Comparison: eq, ne, lt, le, gt, ge.
    /// Lowers to: `icmp`/`fcmp` with predicate selected by input type.
    Compare { op: CmpOp },

    // -- Logic --
    /// Binary logic: and, or, xor.
    /// Lowers to: `and`, `or`, `xor`.
    BinaryLogic { op: LogicOp },
    /// Logical/bitwise NOT.
    /// Lowers to: `xor %val, -1` (all-ones mask).
    Not,

    // -- Bitwise --
    /// Bit shifts: shl, shr_logical, shr_arith.
    /// Lowers to: `shl`, `lshr`, `ashr`.
    Shift { op: ShiftOp },

    // -- Control Flow (high-level structured) --
    /// High-level if-then-else construct.
    /// Takes a boolean condition input, then-branch body, else-branch body.
    /// Lowers to: `br i1 %cond, label %then_bb, label %else_bb` + merge_bb + `phi`.
    IfElse,
    /// High-level loop construct.
    /// Takes a condition, loop body; produces a value on exit.
    /// Lowers to: loop_header_bb + `phi` + body_bb + `br` back-edge to header.
    Loop,
    /// High-level match/switch on discriminant.
    /// Takes discriminant input, N match arms.
    /// Lowers to: `switch` instruction or chain of `br` instructions.
    Match,

    // -- Control Flow (low-level) --
    /// Low-level conditional branch to one of two targets.
    /// Lowers to: `br i1 %cond, label %true_bb, label %false_bb`.
    Branch,
    /// Low-level unconditional jump to a single target.
    /// Lowers to: `br label %target_bb`.
    Jump,
    /// SSA phi node: merges values from different control flow paths.
    /// Lowers to: `phi <ty> [%val1, %bb1], [%val2, %bb2], ...`.
    Phi,

    // -- Memory --
    /// Allocate memory on the stack (or heap, determined by lowering).
    /// Lowers to: `alloca <ty>`.
    Alloc,
    /// Read a value from a memory location / reference.
    /// Lowers to: `load <ty>, ptr %addr`.
    Load,
    /// Write a value to a memory location / reference.
    /// Lowers to: `store <ty> %val, ptr %addr`.
    Store,
    /// Compute the address of a struct field or array element.
    /// Lowers to: `getelementptr inbounds <ty>, ptr %base, i32 %idx`.
    GetElementPtr,

    // -- Functions --
    /// Direct call to a known function.
    /// Lowers to: `call <ret_ty> @<target>(<args>)`.
    Call { target: FunctionId },
    /// Indirect call through a function pointer (closures, virtual dispatch).
    /// Lowers to: `call <ret_ty> %fn_ptr(<args>)`.
    IndirectCall,
    /// Return from the current function.
    /// Lowers to: `ret <ty> %val` or `ret void`.
    Return,
    /// Function parameter input node. Each parameter gets its own node with
    /// a unique index.
    /// Lowers to: LLVM function argument at the given position.
    Parameter { index: u32 },

    // -- I/O (console) --
    /// Output a value to stdout.
    /// Lowers to: `call @printf(...)` or runtime print function.
    Print,
    /// Read a line from stdin.
    /// Lowers to: `call @readline(...)` or runtime readline function.
    ReadLine,

    // -- I/O (file) --
    /// Open a file, producing a file handle.
    /// Lowers to: `call @fopen(...)`.
    FileOpen,
    /// Read data from an open file handle.
    /// Lowers to: `call @fread(...)`.
    FileRead,
    /// Write data to an open file handle.
    /// Lowers to: `call @fwrite(...)`.
    FileWrite,
    /// Close an open file handle.
    /// Lowers to: `call @fclose(...)`.
    FileClose,

    // -- Closures --
    /// Create a closure by capturing environment values.
    /// Takes captured values as data flow inputs, produces a closure value
    /// (function pointer + environment struct pointer).
    /// Lowers to: allocate environment struct, store captured values, produce
    /// `{ ptr @fn, ptr %env }` struct.
    MakeClosure { function: FunctionId },
    /// Access a captured variable from the enclosing scope by index into
    /// the closure's capture list.
    /// Lowers to: `getelementptr` on the environment struct pointer.
    CaptureAccess { index: u32 },
}

impl ComputeOp {
    /// Returns `true` if this op is a control flow operation.
    ///
    /// Control flow ops are: `IfElse`, `Loop`, `Match`, `Branch`, `Jump`, `Phi`.
    pub fn is_control_flow(&self) -> bool {
        matches!(
            self,
            ComputeOp::IfElse
                | ComputeOp::Loop
                | ComputeOp::Match
                | ComputeOp::Branch
                | ComputeOp::Jump
                | ComputeOp::Phi
        )
    }

    /// Returns `true` if this op is an I/O operation (console or file).
    ///
    /// I/O ops are: `Print`, `ReadLine`, `FileOpen`, `FileRead`, `FileWrite`, `FileClose`.
    pub fn is_io(&self) -> bool {
        matches!(
            self,
            ComputeOp::Print
                | ComputeOp::ReadLine
                | ComputeOp::FileOpen
                | ComputeOp::FileRead
                | ComputeOp::FileWrite
                | ComputeOp::FileClose
        )
    }

    /// Returns `true` if this op is a basic block terminator.
    ///
    /// Terminators end a basic block and transfer control elsewhere.
    /// Terminator ops are: `Return`, `Branch`, `Jump`.
    pub fn is_terminator(&self) -> bool {
        matches!(
            self,
            ComputeOp::Return | ComputeOp::Branch | ComputeOp::Jump
        )
    }
}

// ---------------------------------------------------------------------------
// Tier 2: Structured/aggregate operations (10 ops)
// ---------------------------------------------------------------------------

/// Tier 2: Structured and aggregate operations.
///
/// These operations work with composite types (structs, arrays, enums) and
/// type conversions. They carry `TypeId` or index fields because the target
/// type/field cannot be inferred from input edges alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StructuredOp {
    /// Create a struct value from field values (one data input per field).
    /// Lowers to: sequential `insertvalue` instructions or direct struct literal.
    StructCreate { type_id: TypeId },
    /// Extract a field from a struct by index.
    /// Lowers to: `extractvalue %struct, <field_index>`.
    StructGet { field_index: u32 },
    /// Produce a new struct with one field replaced (functional update).
    /// Lowers to: `insertvalue %struct, %new_val, <field_index>`.
    StructSet { field_index: u32 },

    /// Create a fixed-size array from element values.
    /// Lowers to: sequential `insertvalue` instructions into an array type.
    ArrayCreate { length: u32 },
    /// Get an element from an array by index (index provided via data edge).
    /// Lowers to: `extractvalue` (constant index) or `getelementptr` + `load` (dynamic).
    ArrayGet,
    /// Produce a new array with one element replaced (functional update).
    /// Lowers to: `insertvalue` (constant index) or `getelementptr` + `store` (dynamic).
    ArraySet,

    /// Type cast / conversion between types.
    /// Lowers to: `trunc`/`zext`/`sext`/`fptrunc`/`fpext`/`fptosi`/`sitofp`/etc.
    /// The specific instruction is selected based on source type (from input edge)
    /// and `target_type`.
    Cast { target_type: TypeId },

    /// Create an enum/tagged union value for a specific variant.
    /// Lowers to: store discriminant + store payload into `{ i8, [max_payload x i8] }` struct.
    EnumCreate {
        type_id: TypeId,
        variant_index: u32,
    },
    /// Extract the discriminant from an enum value (returns an integer).
    /// Lowers to: `extractvalue %enum, 0` (first field of the enum struct).
    EnumDiscriminant,
    /// Extract the payload from a specific enum variant (requires prior discriminant check).
    /// Lowers to: `extractvalue %enum, 1` + bitcast to variant's payload type.
    EnumPayload { variant_index: u32 },
}

// ---------------------------------------------------------------------------
// ComputeNodeOp: wraps both tiers
// ---------------------------------------------------------------------------

/// Either a core (Tier 1) or structured (Tier 2) operation.
///
/// Used as the `op` field in [`ComputeNode`](crate::node::ComputeNode) to allow
/// a single node type to represent any operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputeNodeOp {
    /// Tier 1 core operation.
    Core(ComputeOp),
    /// Tier 2 structured/aggregate operation.
    Structured(StructuredOp),
}

impl ComputeNodeOp {
    /// Returns the tier of this operation: 1 for core, 2 for structured.
    pub fn tier(&self) -> u8 {
        match self {
            ComputeNodeOp::Core(_) => 1,
            ComputeNodeOp::Structured(_) => 2,
        }
    }

    /// Delegates to [`ComputeOp::is_control_flow`]. Always `false` for structured ops.
    pub fn is_control_flow(&self) -> bool {
        match self {
            ComputeNodeOp::Core(op) => op.is_control_flow(),
            ComputeNodeOp::Structured(_) => false,
        }
    }

    /// Delegates to [`ComputeOp::is_terminator`]. Always `false` for structured ops.
    pub fn is_terminator(&self) -> bool {
        match self {
            ComputeNodeOp::Core(op) => op.is_terminator(),
            ComputeNodeOp::Structured(_) => false,
        }
    }

    /// Delegates to [`ComputeOp::is_io`]. Always `false` for structured ops.
    pub fn is_io(&self) -> bool {
        match self {
            ComputeNodeOp::Core(op) => op.is_io(),
            ComputeNodeOp::Structured(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ConstValue;

    #[test]
    fn is_control_flow_returns_true_for_all_cf_ops() {
        let cf_ops = vec![
            ComputeOp::IfElse,
            ComputeOp::Loop,
            ComputeOp::Match,
            ComputeOp::Branch,
            ComputeOp::Jump,
            ComputeOp::Phi,
        ];

        for op in &cf_ops {
            assert!(
                op.is_control_flow(),
                "{:?} should be control flow",
                op
            );
        }
    }

    #[test]
    fn is_control_flow_returns_false_for_non_cf_ops() {
        let non_cf_ops = vec![
            ComputeOp::Const {
                value: ConstValue::I32(0),
            },
            ComputeOp::BinaryArith { op: ArithOp::Add },
            ComputeOp::Compare { op: CmpOp::Eq },
            ComputeOp::Not,
            ComputeOp::Alloc,
            ComputeOp::Load,
            ComputeOp::Store,
            ComputeOp::Return,
            ComputeOp::Print,
            ComputeOp::Call {
                target: FunctionId(0),
            },
            ComputeOp::MakeClosure {
                function: FunctionId(0),
            },
        ];

        for op in &non_cf_ops {
            assert!(
                !op.is_control_flow(),
                "{:?} should NOT be control flow",
                op
            );
        }
    }

    #[test]
    fn is_terminator_returns_true_for_terminators() {
        let terminators = vec![
            ComputeOp::Return,
            ComputeOp::Branch,
            ComputeOp::Jump,
        ];

        for op in &terminators {
            assert!(
                op.is_terminator(),
                "{:?} should be a terminator",
                op
            );
        }
    }

    #[test]
    fn is_terminator_returns_false_for_non_terminators() {
        let non_terminators = vec![
            ComputeOp::IfElse,
            ComputeOp::Loop,
            ComputeOp::Match,
            ComputeOp::Phi,
            ComputeOp::Const {
                value: ConstValue::Bool(true),
            },
            ComputeOp::BinaryArith { op: ArithOp::Mul },
            ComputeOp::Print,
        ];

        for op in &non_terminators {
            assert!(
                !op.is_terminator(),
                "{:?} should NOT be a terminator",
                op
            );
        }
    }

    #[test]
    fn is_io_returns_true_for_io_ops() {
        let io_ops = vec![
            ComputeOp::Print,
            ComputeOp::ReadLine,
            ComputeOp::FileOpen,
            ComputeOp::FileRead,
            ComputeOp::FileWrite,
            ComputeOp::FileClose,
        ];

        for op in &io_ops {
            assert!(op.is_io(), "{:?} should be I/O", op);
        }
    }

    #[test]
    fn is_io_returns_false_for_non_io_ops() {
        let non_io = vec![
            ComputeOp::Const {
                value: ConstValue::I32(0),
            },
            ComputeOp::BinaryArith { op: ArithOp::Add },
            ComputeOp::Return,
            ComputeOp::IfElse,
            ComputeOp::Alloc,
        ];

        for op in &non_io {
            assert!(!op.is_io(), "{:?} should NOT be I/O", op);
        }
    }

    #[test]
    fn serde_roundtrip_const() {
        let op = ComputeOp::Const {
            value: ConstValue::I64(42),
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: ComputeOp = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_binary_arith() {
        let op = ComputeOp::BinaryArith { op: ArithOp::Div };
        let json = serde_json::to_string(&op).unwrap();
        let back: ComputeOp = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_call() {
        let op = ComputeOp::Call {
            target: FunctionId(7),
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: ComputeOp = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_struct_create() {
        let op = StructuredOp::StructCreate {
            type_id: TypeId(42),
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: StructuredOp = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_cast() {
        let op = StructuredOp::Cast {
            target_type: TypeId(5),
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: StructuredOp = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_compute_node_op() {
        let core_op = ComputeNodeOp::Core(ComputeOp::BinaryArith { op: ArithOp::Add });
        let json = serde_json::to_string(&core_op).unwrap();
        let back: ComputeNodeOp = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);

        let struct_op = ComputeNodeOp::Structured(StructuredOp::EnumCreate {
            type_id: TypeId(100),
            variant_index: 2,
        });
        let json = serde_json::to_string(&struct_op).unwrap();
        let back: ComputeNodeOp = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn compute_node_op_tier() {
        assert_eq!(
            ComputeNodeOp::Core(ComputeOp::Alloc).tier(),
            1
        );
        assert_eq!(
            ComputeNodeOp::Structured(StructuredOp::ArrayGet).tier(),
            2
        );
    }

    #[test]
    fn compute_node_op_delegates_is_control_flow() {
        assert!(ComputeNodeOp::Core(ComputeOp::IfElse).is_control_flow());
        assert!(!ComputeNodeOp::Core(ComputeOp::Alloc).is_control_flow());
        assert!(!ComputeNodeOp::Structured(StructuredOp::ArrayGet).is_control_flow());
    }

    #[test]
    fn compute_node_op_delegates_is_terminator() {
        assert!(ComputeNodeOp::Core(ComputeOp::Return).is_terminator());
        assert!(!ComputeNodeOp::Core(ComputeOp::IfElse).is_terminator());
        assert!(!ComputeNodeOp::Structured(StructuredOp::Cast { target_type: TypeId(0) }).is_terminator());
    }

    #[test]
    fn compute_node_op_delegates_is_io() {
        assert!(ComputeNodeOp::Core(ComputeOp::Print).is_io());
        assert!(!ComputeNodeOp::Core(ComputeOp::Alloc).is_io());
        assert!(!ComputeNodeOp::Structured(StructuredOp::ArrayGet).is_io());
    }
}
