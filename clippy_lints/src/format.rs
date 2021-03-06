use rustc::hir::*;
use rustc::lint::*;
use rustc::{declare_lint, lint_array};
use if_chain::if_chain;
use rustc::ty;
use syntax::ast::LitKind;
use syntax_pos::Span;
use crate::utils::paths;
use crate::utils::{in_macro, is_expn_of, last_path_segment, match_def_path, match_type, opt_def_id, resolve_node, snippet, span_lint_and_then, walk_ptrs_ty};

/// **What it does:** Checks for the use of `format!("string literal with no
/// argument")` and `format!("{}", foo)` where `foo` is a string.
///
/// **Why is this bad?** There is no point of doing that. `format!("too")` can
/// be replaced by `"foo".to_owned()` if you really need a `String`. The even
/// worse `&format!("foo")` is often encountered in the wild. `format!("{}",
/// foo)` can be replaced by `foo.clone()` if `foo: String` or `foo.to_owned()`
/// if `foo: &str`.
///
/// **Known problems:** None.
///
/// **Examples:**
/// ```rust
/// format!("foo")
/// format!("{}", foo)
/// ```
declare_clippy_lint! {
    pub USELESS_FORMAT,
    complexity,
    "useless use of `format!`"
}

#[derive(Copy, Clone, Debug)]
pub struct Pass;

impl LintPass for Pass {
    fn get_lints(&self) -> LintArray {
        lint_array![USELESS_FORMAT]
    }
}

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for Pass {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if let Some(span) = is_expn_of(expr.span, "format") {
            if in_macro(span) {
                return;
            }
            match expr.node {

                // `format!("{}", foo)` expansion
                ExprKind::Call(ref fun, ref args) => {
                    if_chain! {
                        if let ExprKind::Path(ref qpath) = fun.node;
                        if args.len() == 3;
                        if let Some(fun_def_id) = opt_def_id(resolve_node(cx, qpath, fun.hir_id));
                        if match_def_path(cx.tcx, fun_def_id, &paths::FMT_ARGUMENTS_NEWV1FORMATTED);
                        if check_single_piece(&args[0]);
                        if let Some(format_arg) = get_single_string_arg(cx, &args[1]);
                        if check_unformatted(&args[2]);
                        then {
                            let sugg = format!("{}.to_string()", snippet(cx, format_arg, "<arg>").into_owned());
                            span_lint_and_then(cx, USELESS_FORMAT, span, "useless use of `format!`", |db| {
                                db.span_suggestion(expr.span, "consider using .to_string()", sugg);
                            });
                        }
                    }
                },
                // `format!("foo")` expansion contains `match () { () => [], }`
                ExprKind::Match(ref matchee, _, _) => if let ExprKind::Tup(ref tup) = matchee.node {
                    if tup.is_empty() {
                        let sugg = format!("{}.to_string()", snippet(cx, expr.span, "<expr>").into_owned());
                        span_lint_and_then(cx, USELESS_FORMAT, span, "useless use of `format!`", |db| {
                            db.span_suggestion(span, "consider using .to_string()", sugg);
                        });
                    }
                },
                _ => (),
            }
        }
    }
}

/// Checks if the expressions matches `&[""]`
fn check_single_piece(expr: &Expr) -> bool {
    if_chain! {
        if let ExprKind::AddrOf(_, ref expr) = expr.node; // &[""]
        if let ExprKind::Array(ref exprs) = expr.node; // [""]
        if exprs.len() == 1;
        if let ExprKind::Lit(ref lit) = exprs[0].node;
        if let LitKind::Str(ref lit, _) = lit.node;
        then {
            return lit.as_str().is_empty();
        }
    }

    false
}

/// Checks if the expressions matches
/// ```rust,ignore
/// &match (&"arg",) {
/// (__arg0,) => [::std::fmt::ArgumentV1::new(__arg0,
/// ::std::fmt::Display::fmt)],
/// }
/// ```
/// and that type of `__arg0` is `&str` or `String`
/// then returns the span of first element of the matched tuple
fn get_single_string_arg(cx: &LateContext, expr: &Expr) -> Option<Span> {
    if_chain! {
        if let ExprKind::AddrOf(_, ref expr) = expr.node;
        if let ExprKind::Match(ref match_expr, ref arms, _) = expr.node;
        if arms.len() == 1;
        if arms[0].pats.len() == 1;
        if let PatKind::Tuple(ref pat, None) = arms[0].pats[0].node;
        if pat.len() == 1;
        if let ExprKind::Array(ref exprs) = arms[0].body.node;
        if exprs.len() == 1;
        if let ExprKind::Call(_, ref args) = exprs[0].node;
        if args.len() == 2;
        if let ExprKind::Path(ref qpath) = args[1].node;
        if let Some(fun_def_id) = opt_def_id(resolve_node(cx, qpath, args[1].hir_id));
        if match_def_path(cx.tcx, fun_def_id, &paths::DISPLAY_FMT_METHOD);
        then {
            let ty = walk_ptrs_ty(cx.tables.pat_ty(&pat[0]));
            if ty.sty == ty::TyStr || match_type(cx, ty, &paths::STRING) {
                if let ExprKind::Tup(ref values) = match_expr.node {
                    return Some(values[0].span);
                }
            }
        }
    }

    None
}

/// Checks if the expression matches
/// ```rust,ignore
/// &[_ {
///    format: _ {
///         width: _::Implied,
///         ...
///    },
///    ...,
/// }]
/// ```
fn check_unformatted(expr: &Expr) -> bool {
    if_chain! {
        if let ExprKind::AddrOf(_, ref expr) = expr.node;
        if let ExprKind::Array(ref exprs) = expr.node;
        if exprs.len() == 1;
        if let ExprKind::Struct(_, ref fields, _) = exprs[0].node;
        if let Some(format_field) = fields.iter().find(|f| f.ident.name == "format");
        if let ExprKind::Struct(_, ref fields, _) = format_field.expr.node;
        if let Some(align_field) = fields.iter().find(|f| f.ident.name == "width");
        if let ExprKind::Path(ref qpath) = align_field.expr.node;
        if last_path_segment(qpath).ident.name == "Implied";
        then {
            return true;
        }
    }

    false
}
