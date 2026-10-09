declare const props: object;
declare const b: string;
const a = <div className="x" data-a='y\n' {...props}>text {b} &amp; more</div>;
const c = <><span /></>;
