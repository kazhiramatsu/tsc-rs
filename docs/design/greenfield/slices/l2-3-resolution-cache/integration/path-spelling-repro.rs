use tsc_host::MemoryCompilerHost;
use tsc_program::{CancellationToken,ChangeBatch,CompilerOptions,ProgramOptions,ProgramPath,ResolutionCache,ResolutionMode,RetentionLimits};
#[test]
fn case_insensitive_type_root_spelling_change_matches_fresh() {
 let host=MemoryCompilerHost::builder_js("/p").case_sensitive(false)
 .file_js("/p/types/pkg/index.d.ts",b"export {};".to_vec()).build().unwrap();
 let options=CompilerOptions {module_resolution:Some(99),module:Some(199),..CompilerOptions::default()};
 let lower=ProgramOptions::default().with_type_roots(vec![ProgramPath::from_js_parts("/p/types".into(),"/p/types".into()).unwrap()]);
 let upper=ProgramOptions::default().with_type_roots(vec![ProgramPath::from_js_parts("/p/TYPES".into(),"/p/types".into()).unwrap()]);
 let token=CancellationToken::default(); let mut cache=ResolutionCache::new(RetentionLimits::default());
 let mut first=cache.begin(ChangeBatch::default(),&host,&options,&lower);
 let before=first.resolve_type_reference("/p/main.ts".into(),"pkg".into(),ResolutionMode::CommonJs,false,&token).unwrap();
 cache.publish(first).unwrap();
 let mut next=cache.begin(ChangeBatch::default(),&host,&options,&upper);
 let after=next.resolve_type_reference("/p/main.ts".into(),"pkg".into(),ResolutionMode::CommonJs,false,&token).unwrap();
 let mut fresh_cache=ResolutionCache::new(RetentionLimits::default());
 let mut fresh=fresh_cache.begin(ChangeBatch::default(),&host,&options,&upper);
 let expected=fresh.resolve_type_reference("/p/main.ts".into(),"pkg".into(),ResolutionMode::CommonJs,false,&token).unwrap();
 eprintln!("before={} after={} fresh={}",before.value().summary_json(),after.value().summary_json(),expected.value().summary_json());
 assert_eq!(after.value(),expected.value());
}

#[test]
fn containing_directory_spelling_matches_fresh() {
 let host=MemoryCompilerHost::builder_js("/p").case_sensitive(false)
 .file_js("/p/util.ts",b"export {};".to_vec()).build().unwrap();
 let options=CompilerOptions {module_resolution:Some(99),module:Some(199),..CompilerOptions::default()};
 let program=ProgramOptions::default(); let token=CancellationToken::default();
 let mut cache=ResolutionCache::new(RetentionLimits::default());
 let mut c=cache.begin(ChangeBatch::default(),&host,&options,&program);
 c.resolve_module("/p/main.ts".into(),"./util".into(),ResolutionMode::CommonJs,&token).unwrap();
 let after=c.resolve_module("/P/main.ts".into(),"./util".into(),ResolutionMode::CommonJs,&token).unwrap();
 let mut fresh_cache=ResolutionCache::new(RetentionLimits::default());
 let mut fresh=fresh_cache.begin(ChangeBatch::default(),&host,&options,&program);
 let expected=fresh.resolve_module("/P/main.ts".into(),"./util".into(),ResolutionMode::CommonJs,&token).unwrap();
 eprintln!("after={} fresh={}",after.value().summary_json(),expected.value().summary_json());
 assert_eq!(after.value(),expected.value());
}
