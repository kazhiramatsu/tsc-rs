declare module "amb" {
    export const x: number;
    import y = require("inner");
    import z = require("./relative");
    export * from "outer";
}
declare module "other" {}
