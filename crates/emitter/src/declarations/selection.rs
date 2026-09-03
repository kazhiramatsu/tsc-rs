use tsc_types::CompilerOptions;

use crate::{EmitHost, EmitResolver, TransformError, Transformer};

use super::{
    BoundaryEvent, DeclarationCustomTransformers, DeclarationPathResolver, DeclarationTransformer,
};

/// tsc-port: getDeclarationTransformers @6.0.3
/// tsc-hash: 928989592ec5ec6efadb06820d80e77af5a7d283616b243f8df3d7a321a47242
/// tsc-span: _tsc.js:115950-115955
pub(crate) fn get_declaration_transformers<'t>(
    options: &'t CompilerOptions,
    resolver: &'t dyn EmitResolver,
    host: &'t dyn EmitHost,
    paths: &'t dyn DeclarationPathResolver,
    custom: &DeclarationCustomTransformers,
) -> Result<Vec<Box<dyn Transformer + 't>>, TransformError> {
    if !custom.is_empty() {
        return Err(TransformError::Unsupported(
            crate::UnsupportedEmitFeature::CustomTransformers,
        ));
    }
    // This is the only call site for DeclarationTransformer::new (L3
    // dormancy control; H2.7b owns production selection).
    Ok(vec![Box::new(DeclarationTransformer::new(
        options, resolver, host, paths,
    ))])
}

/// tsrs-native: harness-only declaration selection with boundary observation;
/// production selection remains the observer-free function above.
#[doc(hidden)]
pub(crate) fn get_declaration_transformers_with_observer<'t>(
    options: &'t CompilerOptions,
    resolver: &'t dyn EmitResolver,
    host: &'t dyn EmitHost,
    paths: &'t dyn DeclarationPathResolver,
    custom: &DeclarationCustomTransformers,
    observer: &'t mut dyn FnMut(BoundaryEvent),
) -> Result<Vec<Box<dyn Transformer + 't>>, TransformError> {
    if !custom.is_empty() {
        return Err(TransformError::Unsupported(
            crate::UnsupportedEmitFeature::CustomTransformers,
        ));
    }
    Ok(vec![Box::new(
        DeclarationTransformer::new(options, resolver, host, paths)
            .with_boundary_observer(observer),
    )])
}
