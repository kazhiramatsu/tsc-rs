
use super::{ProgramLoadError, ProgramLoadOperation};
use crate::{PreparationError, PreparationOperation, ResolutionError};
use std::error::Error;
use tsc_host::{HostError, HostErrorKind, HostOperation};

#[test]
fn boxed_causes_remain_downcastable_to_the_public_error_types() {
    let host = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        None,
        "fixture read denied",
    );
    let load = ProgramLoadError::host(ProgramLoadOperation::ReadSource, None, host.clone());
    assert_eq!(
        load.source().unwrap().downcast_ref::<HostError>(),
        Some(&host)
    );

    let resolution = ResolutionError::Host(host);
    let load = ProgramLoadError::resolution(
        ProgramLoadOperation::ResolveModule,
        None,
        Some("fixture".into()),
        resolution.clone(),
    );
    assert_eq!(
        load.source().unwrap().downcast_ref::<ResolutionError>(),
        Some(&resolution),
    );
    assert!(load.source().unwrap().source().unwrap().is::<HostError>());

    let preparation = PreparationError::from_resolution_js(
        PreparationOperation::AddModuleResolution,
        None,
        resolution.clone(),
    );
    assert_eq!(preparation.resolution(), Some(&resolution));
    assert_eq!(
        preparation
            .source()
            .unwrap()
            .downcast_ref::<ResolutionError>(),
        Some(&resolution),
    );
    let load = ProgramLoadError::Preparation {
        operation: ProgramLoadOperation::BuildPreparedProgram,
        source: preparation.clone(),
    };
    assert_eq!(
        load.source().unwrap().downcast_ref::<PreparationError>(),
        Some(&preparation),
    );
}
