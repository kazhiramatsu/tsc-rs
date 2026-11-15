//! Dense load identities with a value-indexed fallback for owned API rows.
use super::*;

pub(super) enum LoadedPackageIds {
    Dense(Vec<Arc<PreparedPackageId>>),
    Indexed(BTreeMap<Arc<PreparedPackageId>, NonZeroU32>),
}
impl Default for LoadedPackageIds {
    fn default() -> Self {
        Self::Dense(Vec::new())
    }
}
impl LoadedPackageIds {
    fn next(len: usize) -> NonZeroU32 {
        NonZeroU32::new(u32::try_from(len + 1).expect("package id fits u32")).unwrap()
    }
    pub(super) fn intern_prepared(&mut self, package: &PreparedPackageRef) -> NonZeroU32 {
        match self {
            Self::Dense(values) => {
                let id = Self::next(values.len());
                values.push(Arc::clone(&package.id));
                id
            }
            Self::Indexed(values) => {
                let next = Self::next(values.len());
                *values.entry(Arc::clone(&package.id)).or_insert(next)
            }
        }
    }
    // The cached display does not participate in equality or ordering.
    #[allow(clippy::mutable_key_type)]
    pub(super) fn intern_owned(&mut self, package: PackageId) -> NonZeroU32 {
        if let Self::Dense(values) = self {
            let values = std::mem::take(values);
            *self = Self::Indexed(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(index, package)| (package, Self::next(index)))
                    .collect(),
            );
        }
        let Self::Indexed(values) = self else {
            unreachable!("owned insertion installs the value index")
        };
        let next = Self::next(values.len());
        *values
            .entry(Arc::new(PreparedPackageId::new(package)))
            .or_insert(next)
    }
    pub(super) fn into_values(self) -> Vec<PackageId> {
        let values = match self {
            Self::Dense(values) => values,
            Self::Indexed(values) => {
                let mut values: Vec<_> =
                    values.into_iter().map(|(value, id)| (id, value)).collect();
                values.sort_unstable_by_key(|(id, _)| *id);
                values.into_iter().map(|(_, value)| value).collect()
            }
        };
        values
            .into_iter()
            .map(PreparedPackageId::into_value)
            .collect()
    }
}
