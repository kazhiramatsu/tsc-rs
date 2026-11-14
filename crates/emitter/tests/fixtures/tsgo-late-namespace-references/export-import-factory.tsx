declare namespace GlobalReact { function createElement(...args: any[]): any; } namespace N { export import React = GlobalReact; export const x = <div />; }
