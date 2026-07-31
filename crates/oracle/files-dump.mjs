// One-shot probe: for each program.json argument, print the oracle
// program's source files in getSourceFiles() order (public names).
// Ground truth for the lib-loading order contract: under the host's
// noLib:true + libs-as-roots construction, the order must equal
// ProgramJson.libs ++ ProgramJson.files (m4-lib-loading-steps.md §1).
import {
  createProgramFromJsonPath,
  publicFileName,
} from "./program-host.mjs";

const out = [];
for (const programJsonPath of process.argv.slice(2)) {
  const { program, publicNames } = createProgramFromJsonPath(programJsonPath);
  out.push({
    program: programJsonPath,
    files: program
      .getSourceFiles()
      .map((sf) => publicFileName(sf, publicNames)),
  });
}
console.log(JSON.stringify(out));
