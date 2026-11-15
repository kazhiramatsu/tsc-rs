//! Publication's package-name union, with the original ordered comparisons.
use super::*;

pub(crate) struct PreparedPackageMaps {
    pub(crate) legacy: BTreeMap<JsString, bool>,
    by_package: Vec<u8>,
}

impl PreparedPackageMaps {
    pub(crate) fn new<'a>(facts: impl IntoIterator<Item = (&'a PreparedPackageRef, bool)>) -> Self {
        struct Name {
            sample: Arc<PreparedPackageId>,
            declaration: bool,
        }
        let mut names: Vec<Name> = Vec::new();
        let mut buckets: rustc_hash::FxHashMap<u64, Vec<u32>> = Default::default();
        let mut package_names: Vec<u32> = Vec::new();
        for (package, declaration) in facts {
            if package.index >= package_names.len() {
                package_names.resize(package.index + 1, 0);
            }
            let name = match package_names[package.index] {
                0 => {
                    let bucket = buckets.entry(package.id.name_hash()).or_default();
                    let name = bucket.iter().copied().find(|&index| {
                        names[index as usize].sample.name() == package.value().name()
                    });
                    let index = name.unwrap_or_else(|| {
                        let index = u32::try_from(names.len()).expect("package names fit u32");
                        names.push(Name {
                            sample: Arc::clone(&package.id),
                            declaration: false,
                        });
                        bucket.push(index);
                        index
                    });
                    package_names[package.index] = index.checked_add(1).expect("name fits u32");
                    index as usize
                }
                index => index as usize - 1,
            };
            names[name].declaration |= declaration;
        }
        // Preserve first insertion order as well as values. The existing
        // byte-borrowed lookups must traverse the same ordered name table.
        let mut legacy = BTreeMap::new();
        for name in &names {
            legacy.insert(name.sample.name().to_owned(), name.declaration);
        }
        let flags: Vec<_> = names
            .iter()
            .map(|name| {
                let name = name.sample.name();
                flag(
                    legacy.contains_key(&crate::js_string_ops::types_package_name(name)),
                    TYPES_PACKAGE,
                ) | flag(
                    legacy.get(name.as_bytes()).copied().unwrap_or(false),
                    BUNDLES_TYPES,
                )
            })
            .collect();
        let by_package = package_names
            .into_iter()
            .map(|name| {
                if name == 0 {
                    0
                } else {
                    flags[name as usize - 1]
                }
            })
            .collect();
        Self { legacy, by_package }
    }

    pub(super) fn flags(&self, package: &PreparedPackageRef) -> u8 {
        self.by_package[package.index]
    }
}
