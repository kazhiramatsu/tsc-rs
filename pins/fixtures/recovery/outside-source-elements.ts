// @noLib: true

function implemented(): void {}

function signature(): void;

function overloaded(value: string): string;
function overloaded(value: string): string { return value; }

function duplicateBodies(): void {}
function duplicateBodies(): void {}

class PlainClass {}

(
