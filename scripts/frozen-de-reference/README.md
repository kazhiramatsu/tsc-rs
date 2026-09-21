The original H2.7d/e candidate inputs, census and TypeScript observations are
frozen reference evidence. Their complete artifact hashes are consumed by
qualification generators, Rust acceptance tests and generated fixtures.

The chain walk refreshes four parent ratchets. It preserves the original D/E
artifacts and checks them with:

```sh
node scripts/check-frozen-de-reference.mjs --walk-preflight
node scripts/check-frozen-de-reference.mjs --check h2-7de-candidates h2-7de-observations
```

Every invocation first verifies the pinned manifest and all 587 input files.
The four compressed snapshots supply the exact historical parent bytes;
all other files must still match their original hashes in the current tree.
The current, unchanged candidate generator then prepares all 325 inputs.
Those complete inputs and the complete census must equal the original,
except for the four enumerated parent provenance hashes, each checked against
the actual current file. A change in membership, owners, source facts,
options, roots, contracts or any other field fails.

For replay, the verifier copies the unchanged generators, vendor, corpus and
original expected artifacts to a temporary tree, restores the four historical
parents there, and executes only `--check`. Symlinks are rejected: the real
location of TypeScript determines its default library path. All copied files
are hashed before and after execution; canonical inputs are rechecked too.
The 323 whole-program observations run twice each. These are historical
TypeScript reference observations, with a separate current-input comparison;
they confer no Rust admission and do not claim current-parent provenance.

The two standalone observers which consume the same original census can be
checked through this context as well:

```sh
node scripts/check-frozen-de-reference.mjs --check bundle-plan output-directory-corpus
```

`CHECK_ONLY` explicitly registers the D/E pair in this reference context and
the independent map-option projection in the canonical tree. The walk checks
registry coverage, snapshot integrity and current projection before minting,
then runs all three reference checks at its tail. ORDER remains the 75 active
minting scripts, ordered by their actual dependencies: the H1 emit oracle
precedes active-transform and the Rust omission inventory, and the H2.7b/c/d/e
qualifications precede the H2.5g profile which consumes them. The topology
audit includes every declared output and imported producer paths. Unknown,
missing, duplicate or overlapping entries refuse.
Mutation tests run as part of the existing qualification test suite.

The preflight checks the parents that exist before minting; the tail comparison
checks the newly minted parents. Neither the number of changed parent hashes
nor a previous pass replaces that final comparison on later walks.

Snapshot origin: canonical head
`2ee2bd6a96f0e8bd0605ffa4f6e59ffc9dbcad69`. The manifest binds each compressed
snapshot and decompressed file separately, and the verifier pins the manifest.
Updating this reference requires explicit evidence review; the chain walk
never rewrites it.
