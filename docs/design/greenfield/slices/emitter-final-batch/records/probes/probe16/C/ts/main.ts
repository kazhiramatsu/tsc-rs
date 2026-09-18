async function main() {
    for (using d1 of [{ [Symbol.dispose]() {} }, null]) {
        await d1;
    }
}
