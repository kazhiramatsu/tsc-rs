//! tsgo `ls/hovericon.go`: the Visual Studio image beside a hover's symbol
//! and the rich content (`_vs_rawContent`) Visual Studio shows in place of
//! the markdown.

use tsc_lsproto::{
    VSClassifiedTextElement, VSClassifiedTextRun, VSContainerElement, VSContainerElementStyle,
    VSImageElement, VSImageElementOrClassifiedTextElementOrContainerElement, VSImageId,
};

use crate::lsutil::{ScriptElementKind, ScriptElementKindModifier};

/// The GUID of Visual Studio's shared image catalog
/// (Microsoft.VisualStudio.Imaging.KnownImageIds).
const VS_IMAGE_CATALOG_GUID: &str = "ae27a6b0-e345-4288-96df-5eaf394ee369";

// The known image ids TypeScript-VS's ImageIdMapping.cs uses for hovers.
const IMAGE_ID_WARNING: i32 = 0x0000_0637;
const IMAGE_ID_KEYWORD: i32 = 0x0000_0635;
const IMAGE_ID_MODULE_PRIVATE: i32 = 0x0000_077D;
const IMAGE_ID_MODULE_PROTECTED: i32 = 0x0000_077E;
const IMAGE_ID_MODULE_PUBLIC: i32 = 0x0000_077F;
const IMAGE_ID_TYPE: i32 = 0x0000_0CA1;
const IMAGE_ID_NAMESPACE: i32 = 0x0000_079F;
const IMAGE_ID_CLASS_PRIVATE: i32 = 0x0000_01D7;
const IMAGE_ID_CLASS_PROTECTED: i32 = 0x0000_01D8;
const IMAGE_ID_CLASS_PUBLIC: i32 = 0x0000_01D9;
const IMAGE_ID_INTERFACE_PRIVATE: i32 = 0x0000_0646;
const IMAGE_ID_INTERFACE_PROTECTED: i32 = 0x0000_0647;
const IMAGE_ID_INTERFACE_PUBLIC: i32 = 0x0000_0648;
const IMAGE_ID_ENUM_PRIVATE: i32 = 0x0000_0469;
const IMAGE_ID_ENUM_PROTECTED: i32 = 0x0000_046A;
const IMAGE_ID_ENUM_PUBLIC: i32 = 0x0000_046B;
const IMAGE_ID_ENUM_MEMBER: i32 = 0x0000_0465;
const IMAGE_ID_LOCAL_VARIABLE: i32 = 0x0000_06D3;
const IMAGE_ID_PROPERTY_PRIVATE: i32 = 0x0000_0982;
const IMAGE_ID_PROPERTY_PROTECTED: i32 = 0x0000_0983;
const IMAGE_ID_PROPERTY_PUBLIC: i32 = 0x0000_0984;
const IMAGE_ID_METHOD_PRIVATE: i32 = 0x0000_0756;
const IMAGE_ID_METHOD_PROTECTED: i32 = 0x0000_0757;
const IMAGE_ID_METHOD_PUBLIC: i32 = 0x0000_0758;
const IMAGE_ID_LABEL: i32 = 0x0000_067D;
const IMAGE_ID_ASSEMBLY: i32 = 0x0000_00C4;
const IMAGE_ID_CONSTANT_PRIVATE: i32 = 0x0000_026A;
const IMAGE_ID_CONSTANT_PROTECTED: i32 = 0x0000_026B;
const IMAGE_ID_CONSTANT_PUBLIC: i32 = 0x0000_026C;

fn image(id: i32) -> VSImageId {
    VSImageId {
        guid: VS_IMAGE_CATALOG_GUID.to_owned(),
        id,
        _vs_type: Default::default(),
    }
}

/// tsgo `getVSHoverImageId`: the image for a symbol's kind and modifiers,
/// as TypeScript-VS's ImageIdMapping.GetImageId picks it (no internal
/// variant: its chevron is C#'s assembly visibility).
pub(crate) fn vs_hover_image_id(
    kind: ScriptElementKind,
    modifiers: ScriptElementKindModifier,
) -> VSImageId {
    let pick = |private: i32, protected: i32, public: i32| {
        image(if modifiers.contains(ScriptElementKindModifier::PRIVATE) {
            private
        } else if modifiers.contains(ScriptElementKindModifier::PROTECTED) {
            protected
        } else {
            public
        })
    };
    use ScriptElementKind as Kind;
    match kind {
        Kind::Warning => image(IMAGE_ID_WARNING),
        Kind::Keyword => image(IMAGE_ID_KEYWORD),
        Kind::ScriptElement => pick(
            IMAGE_ID_MODULE_PRIVATE,
            IMAGE_ID_MODULE_PROTECTED,
            IMAGE_ID_MODULE_PUBLIC,
        ),
        Kind::PrimitiveType => image(IMAGE_ID_TYPE),
        Kind::ModuleElement => image(IMAGE_ID_NAMESPACE),
        Kind::ConstructorImplementationElement
        | Kind::ClassElement
        | Kind::LocalClassElement
        | Kind::TypeElement => pick(
            IMAGE_ID_CLASS_PRIVATE,
            IMAGE_ID_CLASS_PROTECTED,
            IMAGE_ID_CLASS_PUBLIC,
        ),
        Kind::InterfaceElement => pick(
            IMAGE_ID_INTERFACE_PRIVATE,
            IMAGE_ID_INTERFACE_PROTECTED,
            IMAGE_ID_INTERFACE_PUBLIC,
        ),
        Kind::EnumElement => pick(
            IMAGE_ID_ENUM_PRIVATE,
            IMAGE_ID_ENUM_PROTECTED,
            IMAGE_ID_ENUM_PUBLIC,
        ),
        Kind::EnumMemberElement => image(IMAGE_ID_ENUM_MEMBER),
        Kind::ParameterElement
        | Kind::VariableElement
        | Kind::LocalVariableElement
        | Kind::VariableUsingElement
        | Kind::VariableAwaitUsingElement
        | Kind::LetElement
        | Kind::String => image(IMAGE_ID_LOCAL_VARIABLE),
        Kind::ConstElement => pick(
            IMAGE_ID_CONSTANT_PRIVATE,
            IMAGE_ID_CONSTANT_PROTECTED,
            IMAGE_ID_CONSTANT_PUBLIC,
        ),
        Kind::MemberGetAccessorElement
        | Kind::MemberSetAccessorElement
        | Kind::MemberVariableElement
        | Kind::MemberAccessorVariableElement => pick(
            IMAGE_ID_PROPERTY_PRIVATE,
            IMAGE_ID_PROPERTY_PROTECTED,
            IMAGE_ID_PROPERTY_PUBLIC,
        ),
        Kind::FunctionElement
        | Kind::LocalFunctionElement
        | Kind::MemberFunctionElement
        | Kind::CallSignatureElement
        | Kind::IndexSignatureElement
        | Kind::ConstructSignatureElement => pick(
            IMAGE_ID_METHOD_PRIVATE,
            IMAGE_ID_METHOD_PROTECTED,
            IMAGE_ID_METHOD_PUBLIC,
        ),
        Kind::TypeParameterElement => image(IMAGE_ID_TYPE),
        Kind::Label => image(IMAGE_ID_LABEL),
        Kind::Alias => image(IMAGE_ID_MODULE_PUBLIC),
        _ => image(IMAGE_ID_ASSEMBLY),
    }
}

/// tsgo `buildVSHoverRawContent`: the symbol's image beside its colorized
/// declaration line, over the colorized documentation when there is some.
pub(crate) fn build_vs_hover_raw_content(
    image_id: VSImageId,
    quick_info_runs: Vec<VSClassifiedTextRun>,
    documentation_runs: Vec<VSClassifiedTextRun>,
) -> Option<VSContainerElement> {
    if quick_info_runs.is_empty() {
        return None;
    }
    let display_line = VSContainerElement {
        style: VSContainerElementStyle::WRAPPED,
        elements: vec![
            VSImageElementOrClassifiedTextElementOrContainerElement::ImageElement(VSImageElement {
                image_id,
                _vs_type: Default::default(),
            }),
            VSImageElementOrClassifiedTextElementOrContainerElement::ClassifiedTextElement(
                VSClassifiedTextElement {
                    runs: quick_info_runs,
                    _vs_type: Default::default(),
                },
            ),
        ],
        _vs_type: Default::default(),
    };
    if documentation_runs.is_empty() {
        return Some(display_line);
    }
    Some(VSContainerElement {
        style: VSContainerElementStyle::STACKED,
        elements: vec![
            VSImageElementOrClassifiedTextElementOrContainerElement::ContainerElement(display_line),
            VSImageElementOrClassifiedTextElementOrContainerElement::ClassifiedTextElement(
                VSClassifiedTextElement {
                    runs: documentation_runs,
                    _vs_type: Default::default(),
                },
            ),
        ],
        _vs_type: Default::default(),
    })
}

/// tsgo `ProvideHover`'s Visual Studio contents of several projections:
/// stacked.
pub(crate) fn stacked_raw_contents(contents: Vec<VSContainerElement>) -> VSContainerElement {
    VSContainerElement {
        style: VSContainerElementStyle::STACKED,
        elements: contents
            .into_iter()
            .map(VSImageElementOrClassifiedTextElementOrContainerElement::ContainerElement)
            .collect(),
        _vs_type: Default::default(),
    }
}
