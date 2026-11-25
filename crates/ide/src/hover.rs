use hir::{
    BinaryOrigin, BinaryTarget, DefDatabase, FileId, ResolvedSymbol, ResolvedSymbolKind,
    SymbolDetail,
};
use syntax::syntax_kind::SyntaxKind::{CLASS_KW, IDENT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverResult {
    pub content: String,
    pub binary_target: Option<BinaryTarget>,
}

pub fn hover(db: &dyn DefDatabase, file_id: FileId, offset: u32) -> Option<HoverResult> {
    if let Some(resolved) = hir::resolve_at_offset(db, file_id, offset) {
        return Some(HoverResult {
            content: format_resolved_hover(&resolved),
            binary_target: resolved.binary_target,
        });
    }

    let syntax_node = db.syntax_tree(file_id);

    // Hover positions refer to the character under the pointer, so prefer the token that
    // starts at `offset` over the one that ends there.
    let token = syntax_node.token_at_offset(offset.into()).right_biased()?;

    match token.kind() {
        CLASS_KW => Some(HoverResult {
            content: "Keyword `class` declares a new class".to_string(),
            binary_target: None,
        }),
        IDENT => Some(HoverResult {
            content: format!("Identifier `{}`", token.text()),
            binary_target: None,
        }),
        _ => None,
    }
}

pub(crate) fn format_resolved_hover(sym: &ResolvedSymbol) -> String {
    let base = match sym.kind {
        ResolvedSymbolKind::Class
        | ResolvedSymbolKind::Interface
        | ResolvedSymbolKind::Enum
        | ResolvedSymbolKind::Record
        | ResolvedSymbolKind::Annotation => {
            let kw = match sym.kind {
                ResolvedSymbolKind::Class => "class",
                ResolvedSymbolKind::Interface => "interface",
                ResolvedSymbolKind::Enum => "enum",
                ResolvedSymbolKind::Record => "record",
                ResolvedSymbolKind::Annotation => "@interface",
                _ => unreachable!(),
            };
            let tparams = match &sym.detail {
                Some(SymbolDetail::TypeParams(params)) if !params.is_empty() => {
                    format!("<{}>", params.join(", "))
                }
                _ => String::new(),
            };
            format!("```java\n{kw} {}{tparams}\n```", sym.qualified_name)
        }
        ResolvedSymbolKind::Field | ResolvedSymbolKind::RecordComponent => match &sym.detail {
            Some(SymbolDetail::TypeRef(ty)) => format!("```java\n{ty} {}\n```", sym.qualified_name),
            _ => format!("```java\n{}\n```", sym.qualified_name),
        },
        ResolvedSymbolKind::Method => {
            let sig = match &sym.detail {
                Some(SymbolDetail::Method {
                    return_type: Some(ret),
                    params,
                }) => format!("{ret} {}{params}", sym.qualified_name),
                Some(SymbolDetail::Method {
                    return_type: None,
                    params,
                }) => format!("{}{params}", sym.qualified_name),
                _ => format!("{}()", sym.qualified_name),
            };
            format!("```java\n{sig}\n```")
        }
        ResolvedSymbolKind::Constructor => {
            let params = match &sym.detail {
                Some(SymbolDetail::Constructor { params }) => params.as_str(),
                _ => "()",
            };
            format!("```java\n{}{params}\n```", sym.qualified_name)
        }
        ResolvedSymbolKind::EnumConstant => format!("```java\n{}\n```", sym.qualified_name),
        ResolvedSymbolKind::LocalVar
        | ResolvedSymbolKind::Param
        | ResolvedSymbolKind::LambdaParam
        | ResolvedSymbolKind::ForEachVar
        | ResolvedSymbolKind::CatchParam
        | ResolvedSymbolKind::ResourceVar
        | ResolvedSymbolKind::PatternVar => {
            let ty = match &sym.detail {
                Some(SymbolDetail::TypeRef(ty)) => ty.as_str(),
                _ => "var",
            };
            let label = match sym.kind {
                ResolvedSymbolKind::LocalVar => "local variable",
                ResolvedSymbolKind::Param => "parameter",
                ResolvedSymbolKind::LambdaParam => "lambda parameter",
                ResolvedSymbolKind::ForEachVar => "for-each variable",
                ResolvedSymbolKind::CatchParam => "catch parameter",
                ResolvedSymbolKind::ResourceVar => "resource variable",
                ResolvedSymbolKind::PatternVar => "pattern variable",
                _ => unreachable!(),
            };
            format!("```java\n{ty} {}\n```\n{label}", sym.name)
        }
        ResolvedSymbolKind::TypeParam => match &sym.detail {
            Some(SymbolDetail::Bounds(bounds)) if !bounds.is_empty() => format!(
                "```java\ntype parameter {} extends {}\n```",
                sym.name,
                bounds.join(" & ")
            ),
            _ => format!("```java\ntype parameter {}\n```", sym.name),
        },
        ResolvedSymbolKind::Package => format!("```java\npackage {}\n```", sym.qualified_name),
    };

    let mut out = base;
    if let Some(ov) = &sym.overrides {
        out.push_str("\n*overrides `");
        out.push_str(ov);
        out.push_str("`*");
    }
    if let Some(target) = &sym.binary_target {
        match &target.origin {
            BinaryOrigin::Jdk {
                module_name: Some(module_name),
            } => {
                out.push_str("\n*from JDK module `");
                out.push_str(module_name);
                out.push_str("`*");
            }
            BinaryOrigin::Jdk { module_name: None } => out.push_str("\n*from JDK*"),
            BinaryOrigin::Jar { label } => {
                out.push_str("\n*from `");
                out.push_str(label);
                out.push_str("`*");
            }
        }
    }
    out
}
