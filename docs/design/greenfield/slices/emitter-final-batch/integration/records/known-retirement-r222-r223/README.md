# Known retirement after r211–r213

The frozen runtime source `0336c56663ff243503a18987a2cf8021109e8295`
passed 1,600 new controls twice, 48 selected commands and 108 projects twice,
774 older commands twice, 1,015 emitter tests and 1,739 checker tests.
The transpile suite measured 289 exact and two independent known differences;
its two failing assertions require retiring six rows that now match exactly.
See [the complete raw archive](../layout-controls-r211-r213-complete/manifest.json).

The applied changes retire all 36 parse-recovery universe exceptions and six
transpile exceptions. Historical native observations remain in strict comparator
mutation tests and are explicit witness inputs. The r223 registry tests passed
all 84 tests. `applied-manifest.json` records the actual formatted bytes; the
proposal preserves the pre-format bytes. The post-retirement Rust replay and
workspace Clippy are pending at this record and run separately as r215.
