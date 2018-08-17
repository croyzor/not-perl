use vm;
use syntax::tree;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TyExpr {
    Any,
    Definite(String),
    None,
}

impl From<vm::Ty> for TyExpr {
    fn from(other: vm::Ty) -> Self {
        match other {
            vm::Ty::Float => TyExpr::Definite("Float".to_string()),
            vm::Ty::Bool => TyExpr::Definite("Bool".to_string()),
            vm::Ty::Int => TyExpr::Definite("Int".to_string()),
            vm::Ty::Array => TyExpr::Definite("Array".to_string()),
            vm::Ty::Str => TyExpr::Definite("Str".to_string()),
            vm::Ty::User(s) => TyExpr::Definite(s),
            vm::Ty::Any => TyExpr::Any,
            vm::Ty::None => TyExpr::None,
        }
    }
}

/// Type alias for a user-defined type.
///
/// Since the syntax and IR would effectively be the same, it would be more work to keep two
/// different structures in tandem with one another.
pub type UserTy<'n> = tree::UserTy<'n>;
