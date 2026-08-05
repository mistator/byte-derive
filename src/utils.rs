
pub(crate) fn bool_expr(expr: &syn::Expr) -> syn::Result<bool> {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(lit), .. }) => Ok(lit.value),
        _ => Err(syn::Error::new_spanned(expr, "invalid value for bool expr")),
    }
}