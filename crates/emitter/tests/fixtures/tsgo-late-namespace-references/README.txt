Exact pinned tsgo 7.1 JavaScript from target/typescript7/bin/tsc.
The tsxReactEmit6 consumer was split from the copied vendored conformance harness; it uses es2015/commonjs.
Others use es2024/esnext; all jsx=react, strict=true, alwaysStrict=true. Custom factory/fragment options match their names.
metadata-helper additionally uses experimentalDecorators and emitDecoratorMetadata.
Controls pin shadowed factory roots, global helper Object.assign despite N.Object, and standard-decorator context Symbol despite N.Symbol.
