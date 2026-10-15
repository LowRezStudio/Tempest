//! UnrealScript compiler: a port of UE3's `FScriptCompiler` producing the
//! same object graph and bytecode as `make`, targeting Paladins' engine.

pub mod engine;
pub mod lexer;
pub mod model;
pub mod scope;
pub mod types;
pub mod expr;
pub mod field;
pub mod link;
pub mod decl;
pub mod stmt;
pub mod preprocess;
pub mod output;
pub mod exportsort;
pub mod driver;
pub mod defaults;
pub mod values;
