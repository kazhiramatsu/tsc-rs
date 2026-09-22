use quote::ToTokens;
use serde_json::json;
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
struct NormalizeIncludes { base: std::path::PathBuf, workspace: std::path::PathBuf, references: Vec<serde_json::Value> }
impl VisitMut for NormalizeIncludes {
    fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
        if mac.path.is_ident("include_str") || mac.path.is_ident("include_bytes") {
            let literal: syn::LitStr = syn::parse2(mac.tokens.clone()).expect("single literal include path required");
            let target = std::fs::canonicalize(self.base.join(literal.value())).expect("include target must exist");
            self.references.push(json!({"kind": mac.path.to_token_stream().to_string(),
                "literal": literal.value(), "target": target, "start": location(literal.span().start()),
                "end": location(literal.span().end())}));
            let relative = target.strip_prefix(&self.workspace).expect("include must stay inside the measured workspace");
            let normalized = syn::LitStr::new(&format!("<workspace>/{}", relative.display()), literal.span());
            mac.tokens = normalized.to_token_stream();
        }
        syn::visit_mut::visit_macro_mut(self, mac);
    }
}
fn location(p: proc_macro2::LineColumn) -> serde_json::Value { json!([p.line, p.column]) }
fn main() {
    let mut files = Vec::new();
    for file in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&file).unwrap();
        let mut syntax = syn::parse_file(&text).unwrap();
        let mut normalizer = NormalizeIncludes { base: std::path::Path::new(&file).parent().unwrap().into(),
            workspace: std::fs::canonicalize(std::env::var("PROOF_WORKSPACE_ROOT").expect("explicit proof root")).unwrap(),
            references: Vec::new() };
        normalizer.visit_file_mut(&mut syntax);
        let complete_tokens = syntax.to_token_stream().to_string();
        let mut modules = Vec::new();
        syntax.items.retain(|item| {
            let syn::Item::Mod(module) = item else { return true; };
            let is_test = module.attrs.iter().any(|a| a.path().is_ident("cfg")
                && a.meta.require_list().is_ok_and(|m| m.tokens.to_string() == "test"));
            if !is_test { return true; }
            let mut value = json!({"name": module.ident.to_string(),
                "start": location(module.span().start()), "end": location(module.span().end()),
                "attrs": module.attrs.iter().map(|a|a.to_token_stream().to_string()).collect::<Vec<_>>(),
                "visibility": module.vis.to_token_stream().to_string()});
            if let Some((brace, items)) = &module.content {
                value["open_start"] = location(brace.span.open().start());
                value["open_end"] = location(brace.span.open().end());
                value["close_start"] = location(brace.span.close().start());
                value["close_end"] = location(brace.span.close().end());
                value["body_tokens"] = json!(items.iter().map(|i|i.to_token_stream().to_string()).collect::<Vec<_>>().join(" "));
            }
            modules.push(value);
            false
        });
        files.push(json!({"path": file, "modules": modules,
            "include_references": normalizer.references,
            "production_tokens": syntax.to_token_stream().to_string(), "complete_tokens": complete_tokens}));
    }
    println!("{}", serde_json::to_string(&files).unwrap());
}
