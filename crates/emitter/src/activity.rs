//! H2 runtime-activity boundary.
//!
//! H2.0b installs this session-owned observer before the first broad-emit
//! runtime slice is admitted.  Existing H1 activity is counted as a positive
//! wiring canary.  Every later H2 slice has a reserved counter and is rejected
//! while the observer remains in the frozen H1 profile.

/// Runtime slices whose newly admitted behavior must be observed explicitly.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum H2RuntimeSlice {
    H2_1a,
    H2_1b,
    H2_1c,
    H2_1d,
    H2_1e,
    H2_2a,
    H2_2b,
    H2_2c,
    H2_2d,
    H2_3a,
    H2_3b,
    H2_3c,
    H2_3d,
    H2_4a,
    H2_4b,
    H2_5a,
    H2_5b,
    H2_5c,
    H2_5d,
    H2_5e,
    H2_5f,
    H2_5g,
    H2_5h,
    H2_6a,
    H2_6b,
    H2_6c,
    H2_7a,
    H2_7b,
    H2_7c,
    H2_7d,
    H2_7e,
    H2_8a,
    H2_8b,
    H2_8c,
    H2_8d,
    H2_8e,
    H2_9,
}

impl H2RuntimeSlice {
    pub const ALL: [Self; 37] = [
        Self::H2_1a,
        Self::H2_1b,
        Self::H2_1c,
        Self::H2_1d,
        Self::H2_1e,
        Self::H2_2a,
        Self::H2_2b,
        Self::H2_2c,
        Self::H2_2d,
        Self::H2_3a,
        Self::H2_3b,
        Self::H2_3c,
        Self::H2_3d,
        Self::H2_4a,
        Self::H2_4b,
        Self::H2_5a,
        Self::H2_5b,
        Self::H2_5c,
        Self::H2_5d,
        Self::H2_5e,
        Self::H2_5f,
        Self::H2_5g,
        Self::H2_5h,
        Self::H2_6a,
        Self::H2_6b,
        Self::H2_6c,
        Self::H2_7a,
        Self::H2_7b,
        Self::H2_7c,
        Self::H2_7d,
        Self::H2_7e,
        Self::H2_8a,
        Self::H2_8b,
        Self::H2_8c,
        Self::H2_8d,
        Self::H2_8e,
        Self::H2_9,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::H2_1a => "H2.1a",
            Self::H2_1b => "H2.1b",
            Self::H2_1c => "H2.1c",
            Self::H2_1d => "H2.1d",
            Self::H2_1e => "H2.1e",
            Self::H2_2a => "H2.2a",
            Self::H2_2b => "H2.2b",
            Self::H2_2c => "H2.2c",
            Self::H2_2d => "H2.2d",
            Self::H2_3a => "H2.3a",
            Self::H2_3b => "H2.3b",
            Self::H2_3c => "H2.3c",
            Self::H2_3d => "H2.3d",
            Self::H2_4a => "H2.4a",
            Self::H2_4b => "H2.4b",
            Self::H2_5a => "H2.5a",
            Self::H2_5b => "H2.5b",
            Self::H2_5c => "H2.5c",
            Self::H2_5d => "H2.5d",
            Self::H2_5e => "H2.5e",
            Self::H2_5f => "H2.5f",
            Self::H2_5g => "H2.5g",
            Self::H2_5h => "H2.5h",
            Self::H2_6a => "H2.6a",
            Self::H2_6b => "H2.6b",
            Self::H2_6c => "H2.6c",
            Self::H2_7a => "H2.7a",
            Self::H2_7b => "H2.7b",
            Self::H2_7c => "H2.7c",
            Self::H2_7d => "H2.7d",
            Self::H2_7e => "H2.7e",
            Self::H2_8a => "H2.8a",
            Self::H2_8b => "H2.8b",
            Self::H2_8c => "H2.8c",
            Self::H2_8d => "H2.8d",
            Self::H2_8e => "H2.8e",
            Self::H2_9 => "H2.9",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Exact activity observed during one emitting session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct H2ActivityCounters {
    emit_session_constructions: u64,
    output_plan_constructions: u64,
    emit_resolver_borrows: u64,
    script_transformer_list_constructions: u64,
    transform_typescript_constructions: u64,
    transform_class_fields_constructions: u64,
    transform_ecmascript_module_constructions: u64,
    transform_context_constructions: u64,
    printer_constructions: u64,
    javascript_artifact_creations: u64,
    output_sink_write_attempts: u64,
    output_sink_failures: u64,
    runtime_slices: [u64; 37],
}

impl Default for H2ActivityCounters {
    fn default() -> Self {
        H2ActivityCanary::h1_profile().counters()
    }
}

macro_rules! counter_accessors {
    ($($name:ident),+ $(,)?) => {
        $(
            pub const fn $name(self) -> u64 {
                self.$name
            }
        )+
    };
}

impl H2ActivityCounters {
    counter_accessors!(
        emit_session_constructions,
        output_plan_constructions,
        emit_resolver_borrows,
        script_transformer_list_constructions,
        transform_typescript_constructions,
        transform_class_fields_constructions,
        transform_ecmascript_module_constructions,
        transform_context_constructions,
        printer_constructions,
        javascript_artifact_creations,
        output_sink_write_attempts,
        output_sink_failures,
    );

    pub const fn runtime_slice(self, slice: H2RuntimeSlice) -> u64 {
        self.runtime_slices[slice.index()]
    }

    pub fn h2_runtime_is_zero(self) -> bool {
        self.runtime_slices.iter().all(|count| *count == 0)
    }

    pub fn all_zero(self) -> bool {
        self == Self::default()
    }
}

/// Session-local recorder threaded through every current H1 construction seam.
#[derive(Debug, Default)]
#[doc(hidden)]
pub struct H2ActivityCanary {
    counters: H2ActivityCounters,
    admitted_runtime_slices: u64,
}

impl H2ActivityCanary {
    /// Construct the frozen H1 profile: no H2 runtime slice is admitted.
    #[doc(hidden)]
    pub const fn h1_profile() -> Self {
        Self {
            counters: H2ActivityCounters {
                emit_session_constructions: 0,
                output_plan_constructions: 0,
                emit_resolver_borrows: 0,
                script_transformer_list_constructions: 0,
                transform_typescript_constructions: 0,
                transform_class_fields_constructions: 0,
                transform_ecmascript_module_constructions: 0,
                transform_context_constructions: 0,
                printer_constructions: 0,
                javascript_artifact_creations: 0,
                output_sink_write_attempts: 0,
                output_sink_failures: 0,
                runtime_slices: [0; 37],
            },
            admitted_runtime_slices: 0,
        }
    }

    /// Construct the current production admission profile. H2.1a is the
    /// first runtime expansion; every later slice remains fail-closed.
    #[doc(hidden)]
    pub const fn h2_1a_profile() -> Self {
        let mut profile = Self::h1_profile();
        profile.admitted_runtime_slices = 1_u64 << H2RuntimeSlice::H2_1a.index();
        profile
    }

    /// Construct the current production admission profile. H2.1b adds the
    /// CommonJS arm while retaining the H2.1a implied-ESM owner that selects
    /// the per-source branch.
    #[doc(hidden)]
    pub const fn h2_1b_profile() -> Self {
        let mut profile = Self::h2_1a_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_1b.index();
        profile
    }

    /// Construct the current production admission profile. H2.1c activates
    /// the AMD and UMD delegates that share transformModule's H2.1b core.
    #[doc(hidden)]
    pub const fn h2_1c_profile() -> Self {
        let mut profile = Self::h2_1b_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_1c.index();
        profile
    }

    /// Construct the current production admission profile. H2.1d activates
    /// the dedicated System.register module transformer.
    #[doc(hidden)]
    pub const fn h2_1d_profile() -> Self {
        let mut profile = Self::h2_1c_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_1d.index();
        profile
    }

    /// Construct the current production admission profile. H2.1e activates
    /// Node16/18/20/Next per-file dispatch, Node output extensions, import
    /// attributes, and relative TypeScript-extension rewriting.
    #[doc(hidden)]
    pub const fn h2_1e_profile() -> Self {
        let mut profile = Self::h2_1d_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_1e.index();
        profile
    }

    /// Construct the current production admission profile. H2.2a activates
    /// runtime enum emission and const-enum preservation/inlining inside
    /// `transformTypeScript`.
    #[doc(hidden)]
    pub const fn h2_2a_profile() -> Self {
        let mut profile = Self::h2_1e_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_2a.index();
        profile
    }

    /// Construct the current production admission profile. H2.2b activates
    /// runtime namespace/module-declaration emission and export-container
    /// substitution inside `transformTypeScript`.
    #[doc(hidden)]
    pub const fn h2_2b_profile() -> Self {
        let mut profile = Self::h2_2a_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_2b.index();
        profile
    }

    /// Construct the current production admission profile. H2.2c activates
    /// parameter-property field projection and constructor assignment
    /// ordering inside `transformTypeScript`.
    #[doc(hidden)]
    pub const fn h2_2c_profile() -> Self {
        let mut profile = Self::h2_2b_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_2c.index();
        profile
    }

    /// Construct the current production admission profile. H2.2d activates
    /// import/export-equals erasure, value preservation, and their module
    /// format projections.
    #[doc(hidden)]
    pub const fn h2_2d_profile() -> Self {
        let mut profile = Self::h2_2c_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_2d.index();
        profile
    }

    /// Construct the current production admission profile. H2.3a activates
    /// `.js`/`.mjs`/`.cjs` source and output routing under the effective
    /// `allowJs` option. JSX-family sources remain fail-closed for H2.3b.
    #[doc(hidden)]
    pub const fn h2_3a_profile() -> Self {
        let mut profile = Self::h2_2d_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_3a.index();
        profile
    }

    /// Construct the current production admission profile. H2.3b activates
    /// classic JSX/TSX preservation and React-factory lowering, including
    /// `.jsx` output selection. Automatic JSX runtimes remain fail-closed for
    /// H2.3c.
    #[doc(hidden)]
    pub const fn h2_3b_profile() -> Self {
        let mut profile = Self::h2_3a_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_3b.index();
        profile
    }

    /// Construct the current production admission profile. H2.3c activates
    /// automatic/development JSX runtimes, implicit helper imports, import
    /// source precedence, and automatic-runtime file-kind interactions.
    #[doc(hidden)]
    pub const fn h2_3c_profile() -> Self {
        let mut profile = Self::h2_3b_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_3c.index();
        profile
    }

    /// Construct the current production admission profile. H2.3d activates
    /// JSON source eligibility, relocated `.json` output, and JSON-specific
    /// printer formatting under effective `resolveJsonModule`.
    #[doc(hidden)]
    pub const fn h2_3d_profile() -> Self {
        let mut profile = Self::h2_3c_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_3d.index();
        profile
    }

    /// Construct the current production admission profile. H2.4a activates
    /// legacy decorator lowering, decorator metadata, and the checker-owned
    /// referenced-value/check-flag facts consumed by that transformer.
    #[doc(hidden)]
    pub const fn h2_4a_profile() -> Self {
        let mut profile = Self::h2_3d_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_4a.index();
        profile
    }

    /// Construct the current production admission profile. H2.4b activates
    /// standard decorators and the ESNext class-fields branches selected by
    /// `useDefineForClassFields`.
    #[doc(hidden)]
    pub const fn h2_4b_profile() -> Self {
        let mut profile = Self::h2_4a_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_4b.index();
        profile
    }

    /// Construct the current production admission profile. H2.5a admits the
    /// ES2021-through-latest-standard target band and the `transformESNext`
    /// explicit-resource-management boundary reached below ESNext.
    #[doc(hidden)]
    pub const fn h2_5a_profile() -> Self {
        let mut profile = Self::h2_4b_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_5a.index();
        profile
    }

    /// Construct the current production admission profile. H2.5b admits the
    /// ES2020 target boundary and its scoped logical-assignment lowering.
    #[doc(hidden)]
    pub const fn h2_5b_profile() -> Self {
        let mut profile = Self::h2_5a_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_5b.index();
        profile
    }

    /// Construct the current production admission profile. H2.5c admits the
    /// ES2019 target boundary and its optional-chain/nullish lowering.
    #[doc(hidden)]
    pub const fn h2_5c_profile() -> Self {
        let mut profile = Self::h2_5b_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_5c.index();
        profile
    }

    /// Construct the current production admission profile. H2.5d admits the
    /// ES2018 target boundary and optional-catch-binding lowering.
    #[doc(hidden)]
    pub const fn h2_5d_profile() -> Self {
        let mut profile = Self::h2_5c_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_5d.index();
        profile
    }

    /// Construct the current production admission profile. H2.5e admits the
    /// ES2017 target boundary and its ES2018 object-rest/spread, async
    /// generator, and asynchronous-iteration lowering.
    #[doc(hidden)]
    pub const fn h2_5e_profile() -> Self {
        let mut profile = Self::h2_5d_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_5e.index();
        profile
    }

    /// Construct the current production admission profile. H2.5f admits the
    /// ES2016 target boundary and async-function lowering.
    #[doc(hidden)]
    pub const fn h2_5f_profile() -> Self {
        let mut profile = Self::h2_5e_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_5f.index();
        profile
    }

    /// Construct the current production admission profile. H2.5g admits the
    /// ES2015 target boundary and exponentiation lowering.
    #[doc(hidden)]
    pub const fn h2_5g_profile() -> Self {
        let mut profile = Self::h2_5f_profile();
        profile.admitted_runtime_slices |= 1_u64 << H2RuntimeSlice::H2_5g.index();
        profile
    }

    pub const fn counters(&self) -> H2ActivityCounters {
        self.counters
    }

    fn increment(counter: &mut u64, activity: &str) {
        *counter = counter
            .checked_add(1)
            .unwrap_or_else(|| panic!("H2 activity counter overflow: {activity}"));
    }

    pub fn construct_emit_session(&mut self) {
        Self::increment(
            &mut self.counters.emit_session_constructions,
            "emit session",
        );
    }

    pub fn construct_output_plan(&mut self) {
        Self::increment(&mut self.counters.output_plan_constructions, "output plan");
    }

    pub fn borrow_emit_resolver(&mut self) {
        Self::increment(&mut self.counters.emit_resolver_borrows, "emit resolver");
    }

    pub fn construct_script_transformer_list(&mut self) {
        Self::increment(
            &mut self.counters.script_transformer_list_constructions,
            "script transformer list",
        );
    }

    pub fn construct_transform_typescript(&mut self) {
        Self::increment(
            &mut self.counters.transform_typescript_constructions,
            "transformTypeScript",
        );
    }

    pub fn construct_transform_class_fields(&mut self) {
        Self::increment(
            &mut self.counters.transform_class_fields_constructions,
            "transformClassFields",
        );
    }

    pub fn construct_transform_ecmascript_module(&mut self) {
        Self::increment(
            &mut self.counters.transform_ecmascript_module_constructions,
            "transformECMAScriptModule",
        );
    }

    pub fn construct_transform_context(&mut self) {
        Self::increment(
            &mut self.counters.transform_context_constructions,
            "transform context",
        );
    }

    pub fn construct_printer(&mut self) {
        Self::increment(&mut self.counters.printer_constructions, "printer");
    }

    pub fn create_javascript_artifact(&mut self) {
        Self::increment(
            &mut self.counters.javascript_artifact_creations,
            "JavaScript artifact",
        );
    }

    pub fn attempt_output_sink_write(&mut self) {
        Self::increment(
            &mut self.counters.output_sink_write_attempts,
            "output sink write",
        );
    }

    pub fn observe_output_sink_failure(&mut self) {
        Self::increment(
            &mut self.counters.output_sink_failures,
            "output sink failure",
        );
    }

    /// Record newly admitted behavior. The H1 profile panics before the
    /// behavior can proceed, so adding an H2 constructor without expanding
    /// the explicit admission mask cannot silently broaden the runtime.
    #[cold]
    #[track_caller]
    pub fn observe_runtime_slice(&mut self, slice: H2RuntimeSlice) {
        let bit = 1_u64 << slice.index();
        assert!(
            self.admitted_runtime_slices & bit != 0,
            "unadmitted H2 runtime activity: {}",
            slice.name(),
        );
        Self::increment(
            &mut self.counters.runtime_slices[slice.index()],
            slice.name(),
        );
    }
}

#[cfg(test)]
#[path = "../tests/unit/activity/tests.rs"]
mod tests;
