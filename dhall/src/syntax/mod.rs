#![expect(clippy::many_single_char_names, clippy::should_implement_trait)]

mod ast;
pub use crate::syntax::ast::visitor;
pub use crate::syntax::ast::*;
pub use crate::syntax::text::parser::*;
pub mod binary;
pub mod text;
