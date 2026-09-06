
pub(crate) fn bool_expr(expr: &syn::Expr) -> syn::Result<bool> {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(lit), .. }) => Ok(lit.value),
        _ => Err(syn::Error::new_spanned(expr, "invalid value for bool expr")),
    }
}

const PRIMITIVE_TYPES: [&str; 18] = [
    "bool", "char", "str",
    "u8", "u16", "u32", "u64", "u128",
    "i8", "i16", "i32", "i64", "i128",
    "f32", "f64", "f128",
    "usize", "isize"
];

pub(crate) fn is_primitive(ty: &syn::Type) -> bool {
    if let syn::Type::Path(path) = ty {
        PRIMITIVE_TYPES.iter().any(|primitive| path.path.is_ident(primitive))
    } else {
        false
    }
}